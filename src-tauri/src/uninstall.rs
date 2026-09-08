use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const UNINSTALL_DATA_ROOT_RECORD_FILENAME: &str = "uninstall-data-root.json";

const BUNDLE_ID: &str = "com.zhitiku.desktop";
const DATA_ROOT_MARKER_FILENAME: &str = ".tktiku-managed-data-v1";
const RECORD_SCHEMA_VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 16 * 1024;
const MANAGED_DIRECTORIES: [&str; 7] = [
    "database",
    "resources",
    "templates",
    "backup",
    "export",
    ".restore",
    ".data-move",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DataRootMarker {
    schema_version: u32,
    bundle_id: String,
    ownership_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UninstallDataRootRecord {
    schema_version: u32,
    bundle_id: String,
    ownership_id: String,
    data_root: String,
}

/// Register only a successfully initialized, app-owned data root. The NSIS
/// uninstaller copies this record before Tauri clears LocalAppData and passes
/// it to a temporary copy of the Rust executable after the app has stopped.
pub fn register_managed_data_root(app_local_root: &Path, data_root: &Path) -> Result<(), String> {
    let data_root = canonical_real_directory(data_root, "题库数据目录")?;
    validate_initialized_data_root(&data_root)?;
    let marker = load_or_create_data_root_marker(&data_root)?;

    fs::create_dir_all(app_local_root).map_err(|error| format!("无法创建卸载记录目录：{error}"))?;
    canonical_real_directory(app_local_root, "卸载记录目录")?;
    let record = UninstallDataRootRecord {
        schema_version: RECORD_SCHEMA_VERSION,
        bundle_id: BUNDLE_ID.to_owned(),
        ownership_id: marker.ownership_id,
        data_root: data_root
            .to_str()
            .ok_or_else(|| "题库数据目录包含无法安全记录的字符。".to_owned())?
            .to_owned(),
    };
    let bytes = serde_json::to_vec(&record)
        .map_err(|error| format!("无法生成卸载数据目录记录：{error}"))?;
    replace_synced_file(
        &app_local_root.join(UNINSTALL_DATA_ROOT_RECORD_FILENAME),
        &bytes,
    )
}

/// Delete only directories owned by this application. Unknown files directly
/// under the selected root are retained, and any symlink/reparse point aborts
/// the entire cleanup before the first managed directory is removed.
pub fn cleanup_managed_data_from_record(record_path: &Path) -> Result<(), String> {
    let record_bytes = read_guarded_file(record_path, MAX_RECORD_BYTES, "卸载数据目录记录")?;
    let record: UninstallDataRootRecord = serde_json::from_slice(&record_bytes)
        .map_err(|error| format!("卸载数据目录记录格式无效：{error}"))?;
    validate_record(&record)?;

    let data_root = canonical_real_directory(Path::new(&record.data_root), "待删除题库数据目录")?;
    let marker_path = data_root.join(DATA_ROOT_MARKER_FILENAME);
    let marker_bytes = read_guarded_file(&marker_path, MAX_RECORD_BYTES, "题库数据目录标记")?;
    let marker: DataRootMarker = serde_json::from_slice(&marker_bytes)
        .map_err(|error| format!("题库数据目录标记格式无效：{error}"))?;
    validate_marker(&marker)?;
    if marker.ownership_id != record.ownership_id {
        return Err("卸载记录与题库数据目录标记不匹配，已拒绝删除。".to_owned());
    }

    let mut existing_targets = Vec::new();
    for name in MANAGED_DIRECTORIES {
        let target = data_root.join(name);
        match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if link_or_reparse(&metadata) || !metadata.is_dir() {
                    return Err(format!("受管数据路径“{name}”不是安全的普通目录。"));
                }
                validate_tree_without_links(&target, &data_root)?;
                existing_targets.push(target);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("无法检查受管数据路径“{name}”：{error}")),
        }
    }

    for target in existing_targets {
        fs::remove_dir_all(&target)
            .map_err(|error| format!("无法删除受管数据目录“{}”：{error}", target.display()))?;
    }
    fs::remove_file(&marker_path).map_err(|error| format!("无法删除题库数据目录标记：{error}"))?;
    match fs::remove_dir(&data_root) {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            // Preserve the root when the user placed unrelated files in it.
        }
        Err(error) => return Err(format!("无法收尾题库数据目录：{error}")),
    }
    Ok(())
}

