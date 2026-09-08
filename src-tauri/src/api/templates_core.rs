//! Filesystem-only core for managed Word templates.
//!
//! This module intentionally does not write SQLite. A command layer should
//! insert the returned metadata in one transaction and call
//! [`delete_managed_template_file`] to roll back the managed copy if that
//! transaction fails.

use std::{
    error::Error,
    ffi::OsStr,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::docx::{
    Diagnostic, DocxError, DocxLimits, PackageKind, TemplateAnchor, TemplatePackageAnalysis,
    analyze_template_package_with_requirements, read_mathtype_from_docx,
};

pub(crate) const TEMPLATE_ANALYSIS_SCHEMA_VERSION: u32 = 1;
pub(crate) const TEMPLATE_PARSER_VERSION: &str = "w1-opc-1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedTemplateFileMetadata {
    /// Random backend-generated single-component file name.
    pub managed_file_name: String,
    /// Path relative to `DatabasePaths::templates_dir()`.
    pub file_rel_path: String,
    pub file_sha256: [u8; 32],
    pub file_sha256_hex: String,
    pub file_byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedTemplateImportResult {
    pub imported: bool,
    /// Original base name only; the external absolute path is never retained.
    pub original_file_name: String,
    pub package_kind: PackageKind,
    pub archive_bytes: u64,
    pub part_count: usize,
    pub total_uncompressed_bytes: u64,
    pub file: Option<ManagedTemplateFileMetadata>,
    pub anchors: Vec<TemplateAnchor>,
    pub diagnostics: Vec<Diagnostic>,
    pub analysis_schema_version: u32,
    pub parser_version: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedTemplateConfigurationResult {
    pub package_kind: PackageKind,
    pub archive_bytes: u64,
    pub part_count: usize,
    pub total_uncompressed_bytes: u64,
    pub file: ManagedTemplateFileMetadata,
    pub anchors: Vec<TemplateAnchor>,
    pub diagnostics: Vec<Diagnostic>,
    pub analysis_schema_version: u32,
    pub parser_version: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedTemplateTombstone {
    original_file_name: String,
    tombstone_file_name: String,
}

#[derive(Debug)]
pub(crate) enum ManagedTemplateError {
    InvalidSource {
        code: &'static str,
        message: String,
    },
    UnsafeManagedFileName(String),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Docx(DocxError),
}

impl ManagedTemplateError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::InvalidSource { code, .. } => code,
            Self::UnsafeManagedFileName(_) => "TEMPLATE_MANAGED_NAME_UNSAFE",
            Self::Io { .. } => "TEMPLATE_FILESYSTEM_ERROR",
            Self::Docx(_) => "TEMPLATE_DOCX_ERROR",
        }
    }

    fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for ManagedTemplateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSource { message, .. } => formatter.write_str(message),
            Self::UnsafeManagedFileName(name) => {
                write!(formatter, "unsafe managed template file name: {name}")
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "template filesystem operation '{operation}' failed for {}: {source}",
                path.display()
            ),
            Self::Docx(error) => write!(formatter, "template DOCX analysis failed: {error}"),
        }
    }
}

impl Error for ManagedTemplateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Docx(error) => Some(error),
            Self::InvalidSource { .. } | Self::UnsafeManagedFileName(_) => None,
        }
    }
}

impl From<DocxError> for ManagedTemplateError {
    fn from(error: DocxError) -> Self {
        Self::Docx(error)
    }
}

