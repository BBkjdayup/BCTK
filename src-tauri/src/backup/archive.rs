use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::{
    BACKUP_FORMAT, BACKUP_FORMAT_VERSION, BackupCreation, BackupError, BackupFileRole,
    BackupInspection, BackupLimits, BackupManifest, BackupManifestFile, BackupResult,
    CreateBackupRequest, DATABASE_ARCHIVE_PATH, ExtractedBackup, MANIFEST_PATH,
    path::{
        archive_name_to_path, cleanup_staging, ensure_empty_staging, ensure_safe_parent,
        relative_archive_path, require_absolute, require_tqb_extension, validate_archive_name,
        validate_no_path_collisions,
    },
};

#[derive(Clone, Debug)]
struct SourceFile {
    archive_path: String,
    role: BackupFileRole,
    source_path: PathBuf,
}

struct TemporaryFileGuard {
    path: PathBuf,
    active: bool,
}

impl TemporaryFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TemporaryFileGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Clone, Debug)]
struct EntryMetadata {
    index: usize,
    name: String,
    size: u64,
}

#[derive(Clone, Debug)]
struct ValidatedArchive {
    inspection: BackupInspection,
    entries: BTreeMap<String, EntryMetadata>,
}

#[derive(Clone, Debug)]
struct RawCentralEntry {
    name: String,
    name_bytes: Vec<u8>,
    flags: u16,
    compression_method: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
}

pub fn create_backup(
    request: &CreateBackupRequest,
    limits: &BackupLimits,
) -> BackupResult<BackupCreation> {
    validate_create_request(request)?;
    let sources = collect_sources(request, limits)?;
    if sources.len().saturating_add(1) > limits.max_entries {
        return Err(BackupError::LimitExceeded {
            resource: "archive entries".to_owned(),
            limit: limits.max_entries as u64,
            actual: sources.len().saturating_add(1) as u64,
        });
    }

    let parent = request.output_path.parent().ok_or_else(|| {
        BackupError::invalid("backup output path does not have a parent directory")
    })?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
        BackupError::io(
            format!(
                "cannot inspect backup output directory {}",
                parent.display()
            ),
            error,
        )
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(BackupError::invalid(format!(
            "backup output parent must be a real directory: {}",
            parent.display()
        )));
    }
    if path_exists(&request.output_path)? {
        return Err(BackupError::invalid(format!(
            "backup destination already exists; choose a new filename: {}",
            request.output_path.display()
        )));
    }

    let temporary_path = parent.join(format!(
        ".{}.{}.partial.tqb",
        request
            .output_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("backup"),
        Uuid::now_v7()
    ));
    let mut temporary_guard = TemporaryFileGuard::new(temporary_path.clone());
    let (manifest, manifest_hash, payload_size) =
        create_archive_file(&temporary_path, request, &sources, limits)?;
    let verified = inspect_backup(&temporary_path, limits)?;
    if verified.manifest != manifest
        || verified.manifest_sha256_hex != manifest_hash
        || verified.payload_size_bytes != payload_size
    {
        return Err(BackupError::integrity(
            MANIFEST_PATH,
            "the completed archive did not match the data written by the creator",
        ));
    }
    if path_exists(&request.output_path)? {
        return Err(BackupError::invalid(format!(
            "backup destination appeared while the archive was being created: {}",
            request.output_path.display()
        )));
    }
    fs::rename(&temporary_path, &request.output_path).map_err(|error| {
        BackupError::io(
            format!(
                "cannot commit backup archive to {}",
                request.output_path.display()
            ),
            error,
        )
    })?;
    temporary_guard.disarm();
    Ok(BackupCreation {
        output_path: request.output_path.clone(),
        inspection: verified,
    })
}

pub fn inspect_backup(path: &Path, limits: &BackupLimits) -> BackupResult<BackupInspection> {
    let (mut archive, archive_size, archive_hash) = open_backup_archive(path, limits)?;
    let validated = validate_archive(&mut archive, archive_size, archive_hash.clone(), limits)?;
    verify_archive_unchanged(archive.into_inner(), archive_size, &archive_hash, limits)?;
    Ok(validated.inspection)
}

pub fn extract_backup(
    archive_path: &Path,
    staging_dir: &Path,
    limits: &BackupLimits,
) -> BackupResult<ExtractedBackup> {
    let (mut archive, archive_size, archive_hash) = open_backup_archive(archive_path, limits)?;
    let validated = validate_archive(&mut archive, archive_size, archive_hash.clone(), limits)?;
    let staging_root = ensure_empty_staging(staging_dir)?;

    let extraction = extract_validated_archive(&mut archive, &validated, &staging_root, limits);
    if let Err(error) = extraction {
        return Err(cleanup_after_failed_extraction(&staging_root, error));
    }
    if let Err(error) =
        verify_archive_unchanged(archive.into_inner(), archive_size, &archive_hash, limits)
    {
        return Err(cleanup_after_failed_extraction(&staging_root, error));
    }

    let database_path = archive_name_to_path(&staging_root, DATABASE_ARCHIVE_PATH);
    Ok(ExtractedBackup {
        staging_dir: staging_root,
        database_path,
        #[cfg(test)]
        resources_dir: {
            let path = staging_dir.join("resources");
            path.is_dir().then_some(path)
        },
        #[cfg(test)]
        templates_dir: {
            let path = staging_dir.join("templates");
            path.is_dir().then_some(path)
        },
        inspection: validated.inspection,
    })
}

fn validate_create_request(request: &CreateBackupRequest) -> BackupResult<()> {
    require_absolute(&request.output_path, "backup output")?;
    require_tqb_extension(&request.output_path)?;
    require_absolute(&request.database_snapshot_path, "database snapshot")?;
    if request.metadata.application_version.trim().is_empty() {
        return Err(BackupError::invalid("application version cannot be empty"));
    }
    Uuid::parse_str(&request.metadata.database_uuid)
        .map_err(|_| BackupError::invalid("database UUID is invalid"))?;
    Ok(())
}

fn collect_sources(
    request: &CreateBackupRequest,
    limits: &BackupLimits,
) -> BackupResult<Vec<SourceFile>> {
    let mut sources = vec![SourceFile {
        archive_path: DATABASE_ARCHIVE_PATH.to_owned(),
        role: BackupFileRole::Database,
        source_path: checked_regular_file(&request.database_snapshot_path, "database snapshot")?,
    }];
    if let Some(resources) = request.resources_dir.as_deref() {
        collect_directory(
            resources,
            "resources",
            BackupFileRole::Resource,
            limits,
            &mut sources,
        )?;
    }
    if let Some(templates) = request.templates_dir.as_deref() {
        collect_directory(
            templates,
            "templates",
            BackupFileRole::Template,
            limits,
            &mut sources,
        )?;
    }
    sources.sort_by(|left, right| left.archive_path.cmp(&right.archive_path));
    let mut names = sources
        .iter()
        .map(|source| source.archive_path.as_str())
        .collect::<Vec<_>>();
    names.push(MANIFEST_PATH);
    validate_no_path_collisions(names)?;
    Ok(sources)
}