fn validate_initialized_data_root(root: &Path) -> Result<(), String> {
    for name in ["database", "resources", "templates", "backup", "export"] {
        let directory = root.join(name);
        let metadata = fs::symlink_metadata(&directory)
            .map_err(|error| format!("题库数据目录缺少受管子目录“{name}”：{error}"))?;
        if link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(format!("题库受管子目录“{name}”不是安全的普通目录。"));
        }
        let canonical = fs::canonicalize(&directory)
            .map_err(|error| format!("无法解析题库受管子目录“{name}”：{error}"))?;
        if canonical.parent() != Some(root) {
            return Err(format!("题库受管子目录“{name}”超出数据根目录。"));
        }
    }
    let database = root.join("database").join("zhitiku.sqlite3");
    let metadata = fs::symlink_metadata(&database)
        .map_err(|error| format!("题库数据目录缺少数据库文件：{error}"))?;
    if link_or_reparse(&metadata) || !metadata.is_file() || metadata.len() == 0 {
        return Err("题库数据库文件不是安全且非空的普通文件。".to_owned());
    }
    Ok(())
}

fn load_or_create_data_root_marker(root: &Path) -> Result<DataRootMarker, String> {
    let path = root.join(DATA_ROOT_MARKER_FILENAME);
    if path.exists() {
        let bytes = read_guarded_file(&path, MAX_RECORD_BYTES, "题库数据目录标记")?;
        let marker: DataRootMarker = serde_json::from_slice(&bytes)
            .map_err(|error| format!("题库数据目录标记格式无效：{error}"))?;
        validate_marker(&marker)?;
        return Ok(marker);
    }
    let marker = DataRootMarker {
        schema_version: RECORD_SCHEMA_VERSION,
        bundle_id: BUNDLE_ID.to_owned(),
        ownership_id: Uuid::now_v7().to_string(),
    };
    let bytes = serde_json::to_vec(&marker)
        .map_err(|error| format!("无法生成题库数据目录标记：{error}"))?;
    write_new_synced_file(&path, &bytes)?;
    Ok(marker)
}

fn validate_record(record: &UninstallDataRootRecord) -> Result<(), String> {
    if record.schema_version != RECORD_SCHEMA_VERSION
        || record.bundle_id != BUNDLE_ID
        || Uuid::parse_str(&record.ownership_id).is_err()
        || !Path::new(&record.data_root).is_absolute()
    {
        return Err("卸载数据目录记录身份或路径无效。".to_owned());
    }
    Ok(())
}

fn validate_marker(marker: &DataRootMarker) -> Result<(), String> {
    if marker.schema_version != RECORD_SCHEMA_VERSION
        || marker.bundle_id != BUNDLE_ID
        || Uuid::parse_str(&marker.ownership_id).is_err()
    {
        return Err("题库数据目录标记身份无效。".to_owned());
    }
    Ok(())
}

fn validate_tree_without_links(path: &Path, root: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("无法检查受管数据路径“{}”：{error}", path.display()))?;
    if link_or_reparse(&metadata) {
        return Err(format!(
            "受管数据路径包含符号链接或重解析点：{}",
            path.display()
        ));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("无法解析受管数据路径“{}”：{error}", path.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!("受管数据路径超出题库数据目录：{}", path.display()));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)
            .map_err(|error| format!("无法枚举受管数据目录“{}”：{error}", path.display()))?
        {
            let entry = entry.map_err(|error| format!("无法读取受管数据条目：{error}"))?;
            validate_tree_without_links(&entry.path(), root)?;
        }
    } else if !metadata.is_file() {
        return Err(format!(
            "受管数据路径包含不支持的文件类型：{}",
            path.display()
        ));
    }
    Ok(())
}

fn canonical_real_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("无法检查{label}：{error}"))?;
    if link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(format!("{label}必须是安全的普通目录。"));
    }
    let canonical = fs::canonicalize(path).map_err(|error| format!("无法解析{label}：{error}"))?;
    if canonical.parent().is_none() {
        return Err(format!("{label}不能是磁盘或文件系统根目录。"));
    }
    let canonical_metadata =
        fs::symlink_metadata(&canonical).map_err(|error| format!("无法复检{label}：{error}"))?;
    if link_or_reparse(&canonical_metadata) || !canonical_metadata.is_dir() {
        return Err(format!("{label}解析后不是安全的普通目录。"));
    }
    Ok(canonical)
}