/// Analyzes and imports exactly the same bounded bytes. Invalid or ambiguous
/// templates return `Ok(imported = false)` with diagnostics and never create a
/// managed file.
pub(crate) fn import_managed_template(
    source_path: &Path,
    templates_dir: &Path,
    required_anchor_names: &[&str],
    limits: &DocxLimits,
) -> Result<ManagedTemplateImportResult, ManagedTemplateError> {
    let source_extension = source_extension(source_path)?;
    let canonical_source = fs::canonicalize(source_path)
        .map_err(|error| ManagedTemplateError::io("canonicalize source", source_path, error))?;
    let source_metadata = fs::metadata(&canonical_source).map_err(|error| {
        ManagedTemplateError::io("read source metadata", &canonical_source, error)
    })?;
    if !source_metadata.is_file() {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_SOURCE_NOT_FILE",
            message: "selected template path is not a regular file".to_owned(),
        });
    }
    let original_file_name = canonical_source
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_SOURCE_NAME_INVALID",
            message: "template file name is not valid UTF-8".to_owned(),
        })?
        .to_owned();

    if source_metadata.len() > limits.max_archive_bytes {
        return Ok(rejected_import(
            original_file_name,
            TemplatePackageAnalysis {
                archive_bytes: source_metadata.len(),
                diagnostics: vec![Diagnostic::error(
                    "DOCX_ARCHIVE_TOO_LARGE",
                    None::<String>,
                    format!(
                        "package is {} bytes; the configured limit is {} bytes",
                        source_metadata.len(),
                        limits.max_archive_bytes
                    ),
                )],
                ..TemplatePackageAnalysis::default()
            },
        ));
    }

    let bytes = read_bounded_source(&canonical_source, source_metadata.len(), limits)?;
    let mathtype_formula_count = if limits.allow_mathtype_ole {
        read_mathtype_from_docx(Cursor::new(&bytes), limits)
            .map_err(|error| ManagedTemplateError::InvalidSource {
                code: "TEMPLATE_EMBEDDED_OBJECT_UNSUPPORTED",
                message: format!(
                    "模板包含无法安全处理的嵌入对象。模板导入只支持能够完整解析的 MathType 公式：{error}"
                ),
            })?
            .len()
    } else {
        0
    };
    let mut analysis = analyze_template_package_with_requirements(
        Cursor::new(&bytes),
        required_anchor_names,
        limits,
    )?;
    if mathtype_formula_count > 0 {
        analysis.diagnostics.push(Diagnostic::warning(
            "TEMPLATE_MATHTYPE_REQUIRES_CONFIGURATION",
            Some("word/document.xml"),
            format!(
                "检测到 {mathtype_formula_count} 个 MathType 公式。请在“调整区域”中完整选择试题范围；保存正式模板时会删除这些 OLE 对象。"
            ),
        ));
    }
    let expected_kind = match source_extension {
        "docx" => PackageKind::Document,
        "dotx" => PackageKind::Template,
        _ => unreachable!("source_extension only returns supported extensions"),
    };
    if analysis.package_kind != PackageKind::Unknown && analysis.package_kind != expected_kind {
        analysis.diagnostics.push(Diagnostic::error(
            "DOCX_TEMPLATE_EXTENSION_MISMATCH",
            None::<String>,
            format!(
                "file extension '.{source_extension}' does not match package kind {:?}",
                analysis.package_kind
            ),
        ));
    }
    if !analysis.is_acceptable() {
        return Ok(rejected_import(original_file_name, analysis));
    }

    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let digest_hex = bytes_to_lower_hex(&digest);
    let canonical_templates_dir = prepare_templates_dir(templates_dir)?;
    let managed_file_name = format!("{}.{}", uuid::Uuid::now_v7(), source_extension);
    debug_assert!(is_managed_template_file_name(&managed_file_name));
    let final_path = canonical_templates_dir.join(&managed_file_name);
    let temporary_path = canonical_templates_dir.join(format!(
        ".{}.{}.importing",
        managed_file_name,
        uuid::Uuid::now_v7()
    ));

    if path_entry_exists(&final_path).map_err(|error| {
        ManagedTemplateError::io("check managed destination", &final_path, error)
    })? {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_MANAGED_NAME_COLLISION",
            message: "generated managed template name already exists".to_owned(),
        });
    }

    let write_result = write_managed_file(&temporary_path, &final_path, &bytes);
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result?;

    Ok(ManagedTemplateImportResult {
        imported: true,
        original_file_name,
        package_kind: analysis.package_kind,
        archive_bytes: analysis.archive_bytes,
        part_count: analysis.part_count,
        total_uncompressed_bytes: analysis.total_uncompressed_bytes,
        file: Some(ManagedTemplateFileMetadata {
            managed_file_name: managed_file_name.clone(),
            file_rel_path: managed_file_name,
            file_sha256: digest,
            file_sha256_hex: digest_hex,
            file_byte_size: bytes.len() as u64,
        }),
        anchors: analysis.anchors,
        diagnostics: analysis.diagnostics,
        analysis_schema_version: TEMPLATE_ANALYSIS_SCHEMA_VERSION,
        parser_version: TEMPLATE_PARSER_VERSION,
    })
}