fn checked_regular_file(path: &Path, purpose: &str) -> BackupResult<PathBuf> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        BackupError::io(
            format!("cannot inspect {purpose} {}", path.display()),
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BackupError::invalid(format!(
            "{purpose} must be a real regular file: {}",
            path.display()
        )));
    }
    fs::canonicalize(path).map_err(|error| {
        BackupError::io(
            format!("cannot canonicalize {purpose} {}", path.display()),
            error,
        )
    })
}

fn collect_directory(
    root: &Path,
    prefix: &str,
    role: BackupFileRole,
    limits: &BackupLimits,
    output: &mut Vec<SourceFile>,
) -> BackupResult<()> {
    require_absolute(root, prefix)?;
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        BackupError::io(
            format!("cannot inspect {prefix} directory {}", root.display()),
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BackupError::invalid(format!(
            "{prefix} source must be a real directory: {}",
            root.display()
        )));
    }
    let canonical_root = fs::canonicalize(root).map_err(|error| {
        BackupError::io(
            format!("cannot canonicalize {prefix} directory {}", root.display()),
            error,
        )
    })?;
    let mut stack = vec![canonical_root.clone()];
    let mut visited_nodes = 0usize;
    while let Some(current) = stack.pop() {
        let mut entries = fs::read_dir(&current)
            .map_err(|error| {
                BackupError::io(
                    format!("cannot read backup source {}", current.display()),
                    error,
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                BackupError::io(
                    format!("cannot enumerate backup source {}", current.display()),
                    error,
                )
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut child_directories = Vec::new();
        for entry in entries {
            let path = entry.path();
            let relative = path.strip_prefix(&canonical_root).map_err(|_| {
                BackupError::invalid(format!(
                    "backup source escaped its root: {}",
                    path.display()
                ))
            })?;
            if should_skip_transient_source(prefix, relative) {
                continue;
            }
            visited_nodes = visited_nodes.saturating_add(1);
            if visited_nodes > limits.max_entries {
                return Err(BackupError::LimitExceeded {
                    resource: format!("{prefix} source entries"),
                    limit: limits.max_entries as u64,
                    actual: visited_nodes as u64,
                });
            }
            let depth = relative.components().count().saturating_add(1);
            if depth > limits.max_path_depth {
                return Err(BackupError::rejected(format!(
                    "backup source is nested too deeply: {}",
                    path.display()
                )));
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                BackupError::io(
                    format!("cannot inspect backup source {}", path.display()),
                    error,
                )
            })?;
            if metadata.file_type().is_symlink() {
                return Err(BackupError::invalid(format!(
                    "backup sources cannot contain symbolic links: {}",
                    path.display()
                )));
            }
            if metadata.is_dir() {
                child_directories.push(path);
            } else if metadata.is_file() {
                if output.len().saturating_add(2) > limits.max_entries {
                    return Err(BackupError::LimitExceeded {
                        resource: "archive entries".to_owned(),
                        limit: limits.max_entries as u64,
                        actual: output.len().saturating_add(2) as u64,
                    });
                }
                let canonical = fs::canonicalize(&path).map_err(|error| {
                    BackupError::io(
                        format!("cannot canonicalize backup source {}", path.display()),
                        error,
                    )
                })?;
                if !canonical.starts_with(&canonical_root) {
                    return Err(BackupError::invalid(format!(
                        "backup source resolves outside its root: {}",
                        path.display()
                    )));
                }
                output.push(SourceFile {
                    archive_path: relative_archive_path(relative, prefix, limits)?,
                    role,
                    source_path: canonical,
                });
            } else {
                return Err(BackupError::invalid(format!(
                    "backup source contains a non-file entry: {}",
                    path.display()
                )));
            }
        }
        child_directories.reverse();
        stack.extend(child_directories);
    }
    Ok(())
}

fn should_skip_transient_source(prefix: &str, relative: &Path) -> bool {
    let Some(first) = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
    else {
        return false;
    };
    if prefix == "resources" && first.eq_ignore_ascii_case(".staging") {
        return true;
    }
    if prefix != "templates" || relative.components().count() != 1 {
        return false;
    }
    let lower = first.to_ascii_lowercase();
    (lower.starts_with(".zt-import-") && lower.ends_with(".partial"))
        || (lower.starts_with(".zt-delete-") && lower.ends_with(".tombstone"))
}

fn create_archive_file(
    temporary_path: &Path,
    request: &CreateBackupRequest,
    sources: &[SourceFile],
    limits: &BackupLimits,
) -> BackupResult<(BackupManifest, String, u64)> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary_path)
        .map_err(|error| {
            BackupError::io(
                format!(
                    "cannot create backup temporary file {}",
                    temporary_path.display()
                ),
                error,
            )
        })?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600);
    let mut manifest_files = Vec::with_capacity(sources.len());
    let mut total_payload = 0u64;
    let mut buffer = vec![0u8; 128 * 1024];

    for source in sources {
        writer.start_file(&source.archive_path, options)?;
        ensure_writer_size(&writer, limits)?;
        let source_metadata = fs::symlink_metadata(&source.source_path).map_err(|error| {
            BackupError::io(
                format!(
                    "cannot re-check backup source {}",
                    source.source_path.display()
                ),
                error,
            )
        })?;
        if source_metadata.file_type().is_symlink() || !source_metadata.is_file() {
            return Err(BackupError::invalid(format!(
                "backup source changed into a non-file: {}",
                source.source_path.display()
            )));
        }
        let source_file = File::open(&source.source_path).map_err(|error| {
            BackupError::io(
                format!("cannot open backup source {}", source.source_path.display()),
                error,
            )
        })?;
        let opened_metadata = source_file.metadata().map_err(|error| {
            BackupError::io(
                format!(
                    "cannot inspect opened backup source {}",
                    source.source_path.display()
                ),
                error,
            )
        })?;
        if !opened_metadata.is_file() {
            return Err(BackupError::invalid(format!(
                "opened backup source is not a regular file: {}",
                source.source_path.display()
            )));
        }
        let initial_size = opened_metadata.len();
        if initial_size > limits.max_entry_bytes {
            return Err(BackupError::LimitExceeded {
                resource: source.archive_path.clone(),
                limit: limits.max_entry_bytes,
                actual: initial_size,
            });
        }
        let projected_total = total_payload
            .checked_add(initial_size)
            .ok_or_else(|| BackupError::rejected("total backup payload size overflows"))?;
        if projected_total > limits.max_total_uncompressed_bytes {
            return Err(BackupError::LimitExceeded {
                resource: "total backup payload".to_owned(),
                limit: limits.max_total_uncompressed_bytes,
                actual: projected_total,
            });
        }
        let initial_modified = opened_metadata.modified().ok();
        let mut reader = BufReader::new(source_file);
        let mut hasher = Sha256::new();
        let mut written = 0u64;
        loop {
            let count = reader.read(&mut buffer).map_err(|error| {
                BackupError::io(
                    format!("cannot read backup source {}", source.source_path.display()),
                    error,
                )
            })?;
            if count == 0 {
                break;
            }
            written = written.saturating_add(count as u64);
            if written > limits.max_entry_bytes {
                return Err(BackupError::LimitExceeded {
                    resource: source.archive_path.clone(),
                    limit: limits.max_entry_bytes,
                    actual: written,
                });
            }
            total_payload = total_payload.saturating_add(count as u64);
            if total_payload > limits.max_total_uncompressed_bytes {
                return Err(BackupError::LimitExceeded {
                    resource: "total backup payload".to_owned(),
                    limit: limits.max_total_uncompressed_bytes,
                    actual: total_payload,
                });
            }
            hasher.update(&buffer[..count]);
            writer.write_all(&buffer[..count]).map_err(|error| {
                BackupError::io(
                    format!("cannot write {} into backup archive", source.archive_path),
                    error,
                )
            })?;
            ensure_writer_size(&writer, limits)?;
        }
        let final_metadata = reader.get_ref().metadata().map_err(|error| {
            BackupError::io(
                format!(
                    "cannot re-check opened backup source {}",
                    source.source_path.display()
                ),
                error,
            )
        })?;
        if written != initial_size
            || final_metadata.len() != initial_size
            || initial_modified
                .zip(final_metadata.modified().ok())
                .is_some_and(|(before, after)| before != after)
        {
            return Err(BackupError::integrity(
                &source.archive_path,
                "source file changed while the backup was being created",
            ));
        }
        manifest_files.push(BackupManifestFile {
            path: source.archive_path.clone(),
            role: source.role,
            size_bytes: written,
            sha256_hex: lower_hex(&hasher.finalize()),
        });
    }

    let manifest = BackupManifest {
        format: BACKUP_FORMAT.to_owned(),
        version: BACKUP_FORMAT_VERSION,
        created_at_unix_ms: now_millis(),
        application_version: request.metadata.application_version.trim().to_owned(),
        database_schema_version: request.metadata.database_schema_version,
        database_uuid: request.metadata.database_uuid.clone(),
        files: manifest_files,
    };
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    if manifest_bytes.len() as u64 > limits.max_manifest_bytes {
        return Err(BackupError::LimitExceeded {
            resource: MANIFEST_PATH.to_owned(),
            limit: limits.max_manifest_bytes,
            actual: manifest_bytes.len() as u64,
        });
    }
    let manifest_hash = lower_hex(&Sha256::digest(&manifest_bytes));
    writer.start_file(MANIFEST_PATH, options)?;
    ensure_writer_size(&writer, limits)?;
    writer
        .write_all(&manifest_bytes)
        .map_err(|error| BackupError::io("cannot write backup manifest", error))?;
    ensure_writer_size(&writer, limits)?;
    let mut file = writer.finish()?;
    file.flush()
        .map_err(|error| BackupError::io("cannot flush completed backup archive", error))?;
    file.sync_all()
        .map_err(|error| BackupError::io("cannot sync completed backup archive", error))?;
    let archive_size = file
        .metadata()
        .map_err(|error| BackupError::io("cannot inspect completed backup temporary file", error))?
        .len();
    if archive_size > limits.max_archive_bytes {
        return Err(BackupError::LimitExceeded {
            resource: "backup archive".to_owned(),
            limit: limits.max_archive_bytes,
            actual: archive_size,
        });
    }
    Ok((manifest, manifest_hash, total_payload))
}

