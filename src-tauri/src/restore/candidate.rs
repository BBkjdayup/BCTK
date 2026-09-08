use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

use crate::{
    api::models::{CommandError, CommandResult},
    backup::ExtractedBackup,
    db::{APPLICATION_ID, Database},
};

const RESTORE_FILE_LOCK_RETRY_DELAYS_MS: [u64; 8] = [20, 50, 100, 200, 400, 800, 1_000, 1_500];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreCandidateSummary {
    pub database_uuid: String,
    pub database_schema_version: i64,
    pub question_count: u64,
    pub paper_count: u64,
    pub template_count: u64,
    pub resource_count: u64,
}

#[derive(Clone, Debug)]
struct ManagedFileExpectation {
    relative_path: String,
    sha256: Vec<u8>,
    byte_size: u64,
    label: &'static str,
}

struct SnapshotGuard(PathBuf);

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub async fn prepare_restore_candidate(
    extracted: &ExtractedBackup,
    app_version: &str,
) -> CommandResult<RestoreCandidateSummary> {
    let expected_uuid = extracted.inspection.manifest.database_uuid.as_str();
    let expected_schema = i64::from(extracted.inspection.manifest.database_schema_version);
    preflight_database(
        &extracted.database_path,
        expected_uuid,
        expected_schema,
        false,
    )
    .await?;
    if expected_schema > Database::current_schema_version() {
        return Err(CommandError::new(
            "RESTORE_DATABASE_VERSION_TOO_NEW",
            format!(
                "备份数据库版本 {expected_schema} 高于当前软件支持的版本 {}，请升级软件后再恢复。",
                Database::current_schema_version()
            ),
        ));
    }

    // The archive extractor places database/resources/templates in exactly the
    // same layout as a normal data root. The non-empty/application-id preflight
    // above is mandatory: it prevents Database::open from treating a corrupt
    // zero-byte candidate as a new empty database.
    let database = Database::open(extracted.staging_dir.clone(), app_version)
        .await
        .map_err(|error| {
            CommandError::new(
                "RESTORE_DATABASE_MIGRATION_FAILED",
                format!("备份数据库无法安全迁移到当前版本：{error}"),
            )
        })?;
    let database_path = database.paths().database_file().to_path_buf();
    let snapshot_path = database_path.with_file_name(format!(
        ".restore-ready-{}.sqlite3",
        Uuid::now_v7().simple()
    ));
    let _snapshot_guard = SnapshotGuard(snapshot_path.clone());
    let preparation_result: CommandResult<()> = async {
        let migrated_uuid = database
            .database_uuid()
            .await
            .map_err(CommandError::database)?;
        if migrated_uuid != expected_uuid {
            return Err(CommandError::new(
                "RESTORE_DATABASE_IDENTITY_MISMATCH",
                "备份数据库身份与归档清单不一致。",
            ));
        }
        let migrated_schema = database
            .schema_version()
            .await
            .map_err(CommandError::database)?;
        if migrated_schema != Database::current_schema_version() {
            return Err(CommandError::new(
                "RESTORE_DATABASE_SCHEMA_INCOMPLETE",
                "备份数据库未能迁移到当前软件要求的完整结构。",
            ));
        }
        database.verify_integrity().await.map_err(|error| {
            CommandError::new(
                "RESTORE_DATABASE_INTEGRITY_FAILED",
                format!("迁移后的备份数据库完整性检查失败：{error}"),
            )
        })?;
        let expectations = managed_file_expectations(database.pool()).await?;
        validate_managed_files(
            extracted.staging_dir.join("resources"),
            extracted.staging_dir.join("templates"),
            expectations,
        )
        .await?;

        let snapshot_text = snapshot_path.to_str().ok_or_else(|| {
            CommandError::new(
                "RESTORE_DATABASE_PATH_INVALID",
                "恢复暂存数据库路径包含当前系统无法处理的字符。",
            )
        })?;
        sqlx::query("VACUUM INTO ?")
            .bind(snapshot_text)
            .execute(database.pool())
            .await
            .map_err(|error| {
                CommandError::new(
                    "RESTORE_DATABASE_SNAPSHOT_FAILED",
                    format!("无法生成自包含的恢复数据库：{error}"),
                )
            })?;
        Ok(())
    }
    .await;

    // SQLx closes SQLite connections asynchronously. Always wait for the pool
    // to finish closing, including validation failures, before touching WAL/SHM
    // files on Windows.
    database.close().await;
    drop(database);
    preparation_result?;

    sync_file(&snapshot_path)?;
    remove_sqlite_sidecars(&database_path).await?;
    remove_restore_work_file(
        &database_path,
        false,
        "RESTORE_DATABASE_REPLACE_FAILED",
        "无法移除暂存数据库的旧工作副本",
    )
    .await?;
    fs::rename(&snapshot_path, &database_path).map_err(|error| {
        CommandError::new(
            "RESTORE_DATABASE_REPLACE_FAILED",
            format!("无法提交自包含的恢复数据库：{error}"),
        )
    })?;
    sync_file(&database_path)?;

    inspect_prepared_candidate(&extracted.staging_dir, expected_uuid).await
}