/// Stores a newly configured package under a fresh managed name. The existing
/// catalogue file remains untouched until the command layer commits the new
/// metadata, so database failure can be rolled back by deleting this file.
pub(crate) fn store_configured_managed_template(
    bytes: &[u8],
    templates_dir: &Path,
    limits: &DocxLimits,
) -> Result<ManagedTemplateConfigurationResult, ManagedTemplateError> {
    let analysis =
        analyze_template_package_with_requirements(Cursor::new(bytes), &["ZT_QUESTIONS"], limits)?;
    if !analysis.is_acceptable() || analysis.package_kind != PackageKind::Document {
        return Err(ManagedTemplateError::Docx(DocxError::Rejected {
            diagnostics: analysis.diagnostics,
        }));
    }
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let digest_hex = bytes_to_lower_hex(&digest);
    let canonical_templates_dir = prepare_templates_dir(templates_dir)?;
    let managed_file_name = format!("{}.docx", uuid::Uuid::now_v7());
    let final_path = canonical_templates_dir.join(&managed_file_name);
    let temporary_path = canonical_templates_dir.join(format!(
        ".{}.{}.configuring",
        managed_file_name,
        uuid::Uuid::now_v7()
    ));
    let write_result = write_managed_file(&temporary_path, &final_path, bytes);
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result?;

    Ok(ManagedTemplateConfigurationResult {
        package_kind: analysis.package_kind,
        archive_bytes: analysis.archive_bytes,
        part_count: analysis.part_count,
        total_uncompressed_bytes: analysis.total_uncompressed_bytes,
        file: ManagedTemplateFileMetadata {
            managed_file_name: managed_file_name.clone(),
            file_rel_path: managed_file_name,
            file_sha256: digest,
            file_sha256_hex: digest_hex,
            file_byte_size: bytes.len() as u64,
        },
        anchors: analysis.anchors,
        diagnostics: analysis.diagnostics,
        analysis_schema_version: TEMPLATE_ANALYSIS_SCHEMA_VERSION,
        parser_version: TEMPLATE_PARSER_VERSION,
    })
}

/// Deletes only a single backend-generated file directly inside the canonical
/// managed templates directory. Client-provided paths are never accepted.
pub(crate) fn delete_managed_template_file(
    templates_dir: &Path,
    managed_file_name: &str,
) -> Result<bool, ManagedTemplateError> {
    let Some(tombstone) = stage_managed_template_delete(templates_dir, managed_file_name)? else {
        return Ok(false);
    };
    finalize_managed_template_delete(templates_dir, &tombstone)?;
    Ok(true)
}

