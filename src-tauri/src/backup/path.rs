use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use unicode_normalization::UnicodeNormalization;

use super::{BackupError, BackupLimits, BackupResult};

pub(crate) fn require_absolute(path: &Path, purpose: &str) -> BackupResult<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(BackupError::invalid(format!(
            "{purpose} path must be absolute: {}",
            path.display()
        )))
    }
}

pub(crate) fn require_tqb_extension(path: &Path) -> BackupResult<()> {
    let valid = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("tqb"));
    if valid {
        Ok(())
    } else {
        Err(BackupError::invalid(format!(
            "backup filename must use the .tqb extension: {}",
            path.display()
        )))
    }
}

pub(crate) fn validate_archive_name(name: &str, limits: &BackupLimits) -> BackupResult<()> {
    if name.is_empty() || name.len() > limits.max_path_bytes {
        return Err(BackupError::rejected(format!(
            "archive entry path length is invalid: {name:?}"
        )));
    }
    if name.starts_with('/') || name.starts_with('\\') || name.contains('\\') {
        return Err(BackupError::rejected(format!(
            "archive entry uses an absolute or ambiguous path: {name:?}"
        )));
    }
    let lower = name.to_ascii_lowercase();
    if lower.contains("%2f") || lower.contains("%5c") || lower.contains("%00") {
        return Err(BackupError::rejected(format!(
            "archive entry contains an encoded separator or NUL: {name:?}"
        )));
    }

    let segments = name.split('/').collect::<Vec<_>>();
    if segments.len() > limits.max_path_depth {
        return Err(BackupError::rejected(format!(
            "archive entry is nested too deeply: {name:?}"
        )));
    }
    for segment in segments {
        validate_segment(segment, name)?;
    }
    Ok(())
}

fn validate_segment(segment: &str, full_name: &str) -> BackupResult<()> {
    if segment.is_empty() || segment == "." || segment == ".." {
        return Err(BackupError::rejected(format!(
            "archive entry contains an empty, '.' or '..' path segment: {full_name:?}"
        )));
    }
    if segment.len() > 255
        || segment.ends_with('.')
        || segment.ends_with(' ')
        || segment.chars().any(|value| {
            value == '\0'
                || value.is_control()
                || matches!(value, '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
    {
        return Err(BackupError::rejected(format!(
            "archive entry contains a Windows-unsafe path segment: {full_name:?}"
        )));
    }

    let stem = segment.split('.').next().unwrap_or(segment);
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper
            .strip_prefix("COM")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=9).contains(&value))
        || upper
            .strip_prefix("LPT")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=9).contains(&value));
    if reserved {
        return Err(BackupError::rejected(format!(
            "archive entry uses a reserved Windows device name: {full_name:?}"
        )));
    }
    Ok(())
}

pub(crate) fn collision_key(name: &str) -> String {
    name.nfkc().flat_map(char::to_lowercase).collect()
}

pub(crate) fn validate_no_path_collisions<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> BackupResult<()> {
    let mut keys = BTreeSet::new();
    for name in names {
        let key = collision_key(name);
        if !keys.insert(key.clone()) {
            return Err(BackupError::rejected(format!(
                "archive contains duplicate or case/Unicode-ambiguous path {name:?}"
            )));
        }
        let mut prefix = key.as_str();
        while let Some((parent, _)) = prefix.rsplit_once('/') {
            if keys.contains(parent) {
                return Err(BackupError::rejected(format!(
                    "archive path {name:?} conflicts with a file at parent path {parent:?}"
                )));
            }
            prefix = parent;
        }
        if let Some(existing) = keys
            .range(format!("{key}/")..)
            .next()
            .filter(|candidate| candidate.starts_with(&format!("{key}/")))
        {
            return Err(BackupError::rejected(format!(
                "archive path {name:?} conflicts with child path {existing:?}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn relative_archive_path(
    relative: &Path,
    prefix: &str,
    limits: &BackupLimits,
) -> BackupResult<String> {
    let mut segments = vec![prefix.to_owned()];
    for component in relative.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    BackupError::invalid(format!(
                        "source filename is not valid UTF-8: {}",
                        relative.display()
                    ))
                })?;
                segments.push(value.to_owned());
            }
            _ => {
                return Err(BackupError::invalid(format!(
                    "source relative path contains an unsafe component: {}",
                    relative.display()
                )));
            }
        }
    }
    let name = segments.join("/");
    validate_archive_name(&name, limits)?;
    Ok(name)
}

pub(crate) fn archive_name_to_path(root: &Path, name: &str) -> PathBuf {
    let mut result = root.to_path_buf();
    for segment in name.split('/') {
        result.push(segment);
    }
    result
}