fn ensure_writer_size(writer: &ZipWriter<File>, limits: &BackupLimits) -> BackupResult<()> {
    let file = writer
        .get_ref()
        .ok_or_else(|| BackupError::rejected("backup ZIP writer closed unexpectedly"))?;
    let actual = file
        .metadata()
        .map_err(|error| BackupError::io("cannot inspect backup archive while writing", error))?
        .len();
    if actual > limits.max_archive_bytes {
        return Err(BackupError::LimitExceeded {
            resource: "backup archive".to_owned(),
            limit: limits.max_archive_bytes,
            actual,
        });
    }
    Ok(())
}

fn open_backup_archive(
    path: &Path,
    limits: &BackupLimits,
) -> BackupResult<(ZipArchive<File>, u64, String)> {
    require_absolute(path, "backup archive")?;
    require_tqb_extension(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        BackupError::io(
            format!("cannot inspect backup archive {}", path.display()),
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BackupError::invalid(format!(
            "backup archive must be a real regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > limits.max_archive_bytes {
        return Err(BackupError::LimitExceeded {
            resource: "backup archive".to_owned(),
            limit: limits.max_archive_bytes,
            actual: metadata.len(),
        });
    }
    let mut file = File::open(path).map_err(|error| {
        BackupError::io(
            format!("cannot open backup archive {}", path.display()),
            error,
        )
    })?;
    let (actual_size, archive_hash) = hash_open_file(
        &mut file,
        limits.max_archive_bytes,
        &format!("backup archive {}", path.display()),
    )?;
    if actual_size != metadata.len() {
        return Err(BackupError::integrity(
            path.display().to_string(),
            "archive size changed while it was opened",
        ));
    }
    preflight_standard_zip(&mut file, actual_size, limits)?;
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        BackupError::io(
            format!("cannot rewind backup archive {}", path.display()),
            error,
        )
    })?;
    Ok((ZipArchive::new(file)?, actual_size, archive_hash))
}

/// Performs a bounded, ZIP32-only central-directory pass before `zip` is
/// allowed to allocate its internal index. This catches duplicate raw names,
/// ZIP64 sentinels and overlapping/local-header ranges that `ZipArchive` would
/// otherwise normalize or inspect only after allocation.
fn preflight_standard_zip(
    file: &mut File,
    archive_size: u64,
    limits: &BackupLimits,
) -> BackupResult<()> {
    const EOCD_SIGNATURE: u32 = 0x0605_4b50;
    const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
    const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
    const EOCD_MIN: usize = 22;
    const MAX_COMMENT: usize = u16::MAX as usize;

    if archive_size < EOCD_MIN as u64 {
        return Err(BackupError::rejected(
            "ZIP archive is shorter than an EOCD record",
        ));
    }
    let tail_len = usize::try_from(archive_size.min((EOCD_MIN + MAX_COMMENT) as u64))
        .map_err(|_| BackupError::rejected("ZIP tail length is not representable"))?;
    let tail_offset = archive_size - tail_len as u64;
    let tail = read_exact_at(file, tail_offset, tail_len, "ZIP end record")?;
    let eocd_index = (0..=tail_len - EOCD_MIN)
        .rev()
        .find(|index| {
            le_u32(&tail, *index) == EOCD_SIGNATURE
                && (*index)
                    .checked_add(EOCD_MIN)
                    .and_then(|value| value.checked_add(le_u16(&tail, *index + 20) as usize))
                    == Some(tail_len)
        })
        .ok_or_else(|| BackupError::rejected("ZIP end-of-central-directory record is missing"))?;
    let eocd_offset = tail_offset + eocd_index as u64;
    let disk_number = le_u16(&tail, eocd_index + 4);
    let central_disk = le_u16(&tail, eocd_index + 6);
    let entries_on_disk = le_u16(&tail, eocd_index + 8);
    let entry_count_u16 = le_u16(&tail, eocd_index + 10);
    let central_size_u32 = le_u32(&tail, eocd_index + 12);
    let central_offset_u32 = le_u32(&tail, eocd_index + 16);
    if disk_number != 0 || central_disk != 0 || entries_on_disk != entry_count_u16 {
        return Err(BackupError::rejected(
            "multi-disk ZIP archives are not supported",
        ));
    }
    if entry_count_u16 == u16::MAX || central_size_u32 == u32::MAX || central_offset_u32 == u32::MAX
    {
        return Err(BackupError::rejected(
            "ZIP64 archives are not supported by the W1 backup format",
        ));
    }
    let entry_count = usize::from(entry_count_u16);
    if entry_count == 0 {
        return Err(BackupError::rejected("backup ZIP contains no entries"));
    }
    if entry_count > limits.max_entries {
        return Err(BackupError::LimitExceeded {
            resource: "archive entries".to_owned(),
            limit: limits.max_entries as u64,
            actual: entry_count as u64,
        });
    }
    let central_size = u64::from(central_size_u32);
    let central_offset = u64::from(central_offset_u32);
    if central_size > limits.max_central_directory_bytes {
        return Err(BackupError::LimitExceeded {
            resource: "ZIP central directory".to_owned(),
            limit: limits.max_central_directory_bytes,
            actual: central_size,
        });
    }
    let central_end = central_offset
        .checked_add(central_size)
        .ok_or_else(|| BackupError::rejected("ZIP central-directory range overflows"))?;
    if central_end != eocd_offset {
        return Err(BackupError::rejected(
            "ZIP central directory is not contiguous with the end record",
        ));
    }
    let minimum_central = (entry_count as u64)
        .checked_mul(46)
        .ok_or_else(|| BackupError::rejected("ZIP entry count overflows"))?;
    if central_size < minimum_central {
        return Err(BackupError::rejected(
            "ZIP central directory is too small for its declared entry count",
        ));
    }

    file.seek(SeekFrom::Start(central_offset))
        .map_err(|error| BackupError::io("cannot seek to ZIP central directory", error))?;
    let mut central_cursor = central_offset;
    let mut raw_entries = Vec::with_capacity(entry_count);
    let mut decoded_names = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let mut fixed = [0u8; 46];
        file.read_exact(&mut fixed)
            .map_err(|error| BackupError::io("cannot read ZIP central-directory header", error))?;
        if le_u32(&fixed, 0) != CENTRAL_SIGNATURE {
            return Err(BackupError::rejected(
                "ZIP central-directory header signature is invalid",
            ));
        }
        let flags = le_u16(&fixed, 8);
        let compression_method = le_u16(&fixed, 10);
        let crc32 = le_u32(&fixed, 16);
        let compressed_u32 = le_u32(&fixed, 20);
        let uncompressed_u32 = le_u32(&fixed, 24);
        let name_len = usize::from(le_u16(&fixed, 28));
        let extra_len = usize::from(le_u16(&fixed, 30));
        let comment_len = usize::from(le_u16(&fixed, 32));
        let disk_start = le_u16(&fixed, 34);
        let local_offset_u32 = le_u32(&fixed, 42);
        // Version 1 archives are written by our seekable ZipWriter and only
        // use the UTF-8 filename bit. Reject every other general-purpose bit
        // before handing the archive to the ZIP library; this also covers
        // strong encryption, patched data and masked local headers.
        if flags & !0x0800 != 0 {
            return Err(BackupError::rejected(
                "ZIP entry uses unsupported encryption, descriptor or reserved flags",
            ));
        }
        if !matches!(compression_method, 0 | 8) {
            return Err(BackupError::rejected(format!(
                "unsupported ZIP compression method {compression_method}"
            )));
        }
        if disk_start != 0
            || compressed_u32 == u32::MAX
            || uncompressed_u32 == u32::MAX
            || local_offset_u32 == u32::MAX
        {
            return Err(BackupError::rejected(
                "ZIP64 or multi-disk entry metadata is not supported",
            ));
        }
        if name_len == 0 || name_len > limits.max_path_bytes {
            return Err(BackupError::rejected(
                "ZIP entry filename length exceeds the configured limit",
            ));
        }
        let variable_len = name_len
            .checked_add(extra_len)
            .and_then(|value| value.checked_add(comment_len))
            .ok_or_else(|| BackupError::rejected("ZIP central header length overflows"))?;
        let next_cursor = central_cursor
            .checked_add(46)
            .and_then(|value| value.checked_add(variable_len as u64))
            .ok_or_else(|| BackupError::rejected("ZIP central-directory cursor overflows"))?;
        if next_cursor > central_end {
            return Err(BackupError::rejected(
                "ZIP central-directory entry extends past its declared range",
            ));
        }
        let mut name_bytes = vec![0u8; name_len];
        file.read_exact(&mut name_bytes)
            .map_err(|error| BackupError::io("cannot read ZIP entry filename", error))?;
        let name = decode_strict_zip_name(&name_bytes, flags)?;
        validate_archive_name(&name, limits)?;
        if extra_len.saturating_add(comment_len) > 0 {
            file.seek(SeekFrom::Current(
                i64::try_from(extra_len.saturating_add(comment_len))
                    .map_err(|_| BackupError::rejected("ZIP variable header is too large"))?,
            ))
            .map_err(|error| BackupError::io("cannot skip ZIP central metadata", error))?;
        }
        decoded_names.push(name.clone());
        raw_entries.push(RawCentralEntry {
            name,
            name_bytes,
            flags,
            compression_method,
            crc32,
            compressed_size: u64::from(compressed_u32),
            uncompressed_size: u64::from(uncompressed_u32),
            local_header_offset: u64::from(local_offset_u32),
        });
        central_cursor = next_cursor;
    }
    if central_cursor != central_end {
        return Err(BackupError::rejected(
            "ZIP central directory contains unparsed trailing data",
        ));
    }
    validate_no_path_collisions(decoded_names.iter().map(String::as_str))?;

    let mut local_spans = Vec::with_capacity(raw_entries.len());
    for entry in raw_entries {
        let fixed = read_exact_at(file, entry.local_header_offset, 30, "ZIP local-file header")?;
        if le_u32(&fixed, 0) != LOCAL_SIGNATURE {
            return Err(BackupError::rejected(format!(
                "ZIP local header for {:?} has an invalid signature",
                entry.name
            )));
        }
        let local_flags = le_u16(&fixed, 6);
        let local_method = le_u16(&fixed, 8);
        let local_crc = le_u32(&fixed, 14);
        let local_compressed = le_u32(&fixed, 18);
        let local_uncompressed = le_u32(&fixed, 22);
        let local_name_len = usize::from(le_u16(&fixed, 26));
        let local_extra_len = usize::from(le_u16(&fixed, 28));
        if local_flags != entry.flags
            || local_method != entry.compression_method
            || local_crc != entry.crc32
            || u64::from(local_compressed) != entry.compressed_size
            || u64::from(local_uncompressed) != entry.uncompressed_size
            || local_name_len != entry.name_bytes.len()
        {
            return Err(BackupError::rejected(format!(
                "ZIP local and central metadata disagree for {:?}",
                entry.name
            )));
        }
        let name_offset = entry
            .local_header_offset
            .checked_add(30)
            .ok_or_else(|| BackupError::rejected("ZIP local name offset overflows"))?;
        let local_name = read_exact_at(file, name_offset, local_name_len, "ZIP local filename")?;
        if local_name != entry.name_bytes {
            return Err(BackupError::rejected(format!(
                "ZIP local filename differs from central filename {:?}",
                entry.name
            )));
        }
        let data_start = name_offset
            .checked_add(local_name_len as u64)
            .and_then(|value| value.checked_add(local_extra_len as u64))
            .ok_or_else(|| BackupError::rejected("ZIP local data offset overflows"))?;
        let data_end = data_start
            .checked_add(entry.compressed_size)
            .ok_or_else(|| BackupError::rejected("ZIP compressed data range overflows"))?;
        if data_end > central_offset {
            return Err(BackupError::rejected(format!(
                "ZIP data for {:?} overlaps the central directory",
                entry.name
            )));
        }
        local_spans.push((entry.local_header_offset, data_end, entry.name));
    }
    local_spans.sort_by_key(|span| span.0);
    let mut expected_start = 0u64;
    for (start, end, name) in local_spans {
        if start != expected_start || end < start {
            return Err(BackupError::rejected(format!(
                "ZIP local entry {:?} overlaps another entry or leaves hidden data",
                name
            )));
        }
        expected_start = end;
    }
    if expected_start != central_offset {
        return Err(BackupError::rejected(
            "ZIP contains hidden data between local entries and the central directory",
        ));
    }
    Ok(())
}