/// Reads a catalogue-owned template only from the private templates directory
/// and verifies all immutable database metadata before returning bytes.
/// Friendly names and external absolute paths are deliberately not accepted.
pub(crate) fn read_verified_managed_template(
    templates_dir: &Path,
    managed_file_name: &str,
    expected_byte_size: u64,
    expected_sha256: &[u8; 32],
    limits: &DocxLimits,
) -> Result<Vec<u8>, ManagedTemplateError> {
    if !is_managed_template_file_name(managed_file_name) {
        return Err(ManagedTemplateError::UnsafeManagedFileName(
            managed_file_name.to_owned(),
        ));
    }
    if expected_byte_size > limits.max_archive_bytes {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_MANAGED_SIZE_INVALID",
            message: "managed template size exceeds the DOCX safety limit".to_owned(),
        });
    }

    let canonical_dir = canonical_templates_dir(templates_dir)?;
    let candidate = canonical_dir.join(managed_file_name);
    let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
        ManagedTemplateError::io("read managed template metadata", &candidate, error)
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_MANAGED_FILE_UNSAFE",
            message: "managed template is not a regular non-symlink file".to_owned(),
        });
    }
    if metadata.len() != expected_byte_size {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_MANAGED_SIZE_MISMATCH",
            message: format!(
                "managed template size changed: expected {expected_byte_size}, actual {}",
                metadata.len()
            ),
        });
    }

    let canonical_file = fs::canonicalize(&candidate).map_err(|error| {
        ManagedTemplateError::io("canonicalize managed template", &candidate, error)
    })?;
    if canonical_file.parent() != Some(canonical_dir.as_path()) {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_MANAGED_PATH_ESCAPE",
            message: "managed template resolved outside the private templates directory".to_owned(),
        });
    }
    let bytes = read_bounded_source(&canonical_file, expected_byte_size, limits)?;
    if bytes.len() as u64 != expected_byte_size {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_MANAGED_SIZE_CHANGED_DURING_READ",
            message: "managed template size changed while it was being read".to_owned(),
        });
    }
    let actual_sha256: [u8; 32] = Sha256::digest(&bytes).into();
    if &actual_sha256 != expected_sha256 {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_MANAGED_HASH_MISMATCH",
            message: "managed template content no longer matches its catalogue hash".to_owned(),
        });
    }
    Ok(bytes)
}

/// Atomically hides a managed template before its SQLite row is deleted. If
/// the database transaction fails, call [`restore_managed_template_delete`].
pub(crate) fn stage_managed_template_delete(
    templates_dir: &Path,
    managed_file_name: &str,
) -> Result<Option<ManagedTemplateTombstone>, ManagedTemplateError> {
    if !is_managed_template_file_name(managed_file_name) {
        return Err(ManagedTemplateError::UnsafeManagedFileName(
            managed_file_name.to_owned(),
        ));
    }
    let canonical_templates_dir = canonical_templates_dir(templates_dir)?;
    let candidate = canonical_templates_dir.join(managed_file_name);
    let metadata = match fs::symlink_metadata(&candidate) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ManagedTemplateError::io(
                "read managed template metadata",
                candidate,
                error,
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ManagedTemplateError::UnsafeManagedFileName(
            managed_file_name.to_owned(),
        ));
    }
    let tombstone_file_name = format!(".zt-delete-{}.tombstone", uuid::Uuid::now_v7());
    let tombstone_path = canonical_templates_dir.join(&tombstone_file_name);
    if path_entry_exists(&tombstone_path).map_err(|error| {
        ManagedTemplateError::io("check template tombstone", &tombstone_path, error)
    })? {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_TOMBSTONE_COLLISION",
            message: "generated template tombstone already exists".to_owned(),
        });
    }
    fs::rename(&candidate, &tombstone_path).map_err(|error| {
        ManagedTemplateError::io("stage managed template deletion", &candidate, error)
    })?;
    Ok(Some(ManagedTemplateTombstone {
        original_file_name: managed_file_name.to_owned(),
        tombstone_file_name,
    }))
}