pub async fn inspect_prepared_candidate(
    staging_root: &Path,
    expected_uuid: &str,
) -> CommandResult<RestoreCandidateSummary> {
    let database_path = staging_root.join("database").join("zhitiku.sqlite3");
    preflight_database(
        &database_path,
        expected_uuid,
        Database::current_schema_version(),
        true,
    )
    .await?;
    let options = SqliteConnectOptions::new()
        .filename(&database_path)
        .read_only(true)
        .create_if_missing(false)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| {
            CommandError::new(
                "RESTORE_DATABASE_OPEN_FAILED",
                format!("无法只读打开恢复候选数据库：{error}"),
            )
        })?;
    let inspection_result: CommandResult<_> = async {
        let expectations = managed_file_expectations_connection(&mut connection).await?;
        let question_count = nonnegative_count(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM questions WHERE deleted_at_ms IS NULL",
            )
            .fetch_one(&mut connection)
            .await
            .map_err(CommandError::database)?,
            "题目",
        )?;
        let paper_count = nonnegative_count(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM papers WHERE paper_status = 'saved'",
            )
            .fetch_one(&mut connection)
            .await
            .map_err(CommandError::database)?,
            "历史试卷",
        )?;
        let template_count = nonnegative_count(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM word_templates")
                .fetch_one(&mut connection)
                .await
                .map_err(CommandError::database)?,
            "Word 模板",
        )?;
        let resource_count = nonnegative_count(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM resources WHERE availability_status = 'ready'",
            )
            .fetch_one(&mut connection)
            .await
            .map_err(CommandError::database)?,
            "资源",
        )?;
        Ok((
            expectations,
            question_count,
            paper_count,
            template_count,
            resource_count,
        ))
    }
    .await;
    let close_result = connection.close().await.map_err(CommandError::database);
    let (expectations, question_count, paper_count, template_count, resource_count) =
        inspection_result?;
    close_result?;
    validate_managed_files(
        staging_root.join("resources"),
        staging_root.join("templates"),
        expectations,
    )
    .await?;
    Ok(RestoreCandidateSummary {
        database_uuid: expected_uuid.to_owned(),
        database_schema_version: Database::current_schema_version(),
        question_count,
        paper_count,
        template_count,
        resource_count,
    })
}

