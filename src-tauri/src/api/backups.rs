use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use serde_json::{Value, json};
use sqlx::{Connection, FromRow, SqliteConnection, SqlitePool, sqlite::SqliteConnectOptions};
use uuid::Uuid;

use crate::{
    backup::{self, BackupInspection, BackupLimits, BackupMetadata, CreateBackupRequest},
    db::{APPLICATION_ID, Database},
};

use super::models::{
    AutomaticBackupRunApi, BackupCreateResultApi, BackupInspectionApi, BackupRecordApi,
    CommandError, CommandResult,
};

const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1_000;

#[derive(Debug, FromRow)]
struct BackupRecordRow {
    id: String,
    backup_kind: String,
    status: String,
    archive_rel_path: Option<String>,
    display_filename: String,
    format_version: i64,
    database_schema_version: i64,
    source_database_uuid: String,
    source_app_version: String,
    archive_sha256: Option<Vec<u8>>,
    manifest_sha256: Option<Vec<u8>>,
    archive_byte_size: Option<i64>,
    contents_summary_json: String,
    created_at_ms: i64,
    completed_at_ms: Option<i64>,
    error_code: Option<String>,
    error_message: Option<String>,
}

struct SnapshotCleanup(PathBuf);

impl Drop for SnapshotCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) async fn list_backups(database: &Database) -> CommandResult<Vec<BackupRecordApi>> {
    let rows = sqlx::query_as::<_, BackupRecordRow>(
        "SELECT id, backup_kind, status, archive_rel_path, display_filename, format_version, \
                database_schema_version, source_database_uuid, source_app_version, archive_sha256, \
                manifest_sha256, archive_byte_size, contents_summary_json, created_at_ms, \
                completed_at_ms, error_code, error_message \
         FROM backup_records ORDER BY created_at_ms DESC, id DESC",
    )
    .fetch_all(database.pool())
    .await
    .map_err(CommandError::database)?;

    rows.into_iter()
        .map(|row| backup_record_api(row, database.paths().backup_dir()))
        .collect()
}

pub(crate) async fn create_manual_backup(
    database: &Database,
    output_path: PathBuf,
    app_version: &str,
) -> CommandResult<BackupCreateResultApi> {
    create_complete_backup(database, output_path, app_version, "manual").await
}

pub(crate) async fn create_pre_restore_backup(
    database: &Database,
    output_path: PathBuf,
    app_version: &str,
) -> CommandResult<BackupCreateResultApi> {
    create_complete_backup(database, output_path, app_version, "pre_restore").await
}

pub(crate) async fn create_data_move_backup(
    database: &Database,
    output_path: PathBuf,
    app_version: &str,
) -> CommandResult<BackupCreateResultApi> {
    create_complete_backup(database, output_path, app_version, "data_move").await
}

pub(crate) async fn run_automatic_backup_if_due(
    database: &Database,
    app_version: &str,
) -> CommandResult<AutomaticBackupRunApi> {
    let (enabled, interval_days, retention_count): (i64, i64, i64) = sqlx::query_as(
        "SELECT automatic_backup_enabled, automatic_backup_interval_days, \
         automatic_backup_retention_count FROM app_settings WHERE singleton_id = 1",
    )
    .fetch_one(database.pool())
    .await
    .map_err(CommandError::database)?;
    let last_backup_at = latest_available_automatic_backup(database).await?;

    if enabled == 0 {
        return Ok(AutomaticBackupRunApi {
            outcome: "disabled".to_owned(),
            message: "自动备份已关闭。".to_owned(),
            record: None,
            pruned_count: 0,
            last_backup_at,
            next_due_at: None,
            warnings: Vec::new(),
        });
    }

    let interval_days = interval_days.clamp(1, 365);
    let interval_millis = interval_days.saturating_mul(MILLIS_PER_DAY);
    let now = now_millis();
    let next_due_at = last_backup_at.map(|last| last.saturating_add(interval_millis));
    if next_due_at.is_some_and(|due| due > now) {
        return Ok(AutomaticBackupRunApi {
            outcome: "not_due".to_owned(),
            message: format!("自动备份尚未到期，每 {interval_days} 天执行一次。"),
            record: None,
            pruned_count: 0,
            last_backup_at,
            next_due_at,
            warnings: Vec::new(),
        });
    }

    let filename = format!(
        "TK试题题库_自动备份_{}_{}.tqb",
        now,
        &Uuid::now_v7().to_string()[..8]
    );
    let output_path = database.paths().backup_dir().join(filename);
    let created =
        match create_complete_backup(database, output_path, app_version, "automatic").await {
            Ok(created) => created,
            Err(error) => {
                let _ = record_automatic_backup_failure(database.pool(), app_version, &error).await;
                return Err(error);
            }
        };

    let mut warnings = Vec::new();
    let retention_count = retention_count.clamp(1, 50) as u32;
    let pruned_count =
        match prune_old_automatic_backups(database, retention_count, app_version).await {
            Ok(count) => count,
            Err(error) => {
                warnings.push(format!(
                    "新自动备份已经创建，但旧自动备份暂未清理：{}",
                    error.message
                ));
                0
            }
        };
    let completed_at = created.record.completed_at.or(Some(now));

    Ok(AutomaticBackupRunApi {
        outcome: "created".to_owned(),
        message: if pruned_count > 0 {
            format!("自动备份已创建，并清理了 {pruned_count} 份过期自动备份。")
        } else {
            "自动备份已创建并完成校验。".to_owned()
        },
        record: Some(created.record),
        pruned_count,
        last_backup_at: completed_at,
        next_due_at: completed_at.map(|value| value.saturating_add(interval_millis)),
        warnings,
    })
}