pub(crate) fn restore_managed_template_delete(
    templates_dir: &Path,
    tombstone: &ManagedTemplateTombstone,
) -> Result<(), ManagedTemplateError> {
    validate_tombstone(tombstone)?;
    let canonical_templates_dir = canonical_templates_dir(templates_dir)?;
    let tombstone_path = canonical_templates_dir.join(&tombstone.tombstone_file_name);
    let original_path = canonical_templates_dir.join(&tombstone.original_file_name);
    if path_entry_exists(&original_path).map_err(|error| {
        ManagedTemplateError::io("check template restore destination", &original_path, error)
    })? {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_RESTORE_DESTINATION_EXISTS",
            message: "managed template destination already exists during restore".to_owned(),
        });
    }
    fs::rename(&tombstone_path, &original_path).map_err(|error| {
        ManagedTemplateError::io("restore managed template deletion", tombstone_path, error)
    })?;
    Ok(())
}

pub(crate) fn finalize_managed_template_delete(
    templates_dir: &Path,
    tombstone: &ManagedTemplateTombstone,
) -> Result<(), ManagedTemplateError> {
    validate_tombstone(tombstone)?;
    let canonical_templates_dir = canonical_templates_dir(templates_dir)?;
    let tombstone_path = canonical_templates_dir.join(&tombstone.tombstone_file_name);
    let metadata = fs::symlink_metadata(&tombstone_path).map_err(|error| {
        ManagedTemplateError::io("read template tombstone metadata", &tombstone_path, error)
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ManagedTemplateError::UnsafeManagedFileName(
            tombstone.tombstone_file_name.clone(),
        ));
    }
    fs::remove_file(&tombstone_path).map_err(|error| {
        ManagedTemplateError::io("finalize managed template deletion", tombstone_path, error)
    })?;
    Ok(())
}

pub(crate) fn is_managed_template_file_name(name: &str) -> bool {
    let path = Path::new(name);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return false;
    }
    let Some(extension) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    if extension != "docx" && extension != "dotx" {
        return false;
    }
    let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
        return false;
    };
    is_lowercase_uuid_v7(stem)
}

fn source_extension(path: &Path) -> Result<&str, ManagedTemplateError> {
    let extension = path.extension().and_then(OsStr::to_str).ok_or_else(|| {
        ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_SOURCE_TYPE_UNSUPPORTED",
            message: "template must use a .docx or .dotx extension".to_owned(),
        }
    })?;
    if extension.eq_ignore_ascii_case("docx") {
        Ok("docx")
    } else if extension.eq_ignore_ascii_case("dotx") {
        Ok("dotx")
    } else {
        Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_SOURCE_TYPE_UNSUPPORTED",
            message: "template must use a .docx or .dotx extension".to_owned(),
        })
    }
}

fn validate_tombstone(tombstone: &ManagedTemplateTombstone) -> Result<(), ManagedTemplateError> {
    if !is_managed_template_file_name(&tombstone.original_file_name)
        || !is_tombstone_file_name(&tombstone.tombstone_file_name)
    {
        return Err(ManagedTemplateError::UnsafeManagedFileName(
            tombstone.tombstone_file_name.clone(),
        ));
    }
    Ok(())
}

fn is_tombstone_file_name(name: &str) -> bool {
    let Some(stem) = name
        .strip_prefix(".zt-delete-")
        .and_then(|value| value.strip_suffix(".tombstone"))
    else {
        return false;
    };
    is_lowercase_uuid_v7(stem)
}

fn read_bounded_source(
    path: &Path,
    declared_size: u64,
    limits: &DocxLimits,
) -> Result<Vec<u8>, ManagedTemplateError> {
    let file = File::open(path)
        .map_err(|error| ManagedTemplateError::io("open source template", path, error))?;
    let capacity =
        usize::try_from(declared_size).map_err(|_| ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_SOURCE_TOO_LARGE",
            message: "template size cannot be represented on this platform".to_owned(),
        })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limits.max_archive_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| ManagedTemplateError::io("read source template", path, error))?;
    if bytes.len() as u64 > limits.max_archive_bytes {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_SOURCE_CHANGED_OR_TOO_LARGE",
            message: "template grew beyond the configured limit while it was being read".to_owned(),
        });
    }
    Ok(bytes)
}