async fn preflight_database(
    database_path: &Path,
    expected_uuid: &str,
    expected_schema: i64,
    require_current_schema: bool,
) -> CommandResult<()> {
    let metadata = fs::symlink_metadata(database_path).map_err(|error| {
        CommandError::new(
            "RESTORE_DATABASE_FILE_MISSING",
            format!("备份中的数据库文件无法读取：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(CommandError::new(
            "RESTORE_DATABASE_FILE_INVALID",
            "备份中的数据库必须是真实且非空的普通文件。",
        ));
    }
    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .read_only(true)
        .create_if_missing(false)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| {
            CommandError::new(
                "RESTORE_DATABASE_OPEN_FAILED",
                format!("备份数据库无法只读打开：{error}"),
            )
        })?;
    let preflight_result: CommandResult<()> = async {
        let application_id: i64 = sqlx::query_scalar("PRAGMA application_id")
            .fetch_one(&mut connection)
            .await
            .map_err(CommandError::database)?;
        if application_id != APPLICATION_ID {
            return Err(CommandError::new(
                "RESTORE_DATABASE_APPLICATION_ID_INVALID",
                "备份中的数据库不是本软件创建的题库文件。",
            ));
        }
        let quick_check: String = sqlx::query_scalar("PRAGMA quick_check")
            .fetch_one(&mut connection)
            .await
            .map_err(CommandError::database)?;
        if quick_check != "ok" {
            return Err(CommandError::new(
                "RESTORE_DATABASE_INTEGRITY_FAILED",
                format!("备份数据库完整性检查失败：{quick_check}"),
            ));
        }
        let foreign_key_violations: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&mut connection)
                .await
                .map_err(CommandError::database)?;
        if foreign_key_violations != 0 {
            return Err(CommandError::new(
                "RESTORE_DATABASE_FOREIGN_KEY_FAILED",
                format!("备份数据库包含 {foreign_key_violations} 条无效关联。"),
            ));
        }
        let schema_version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
        )
        .fetch_one(&mut connection)
        .await
        .map_err(|error| {
            CommandError::new(
                "RESTORE_DATABASE_MIGRATIONS_INVALID",
                format!("无法读取备份数据库的迁移记录：{error}"),
            )
        })?;
        if schema_version != expected_schema
            || (require_current_schema && schema_version != Database::current_schema_version())
        {
            return Err(CommandError::new(
                "RESTORE_DATABASE_SCHEMA_MISMATCH",
                "备份清单、数据库迁移记录与当前软件支持版本不一致。",
            ));
        }
        let database_uuid: String =
            sqlx::query_scalar("SELECT database_uuid FROM app_meta WHERE singleton_id = 1")
                .fetch_one(&mut connection)
                .await
                .map_err(|error| {
                    CommandError::new(
                        "RESTORE_DATABASE_IDENTITY_INVALID",
                        format!("无法读取备份数据库身份：{error}"),
                    )
                })?;
        if database_uuid != expected_uuid || Uuid::parse_str(&database_uuid).is_err() {
            return Err(CommandError::new(
                "RESTORE_DATABASE_IDENTITY_MISMATCH",
                "备份数据库身份无效或与归档清单不一致。",
            ));
        }
        reject_unexpected_triggers(&mut connection, schema_version).await?;
        Ok(())
    }
    .await;
    let close_result = connection.close().await.map_err(|error| {
        CommandError::new(
            "RESTORE_DATABASE_CLOSE_FAILED",
            format!("无法安全关闭恢复候选数据库：{error}"),
        )
    });
    preflight_result?;
    close_result?;
    Ok(())
}

async fn reject_unexpected_triggers(
    connection: &mut SqliteConnection,
    schema_version: i64,
) -> CommandResult<()> {
    let mut names =
        sqlx::query("SELECT name FROM sqlite_master WHERE type = 'trigger' ORDER BY name")
            .fetch_all(&mut *connection)
            .await
            .map_err(CommandError::database)?
            .into_iter()
            .map(|row| {
                row.try_get::<String, _>("name")
                    .map_err(CommandError::database)
            })
            .collect::<CommandResult<Vec<_>>>()?;
    let mut expected = if schema_version >= 8 {
        vec![
            "question_fts_after_delete".to_owned(),
            "question_fts_after_insert".to_owned(),
            "question_fts_after_search_update".to_owned(),
        ]
    } else {
        Vec::new()
    };
    names.sort();
    expected.sort();
    if names != expected {
        return Err(CommandError::new(
            "RESTORE_DATABASE_TRIGGER_REJECTED",
            "备份数据库包含当前软件未定义的触发器，已拒绝恢复。",
        ));
    }
    Ok(())
}

async fn managed_file_expectations(
    pool: &sqlx::SqlitePool,
) -> CommandResult<Vec<ManagedFileExpectation>> {
    let mut connection = pool.acquire().await.map_err(CommandError::database)?;
    managed_file_expectations_connection(&mut connection).await
}

async fn managed_file_expectations_connection(
    connection: &mut SqliteConnection,
) -> CommandResult<Vec<ManagedFileExpectation>> {
    let mut output = Vec::new();
    let resource_rows = sqlx::query(
        "SELECT storage_rel_path, sha256, byte_size FROM resources WHERE availability_status = 'ready'",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    for row in resource_rows {
        output.push(ManagedFileExpectation {
            relative_path: row
                .try_get("storage_rel_path")
                .map_err(CommandError::database)?,
            sha256: row.try_get("sha256").map_err(CommandError::database)?,
            byte_size: nonnegative_size(
                row.try_get("byte_size").map_err(CommandError::database)?,
                "资源",
            )?,
            label: "资源",
        });
    }
    let template_rows =
        sqlx::query("SELECT file_rel_path, file_sha256, file_byte_size FROM word_templates")
            .fetch_all(&mut *connection)
            .await
            .map_err(CommandError::database)?;
    for row in template_rows {
        output.push(ManagedFileExpectation {
            relative_path: row
                .try_get("file_rel_path")
                .map_err(CommandError::database)?,
            sha256: row.try_get("file_sha256").map_err(CommandError::database)?,
            byte_size: nonnegative_size(
                row.try_get("file_byte_size")
                    .map_err(CommandError::database)?,
                "Word 模板",
            )?,
            label: "Word 模板",
        });
    }
    Ok(output)
}

async fn validate_managed_files(
    resources_root: PathBuf,
    templates_root: PathBuf,
    expectations: Vec<ManagedFileExpectation>,
) -> CommandResult<()> {
    tokio::task::spawn_blocking(move || {
        for expectation in expectations {
            let root = if expectation.label == "资源" {
                &resources_root
            } else {
                &templates_root
            };
            validate_managed_file(root, &expectation)?;
        }
        Ok(())
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "RESTORE_FILE_VERIFY_TASK_FAILED",
            format!("恢复资源校验任务未能正常完成：{error}"),
        )
    })?
}