pub(crate) async fn register_pre_restore_backup_after_restore(
    database: &Database,
    record: &BackupRecordApi,
    operation_id: &str,
) -> CommandResult<()> {
    if record.backup_kind != "pre_restore"
        || record.status != "ready"
        || record.archive_sha256_hex.is_none()
        || record.manifest_sha256_hex.is_none()
        || record.archive_byte_size.is_none()
        || record.completed_at.is_none()
    {
        return Err(CommandError::new(
            "RESTORE_BACKUP_RECORD_INVALID",
            "恢复前备份记录不完整，无法登记到恢复后的题库。",
        ));
    }
    let archive_hash = decode_sha256(record.archive_sha256_hex.as_deref().unwrap_or_default())?;
    let manifest_hash = decode_sha256(record.manifest_sha256_hex.as_deref().unwrap_or_default())?;
    let archive_bytes = i64::try_from(record.archive_byte_size.unwrap_or_default())
        .map_err(|_| CommandError::validation("恢复前备份大小超出数据库记录范围。"))?;
    let payload_file_count = record.payload_file_count;
    let payload_byte_size = record.payload_byte_size;
    let summary = json!({
        "payloadFileCount": payload_file_count,
        "payloadByteSize": payload_byte_size,
        "restoredCatalogueRecord": true,
    })
    .to_string();
    let mut transaction = database
        .pool()
        .begin()
        .await
        .map_err(CommandError::database)?;
    sqlx::query(
        "INSERT OR IGNORE INTO backup_records (id, backup_kind, status, archive_rel_path, \
         display_filename, format_version, database_schema_version, source_database_uuid, \
         source_app_version, archive_sha256, manifest_sha256, archive_byte_size, \
         contents_summary_json, created_at_ms, completed_at_ms, last_verified_at_ms, \
         error_code, error_message) VALUES (?, 'pre_restore', 'ready', ?, ?, ?, ?, ?, ?, ?, ?, \
         ?, ?, ?, ?, ?, NULL, NULL)",
    )
    .bind(&record.id)
    .bind(&record.display_filename)
    .bind(&record.display_filename)
    .bind(i64::from(record.format_version))
    .bind(i64::from(record.database_schema_version))
    .bind(&record.source_database_uuid)
    .bind(&record.source_app_version)
    .bind(&archive_hash)
    .bind(&manifest_hash)
    .bind(archive_bytes)
    .bind(&summary)
    .bind(record.created_at)
    .bind(record.completed_at)
    .bind(record.completed_at)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    let matching: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM backup_records WHERE id = ? AND backup_kind = 'pre_restore' \
         AND status = 'ready' AND archive_rel_path = ? AND archive_sha256 = ? \
         AND manifest_sha256 = ? AND archive_byte_size = ?",
    )
    .bind(&record.id)
    .bind(&record.display_filename)
    .bind(&archive_hash)
    .bind(&manifest_hash)
    .bind(archive_bytes)
    .fetch_one(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    if matching != 1 {
        return Err(CommandError::new(
            "RESTORE_BACKUP_RECORD_CONFLICT",
            "恢复后的题库存在同标识但内容不同的备份记录。",
        ));
    }
    sqlx::query(
        "INSERT OR IGNORE INTO operation_logs (id, occurred_at_ms, level, action, entity_type, \
         entity_id, outcome, summary, details_json, app_version) VALUES (?, ?, 'info', \
         'restore.success', 'backup', ?, 'success', '恢复成功并登记恢复前备份', ?, ?)",
    )
    .bind(operation_id)
    .bind(record.completed_at.unwrap_or(record.created_at))
    .bind(&record.id)
    .bind(
        json!({
            "preRestoreBackupFilename": record.display_filename,
            "archiveByteSize": record.archive_byte_size,
        })
        .to_string(),
    )
    .bind(&record.source_app_version)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

async fn create_complete_backup(
    database: &Database,
    output_path: PathBuf,
    app_version: &str,
    backup_kind: &str,
) -> CommandResult<BackupCreateResultApi> {
    if !matches!(
        backup_kind,
        "manual" | "automatic" | "pre_restore" | "data_move"
    ) {
        return Err(CommandError::validation("备份类型无效。"));
    }
    if !output_path.is_absolute()
        || !output_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tqb"))
    {
        return Err(CommandError::validation(
            "备份保存路径必须是绝对路径，文件名必须以 .tqb 结尾。",
        ));
    }
    reject_backup_destination_inside_managed_payload(database, &output_path)?;
    match fs::symlink_metadata(&output_path) {
        Ok(_) => {
            return Err(CommandError::validation(
                "备份目标已经存在。为避免误覆盖，请选择新的文件名。",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CommandError::new(
                "BACKUP_OUTPUT_CHECK_FAILED",
                format!("无法检查备份目标路径：{error}"),
            ));
        }
    }
    database.verify_integrity().await.map_err(|error| {
        CommandError::new(
            "BACKUP_SOURCE_DATABASE_INVALID",
            format!("当前题库未通过完整性检查，已停止备份：{error}"),
        )
    })?;

    let snapshot_path = database
        .paths()
        .backup_dir()
        .join(format!(".backup-snapshot-{}.sqlite3", Uuid::now_v7()));
    let _cleanup = SnapshotCleanup(snapshot_path.clone());
    let snapshot_text = snapshot_path.to_str().ok_or_else(|| {
        CommandError::new(
            "BACKUP_SNAPSHOT_PATH_INVALID",
            "数据库快照路径包含当前系统无法处理的字符。",
        )
    })?;
    sqlx::query("VACUUM INTO ?")
        .bind(snapshot_text)
        .execute(database.pool())
        .await
        .map_err(|error| {
            CommandError::new(
                "BACKUP_SNAPSHOT_FAILED",
                format!("无法创建一致的数据库快照：{error}"),
            )
        })?;

    let source_schema_version = database
        .schema_version()
        .await
        .map_err(CommandError::database)?;
    let source_database_uuid = database
        .database_uuid()
        .await
        .map_err(CommandError::database)?;
    verify_database_snapshot(&snapshot_path, source_schema_version, &source_database_uuid).await?;

    let request = CreateBackupRequest {
        output_path: output_path.clone(),
        database_snapshot_path: snapshot_path,
        resources_dir: Some(database.paths().resources_dir().to_path_buf()),
        templates_dir: Some(database.paths().templates_dir().to_path_buf()),
        metadata: BackupMetadata {
            application_version: app_version.to_owned(),
            database_schema_version: u32::try_from(source_schema_version).map_err(|_| {
                CommandError::new(
                    "BACKUP_SCHEMA_VERSION_INVALID",
                    "当前数据库版本超出备份格式支持范围。",
                )
            })?,
            database_uuid: source_database_uuid,
        },
    };
    let created = tokio::task::spawn_blocking(move || {
        backup::create_backup(&request, &BackupLimits::default())
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "BACKUP_TASK_FAILED",
            format!("备份任务未能正常完成：{error}"),
        )
    })?
    .map_err(backup_error)?;

    let record = write_ready_record(
        database.pool(),
        database.paths().backup_dir(),
        &created.output_path,
        &created.inspection,
        app_version,
        backup_kind,
    )
    .await
    .map_err(|error| {
        CommandError::new(
            "BACKUP_CATALOG_WRITE_FAILED",
            format!(
                "备份文件已经安全创建在“{}”，但备份历史记录写入失败：{}",
                created.output_path.display(),
                error.message
            ),
        )
    })?;

    Ok(BackupCreateResultApi {
        output_path: created.output_path.to_string_lossy().into_owned(),
        record,
    })
}

async fn latest_available_automatic_backup(database: &Database) -> CommandResult<Option<i64>> {
    let rows = sqlx::query_as::<_, (i64, Option<String>)>(
        "SELECT completed_at_ms, archive_rel_path FROM backup_records \
         WHERE backup_kind = 'automatic' AND status = 'ready' \
         AND completed_at_ms IS NOT NULL ORDER BY completed_at_ms DESC, id DESC",
    )
    .fetch_all(database.pool())
    .await
    .map_err(CommandError::database)?;

    Ok(rows.into_iter().find_map(|(completed_at, relative)| {
        relative
            .as_deref()
            .filter(|value| managed_backup_file_available(database.paths().backup_dir(), value))
            .map(|_| completed_at)
    }))
}

async fn prune_old_automatic_backups(
    database: &Database,
    retention_count: u32,
    app_version: &str,
) -> CommandResult<u32> {
    let rows = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT id, archive_rel_path FROM backup_records \
         WHERE backup_kind = 'automatic' AND status = 'ready' \
         ORDER BY completed_at_ms DESC, id DESC LIMIT -1 OFFSET ?",
    )
    .bind(i64::from(retention_count))
    .fetch_all(database.pool())
    .await
    .map_err(CommandError::database)?;

    let mut pruned = 0u32;
    for (id, relative) in rows {
        let Some(relative) = relative.filter(|value| is_safe_backup_relative(value)) else {
            continue;
        };
        let path = database.paths().backup_dir().join(&relative);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                fs::remove_file(&path).map_err(|error| {
                    CommandError::new(
                        "AUTOMATIC_BACKUP_PRUNE_FAILED",
                        format!("无法删除过期自动备份“{relative}”：{error}"),
                    )
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => continue,
            Err(error) => {
                return Err(CommandError::new(
                    "AUTOMATIC_BACKUP_PRUNE_CHECK_FAILED",
                    format!("无法检查过期自动备份“{relative}”：{error}"),
                ));
            }
        }
        sqlx::query("DELETE FROM backup_records WHERE id = ? AND backup_kind = 'automatic'")
            .bind(&id)
            .execute(database.pool())
            .await
            .map_err(CommandError::database)?;
        pruned = pruned.saturating_add(1);
    }

    if pruned > 0 {
        sqlx::query(
            "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, outcome, \
             summary, details_json, app_version) VALUES (?, ?, 'info', 'backup.prune', 'backup', \
             'success', '清理过期自动备份', ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(now_millis())
        .bind(json!({ "prunedCount": pruned, "retentionCount": retention_count }).to_string())
        .bind(app_version)
        .execute(database.pool())
        .await
        .map_err(CommandError::database)?;
    }
    Ok(pruned)
}

async fn record_automatic_backup_failure(
    pool: &SqlitePool,
    app_version: &str,
    error: &CommandError,
) -> CommandResult<()> {
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, outcome, \
         summary, details_json, error_code, app_version) VALUES (?, ?, 'error', \
         'backup.automatic', 'backup', 'failure', '自动备份失败', ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(now_millis())
    .bind(json!({ "message": error.message }).to_string())
    .bind(&error.code)
    .bind(app_version)
    .execute(pool)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn reject_backup_destination_inside_managed_payload(
    database: &Database,
    output_path: &Path,
) -> CommandResult<()> {
    let parent = output_path
        .parent()
        .ok_or_else(|| CommandError::validation("备份保存路径缺少有效的父目录。"))?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        CommandError::new(
            "BACKUP_OUTPUT_DIRECTORY_INVALID",
            format!("无法检查备份保存目录：{error}"),
        )
    })?;
    for managed in [
        database.paths().database_file().parent(),
        Some(database.paths().resources_dir()),
        Some(database.paths().templates_dir()),
    ]
    .into_iter()
    .flatten()
    {
        let canonical_managed = fs::canonicalize(managed).map_err(|error| {
            CommandError::new(
                "BACKUP_MANAGED_DIRECTORY_INVALID",
                format!("无法检查受管数据目录“{}”：{error}", managed.display()),
            )
        })?;
        if canonical_parent.starts_with(&canonical_managed) {
            return Err(CommandError::validation(
                "备份不能保存在 database、resources 或 templates 目录内；请选择软件 backup 目录或数据目录之外的位置。",
            ));
        }
    }
    Ok(())
}

pub(crate) async fn inspect_backup_file(path: PathBuf) -> CommandResult<BackupInspectionApi> {
    tokio::task::spawn_blocking(move || backup::inspect_backup(&path, &BackupLimits::default()))
        .await
        .map_err(|error| {
            CommandError::new(
                "BACKUP_INSPECTION_TASK_FAILED",
                format!("备份检查任务未能正常完成：{error}"),
            )
        })?
        .map(inspection_api)
        .map_err(backup_error)
}

async fn verify_database_snapshot(
    path: &Path,
    expected_schema_version: i64,
    expected_database_uuid: &str,
) -> CommandResult<()> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| {
            CommandError::new(
                "BACKUP_SNAPSHOT_OPEN_FAILED",
                format!("数据库快照无法重新打开：{error}"),
            )
        })?;
    let application_id: i64 = sqlx::query_scalar("PRAGMA application_id")
        .fetch_one(&mut connection)
        .await
        .map_err(CommandError::database)?;
    if application_id != APPLICATION_ID {
        return Err(CommandError::new(
            "BACKUP_SNAPSHOT_APPLICATION_ID_INVALID",
            "数据库快照不是本软件的题库文件。",
        ));
    }
    let quick_check: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&mut connection)
        .await
        .map_err(CommandError::database)?;
    if quick_check != "ok" {
        return Err(CommandError::new(
            "BACKUP_SNAPSHOT_INTEGRITY_FAILED",
            format!("数据库快照完整性检查失败：{quick_check}"),
        ));
    }
    let foreign_key_violations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .fetch_one(&mut connection)
            .await
            .map_err(CommandError::database)?;
    if foreign_key_violations != 0 {
        return Err(CommandError::new(
            "BACKUP_SNAPSHOT_FOREIGN_KEY_FAILED",
            format!("数据库快照包含 {foreign_key_violations} 条无效关联。"),
        ));
    }
    let schema_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
    )
    .fetch_one(&mut connection)
    .await
    .map_err(CommandError::database)?;
    if schema_version != expected_schema_version {
        return Err(CommandError::new(
            "BACKUP_SNAPSHOT_SCHEMA_MISMATCH",
            "数据库快照版本与当前题库不一致。",
        ));
    }
    let database_uuid: String =
        sqlx::query_scalar("SELECT database_uuid FROM app_meta WHERE singleton_id = 1")
            .fetch_one(&mut connection)
            .await
            .map_err(CommandError::database)?;
    if database_uuid != expected_database_uuid {
        return Err(CommandError::new(
            "BACKUP_SNAPSHOT_IDENTITY_MISMATCH",
            "数据库快照身份与当前题库不一致。",
        ));
    }
    connection.close().await.map_err(CommandError::database)?;
    Ok(())
}