fn prepare_templates_dir(path: &Path) -> Result<PathBuf, ManagedTemplateError> {
    fs::create_dir_all(path)
        .map_err(|error| ManagedTemplateError::io("create templates directory", path, error))?;
    canonical_templates_dir(path)
}

fn canonical_templates_dir(path: &Path) -> Result<PathBuf, ManagedTemplateError> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        ManagedTemplateError::io("canonicalize templates directory", path, error)
    })?;
    if !canonical.is_dir() {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_DIRECTORY_INVALID",
            message: "managed templates path is not a directory".to_owned(),
        });
    }
    Ok(canonical)
}

fn write_managed_file(
    temporary_path: &Path,
    final_path: &Path,
    bytes: &[u8],
) -> Result<(), ManagedTemplateError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary_path)
        .map_err(|error| {
            ManagedTemplateError::io(
                "create managed template temporary file",
                temporary_path,
                error,
            )
        })?;
    file.write_all(bytes).map_err(|error| {
        ManagedTemplateError::io(
            "write managed template temporary file",
            temporary_path,
            error,
        )
    })?;
    file.flush().map_err(|error| {
        ManagedTemplateError::io(
            "flush managed template temporary file",
            temporary_path,
            error,
        )
    })?;
    file.sync_all().map_err(|error| {
        ManagedTemplateError::io(
            "sync managed template temporary file",
            temporary_path,
            error,
        )
    })?;
    drop(file);

    if path_entry_exists(final_path).map_err(|error| {
        ManagedTemplateError::io("check managed template destination", final_path, error)
    })? {
        return Err(ManagedTemplateError::InvalidSource {
            code: "TEMPLATE_MANAGED_NAME_COLLISION",
            message: "managed template destination appeared during import".to_owned(),
        });
    }
    fs::rename(temporary_path, final_path).map_err(|error| {
        ManagedTemplateError::io("commit managed template file", final_path, error)
    })?;
    Ok(())
}

fn rejected_import(
    original_file_name: String,
    analysis: TemplatePackageAnalysis,
) -> ManagedTemplateImportResult {
    ManagedTemplateImportResult {
        imported: false,
        original_file_name,
        package_kind: analysis.package_kind,
        archive_bytes: analysis.archive_bytes,
        part_count: analysis.part_count,
        total_uncompressed_bytes: analysis.total_uncompressed_bytes,
        file: None,
        anchors: analysis.anchors,
        diagnostics: analysis.diagnostics,
        analysis_schema_version: TEMPLATE_ANALYSIS_SCHEMA_VERSION,
        parser_version: TEMPLATE_PARSER_VERSION,
    }
}

fn path_entry_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn bytes_to_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

fn is_lowercase_uuid_v7(stem: &str) -> bool {
    let bytes = stem.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().copied().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte) {
            return false;
        }
    }
    bytes[14] == b'7' && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}

#[cfg(test)]
mod tests {
    use std::{env, io::Write};

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    const DOCUMENT: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>{{ZT_QUESTIONS}}</w:t></w:r></w:p></w:body></w:document>"#;
    const CONTENT_TYPES: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;
    const ROOT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

    struct TestWorkspace(PathBuf);

    impl TestWorkspace {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zhitiku-managed-template-test-{}",
                uuid::Uuid::now_v7()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            if self
                .0
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with("zhitiku-managed-template-test-"))
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    fn minimal_docx() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in [
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", DOCUMENT.as_bytes()),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn mathtype_limits() -> DocxLimits {
        DocxLimits {
            allow_mathtype_ole: true,
            ..DocxLimits::default()
        }
    }