fn decode_strict_zip_name(bytes: &[u8], flags: u16) -> BackupResult<String> {
    if flags & 0x0800 == 0 && bytes.iter().any(|byte| !byte.is_ascii()) {
        return Err(BackupError::rejected(
            "non-ASCII ZIP filenames must use the UTF-8 flag",
        ));
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| BackupError::rejected("ZIP entry filename is not valid UTF-8"))
}

fn hash_open_file(file: &mut File, limit: u64, context: &str) -> BackupResult<(u64, String)> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| BackupError::io(format!("cannot rewind {context}"), error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| BackupError::io(format!("cannot hash {context}"), error))?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > limit {
            return Err(BackupError::LimitExceeded {
                resource: context.to_owned(),
                limit,
                actual: total,
            });
        }
        hasher.update(&buffer[..count]);
    }
    Ok((total, lower_hex(&hasher.finalize())))
}

fn verify_archive_unchanged(
    mut file: File,
    expected_size: u64,
    expected_hash: &str,
    limits: &BackupLimits,
) -> BackupResult<()> {
    let (actual_size, actual_hash) =
        hash_open_file(&mut file, limits.max_archive_bytes, "backup archive")?;
    if actual_size != expected_size || actual_hash != expected_hash {
        return Err(BackupError::integrity(
            "backup archive",
            "archive bytes changed while validation or extraction was running",
        ));
    }
    Ok(())
}