async fn write_ready_record(
    pool: &SqlitePool,
    managed_backup_dir: &Path,
    output_path: &Path,
    inspection: &BackupInspection,
    app_version: &str,
    backup_kind: &str,
) -> CommandResult<BackupRecordApi> {
    let id = Uuid::now_v7().to_string();
    let display_filename = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CommandError::validation("备份文件名无效。"))?
        .to_owned();
    let archive_rel_path = managed_relative_filename(managed_backup_dir, output_path);
    let created_at = i64::try_from(inspection.manifest.created_at_unix_ms).unwrap_or(i64::MAX);
    let archive_bytes = i64::try_from(inspection.archive_size_bytes)
        .map_err(|_| CommandError::validation("备份文件大小超出数据库记录范围。"))?;
    let archive_hash = decode_sha256(&inspection.archive_sha256_hex)?;
    let manifest_hash = decode_sha256(&inspection.manifest_sha256_hex)?;
    let role_counts = inspection.manifest.files.iter().fold(
        (0u64, 0u64, 0u64),
        |(database, resources, templates), item| match item.role {
            backup::BackupFileRole::Database => (database + 1, resources, templates),
            backup::BackupFileRole::Resource => (database, resources + 1, templates),
            backup::BackupFileRole::Template => (database, resources, templates + 1),
        },
    );
    let summary = json!({
        "payloadFileCount": inspection.payload_file_count,
        "payloadByteSize": inspection.payload_size_bytes,
        "databaseFiles": role_counts.0,
        "resourceFiles": role_counts.1,
        "templateFiles": role_counts.2,
    })
    .to_string();

    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    sqlx::query(
        "INSERT INTO backup_records (id, backup_kind, status, archive_rel_path, display_filename, \
         format_version, database_schema_version, source_database_uuid, source_app_version, \
         archive_sha256, manifest_sha256, archive_byte_size, contents_summary_json, created_at_ms, \
         completed_at_ms, last_verified_at_ms, error_code, error_message) \
         VALUES (?, ?, 'ready', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL)",
    )
    .bind(&id)
    .bind(backup_kind)
    .bind(&archive_rel_path)
    .bind(&display_filename)
    .bind(i64::from(inspection.manifest.version))
    .bind(i64::from(inspection.manifest.database_schema_version))
    .bind(&inspection.manifest.database_uuid)
    .bind(&inspection.manifest.application_version)
    .bind(&archive_hash)
    .bind(&manifest_hash)
    .bind(archive_bytes)
    .bind(&summary)
    .bind(created_at)
    .bind(created_at)
    .bind(created_at)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, entity_id, \
         outcome, summary, details_json, app_version) \
         VALUES (?, ?, 'info', 'backup.create', 'backup', ?, 'success', '创建并校验完整备份', ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(created_at)
    .bind(&id)
    .bind(json!({
        "backupKind": backup_kind,
        "displayFilename": display_filename,
        "managed": archive_rel_path.is_some(),
        "archiveByteSize": inspection.archive_size_bytes,
        "payloadFileCount": inspection.payload_file_count,
    }).to_string())
    .bind(app_version)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)?;

    let row = sqlx::query_as::<_, BackupRecordRow>(
        "SELECT id, backup_kind, status, archive_rel_path, display_filename, format_version, \
                database_schema_version, source_database_uuid, source_app_version, archive_sha256, \
                manifest_sha256, archive_byte_size, contents_summary_json, created_at_ms, \
                completed_at_ms, error_code, error_message FROM backup_records WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(CommandError::database)?;
    backup_record_api(row, managed_backup_dir)
}