    fn unsupported_ole_docx() -> Vec<u8> {
        const OLE_DOCUMENT: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:object><o:OLEObject Type="Embed" ProgID="Package" r:id="rId2"/></w:object></w:r></w:p></w:body></w:document>"#;
        const OLE_CONTENT_TYPES: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Default Extension="bin" ContentType="application/vnd.openxmlformats-officedocument.oleObject"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;
        const DOCUMENT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject" Target="embeddings/oleObject1.bin"/>
</Relationships>"#;

        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in [
            ("[Content_Types].xml", OLE_CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", OLE_DOCUMENT.as_bytes()),
            ("word/_rels/document.xml.rels", DOCUMENT_RELS.as_bytes()),
            ("word/embeddings/oleObject1.bin", b"not-a-mathtype-object"),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn imports_the_analyzed_bytes_under_a_random_internal_name() {
        let workspace = TestWorkspace::new();
        let source = workspace.0.join("user-visible-name.docx");
        let templates = workspace.0.join("templates");
        let bytes = minimal_docx();
        fs::write(&source, &bytes).unwrap();

        let result = import_managed_template(
            &source,
            &templates,
            &["ZT_QUESTIONS"],
            &DocxLimits::default(),
        )
        .unwrap();

        assert!(result.imported, "{:?}", result.diagnostics);
        assert_eq!(result.original_file_name, "user-visible-name.docx");
        assert_eq!(result.anchors.len(), 1);
        let metadata = result.file.unwrap();
        assert!(is_managed_template_file_name(&metadata.managed_file_name));
        assert_eq!(metadata.file_rel_path, metadata.managed_file_name);
        assert_eq!(metadata.file_byte_size, bytes.len() as u64);
        assert_eq!(metadata.file_sha256_hex.len(), 64);
        assert_eq!(
            fs::read(templates.join(metadata.managed_file_name)).unwrap(),
            bytes
        );
    }

    #[test]
    fn imports_the_supplied_mathtype_exam_for_region_configuration_when_available() {
        let Ok(path) = env::var("ZHITIKU_MATHTYPE_DOCX") else {
            return;
        };
        let workspace = TestWorkspace::new();
        let templates = workspace.0.join("templates");
        let source_bytes = fs::read(&path).expect("supplied MathType fixture should be readable");

        let result = import_managed_template(Path::new(&path), &templates, &[], &mathtype_limits())
            .expect("fully parseable MathType should be accepted as a staging source");

        assert!(result.imported, "{:?}", result.diagnostics);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.code == "TEMPLATE_MATHTYPE_REQUIRES_CONFIGURATION")
        );
        let metadata = result.file.expect("accepted source should be copied");
        assert_eq!(
            fs::read(templates.join(metadata.managed_file_name)).unwrap(),
            source_bytes
        );
    }

    #[test]
    fn rejects_non_mathtype_ole_even_when_mathtype_staging_is_enabled() {
        let workspace = TestWorkspace::new();
        let source = workspace.0.join("unsupported-ole.docx");
        let templates = workspace.0.join("templates");
        fs::write(&source, unsupported_ole_docx()).unwrap();

        let error = import_managed_template(&source, &templates, &[], &mathtype_limits())
            .expect_err("arbitrary embedded packages must remain blocked");

        assert_eq!(error.code(), "TEMPLATE_EMBEDDED_OBJECT_UNSUPPORTED");
        assert!(!templates.exists());
    }

    #[test]
    fn configured_package_is_stored_under_a_fresh_verified_name() {
        let workspace = TestWorkspace::new();
        let templates = workspace.0.join("templates");
        let bytes = minimal_docx();

        let result =
            store_configured_managed_template(&bytes, &templates, &DocxLimits::default()).unwrap();

        assert_eq!(result.package_kind, PackageKind::Document);
        assert_eq!(result.anchors.len(), 1);
        assert_eq!(result.anchors[0].name, "ZT_QUESTIONS");
        assert!(is_managed_template_file_name(
            &result.file.managed_file_name
        ));
        assert_eq!(
            read_verified_managed_template(
                &templates,
                &result.file.managed_file_name,
                result.file.file_byte_size,
                &result.file.file_sha256,
                &DocxLimits::default(),
            )
            .unwrap(),
            bytes
        );
    }

    #[test]
    fn rejected_template_creates_no_managed_file() {
        let workspace = TestWorkspace::new();
        let source = workspace.0.join("missing-anchor.docx");
        let templates = workspace.0.join("templates");
        fs::write(&source, minimal_docx()).unwrap();

        let result =
            import_managed_template(&source, &templates, &["ZT_ANSWERS"], &DocxLimits::default())
                .unwrap();

        assert!(!result.imported);
        assert!(result.file.is_none());
        assert!(!templates.exists());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.code == "DOCX_TEMPLATE_ANCHOR_MISSING")
        );
    }

    #[test]
    fn deletion_accepts_only_generated_single_component_names() {
        let workspace = TestWorkspace::new();
        let templates = workspace.0.join("templates");
        fs::create_dir(&templates).unwrap();
        let name = format!("{}.docx", uuid::Uuid::now_v7());
        fs::write(templates.join(&name), b"managed").unwrap();

        assert!(delete_managed_template_file(&templates, &name).unwrap());
        assert!(!delete_managed_template_file(&templates, &name).unwrap());
        assert!(delete_managed_template_file(&templates, "../outside.docx").is_err());
        assert!(delete_managed_template_file(&templates, "friendly-name.docx").is_err());
    }

    #[test]
    fn staged_deletion_can_be_restored_or_finalized() {
        let workspace = TestWorkspace::new();
        let templates = workspace.0.join("templates");
        fs::create_dir(&templates).unwrap();
        let name = format!("{}.docx", uuid::Uuid::now_v7());
        fs::write(templates.join(&name), b"managed").unwrap();

        let tombstone = stage_managed_template_delete(&templates, &name)
            .unwrap()
            .expect("file should be staged");
        assert!(!templates.join(&name).exists());
        restore_managed_template_delete(&templates, &tombstone).unwrap();
        assert_eq!(fs::read(templates.join(&name)).unwrap(), b"managed");

        let tombstone = stage_managed_template_delete(&templates, &name)
            .unwrap()
            .expect("restored file should be staged again");
        finalize_managed_template_delete(&templates, &tombstone).unwrap();
        assert!(!templates.join(&name).exists());
    }

    #[test]
    fn verified_reader_rejects_changed_content_and_unsafe_names() {
        let workspace = TestWorkspace::new();
        let templates = workspace.0.join("templates");
        fs::create_dir(&templates).unwrap();
        let name = format!("{}.docx", uuid::Uuid::now_v7());
        let bytes = minimal_docx();
        let expected_hash: [u8; 32] = Sha256::digest(&bytes).into();
        fs::write(templates.join(&name), &bytes).unwrap();

        assert_eq!(
            read_verified_managed_template(
                &templates,
                &name,
                bytes.len() as u64,
                &expected_hash,
                &DocxLimits::default(),
            )
            .unwrap(),
            bytes
        );
        fs::write(templates.join(&name), vec![b'x'; bytes.len()]).unwrap();
        let error = read_verified_managed_template(
            &templates,
            &name,
            bytes.len() as u64,
            &expected_hash,
            &DocxLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "TEMPLATE_MANAGED_HASH_MISMATCH");
        assert!(
            read_verified_managed_template(
                &templates,
                "../outside.docx",
                0,
                &[0; 32],
                &DocxLimits::default(),
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn verified_reader_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let workspace = TestWorkspace::new();
        let templates = workspace.0.join("templates");
        fs::create_dir(&templates).unwrap();
        let outside = workspace.0.join("outside.docx");
        let bytes = minimal_docx();
        fs::write(&outside, &bytes).unwrap();
        let name = format!("{}.docx", uuid::Uuid::now_v7());
        symlink(&outside, templates.join(&name)).unwrap();
        let hash: [u8; 32] = Sha256::digest(&bytes).into();

        let error = read_verified_managed_template(
            &templates,
            &name,
            bytes.len() as u64,
            &hash,
            &DocxLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "TEMPLATE_MANAGED_FILE_UNSAFE");
    }
}