fn cleanup_after_failed_extraction(staging_root: &Path, original: BackupError) -> BackupError {
    match cleanup_staging(staging_root) {
        Ok(()) => original,
        Err(cleanup) => BackupError::rejected(format!(
            "{original}; restore staging cleanup also failed and the directory must be treated as dirty: {cleanup}"
        )),
    }
}

fn read_exact_at(
    file: &mut File,
    offset: u64,
    length: usize,
    context: &str,
) -> BackupResult<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| BackupError::io(format!("cannot seek to {context}"), error))?;
    let mut bytes = vec![0u8; length];
    file.read_exact(&mut bytes)
        .map_err(|error| BackupError::io(format!("cannot read {context}"), error))?;
    Ok(bytes)
}

fn le_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn validate_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    archive_size: u64,
    archive_hash: String,
    limits: &BackupLimits,
) -> BackupResult<ValidatedArchive> {
    if archive.len() > limits.max_entries {
        return Err(BackupError::LimitExceeded {
            resource: "archive entries".to_owned(),
            limit: limits.max_entries as u64,
            actual: archive.len() as u64,
        });
    }
    if archive.offset() != 0 {
        return Err(BackupError::rejected("data appears before the ZIP archive"));
    }
    let mut entries = BTreeMap::new();
    let mut names = Vec::with_capacity(archive.len());
    let mut total_uncompressed = 0u64;
    let mut payload_uncompressed = 0u64;
    for index in 0..archive.len() {
        let file = archive.by_index_raw(index)?;
        let name = file.name().to_owned();
        validate_archive_name(&name, limits)?;
        if file.is_dir() {
            return Err(BackupError::rejected(format!(
                "directory entries are not part of the .tqb format: {name:?}"
            )));
        }
        if file.encrypted() {
            return Err(BackupError::rejected(format!(
                "encrypted backup entries are not supported: {name:?}"
            )));
        }
        if file.is_symlink() {
            return Err(BackupError::rejected(format!(
                "symbolic links are not allowed in backups: {name:?}"
            )));
        }
        let size = file.size();
        let compressed_size = file.compressed_size();
        if size > limits.max_entry_bytes && name != MANIFEST_PATH {
            return Err(BackupError::LimitExceeded {
                resource: name,
                limit: limits.max_entry_bytes,
                actual: size,
            });
        }
        if name == MANIFEST_PATH && size > limits.max_manifest_bytes {
            return Err(BackupError::LimitExceeded {
                resource: MANIFEST_PATH.to_owned(),
                limit: limits.max_manifest_bytes,
                actual: size,
            });
        }
        if compressed_size > 0
            && size >= 1024 * 1024
            && size / compressed_size > limits.max_compression_ratio
        {
            return Err(BackupError::rejected(format!(
                "suspicious compression ratio for {name:?}"
            )));
        }
        total_uncompressed = total_uncompressed.saturating_add(size);
        if total_uncompressed
            > limits
                .max_total_uncompressed_bytes
                .saturating_add(limits.max_manifest_bytes)
        {
            return Err(BackupError::LimitExceeded {
                resource: "total uncompressed archive".to_owned(),
                limit: limits
                    .max_total_uncompressed_bytes
                    .saturating_add(limits.max_manifest_bytes),
                actual: total_uncompressed,
            });
        }
        if name != MANIFEST_PATH {
            payload_uncompressed = payload_uncompressed.saturating_add(size);
            if payload_uncompressed > limits.max_total_uncompressed_bytes {
                return Err(BackupError::LimitExceeded {
                    resource: "total backup payload".to_owned(),
                    limit: limits.max_total_uncompressed_bytes,
                    actual: payload_uncompressed,
                });
            }
        }
        drop(file);
        names.push(name.clone());
        if entries
            .insert(name.clone(), EntryMetadata { index, name, size })
            .is_some()
        {
            return Err(BackupError::rejected(
                "archive contains duplicate entry names",
            ));
        }
    }
    validate_no_path_collisions(names.iter().map(String::as_str))?;

    let manifest_entry = entries.get(MANIFEST_PATH).ok_or_else(|| {
        BackupError::rejected(format!("required entry {MANIFEST_PATH:?} is missing"))
    })?;
    let manifest_bytes = read_entry_bytes(
        archive,
        manifest_entry.index,
        limits.max_manifest_bytes,
        MANIFEST_PATH,
    )?;
    let manifest_hash = lower_hex(&Sha256::digest(&manifest_bytes));
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest(&manifest, &entries, limits)?;

    let manifest_by_path = manifest
        .files
        .iter()
        .map(|item| (item.path.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let mut payload_size = 0u64;
    for (name, entry) in &entries {
        if name == MANIFEST_PATH {
            continue;
        }
        let declared = manifest_by_path
            .get(name.as_str())
            .ok_or_else(|| BackupError::integrity(name, "entry is not declared by the manifest"))?;
        let (actual_size, actual_hash) =
            hash_entry(archive, entry.index, limits.max_entry_bytes, name)?;
        if actual_size != declared.size_bytes || actual_size != entry.size {
            return Err(BackupError::integrity(
                name,
                format!(
                    "size mismatch: manifest {}, ZIP {}, actual {}",
                    declared.size_bytes, entry.size, actual_size
                ),
            ));
        }
        if actual_hash != declared.sha256_hex {
            return Err(BackupError::integrity(name, "SHA-256 hash does not match"));
        }
        payload_size = payload_size.saturating_add(actual_size);
        if payload_size > limits.max_total_uncompressed_bytes {
            return Err(BackupError::LimitExceeded {
                resource: "total verified backup payload".to_owned(),
                limit: limits.max_total_uncompressed_bytes,
                actual: payload_size,
            });
        }
    }

    Ok(ValidatedArchive {
        inspection: BackupInspection {
            payload_file_count: manifest.files.len(),
            manifest,
            manifest_sha256_hex: manifest_hash,
            archive_sha256_hex: archive_hash,
            archive_size_bytes: archive_size,
            payload_size_bytes: payload_size,
        },
        entries,
    })
}

fn validate_manifest(
    manifest: &BackupManifest,
    entries: &BTreeMap<String, EntryMetadata>,
    limits: &BackupLimits,
) -> BackupResult<()> {
    if manifest.format != BACKUP_FORMAT {
        return Err(BackupError::rejected(format!(
            "manifest format is {:?}, expected {:?}",
            manifest.format, BACKUP_FORMAT
        )));
    }
    if manifest.version != BACKUP_FORMAT_VERSION {
        return Err(BackupError::UnsupportedVersion {
            found: manifest.version,
            supported: BACKUP_FORMAT_VERSION,
        });
    }
    if manifest.application_version.trim().is_empty() {
        return Err(BackupError::rejected(
            "manifest application version is empty",
        ));
    }
    Uuid::parse_str(&manifest.database_uuid)
        .map_err(|_| BackupError::rejected("manifest database UUID is invalid"))?;
    if manifest.files.len().saturating_add(1) != entries.len() {
        return Err(BackupError::rejected(
            "manifest file count does not match the ZIP payload",
        ));
    }
    let mut paths = BTreeSet::new();
    let mut database_count = 0usize;
    for item in &manifest.files {
        validate_archive_name(&item.path, limits)?;
        if item.path == MANIFEST_PATH {
            return Err(BackupError::rejected(
                "manifest cannot list itself as a payload file",
            ));
        }
        if !paths.insert(item.path.clone()) {
            return Err(BackupError::rejected(format!(
                "manifest declares duplicate path {:?}",
                item.path
            )));
        }
        let expected_role = classify_payload_path(&item.path)?;
        if expected_role != item.role {
            return Err(BackupError::rejected(format!(
                "manifest role does not match path {:?}",
                item.path
            )));
        }
        if item.role == BackupFileRole::Database {
            database_count += 1;
        }
        if item.size_bytes > limits.max_entry_bytes {
            return Err(BackupError::LimitExceeded {
                resource: item.path.clone(),
                limit: limits.max_entry_bytes,
                actual: item.size_bytes,
            });
        }
        validate_sha256_hex(&item.sha256_hex)
            .map_err(|reason| BackupError::integrity(&item.path, reason))?;
        let entry = entries.get(&item.path).ok_or_else(|| {
            BackupError::integrity(&item.path, "manifest entry is missing from ZIP archive")
        })?;
        if entry.size != item.size_bytes {
            return Err(BackupError::integrity(
                &item.path,
                "manifest size does not match ZIP metadata",
            ));
        }
    }
    if database_count != 1 || !paths.contains(DATABASE_ARCHIVE_PATH) {
        return Err(BackupError::rejected(format!(
            "manifest must contain exactly one database at {DATABASE_ARCHIVE_PATH:?}"
        )));
    }
    Ok(())
}

fn classify_payload_path(path: &str) -> BackupResult<BackupFileRole> {
    if path == DATABASE_ARCHIVE_PATH {
        Ok(BackupFileRole::Database)
    } else if path.starts_with("resources/") && path.len() > "resources/".len() {
        Ok(BackupFileRole::Resource)
    } else if path.starts_with("templates/") && path.len() > "templates/".len() {
        Ok(BackupFileRole::Template)
    } else {
        Err(BackupError::rejected(format!(
            "unknown backup payload path {path:?}"
        )))
    }
}

fn read_entry_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    limit: u64,
    name: &str,
) -> BackupResult<Vec<u8>> {
    let mut file = archive.by_index(index)?;
    let mut bytes = Vec::with_capacity(usize::try_from(file.size().min(limit)).unwrap_or(0));
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| BackupError::io(format!("cannot read archive entry {name:?}"), error))?;
    if bytes.len() as u64 > limit {
        return Err(BackupError::LimitExceeded {
            resource: name.to_owned(),
            limit,
            actual: bytes.len() as u64,
        });
    }
    Ok(bytes)
}