fn backup_record_api(
    row: BackupRecordRow,
    managed_backup_dir: &Path,
) -> CommandResult<BackupRecordApi> {
    let summary: Value = serde_json::from_str(&row.contents_summary_json).map_err(|error| {
        CommandError::new(
            "BACKUP_CATALOG_CORRUPTED",
            format!("备份记录摘要无法读取：{error}"),
        )
    })?;
    Ok(BackupRecordApi {
        id: row.id,
        backup_kind: row.backup_kind,
        status: row.status,
        display_filename: row.display_filename,
        format_version: u32::try_from(row.format_version)
            .map_err(|_| CommandError::database("备份格式版本无效"))?,
        database_schema_version: u32::try_from(row.database_schema_version)
            .map_err(|_| CommandError::database("备份数据库版本无效"))?,
        source_database_uuid: row.source_database_uuid,
        source_app_version: row.source_app_version,
        archive_sha256_hex: stored_hash_hex(row.archive_sha256)?,
        manifest_sha256_hex: stored_hash_hex(row.manifest_sha256)?,
        archive_byte_size: row
            .archive_byte_size
            .map(u64::try_from)
            .transpose()
            .map_err(|_| CommandError::database("备份文件大小无效"))?,
        payload_file_count: summary
            .get("payloadFileCount")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        payload_byte_size: summary
            .get("payloadByteSize")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        created_at: row.created_at_ms,
        completed_at: row.completed_at_ms,
        file_available: row
            .archive_rel_path
            .as_deref()
            .map(|relative| managed_backup_file_available(managed_backup_dir, relative)),
        error_code: row.error_code,
        error_message: row.error_message,
    })
}