pub(crate) fn ensure_empty_staging(path: &Path) -> BackupResult<PathBuf> {
    require_absolute(path, "restore staging directory")?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        BackupError::io(
            format!(
                "cannot inspect restore staging directory {}",
                path.display()
            ),
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BackupError::invalid(format!(
            "restore staging path must be an existing real directory, not a link: {}",
            path.display()
        )));
    }
    let mut entries = fs::read_dir(path).map_err(|error| {
        BackupError::io(
            format!("cannot read restore staging directory {}", path.display()),
            error,
        )
    })?;
    if entries
        .next()
        .transpose()
        .map_err(|error| {
            BackupError::io(
                format!("cannot read restore staging directory {}", path.display()),
                error,
            )
        })?
        .is_some()
    {
        return Err(BackupError::invalid(format!(
            "restore staging directory must be empty: {}",
            path.display()
        )));
    }
    fs::canonicalize(path).map_err(|error| {
        BackupError::io(
            format!(
                "cannot canonicalize restore staging directory {}",
                path.display()
            ),
            error,
        )
    })
}

pub(crate) fn ensure_safe_parent(root: &Path, destination: &Path) -> BackupResult<()> {
    let root_metadata = fs::symlink_metadata(root).map_err(|error| {
        BackupError::io(
            format!("cannot inspect restore staging root {}", root.display()),
            error,
        )
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(BackupError::rejected(format!(
            "restore staging root changed into a link or non-directory: {}",
            root.display()
        )));
    }
    let current_root = fs::canonicalize(root).map_err(|error| {
        BackupError::io(
            format!("cannot re-check restore staging root {}", root.display()),
            error,
        )
    })?;
    if current_root != root {
        return Err(BackupError::rejected(format!(
            "restore staging root changed while extracting: {}",
            root.display()
        )));
    }
    let parent = destination.parent().ok_or_else(|| {
        BackupError::invalid(format!(
            "restore destination has no parent: {}",
            destination.display()
        ))
    })?;
    let relative = parent.strip_prefix(root).map_err(|_| {
        BackupError::rejected(format!(
            "restore destination escaped the staging directory: {}",
            destination.display()
        ))
    })?;

    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(BackupError::rejected(format!(
                "restore destination contains an unsafe component: {}",
                destination.display()
            )));
        };
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(BackupError::rejected(format!(
                        "restore parent is not a real directory: {}",
                        current.display()
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let current_parent = current.parent().ok_or_else(|| {
                    BackupError::rejected("restore directory has no verified parent")
                })?;
                let canonical_parent = fs::canonicalize(current_parent).map_err(|error| {
                    BackupError::io(
                        format!(
                            "cannot re-check restore parent before creating {}",
                            current.display()
                        ),
                        error,
                    )
                })?;
                if !canonical_parent.starts_with(root) {
                    return Err(BackupError::rejected(format!(
                        "restore parent resolves outside the staging root: {}",
                        current_parent.display()
                    )));
                }
                fs::create_dir(&current).map_err(|error| {
                    BackupError::io(
                        format!("cannot create restore directory {}", current.display()),
                        error,
                    )
                })?;
            }
            Err(error) => {
                return Err(BackupError::io(
                    format!("cannot inspect restore directory {}", current.display()),
                    error,
                ));
            }
        }
        let canonical = fs::canonicalize(&current).map_err(|error| {
            BackupError::io(
                format!(
                    "cannot canonicalize restore directory {}",
                    current.display()
                ),
                error,
            )
        })?;
        if !canonical.starts_with(root) {
            return Err(BackupError::rejected(format!(
                "restore directory resolves outside the staging root: {}",
                current.display()
            )));
        }
    }
    Ok(())
}

pub(crate) fn cleanup_staging(path: &Path) -> BackupResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        BackupError::io(
            format!(
                "cannot inspect restore staging directory {}",
                path.display()
            ),
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BackupError::rejected(format!(
            "refusing to clean a staging path that became a link or non-directory: {}",
            path.display()
        )));
    }
    let canonical = fs::canonicalize(path).map_err(|error| {
        BackupError::io(
            format!(
                "cannot re-check restore staging directory {}",
                path.display()
            ),
            error,
        )
    })?;
    if canonical != path {
        return Err(BackupError::rejected(format!(
            "refusing to clean a staging directory that changed identity: {}",
            path.display()
        )));
    }
    let entries = fs::read_dir(path).map_err(|error| {
        BackupError::io(
            format!("cannot clean restore staging directory {}", path.display()),
            error,
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            BackupError::io(
                format!(
                    "cannot enumerate restore staging directory {}",
                    path.display()
                ),
                error,
            )
        })?;
        let item = entry.path();
        let metadata = fs::symlink_metadata(&item).map_err(|error| {
            BackupError::io(
                format!("cannot inspect {} while cleaning", item.display()),
                error,
            )
        })?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(&item).map_err(|error| {
                BackupError::io(
                    format!("cannot remove {} while cleaning", item.display()),
                    error,
                )
            })?;
        } else {
            fs::remove_file(&item).map_err(|error| {
                BackupError::io(
                    format!("cannot remove {} while cleaning", item.display()),
                    error,
                )
            })?;
        }
    }
    Ok(())
}
