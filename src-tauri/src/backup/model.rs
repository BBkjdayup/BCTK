use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const BACKUP_FORMAT: &str = "zhitiku-tqb-backup";
pub const BACKUP_FORMAT_VERSION: u32 = 1;
pub const MANIFEST_PATH: &str = "manifest.json";
pub const DATABASE_ARCHIVE_PATH: &str = "database/zhitiku.sqlite3";

#[derive(Clone, Debug)]
pub struct BackupLimits {
    pub max_archive_bytes: u64,
    pub max_entries: usize,
    pub max_central_directory_bytes: u64,
    pub max_entry_bytes: u64,
    pub max_total_uncompressed_bytes: u64,
    pub max_manifest_bytes: u64,
    pub max_path_bytes: usize,
    pub max_path_depth: usize,
    pub max_compression_ratio: u64,
}

impl Default for BackupLimits {
    fn default() -> Self {
        Self {
            // W1 deliberately rejects ZIP64. A sub-4 GiB archive keeps the
            // central-directory preflight small and predictable.
            max_archive_bytes: (u32::MAX as u64) - 1,
            max_entries: 50_000,
            max_central_directory_bytes: 64 * 1024 * 1024,
            max_entry_bytes: 2 * 1024 * 1024 * 1024,
            max_total_uncompressed_bytes: 16 * 1024 * 1024 * 1024,
            max_manifest_bytes: 16 * 1024 * 1024,
            max_path_bytes: 1_024,
            max_path_depth: 64,
            max_compression_ratio: 1_000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BackupMetadata {
    pub application_version: String,
    pub database_schema_version: u32,
    pub database_uuid: String,
}

#[derive(Clone, Debug)]
pub struct CreateBackupRequest {
    pub output_path: PathBuf,
    pub database_snapshot_path: PathBuf,
    pub resources_dir: Option<PathBuf>,
    pub templates_dir: Option<PathBuf>,
    pub metadata: BackupMetadata,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupFileRole {
    Database,
    Resource,
    Template,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifestFile {
    pub path: String,
    pub role: BackupFileRole,
    pub size_bytes: u64,
    pub sha256_hex: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub format: String,
    pub version: u32,
    pub created_at_unix_ms: u64,
    pub application_version: String,
    pub database_schema_version: u32,
    pub database_uuid: String,
    pub files: Vec<BackupManifestFile>,
}

#[derive(Clone, Debug)]
pub struct BackupInspection {
    pub manifest: BackupManifest,
    pub manifest_sha256_hex: String,
    pub archive_sha256_hex: String,
    pub archive_size_bytes: u64,
    pub payload_size_bytes: u64,
    pub payload_file_count: usize,
}

#[derive(Clone, Debug)]
pub struct BackupCreation {
    pub output_path: PathBuf,
    pub inspection: BackupInspection,
}

#[derive(Clone, Debug)]
pub struct ExtractedBackup {
    pub staging_dir: PathBuf,
    pub database_path: PathBuf,
    #[cfg(test)]
    pub resources_dir: Option<PathBuf>,
    #[cfg(test)]
    pub templates_dir: Option<PathBuf>,
    pub inspection: BackupInspection,
}