fn hash_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    limit: u64,
    name: &str,
) -> BackupResult<(u64, String)> {
    let mut file = archive.by_index(index)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            BackupError::io(format!("cannot verify archive entry {name:?}"), error)
        })?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > limit {
            return Err(BackupError::LimitExceeded {
                resource: name.to_owned(),
                limit,
                actual: total,
            });
        }
        hasher.update(&buffer[..count]);
    }
    Ok((total, lower_hex(&hasher.finalize())))
}

fn extract_validated_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    validated: &ValidatedArchive,
    root: &Path,
    limits: &BackupLimits,
) -> BackupResult<()> {
    let manifest_entry = validated
        .entries
        .get(MANIFEST_PATH)
        .ok_or_else(|| BackupError::rejected("validated archive unexpectedly lost its manifest"))?;
    extract_entry(
        archive,
        manifest_entry,
        root,
        limits.max_manifest_bytes,
        Some(&validated.inspection.manifest_sha256_hex),
    )?;

    let declared = validated
        .inspection
        .manifest
        .files
        .iter()
        .map(|item| (item.path.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    for (path, item) in declared {
        let entry = validated
            .entries
            .get(path)
            .ok_or_else(|| BackupError::integrity(path, "validated payload entry disappeared"))?;
        extract_entry(
            archive,
            entry,
            root,
            limits.max_entry_bytes,
            Some(&item.sha256_hex),
        )?;
    }
    Ok(())
}

fn extract_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    entry: &EntryMetadata,
    root: &Path,
    limit: u64,
    expected_hash: Option<&str>,
) -> BackupResult<()> {
    let destination = archive_name_to_path(root, &entry.name);
    ensure_safe_parent(root, &destination)?;
    if path_exists(&destination)? {
        return Err(BackupError::rejected(format!(
            "restore destination already exists: {}",
            destination.display()
        )));
    }
    let mut source = archive.by_index(entry.index)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| {
            BackupError::io(
                format!("cannot create restored file {}", destination.display()),
                error,
            )
        })?;
    let canonical_destination = fs::canonicalize(&destination).map_err(|error| {
        BackupError::io(
            format!(
                "cannot verify newly created restored file {}",
                destination.display()
            ),
            error,
        )
    })?;
    let created_metadata = fs::symlink_metadata(&destination).map_err(|error| {
        BackupError::io(
            format!(
                "cannot inspect newly created restored file {}",
                destination.display()
            ),
            error,
        )
    })?;
    if !canonical_destination.starts_with(root)
        || created_metadata.file_type().is_symlink()
        || !created_metadata.is_file()
    {
        return Err(BackupError::rejected(format!(
            "restored file resolved outside the staging directory: {}",
            destination.display()
        )));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|error| BackupError::io(format!("cannot extract {:?}", entry.name), error))?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > limit {
            return Err(BackupError::LimitExceeded {
                resource: entry.name.clone(),
                limit,
                actual: total,
            });
        }
        hasher.update(&buffer[..count]);
        output.write_all(&buffer[..count]).map_err(|error| {
            BackupError::io(
                format!("cannot write restored file {}", destination.display()),
                error,
            )
        })?;
    }
    if total != entry.size {
        return Err(BackupError::integrity(
            &entry.name,
            "extracted size changed after archive validation",
        ));
    }
    let actual_hash = lower_hex(&hasher.finalize());
    if expected_hash.is_some_and(|expected| expected != actual_hash) {
        return Err(BackupError::integrity(
            &entry.name,
            "extracted SHA-256 changed after archive validation",
        ));
    }
    output.flush().map_err(|error| {
        BackupError::io(
            format!("cannot flush restored file {}", destination.display()),
            error,
        )
    })?;
    output.sync_all().map_err(|error| {
        BackupError::io(
            format!("cannot sync restored file {}", destination.display()),
            error,
        )
    })?;
    Ok(())
}