fn read_guarded_file(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("无法读取{label}：{error}"))?;
    if link_or_reparse(&metadata) || !metadata.is_file() || metadata.len() == 0 {
        return Err(format!("{label}必须是安全且非空的普通文件。"));
    }
    if metadata.len() > max_bytes {
        return Err(format!("{label}超过安全大小限制。"));
    }
    fs::read(path).map_err(|error| format!("无法读取{label}：{error}"))
}

fn write_new_synced_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("无法创建文件“{}”：{error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("无法持久化文件“{}”：{error}", path.display()))
}

fn replace_synced_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if link_or_reparse(&metadata) || !metadata.is_file() => {
            return Err("卸载数据目录记录不是安全的普通文件。".to_owned());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法检查旧卸载数据目录记录：{error}")),
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::now_v7().simple()));
    write_new_synced_file(&temporary, bytes)?;
    let backup = path.with_extension(format!("backup-{}", Uuid::now_v7().simple()));
    let had_existing = path.exists();
    if had_existing {
        fs::rename(path, &backup).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            format!("无法暂存旧卸载记录：{error}")
        })?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        if had_existing {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("无法启用新卸载记录：{error}"));
    }
    if had_existing {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("tktiku-{label}-{}", Uuid::now_v7()))
    }

    fn create_initialized_data_root(root: &Path) {
        for name in ["database", "resources", "templates", "backup", "export"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        fs::write(
            root.join("database").join("zhitiku.sqlite3"),
            b"sqlite fixture",
        )
        .unwrap();
        fs::write(root.join("resources").join("image.bin"), b"image").unwrap();
    }

    #[test]
    fn registered_cleanup_removes_only_managed_data() {
        let root = fixture_root("uninstall-cleanup");
        let data_root = root.join("teacher-bank");
        let app_local = root.join("app-local");
        create_initialized_data_root(&data_root);
        fs::create_dir(data_root.join(".restore")).unwrap();
        fs::write(data_root.join("keep-me.txt"), b"unrelated").unwrap();
        register_managed_data_root(&app_local, &data_root).unwrap();

        cleanup_managed_data_from_record(&app_local.join(UNINSTALL_DATA_ROOT_RECORD_FILENAME))
            .unwrap();

        assert!(data_root.is_dir());
        assert!(data_root.join("keep-me.txt").is_file());
        assert!(!data_root.join("database").exists());
        assert!(!data_root.join(DATA_ROOT_MARKER_FILENAME).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_mismatch_aborts_before_deleting_anything() {
        let root = fixture_root("uninstall-mismatch");
        let data_root = root.join("teacher-bank");
        let app_local = root.join("app-local");
        create_initialized_data_root(&data_root);
        register_managed_data_root(&app_local, &data_root).unwrap();

        let marker_path = data_root.join(DATA_ROOT_MARKER_FILENAME);
        let mut marker: DataRootMarker =
            serde_json::from_slice(&fs::read(&marker_path).unwrap()).unwrap();
        marker.ownership_id = Uuid::now_v7().to_string();
        fs::write(&marker_path, serde_json::to_vec(&marker).unwrap()).unwrap();

        let result =
            cleanup_managed_data_from_record(&app_local.join(UNINSTALL_DATA_ROOT_RECORD_FILENAME));
        assert!(result.unwrap_err().contains("不匹配"));
        assert!(data_root.join("database").join("zhitiku.sqlite3").is_file());
        assert!(data_root.join("resources").join("image.bin").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registration_rejects_a_root_without_a_real_database() {
        let root = fixture_root("uninstall-invalid");
        let data_root = root.join("teacher-bank");
        let app_local = root.join("app-local");
        for name in ["database", "resources", "templates", "backup", "export"] {
            fs::create_dir_all(data_root.join(name)).unwrap();
        }

        assert!(register_managed_data_root(&app_local, &data_root).is_err());
        assert!(!data_root.join(DATA_ROOT_MARKER_FILENAME).exists());
        fs::remove_dir_all(root).unwrap();
    }
}