fn validate_managed_file(root: &Path, expectation: &ManagedFileExpectation) -> CommandResult<()> {
    if expectation.sha256.len() != 32 {
        return Err(CommandError::new(
            "RESTORE_FILE_HASH_INVALID",
            format!("{}记录的 SHA-256 长度无效。", expectation.label),
        ));
    }
    let relative = Path::new(&expectation.relative_path);
    if expectation.relative_path.contains('\\')
        || relative.is_absolute()
        || relative.components().next().is_none()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CommandError::new(
            "RESTORE_FILE_PATH_INVALID",
            format!("{}记录了不安全的相对路径。", expectation.label),
        ));
    }
    let root_metadata = fs::symlink_metadata(root).map_err(|error| {
        CommandError::new(
            "RESTORE_FILE_ROOT_INVALID",
            format!("无法检查{}目录：{error}", expectation.label),
        )
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(CommandError::new(
            "RESTORE_FILE_ROOT_INVALID",
            format!("{}目录不是安全的真实目录。", expectation.label),
        ));
    }
    let mut candidate = root.to_path_buf();
    for component in relative.components() {
        candidate.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            CommandError::new(
                "RESTORE_FILE_MISSING",
                format!(
                    "{}文件“{}”缺失：{error}",
                    expectation.label, expectation.relative_path
                ),
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(CommandError::new(
                "RESTORE_FILE_SYMLINK_REJECTED",
                format!("{}文件路径包含符号链接。", expectation.label),
            ));
        }
    }
    let metadata = fs::metadata(&candidate).map_err(|error| {
        CommandError::new(
            "RESTORE_FILE_MISSING",
            format!("无法检查{}文件：{error}", expectation.label),
        )
    })?;
    if !metadata.is_file() || metadata.len() != expectation.byte_size {
        return Err(CommandError::new(
            "RESTORE_FILE_SIZE_MISMATCH",
            format!("{}文件大小与数据库记录不一致。", expectation.label),
        ));
    }
    let mut reader = BufReader::new(File::open(&candidate).map_err(|error| {
        CommandError::new(
            "RESTORE_FILE_OPEN_FAILED",
            format!("无法打开{}文件：{error}", expectation.label),
        )
    })?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let count = reader.read(&mut buffer).map_err(|error| {
            CommandError::new(
                "RESTORE_FILE_READ_FAILED",
                format!("无法读取{}文件：{error}", expectation.label),
            )
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if hasher.finalize().as_slice() != expectation.sha256.as_slice() {
        return Err(CommandError::new(
            "RESTORE_FILE_HASH_MISMATCH",
            format!("{}文件内容与数据库 SHA-256 记录不一致。", expectation.label),
        ));
    }
    Ok(())
}

fn nonnegative_count(value: i64, label: &str) -> CommandResult<u64> {
    u64::try_from(value).map_err(|_| {
        CommandError::new(
            "RESTORE_SUMMARY_INVALID",
            format!("恢复候选中的{label}数量无效。"),
        )
    })
}

fn nonnegative_size(value: i64, label: &str) -> CommandResult<u64> {
    u64::try_from(value).map_err(|_| {
        CommandError::new(
            "RESTORE_FILE_SIZE_INVALID",
            format!("{label}文件大小记录无效。"),
        )
    })
}

async fn remove_sqlite_sidecars(database_path: &Path) -> CommandResult<()> {
    let file_name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CommandError::new("RESTORE_DATABASE_PATH_INVALID", "数据库文件名无效。"))?;
    for suffix in ["-wal", "-shm"] {
        let path = database_path.with_file_name(format!("{file_name}{suffix}"));
        remove_restore_work_file(
            &path,
            true,
            "RESTORE_DATABASE_SIDECAR_CLEANUP_FAILED",
            "无法清理暂存数据库工作文件",
        )
        .await?;
    }
    Ok(())
}

async fn remove_restore_work_file(
    path: &Path,
    allow_missing: bool,
    error_code: &'static str,
    error_context: &'static str,
) -> CommandResult<()> {
    let mut retry_index = 0usize;
    loop {
        match tokio::fs::remove_file(path).await {
            Ok(()) => return Ok(()),
            Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(());
            }
            Err(error)
                if is_retryable_restore_file_lock(&error)
                    && retry_index < RESTORE_FILE_LOCK_RETRY_DELAYS_MS.len() =>
            {
                let delay_ms = RESTORE_FILE_LOCK_RETRY_DELAYS_MS[retry_index];
                retry_index += 1;
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            Err(error) => {
                let retry_note = if retry_index == 0 {
                    String::new()
                } else {
                    format!("，等待文件释放并重试 {retry_index} 次后仍未成功")
                };
                return Err(CommandError::new(
                    error_code,
                    format!("{error_context}{retry_note}：{error}"),
                ));
            }
        }
    }
}

#[cfg(windows)]
fn is_retryable_restore_file_lock(error: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32) and ERROR_LOCK_VIOLATION (33).
    matches!(error.raw_os_error(), Some(32 | 33))
}

#[cfg(not(windows))]
fn is_retryable_restore_file_lock(_error: &std::io::Error) -> bool {
    false
}

fn sync_file(path: &Path) -> CommandResult<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            CommandError::new(
                "RESTORE_FILE_SYNC_FAILED",
                format!("无法将恢复文件安全写入磁盘：{error}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zhitiku-restore-candidate-{name}-{}",
            Uuid::now_v7().simple()
        ))
    }

    #[test]
    fn managed_relative_paths_reject_parent_and_windows_separators() {
        let expectation = |relative_path: &str| ManagedFileExpectation {
            relative_path: relative_path.to_owned(),
            sha256: vec![0; 32],
            byte_size: 0,
            label: "资源",
        };
        let missing_root = Path::new("Z:/definitely-missing-restore-root");
        assert!(validate_managed_file(missing_root, &expectation("../escape.png")).is_err());
        assert!(validate_managed_file(missing_root, &expectation("nested\\escape.png")).is_err());
        assert!(validate_managed_file(missing_root, &expectation("/absolute.png")).is_err());
    }

    #[tokio::test]
    async fn prepared_candidate_releases_sqlite_files_before_cleanup() {
        let root = test_root("sqlite-cleanup");
        let database = Database::open(&root, "test").await.unwrap();
        let database_uuid = database.database_uuid().await.unwrap();
        database.close().await;
        drop(database);

        let database_path = root.join("database").join("zhitiku.sqlite3");
        let extracted = ExtractedBackup {
            staging_dir: root.clone(),
            database_path: database_path.clone(),
            resources_dir: Some(root.join("resources")),
            templates_dir: Some(root.join("templates")),
            inspection: crate::backup::BackupInspection {
                manifest: crate::backup::BackupManifest {
                    format: crate::backup::BACKUP_FORMAT.to_owned(),
                    version: crate::backup::BACKUP_FORMAT_VERSION,
                    created_at_unix_ms: 0,
                    application_version: "test".to_owned(),
                    database_schema_version: u32::try_from(Database::current_schema_version())
                        .unwrap(),
                    database_uuid: database_uuid.clone(),
                    files: Vec::new(),
                },
                manifest_sha256_hex: String::new(),
                archive_sha256_hex: String::new(),
                archive_size_bytes: 0,
                payload_size_bytes: 0,
                payload_file_count: 0,
            },
        };

        let summary = prepare_restore_candidate(&extracted, "test").await.unwrap();
        assert_eq!(summary.database_uuid, database_uuid);
        assert!(database_path.is_file());
        assert!(!database_path.with_file_name("zhitiku.sqlite3-wal").exists());
        assert!(!database_path.with_file_name("zhitiku.sqlite3-shm").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_sharing_violations_are_retryable() {
        assert!(is_retryable_restore_file_lock(
            &std::io::Error::from_raw_os_error(32)
        ));
        assert!(is_retryable_restore_file_lock(
            &std::io::Error::from_raw_os_error(33)
        ));
        assert!(!is_retryable_restore_file_lock(
            &std::io::Error::from_raw_os_error(5)
        ));
    }
}