fn inspection_api(inspection: BackupInspection) -> BackupInspectionApi {
    BackupInspectionApi {
        format_version: inspection.manifest.version,
        created_at: i64::try_from(inspection.manifest.created_at_unix_ms).unwrap_or(i64::MAX),
        source_app_version: inspection.manifest.application_version,
        database_schema_version: inspection.manifest.database_schema_version,
        source_database_uuid: inspection.manifest.database_uuid,
        archive_sha256_hex: inspection.archive_sha256_hex,
        manifest_sha256_hex: inspection.manifest_sha256_hex,
        archive_byte_size: inspection.archive_size_bytes,
        payload_byte_size: inspection.payload_size_bytes,
        payload_file_count: inspection.payload_file_count as u64,
    }
}

fn managed_relative_filename(managed_backup_dir: &Path, output_path: &Path) -> Option<String> {
    let output_parent = output_path.parent()?;
    let managed = fs::canonicalize(managed_backup_dir).ok()?;
    let parent = fs::canonicalize(output_parent).ok()?;
    if parent != managed {
        return None;
    }
    let name = output_path.file_name()?.to_str()?;
    is_safe_backup_relative(name).then(|| name.to_owned())
}

fn managed_backup_file_available(managed_backup_dir: &Path, relative: &str) -> bool {
    if !is_safe_backup_relative(relative) {
        return false;
    }
    fs::symlink_metadata(managed_backup_dir.join(relative))
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn is_safe_backup_relative(value: &str) -> bool {
    let path = Path::new(value);
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tqb"))
}

fn decode_sha256(value: &str) -> CommandResult<Vec<u8>> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(CommandError::database("备份 SHA-256 格式无效"));
    }
    (0..64)
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| CommandError::database("备份 SHA-256 格式无效"))
        })
        .collect()
}