fn validate_sha256_hex(value: &str) -> Result<(), &'static str> {
    if value.len() != 64 {
        return Err("SHA-256 must contain exactly 64 hexadecimal characters");
    }
    if value
        .bytes()
        .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err("SHA-256 must be lowercase hexadecimal");
    }
    Ok(())
}

fn path_exists(path: &Path) -> BackupResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(BackupError::io(
            format!("cannot inspect path {}", path.display()),
            error,
        )),
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct TestWorkspace(PathBuf);

    impl TestWorkspace {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("zhitiku-backup-test-{}", Uuid::now_v7()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self, value: &str) -> PathBuf {
            self.0.join(value)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            if self
                .0
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with("zhitiku-backup-test-"))
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    fn request(workspace: &TestWorkspace) -> CreateBackupRequest {
        let database = workspace.path("snapshot.sqlite3");
        let resources = workspace.path("source-resources");
        let templates = workspace.path("source-templates");
        fs::write(&database, b"sqlite snapshot").unwrap();
        fs::create_dir(&resources).unwrap();
        fs::create_dir(&templates).unwrap();
        fs::create_dir(resources.join("images")).unwrap();
        fs::create_dir(resources.join(".staging")).unwrap();
        fs::write(resources.join("images/a.png"), b"image bytes").unwrap();
        fs::write(resources.join(".staging/incomplete.bin"), b"temporary").unwrap();
        fs::write(templates.join("template.docx"), b"docx bytes").unwrap();
        fs::write(
            templates.join(".zt-import-018f4c1a-1234-7abc-8def-0123456789ab.partial"),
            b"temporary",
        )
        .unwrap();
        fs::write(
            templates.join(".zt-delete-018f4c1a-1234-7abc-8def-0123456789ab.tombstone"),
            b"temporary",
        )
        .unwrap();
        CreateBackupRequest {
            output_path: workspace.path("complete.tqb"),
            database_snapshot_path: database,
            resources_dir: Some(resources),
            templates_dir: Some(templates),
            metadata: super::super::BackupMetadata {
                application_version: "0.1.0".to_owned(),
                database_schema_version: 8,
                database_uuid: Uuid::now_v7().to_string(),
            },
        }
    }

    #[test]
    fn creates_inspects_and_extracts_a_complete_backup() {
        let workspace = TestWorkspace::new();
        let request = request(&workspace);
        let created = create_backup(&request, &BackupLimits::default()).unwrap();
        assert_eq!(created.inspection.payload_file_count, 3);

        let inspected = inspect_backup(&request.output_path, &BackupLimits::default()).unwrap();
        assert_eq!(inspected.manifest.database_schema_version, 8);
        assert_eq!(inspected.payload_file_count, 3);

        let staging = workspace.path("staging");
        fs::create_dir(&staging).unwrap();
        let extracted =
            extract_backup(&request.output_path, &staging, &BackupLimits::default()).unwrap();
        assert_eq!(
            fs::read(extracted.database_path).unwrap(),
            b"sqlite snapshot"
        );
        assert_eq!(
            fs::read(extracted.resources_dir.unwrap().join("images/a.png")).unwrap(),
            b"image bytes"
        );
        assert_eq!(
            fs::read(extracted.templates_dir.unwrap().join("template.docx")).unwrap(),
            b"docx bytes"
        );
    }

    #[test]
    fn rejects_path_traversal_and_duplicate_names() {
        let workspace = TestWorkspace::new();
        let traversal = workspace.path("traversal.tqb");
        write_raw_zip(&traversal, &["../escape", MANIFEST_PATH], b"{}");
        assert!(inspect_backup(&traversal, &BackupLimits::default()).is_err());

        let duplicate = workspace.path("duplicate.tqb");
        write_duplicate_name_zip(&duplicate);
        assert!(inspect_backup(&duplicate, &BackupLimits::default()).is_err());
    }

    #[test]
    fn rejects_a_manifest_with_a_tampered_hash() {
        let workspace = TestWorkspace::new();
        let path = workspace.path("tampered.tqb");
        let manifest = BackupManifest {
            format: BACKUP_FORMAT.to_owned(),
            version: BACKUP_FORMAT_VERSION,
            created_at_unix_ms: 1,
            application_version: "0.1.0".to_owned(),
            database_schema_version: 8,
            database_uuid: Uuid::now_v7().to_string(),
            files: vec![BackupManifestFile {
                path: DATABASE_ARCHIVE_PATH.to_owned(),
                role: BackupFileRole::Database,
                size_bytes: 4,
                sha256_hex: "0".repeat(64),
            }],
        };
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let file = File::create(&path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        writer.start_file(DATABASE_ARCHIVE_PATH, options).unwrap();
        writer.write_all(b"data").unwrap();
        writer.start_file(MANIFEST_PATH, options).unwrap();
        writer.write_all(&manifest_bytes).unwrap();
        writer.finish().unwrap();

        assert!(matches!(
            inspect_backup(&path, &BackupLimits::default()),
            Err(BackupError::Integrity { .. })
        ));
    }

    #[test]
    fn rejects_unsupported_general_purpose_flags_before_zip_indexing() {
        let workspace = TestWorkspace::new();
        let request = request(&workspace);
        create_backup(&request, &BackupLimits::default()).unwrap();
        let mut bytes = fs::read(&request.output_path).unwrap();
        let local = bytes
            .windows(4)
            .position(|window| window == [0x50, 0x4b, 0x03, 0x04])
            .unwrap();
        let central = bytes
            .windows(4)
            .position(|window| window == [0x50, 0x4b, 0x01, 0x02])
            .unwrap();
        bytes[local + 6..local + 8].copy_from_slice(&0x0040u16.to_le_bytes());
        bytes[central + 8..central + 10].copy_from_slice(&0x0040u16.to_le_bytes());
        fs::write(&request.output_path, bytes).unwrap();

        assert!(matches!(
            inspect_backup(&request.output_path, &BackupLimits::default()),
            Err(BackupError::Rejected { .. })
        ));
    }

    #[test]
    fn enforces_per_entry_creation_limit_and_removes_partial_file() {
        let workspace = TestWorkspace::new();
        let request = request(&workspace);
        let limits = BackupLimits {
            max_entry_bytes: 4,
            ..BackupLimits::default()
        };
        assert!(matches!(
            create_backup(&request, &limits),
            Err(BackupError::LimitExceeded { .. })
        ));
        assert!(!request.output_path.exists());
        assert!(
            fs::read_dir(&workspace.0)
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().contains(".partial"))
        );
    }

    fn write_raw_zip(path: &Path, names: &[&str], manifest: &[u8]) {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        for name in names {
            writer.start_file(*name, options).unwrap();
            if *name == MANIFEST_PATH {
                writer.write_all(manifest).unwrap();
            } else {
                writer.write_all(b"data").unwrap();
            }
        }
        let bytes = writer.finish().unwrap().into_inner();
        fs::write(path, bytes).unwrap();
    }

    fn write_duplicate_name_zip(path: &Path) {
        const SECOND_NAME: &str = "database/zhitiku.sqlite4";
        debug_assert_eq!(SECOND_NAME.len(), DATABASE_ARCHIVE_PATH.len());
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        for name in [DATABASE_ARCHIVE_PATH, SECOND_NAME, MANIFEST_PATH] {
            writer.start_file(name, options).unwrap();
            writer.write_all(b"data").unwrap();
        }
        let mut bytes = writer.finish().unwrap().into_inner();
        let needle = SECOND_NAME.as_bytes();
        let replacement = DATABASE_ARCHIVE_PATH.as_bytes();
        let offsets = bytes
            .windows(needle.len())
            .enumerate()
            .filter_map(|(index, window)| (window == needle).then_some(index))
            .collect::<Vec<_>>();
        assert_eq!(
            offsets.len(),
            2,
            "local and central names must both be patched"
        );
        for offset in offsets {
            bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
        }
        fs::write(path, bytes).unwrap();
    }
}