fn stored_hash_hex(value: Option<Vec<u8>>) -> CommandResult<Option<String>> {
    value
        .map(|bytes| {
            if bytes.len() != 32 {
                return Err(CommandError::new(
                    "BACKUP_CATALOG_CORRUPTED",
                    "备份记录中的 SHA-256 长度无效。",
                ));
            }
            Ok(lower_hex(&bytes))
        })
        .transpose()
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

fn backup_error(error: backup::BackupError) -> CommandError {
    CommandError::new("BACKUP_ARCHIVE_REJECTED", error.to_string())
}

#[cfg(test)]
mod automatic_backup_tests {
    use std::fs;

    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zhitiku-automatic-backup-{name}-{}",
            Uuid::now_v7().simple()
        ))
    }

    #[tokio::test]
    async fn automatic_backup_is_due_aware_and_prunes_only_automatic_records() {
        let root = test_root("retention");
        let database = Database::open(&root, "test").await.unwrap();
        sqlx::query(
            "UPDATE app_settings SET automatic_backup_interval_days = 1, \
             automatic_backup_retention_count = 1 WHERE singleton_id = 1",
        )
        .execute(database.pool())
        .await
        .unwrap();

        let manual_path = database.paths().backup_dir().join("manual-safety-copy.tqb");
        let manual = create_manual_backup(&database, manual_path.clone(), "test")
            .await
            .unwrap();
        assert_eq!(manual.record.backup_kind, "manual");
        assert!(manual_path.is_file());

        let first = run_automatic_backup_if_due(&database, "test")
            .await
            .unwrap();
        assert_eq!(first.outcome, "created");
        let first_record = first.record.unwrap();
        let first_path = database
            .paths()
            .backup_dir()
            .join(&first_record.display_filename);
        assert!(first_path.is_file());

        sqlx::query(
            "UPDATE backup_records SET completed_at_ms = 0 WHERE backup_kind = 'automatic'",
        )
        .execute(database.pool())
        .await
        .unwrap();
        let second = run_automatic_backup_if_due(&database, "test")
            .await
            .unwrap();
        assert_eq!(second.outcome, "created");
        assert_eq!(second.pruned_count, 1);
        assert!(!first_path.exists());
        assert!(manual_path.is_file(), "manual backups must never be pruned");

        let counts: (i64, i64) = sqlx::query_as(
            "SELECT SUM(backup_kind = 'automatic'), SUM(backup_kind = 'manual') \
             FROM backup_records WHERE status = 'ready'",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(counts, (1, 1));

        let not_due = run_automatic_backup_if_due(&database, "test")
            .await
            .unwrap();
        assert_eq!(not_due.outcome, "not_due");
        assert!(not_due.record.is_none());

        sqlx::query("UPDATE app_settings SET automatic_backup_enabled = 0 WHERE singleton_id = 1")
            .execute(database.pool())
            .await
            .unwrap();
        let disabled = run_automatic_backup_if_due(&database, "test")
            .await
            .unwrap();
        assert_eq!(disabled.outcome, "disabled");

        database.close().await;
        for attempt in 0..10 {
            match fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(_) if attempt < 9 => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean automatic backup test directory: {error}"),
            }
        }
    }
}
