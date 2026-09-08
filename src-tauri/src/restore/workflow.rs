use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    api::{
        AppState, backups,
        models::{
            BackupInspectionApi, BackupRecordApi, CommandError, CommandResult,
            RestoreDataSummaryApi, RestorePlanApi, RestoreResultApi, RestoreScheduledApi,
            ScheduleRestoreRequestApi,
        },
    },
    backup::{self, BackupInspection, BackupLimits},
    db::Database,
};

use super::{RestoreCandidateSummary, inspect_prepared_candidate, prepare_restore_candidate};

const RESTORE_STATE_VERSION: u32 = 1;
const RESTORE_PLAN_LIFETIME_MS: i64 = 30 * 60 * 1_000;
const MAX_RESTORE_STATE_BYTES: u64 = 2 * 1024 * 1024;
const COMPONENTS: [&str; 3] = ["database", "resources", "templates"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestorePlanFile {
    version: u32,
    operation_id: String,
    restore_token: String,
    created_at: i64,
    expires_at: i64,
    source_display_filename: String,
    archive_sha256_hex: String,
    source_database_uuid: String,
    incoming_summary: RestoreCandidateSummary,
    incoming_tree_sha256_hex: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActiveRestoreJournal {
    version: u32,
    operation_id: String,
    restore_token: String,
    scheduled_at: i64,
    source_database_uuid: String,
    archive_sha256_hex: String,
    incoming_summary: RestoreCandidateSummary,
    incoming_tree_sha256_hex: String,
    pre_restore_backup: Option<BackupRecordApi>,
    rescue_mode: bool,
}

struct OperationCleanup {
    path: PathBuf,
    active: bool,
}

impl OperationCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for OperationCleanup {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub async fn prepare_restore(
    state: &AppState,
    backup_path: PathBuf,
) -> CommandResult<RestorePlanApi> {
    let original_inspection = inspect_archive(backup_path.clone()).await?;
    let operation_id = Uuid::now_v7().to_string();
    let restore_token = operation_id.clone();
    let operation_dir = create_operation_dir(state.data_root(), &operation_id)?;
    let mut cleanup = OperationCleanup::new(operation_dir.clone());
    let controlled_archive = operation_dir.join("source.tqb");
    copy_controlled_archive(&backup_path, &controlled_archive, &original_inspection).await?;
    let controlled_inspection = inspect_archive(controlled_archive.clone()).await?;
    if controlled_inspection.archive_sha256_hex != original_inspection.archive_sha256_hex
        || controlled_inspection.manifest_sha256_hex != original_inspection.manifest_sha256_hex
    {
        return Err(CommandError::new(
            "RESTORE_ARCHIVE_CHANGED",
            "备份文件在复制到安全暂存区时发生变化，已停止恢复。",
        ));
    }

    let incoming_dir = operation_dir.join("incoming");
    fs::create_dir(&incoming_dir).map_err(|error| {
        CommandError::new(
            "RESTORE_STAGING_CREATE_FAILED",
            format!("无法创建恢复暂存目录：{error}"),
        )
    })?;
    let archive_for_extract = controlled_archive.clone();
    let incoming_for_extract = incoming_dir.clone();
    let extracted = tokio::task::spawn_blocking(move || {
        backup::extract_backup(
            &archive_for_extract,
            &incoming_for_extract,
            &BackupLimits::default(),
        )
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "RESTORE_EXTRACT_TASK_FAILED",
            format!("恢复解包任务未能正常完成：{error}"),
        )
    })?
    .map_err(backup_error)?;
    let incoming_summary = prepare_restore_candidate(&extracted, state.app_version()).await?;
    let incoming_tree_sha256_hex = hash_candidate_tree(incoming_dir.clone()).await?;
    let current_summary = match state.database().await {
        Ok(database) => Some(database_summary(&database).await?),
        Err(_) => None,
    };
    let now = now_millis()?;
    let expires_at = now.saturating_add(RESTORE_PLAN_LIFETIME_MS);
    let plan = RestorePlanFile {
        version: RESTORE_STATE_VERSION,
        operation_id: operation_id.clone(),
        restore_token: restore_token.clone(),
        created_at: now,
        expires_at,
        source_display_filename: display_filename(&backup_path)?,
        archive_sha256_hex: controlled_inspection.archive_sha256_hex.clone(),
        source_database_uuid: controlled_inspection.manifest.database_uuid.clone(),
        incoming_summary: incoming_summary.clone(),
        incoming_tree_sha256_hex,
    };
    write_json_new(&operation_dir.join("plan.json"), &plan)?;
    cleanup.disarm();

    let mut warnings = vec![
        "确认后软件会先创建当前数据的完整恢复前备份，再自动重启并切换数据。".to_owned(),
        "恢复成功后，原数据库、模板和资源目录仍会保留，不会自动删除。".to_owned(),
    ];
    if current_summary.is_none() {
        warnings.push(
            "当前数据库无法读取，因此不能生成标准 .tqb 恢复前备份；救援恢复会把现有数据库、模板和资源目录原样隔离保留，再切换到已验证的备份。"
                .to_owned(),
        );
    }
    let current_database_healthy = current_summary.is_some();
    Ok(RestorePlanApi {
        restore_token,
        expires_at,
        source_display_filename: plan.source_display_filename,
        archive_inspection: inspection_api(&controlled_inspection),
        current_summary: current_summary.map(summary_api),
        incoming_summary: summary_api(incoming_summary),
        current_database_healthy,
        warnings,
        automatic_pre_restore_backup_required: current_database_healthy,
        requires_restart: true,
    })
}

pub async fn cancel_restore(state: &AppState, restore_token: &str) -> CommandResult<()> {
    let operation_id = canonical_token(restore_token)?;
    let _maintenance = state.exclusive_maintenance_lease().await;
    let restore_root = ensure_restore_root(state.data_root())?;
    let active_path = restore_root.join("active.json");
    if active_path.exists() {
        return Err(CommandError::new(
            "RESTORE_ALREADY_SCHEDULED",
            "恢复已经进入重启切换阶段，不能再取消。",
        ));
    }
    let operation_dir = checked_operation_dir(&restore_root, &operation_id)?;
    if operation_dir.join("old").exists()
        || operation_dir.join("failed-new").exists()
        || operation_dir.join("terminal.json").exists()
    {
        return Err(CommandError::new(
            "RESTORE_ALREADY_FINISHED",
            "恢复已经执行，保留的旧数据不能通过取消操作删除。",
        ));
    }
    let plan: RestorePlanFile = read_json(&operation_dir.join("plan.json"))?;
    validate_plan_identity(&plan, &operation_id)?;
    remove_owned_operation_dir(&restore_root, &operation_dir)
}

pub async fn schedule_restore(
    state: &AppState,
    request: &ScheduleRestoreRequestApi,
) -> CommandResult<RestoreScheduledApi> {
    if !request.confirmed_current_data_replacement {
        return Err(CommandError::validation(
            "必须明确确认当前题库数据将被备份内容替换。",
        ));
    }
    let operation_id = canonical_token(&request.restore_token)?;
    if !valid_sha256(&request.expected_archive_sha256_hex) {
        return Err(CommandError::validation("恢复确认中的归档 SHA-256 无效。"));
    }
    let maintenance = state.restore_maintenance_lease().await;
    let restore_root = ensure_restore_root(state.data_root())?;
    if restore_root.join("last-result.json").exists() {
        return Err(CommandError::new(
            "RESTORE_RESULT_PENDING_ACKNOWLEDGEMENT",
            "请先查看并确认上一次恢复结果，再安排新的恢复。",
        ));
    }
    let operation_dir = checked_operation_dir(&restore_root, &operation_id)?;
    let plan: RestorePlanFile = read_json(&operation_dir.join("plan.json"))?;
    validate_plan_identity(&plan, &operation_id)?;
    if now_millis()? > plan.expires_at {
        return Err(CommandError::new(
            "RESTORE_PLAN_EXPIRED",
            "恢复确认已过期，请重新选择并校验备份文件。",
        ));
    }
    if request.expected_archive_sha256_hex != plan.archive_sha256_hex {
        return Err(CommandError::new(
            "RESTORE_CONFIRMATION_MISMATCH",
            "恢复确认的备份摘要与预检结果不一致。",
        ));
    }

    let active_path = restore_root.join("active.json");
    if active_path.exists() {
        return Err(CommandError::new(
            "RESTORE_ALREADY_SCHEDULED",
            "已经有一项恢复等待重启执行。",
        ));
    }
    let controlled_archive = operation_dir.join("source.tqb");
    let inspection = inspect_archive(controlled_archive).await?;
    if inspection.archive_sha256_hex != plan.archive_sha256_hex
        || inspection.manifest.database_uuid != plan.source_database_uuid
    {
        return Err(CommandError::new(
            "RESTORE_ARCHIVE_CHANGED",
            "受控备份副本在确认后发生变化，已停止恢复。",
        ));
    }
    let summary =
        inspect_prepared_candidate(&operation_dir.join("incoming"), &plan.source_database_uuid)
            .await?;
    if summary != plan.incoming_summary {
        return Err(CommandError::new(
            "RESTORE_CANDIDATE_CHANGED",
            "恢复候选内容在确认后发生变化，已停止恢复。",
        ));
    }
    let tree_sha256 = hash_candidate_tree(operation_dir.join("incoming")).await?;
    if tree_sha256 != plan.incoming_tree_sha256_hex {
        return Err(CommandError::new(
            "RESTORE_CANDIDATE_CHANGED",
            "恢复候选文件在确认后发生变化，已停止恢复。",
        ));
    }

    let timestamp = now_millis()?;
    let rescue_mode = maintenance.database().is_none();
    let pre_restore_backup = if let Some(database) = maintenance.database() {
        let filename = format!(
            "TK试题题库_恢复前_{}_{}.tqb",
            timestamp,
            Uuid::now_v7().simple()
        );
        let path = database.paths().backup_dir().join(&filename);
        Some(
            backups::create_pre_restore_backup(database, path, state.app_version())
                .await?
                .record,
        )
    } else {
        for name in COMPONENTS {
            let _ = direct_real_child(state.data_root(), name, "待救援的当前数据目录")?;
        }
        None
    };
    let pre_restore_filename = pre_restore_backup
        .as_ref()
        .map(|record| record.display_filename.clone());
    let journal = ActiveRestoreJournal {
        version: RESTORE_STATE_VERSION,
        operation_id: operation_id.clone(),
        restore_token: request.restore_token.clone(),
        scheduled_at: timestamp,
        source_database_uuid: plan.source_database_uuid,
        archive_sha256_hex: plan.archive_sha256_hex,
        incoming_summary: plan.incoming_summary,
        incoming_tree_sha256_hex: plan.incoming_tree_sha256_hex,
        pre_restore_backup,
        rescue_mode,
    };
    write_json_new(&active_path, &journal)?;
    state
        .hold_restore_maintenance_until_restart(maintenance)
        .await;
    Ok(RestoreScheduledApi {
        operation_id,
        pre_restore_backup_filename: pre_restore_filename,
        requires_restart: true,
    })
}

pub async fn apply_pending_restore(data_root: &Path) -> CommandResult<Option<RestoreResultApi>> {
    let restore_root = match existing_restore_root(data_root)? {
        Some(path) => path,
        None => return Ok(None),
    };
    let active_path = restore_root.join("active.json");
    if !active_path.exists() {
        return Ok(None);
    }
    let journal: ActiveRestoreJournal = read_json(&active_path)?;
    if journal.version != RESTORE_STATE_VERSION {
        return Err(CommandError::new(
            "RESTORE_JOURNAL_VERSION_UNSUPPORTED",
            "待执行恢复使用了当前软件不支持的状态版本。",
        ));
    }
    let operation_id = canonical_token(&journal.operation_id)?;
    if journal.restore_token != operation_id {
        return Err(CommandError::new(
            "RESTORE_JOURNAL_INVALID",
            "待执行恢复的身份字段不一致。",
        ));
    }
    let operation_dir = checked_operation_dir(&restore_root, &operation_id)?;
    let terminal_path = operation_dir.join("terminal.json");
    if terminal_path.exists() {
        let result: RestoreResultApi = read_json(&terminal_path)?;
        if result.operation_id != operation_id {
            return Err(CommandError::new(
                "RESTORE_TERMINAL_INVALID",
                "恢复终态记录与待执行恢复身份不一致。",
            ));
        }
        persist_last_restore_result(&restore_root, &result)?;
        remove_file_if_exists(&active_path, "无法清除已完成的恢复标记")?;
        return Ok(Some(result));
    }
    if interrupted_switch_started(&operation_dir)? {
        let started_at = now_millis()?;
        rollback_operation(data_root, &operation_dir).map_err(|rollback_error| {
            CommandError::new(
                "RESTORE_INTERRUPTED_ROLLBACK_FAILED",
                format!(
                    "检测到上次恢复在目录切换中中断，自动回滚失败：{}。新旧目录均保持原状，软件不会创建空数据库。",
                    rollback_error.message
                ),
            )
        })?;
        let result = RestoreResultApi {
            operation_id: operation_id.clone(),
            outcome: "rolled_back".to_owned(),
            restored_database_uuid: None,
            pre_restore_backup_filename: journal
                .pre_restore_backup
                .as_ref()
                .map(|record| record.display_filename.clone()),
            summary: None,
            duration_ms: now_millis()?.saturating_sub(started_at),
            warnings: vec!["上次恢复在完成切换前被中断；软件启动时已优先恢复原数据。".to_owned()],
            error_message: Some("恢复过程被异常关机或进程中断，未启用备份中的数据。".to_owned()),
        };
        write_json_new(&terminal_path, &result)?;
        persist_last_restore_result(&restore_root, &result)?;
        remove_file_if_exists(&active_path, "无法清除已回滚的恢复标记")?;
        return Ok(Some(result));
    }
    validate_active_restore(data_root, &operation_dir, &journal).await?;
    let started_at = now_millis()?;
    let switch_result = switch_operation(data_root, &operation_dir, &journal).await;
    let duration_ms = now_millis()?.saturating_sub(started_at);
    let result = match switch_result {
        Ok(summary) => {
            let mut warnings = vec![format!(
                "恢复前的原始数据目录保留在 {}。",
                operation_dir.join("old").display()
            )];
            if journal.rescue_mode {
                warnings.push(
                    "本次为损坏数据库救援恢复：无法生成标准恢复前备份，切换前的全部数据目录已原样隔离保留。"
                        .to_owned(),
                );
            } else if let Err(error) =
                register_pre_restore_backup(data_root, &journal, &operation_id).await
            {
                warnings.push(format!(
                    "恢复已成功，但恢复前备份未能显示在备份历史中：{}",
                    error.message
                ));
            }
            RestoreResultApi {
                operation_id: operation_id.clone(),
                outcome: "success".to_owned(),
                restored_database_uuid: Some(journal.source_database_uuid.clone()),
                pre_restore_backup_filename: journal
                    .pre_restore_backup
                    .as_ref()
                    .map(|record| record.display_filename.clone()),
                summary: Some(summary_api(summary)),
                duration_ms,
                warnings,
                error_message: None,
            }
        }
        Err(error) => {
            let rollback = rollback_operation(data_root, &operation_dir);
            match rollback {
                Ok(()) => RestoreResultApi {
                    operation_id: operation_id.clone(),
                    outcome: "rolled_back".to_owned(),
                    restored_database_uuid: None,
                    pre_restore_backup_filename: journal
                        .pre_restore_backup
                        .as_ref()
                        .map(|record| record.display_filename.clone()),
                    summary: None,
                    duration_ms,
                    warnings: vec![
                        "恢复切换失败，软件已把切换前的数据目录恢复到原位置。".to_owned(),
                    ],
                    error_message: Some(error.message),
                },
                Err(rollback_error) => {
                    return Err(CommandError::new(
                        "RESTORE_ROLLBACK_FAILED",
                        format!(
                            "恢复切换失败（{}），自动回滚也失败（{}）。所有新旧目录均保留在“{}”，软件不会创建空数据库。",
                            error.message,
                            rollback_error.message,
                            operation_dir.display()
                        ),
                    ));
                }
            }
        }
    };
    write_json_new(&terminal_path, &result)?;
    persist_last_restore_result(&restore_root, &result)?;
    remove_file_if_exists(&active_path, "恢复已经结束，但无法清除待执行标记")?;
    Ok(Some(result))
}

pub fn get_last_restore_result(data_root: &Path) -> CommandResult<Option<RestoreResultApi>> {
    let Some(root) = existing_restore_root(data_root)? else {
        return Ok(None);
    };
    let path = root.join("last-result.json");
    if !path.exists() {
        return Ok(None);
    }
    read_json(&path).map(Some)
}

pub fn acknowledge_restore_result(data_root: &Path, operation_id: &str) -> CommandResult<()> {
    let operation_id = canonical_token(operation_id)?;
    let Some(root) = existing_restore_root(data_root)? else {
        return Ok(());
    };
    let path = root.join("last-result.json");
    if !path.exists() {
        return Ok(());
    }
    let result: RestoreResultApi = read_json(&path)?;
    if result.operation_id != operation_id {
        return Err(CommandError::validation("恢复结果身份与确认请求不一致。"));
    }
    fs::remove_file(path).map_err(|error| {
        CommandError::new(
            "RESTORE_RESULT_CLEANUP_FAILED",
            format!("无法清除已经确认的恢复结果：{error}"),
        )
    })
}

async fn validate_active_restore(
    data_root: &Path,
    operation_dir: &Path,
    journal: &ActiveRestoreJournal,
) -> CommandResult<()> {
    let plan: RestorePlanFile = read_json(&operation_dir.join("plan.json"))?;
    validate_plan_identity(&plan, &journal.operation_id)?;
    if plan.archive_sha256_hex != journal.archive_sha256_hex
        || plan.source_database_uuid != journal.source_database_uuid
        || plan.incoming_summary != journal.incoming_summary
        || plan.incoming_tree_sha256_hex != journal.incoming_tree_sha256_hex
    {
        return Err(CommandError::new(
            "RESTORE_JOURNAL_MISMATCH",
            "恢复计划与重启切换记录不一致，已停止恢复。",
        ));
    }

    let controlled = inspect_archive(operation_dir.join("source.tqb")).await?;
    if controlled.archive_sha256_hex != journal.archive_sha256_hex
        || controlled.manifest.database_uuid != journal.source_database_uuid
    {
        return Err(CommandError::new(
            "RESTORE_ARCHIVE_CHANGED",
            "受控备份副本在重启前发生变化，已停止恢复。",
        ));
    }

    let incoming = direct_real_child(operation_dir, "incoming", "恢复候选根目录")?;
    let tree_sha256 = hash_candidate_tree(incoming.clone()).await?;
    if tree_sha256 != journal.incoming_tree_sha256_hex {
        return Err(CommandError::new(
            "RESTORE_CANDIDATE_CHANGED",
            "恢复候选文件与确认时的完整摘要不一致，已停止恢复。",
        ));
    }
    let summary = inspect_prepared_candidate(&incoming, &journal.source_database_uuid).await?;
    if summary != journal.incoming_summary {
        return Err(CommandError::new(
            "RESTORE_CANDIDATE_CHANGED",
            "恢复候选数据库摘要与确认时不一致，已停止恢复。",
        ));
    }
    match (&journal.pre_restore_backup, journal.rescue_mode) {
        (Some(record), false) => verify_pre_restore_backup(data_root, record).await,
        (None, true) => {
            for name in COMPONENTS {
                let _ = direct_real_child(data_root, name, "待救援的当前数据目录")?;
            }
            Ok(())
        }
        _ => Err(CommandError::new(
            "RESTORE_SAFETY_MODE_INVALID",
            "恢复安全备份状态与救援模式不一致。",
        )),
    }
}

async fn register_pre_restore_backup(
    data_root: &Path,
    journal: &ActiveRestoreJournal,
    operation_id: &str,
) -> CommandResult<()> {
    let record = journal.pre_restore_backup.as_ref().ok_or_else(|| {
        CommandError::new("RESTORE_BACKUP_RECORD_MISSING", "恢复前备份记录不存在。")
    })?;
    let database = Database::open(data_root.to_path_buf(), &record.source_app_version)
        .await
        .map_err(CommandError::database)?;
    let result =
        backups::register_pre_restore_backup_after_restore(&database, record, operation_id).await;
    database.close().await;
    result
}

async fn verify_pre_restore_backup(
    data_root: &Path,
    record: &BackupRecordApi,
) -> CommandResult<()> {
    if record.backup_kind != "pre_restore"
        || record.status != "ready"
        || record.file_available != Some(true)
    {
        return Err(CommandError::new(
            "RESTORE_SAFETY_BACKUP_INVALID",
            "恢复前安全备份记录不是可用的完整备份。",
        ));
    }
    let name_path = Path::new(&record.display_filename);
    if name_path.file_name().and_then(|name| name.to_str()) != Some(&record.display_filename)
        || name_path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
    {
        return Err(CommandError::new(
            "RESTORE_SAFETY_BACKUP_PATH_INVALID",
            "恢复前安全备份文件名包含不安全路径。",
        ));
    }
    let backup_dir = direct_real_child(data_root, "backup", "备份目录")?;
    let backup_path = backup_dir.join(&record.display_filename);
    let metadata = fs::symlink_metadata(&backup_path).map_err(|error| {
        CommandError::new(
            "RESTORE_SAFETY_BACKUP_MISSING",
            format!("恢复前安全备份不存在：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CommandError::new(
            "RESTORE_SAFETY_BACKUP_INVALID",
            "恢复前安全备份不是安全的普通文件。",
        ));
    }
    let inspection = inspect_archive(backup_path).await?;
    let archive_hash = record.archive_sha256_hex.as_deref().ok_or_else(|| {
        CommandError::new(
            "RESTORE_SAFETY_BACKUP_INVALID",
            "恢复前安全备份缺少归档摘要。",
        )
    })?;
    let manifest_hash = record.manifest_sha256_hex.as_deref().ok_or_else(|| {
        CommandError::new(
            "RESTORE_SAFETY_BACKUP_INVALID",
            "恢复前安全备份缺少清单摘要。",
        )
    })?;
    if inspection.archive_sha256_hex != archive_hash
        || inspection.manifest_sha256_hex != manifest_hash
        || Some(inspection.archive_size_bytes) != record.archive_byte_size
        || inspection.manifest.database_uuid != record.source_database_uuid
        || inspection.manifest.version != record.format_version
        || inspection.manifest.database_schema_version != record.database_schema_version
    {
        return Err(CommandError::new(
            "RESTORE_SAFETY_BACKUP_CHANGED",
            "恢复前安全备份与创建完成时的摘要不一致，已停止数据切换。",
        ));
    }
    Ok(())
}

fn persist_last_restore_result(root: &Path, result: &RestoreResultApi) -> CommandResult<()> {
    let path = root.join("last-result.json");
    if path.exists() {
        let existing: RestoreResultApi = read_json(&path)?;
        if existing.operation_id == result.operation_id && existing.outcome == result.outcome {
            return Ok(());
        }
        return Err(CommandError::new(
            "RESTORE_RESULT_CONFLICT",
            "已有另一项尚未确认的恢复结果，当前恢复终态仍安全保留。",
        ));
    }
    write_json_new(&path, result)
}

fn remove_file_if_exists(path: &Path, context: &str) -> CommandResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CommandError::new(
            "RESTORE_JOURNAL_CLEANUP_FAILED",
            format!("{context}：{error}"),
        )),
    }
}

fn interrupted_switch_started(operation_dir: &Path) -> CommandResult<bool> {
    let old_path = operation_dir.join("old");
    match fs::symlink_metadata(&old_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(CommandError::new(
                "RESTORE_OLD_DIRECTORY_INVALID",
                format!("无法检查原数据保留目录：{error}"),
            ));
        }
        Ok(_) => {}
    }
    let old = direct_real_child(operation_dir, "old", "原数据保留目录")?;
    for name in COMPONENTS {
        match fs::symlink_metadata(old.join(name)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CommandError::new(
                    "RESTORE_OLD_DIRECTORY_INVALID",
                    format!("无法检查已保留的原数据组件“{name}”：{error}"),
                ));
            }
        }
    }
    Ok(false)
}

async fn switch_operation(
    data_root: &Path,
    operation_dir: &Path,
    journal: &ActiveRestoreJournal,
) -> CommandResult<RestoreCandidateSummary> {
    let incoming = direct_real_child(operation_dir, "incoming", "恢复候选根目录")?;
    let old = operation_dir.join("old");
    let switch_started = COMPONENTS.iter().any(|name| old.join(name).exists());
    if !switch_started {
        inspect_prepared_candidate(&incoming, &journal.source_database_uuid).await?;
        if old.exists() {
            require_real_directory(&old, "原数据保留目录")?;
        } else {
            fs::create_dir(&old).map_err(|error| {
                CommandError::new(
                    "RESTORE_OLD_DIRECTORY_CREATE_FAILED",
                    format!("无法创建原数据保留目录：{error}"),
                )
            })?;
        }
    } else {
        require_real_directory(&old, "原数据保留目录")?;
    }
    let old = direct_real_child(operation_dir, "old", "原数据保留目录")?;

    for name in COMPONENTS {
        let retained = old.join(name);
        if retained.exists() {
            let _ = direct_real_child(&old, name, "已保留的原数据目录")?;
            continue;
        }
        let live = direct_real_child(data_root, name, "当前数据目录")?;
        rename_with_retry(&live, &retained)?;
    }
    for name in COMPONENTS {
        let live = data_root.join(name);
        let candidate = incoming.join(name);
        if live.exists() {
            let _ = direct_real_child(data_root, name, "切换后的正式数据目录")?;
            if candidate.exists() {
                return Err(CommandError::new(
                    "RESTORE_SWITCH_STATE_AMBIGUOUS",
                    format!("恢复组件“{name}”同时出现在正式目录和暂存目录。"),
                ));
            }
            continue;
        }
        let candidate = direct_real_child(&incoming, name, "恢复候选目录")?;
        rename_with_retry(&candidate, &live)?;
    }
    inspect_prepared_candidate(data_root, &journal.source_database_uuid).await
}

fn rollback_operation(data_root: &Path, operation_dir: &Path) -> CommandResult<()> {
    let old_path = operation_dir.join("old");
    if !old_path.exists() {
        for name in COMPONENTS {
            let _ = direct_real_child(data_root, name, "未切换的正式数据目录")?;
        }
        return Ok(());
    }
    let old = direct_real_child(operation_dir, "old", "原数据保留目录")?;
    let failed_new = operation_dir.join("failed-new");
    if !failed_new.exists() {
        fs::create_dir(&failed_new).map_err(|error| {
            CommandError::new(
                "RESTORE_ROLLBACK_STAGING_FAILED",
                format!("无法创建失败候选保留目录：{error}"),
            )
        })?;
    }
    let failed_new = direct_real_child(operation_dir, "failed-new", "失败候选保留目录")?;
    for name in COMPONENTS.into_iter().rev() {
        let live = data_root.join(name);
        let retained = old.join(name);
        if !retained.exists() {
            if !live.exists() {
                return Err(CommandError::new(
                    "RESTORE_ROLLBACK_SOURCE_MISSING",
                    format!("恢复前的“{name}”目录和正式目录都不存在。"),
                ));
            }
            let _ = direct_real_child(data_root, name, "回滚后的正式数据目录")?;
            continue;
        }
        let retained = direct_real_child(&old, name, "待回滚的原数据目录")?;
        if live.exists() {
            let live = direct_real_child(data_root, name, "失败的恢复候选目录")?;
            let failed = failed_new.join(name);
            if failed.exists() {
                return Err(CommandError::new(
                    "RESTORE_ROLLBACK_TARGET_EXISTS",
                    format!("失败候选保留位置“{name}”已存在。"),
                ));
            }
            rename_with_retry(&live, &failed)?;
        }
        rename_with_retry(&retained, &live)?;
    }
    Ok(())
}

async fn database_summary(database: &Database) -> CommandResult<RestoreCandidateSummary> {
    let question_count = count_query(
        database,
        "SELECT COUNT(*) FROM questions WHERE deleted_at_ms IS NULL",
    )
    .await?;
    let paper_count = count_query(
        database,
        "SELECT COUNT(*) FROM papers WHERE paper_status = 'saved'",
    )
    .await?;
    let template_count = count_query(database, "SELECT COUNT(*) FROM word_templates").await?;
    let resource_count = count_query(
        database,
        "SELECT COUNT(*) FROM resources WHERE availability_status = 'ready'",
    )
    .await?;
    Ok(RestoreCandidateSummary {
        database_uuid: database
            .database_uuid()
            .await
            .map_err(CommandError::database)?,
        database_schema_version: database
            .schema_version()
            .await
            .map_err(CommandError::database)?,
        question_count,
        paper_count,
        template_count,
        resource_count,
    })
}

async fn count_query(database: &Database, sql: &str) -> CommandResult<u64> {
    let value: i64 = sqlx::query_scalar(sql)
        .fetch_one(database.pool())
        .await
        .map_err(CommandError::database)?;
    u64::try_from(value).map_err(|_| CommandError::database("恢复统计数量无效"))
}

async fn inspect_archive(path: PathBuf) -> CommandResult<BackupInspection> {
    tokio::task::spawn_blocking(move || backup::inspect_backup(&path, &BackupLimits::default()))
        .await
        .map_err(|error| {
            CommandError::new(
                "RESTORE_INSPECTION_TASK_FAILED",
                format!("恢复备份检查任务未能正常完成：{error}"),
            )
        })?
        .map_err(backup_error)
}

async fn copy_controlled_archive(
    source: &Path,
    destination: &Path,
    expected: &BackupInspection,
) -> CommandResult<()> {
    let source = source.to_path_buf();
    let destination = destination.to_path_buf();
    let worker_destination = destination.clone();
    let expected_hash = expected.archive_sha256_hex.clone();
    tokio::task::spawn_blocking(move || {
        let partial = worker_destination.with_extension("partial");
        if worker_destination.exists() || partial.exists() {
            return Err(CommandError::new(
                "RESTORE_CONTROLLED_COPY_EXISTS",
                "恢复受控副本位置意外已存在。",
            ));
        }
        fs::copy(&source, &partial).map_err(|error| {
            CommandError::new(
                "RESTORE_ARCHIVE_COPY_FAILED",
                format!("无法复制备份到安全暂存区：{error}"),
            )
        })?;
        sync_file(&partial)?;
        fs::rename(&partial, &worker_destination).map_err(|error| {
            CommandError::new(
                "RESTORE_ARCHIVE_COPY_FAILED",
                format!("无法提交备份受控副本：{error}"),
            )
        })?;
        Ok(())
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "RESTORE_COPY_TASK_FAILED",
            format!("恢复备份复制任务未能正常完成：{error}"),
        )
    })??;
    let copied = inspect_archive(destination).await?;
    if copied.archive_sha256_hex != expected_hash {
        return Err(CommandError::new(
            "RESTORE_ARCHIVE_COPY_MISMATCH",
            "恢复受控副本与选择的备份文件摘要不一致。",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct CandidateTreeEntry {
    relative_path: String,
    absolute_path: PathBuf,
    is_directory: bool,
}

async fn hash_candidate_tree(root: PathBuf) -> CommandResult<String> {
    tokio::task::spawn_blocking(move || hash_candidate_tree_blocking(&root))
        .await
        .map_err(|error| {
            CommandError::new(
                "RESTORE_CANDIDATE_HASH_TASK_FAILED",
                format!("恢复候选摘要任务未能正常完成：{error}"),
            )
        })?
}

fn hash_candidate_tree_blocking(root: &Path) -> CommandResult<String> {
    require_real_directory(root, "恢复候选根目录")?;
    let mut entries = Vec::new();
    collect_candidate_tree(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let mut hasher = Sha256::new();
    hasher.update(b"zhitiku-restore-candidate-tree-v1\0");
    let mut buffer = vec![0u8; 128 * 1024];
    for entry in entries {
        hasher.update(if entry.is_directory { b"D\0" } else { b"F\0" });
        let path_bytes = entry.relative_path.as_bytes();
        hasher.update((path_bytes.len() as u64).to_le_bytes());
        hasher.update(path_bytes);
        if entry.is_directory {
            continue;
        }
        let before =
            fs::symlink_metadata(&entry.absolute_path).map_err(io_error("无法检查恢复候选文件"))?;
        if before.file_type().is_symlink() || !before.is_file() {
            return Err(CommandError::new(
                "RESTORE_CANDIDATE_FILE_INVALID",
                "恢复候选摘要中出现了非普通文件。",
            ));
        }
        hasher.update(before.len().to_le_bytes());
        let mut file =
            File::open(&entry.absolute_path).map_err(io_error("无法读取恢复候选文件"))?;
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(io_error("无法读取恢复候选文件"))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        let after =
            fs::symlink_metadata(&entry.absolute_path).map_err(io_error("无法复检恢复候选文件"))?;
        if after.file_type().is_symlink() || !after.is_file() || after.len() != before.len() {
            return Err(CommandError::new(
                "RESTORE_CANDIDATE_CHANGED",
                "恢复候选文件在生成摘要时发生变化。",
            ));
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_candidate_tree(
    root: &Path,
    directory: &Path,
    output: &mut Vec<CandidateTreeEntry>,
) -> CommandResult<()> {
    require_real_directory(directory, "恢复候选目录")?;
    let mut children = fs::read_dir(directory)
        .map_err(io_error("无法读取恢复候选目录"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io_error("无法枚举恢复候选目录"))?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let metadata = fs::symlink_metadata(&path).map_err(io_error("无法检查恢复候选条目"))?;
        if metadata.file_type().is_symlink() {
            return Err(CommandError::new(
                "RESTORE_CANDIDATE_SYMLINK_REJECTED",
                "恢复候选目录包含符号链接或重解析路径。",
            ));
        }
        let relative = path.strip_prefix(root).map_err(|_| {
            CommandError::new(
                "RESTORE_CANDIDATE_PATH_INVALID",
                "恢复候选条目超出受控根目录。",
            )
        })?;
        let relative_path = relative
            .components()
            .map(|component| {
                component.as_os_str().to_str().ok_or_else(|| {
                    CommandError::new(
                        "RESTORE_CANDIDATE_PATH_INVALID",
                        "恢复候选路径包含无法安全记录的字符。",
                    )
                })
            })
            .collect::<CommandResult<Vec<_>>>()?
            .join("/");
        if metadata.is_dir() {
            output.push(CandidateTreeEntry {
                relative_path,
                absolute_path: path.clone(),
                is_directory: true,
            });
            collect_candidate_tree(root, &path, output)?;
        } else if metadata.is_file() {
            output.push(CandidateTreeEntry {
                relative_path,
                absolute_path: path,
                is_directory: false,
            });
        } else {
            return Err(CommandError::new(
                "RESTORE_CANDIDATE_FILE_INVALID",
                "恢复候选目录包含不支持的文件类型。",
            ));
        }
    }
    Ok(())
}

fn create_operation_dir(data_root: &Path, operation_id: &str) -> CommandResult<PathBuf> {
    let restore_root = ensure_restore_root(data_root)?;
    let operation_dir = restore_root.join(operation_id);
    fs::create_dir(&operation_dir).map_err(|error| {
        CommandError::new(
            "RESTORE_OPERATION_CREATE_FAILED",
            format!("无法创建恢复工作目录：{error}"),
        )
    })?;
    require_real_directory(&operation_dir, "恢复工作目录")?;
    Ok(operation_dir)
}

fn ensure_restore_root(data_root: &Path) -> CommandResult<PathBuf> {
    let root = data_root.join(".restore");
    fs::create_dir_all(&root).map_err(|error| {
        CommandError::new(
            "RESTORE_ROOT_CREATE_FAILED",
            format!("无法创建恢复维护目录：{error}"),
        )
    })?;
    require_real_directory(&root, "恢复维护目录")?;
    Ok(root)
}

fn existing_restore_root(data_root: &Path) -> CommandResult<Option<PathBuf>> {
    let root = data_root.join(".restore");
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(Some(root)),
        Ok(_) => Err(CommandError::new(
            "RESTORE_ROOT_INVALID",
            "恢复维护路径不是安全的真实目录。",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CommandError::new(
            "RESTORE_ROOT_INVALID",
            format!("无法检查恢复维护目录：{error}"),
        )),
    }
}

fn checked_operation_dir(root: &Path, operation_id: &str) -> CommandResult<PathBuf> {
    let path = root.join(operation_id);
    require_real_directory(&path, "恢复工作目录")?;
    let canonical_root = fs::canonicalize(root).map_err(io_error("无法规范化恢复维护目录"))?;
    let canonical_path = fs::canonicalize(&path).map_err(io_error("无法规范化恢复工作目录"))?;
    if canonical_path.parent() != Some(canonical_root.as_path()) {
        return Err(CommandError::new(
            "RESTORE_OPERATION_PATH_INVALID",
            "恢复工作目录超出受控维护目录。",
        ));
    }
    Ok(canonical_path)
}

fn remove_owned_operation_dir(root: &Path, operation_dir: &Path) -> CommandResult<()> {
    let canonical_root = fs::canonicalize(root).map_err(io_error("无法规范化恢复维护目录"))?;
    let canonical_operation =
        fs::canonicalize(operation_dir).map_err(io_error("无法规范化恢复工作目录"))?;
    if canonical_operation.parent() != Some(canonical_root.as_path()) {
        return Err(CommandError::new(
            "RESTORE_OPERATION_PATH_INVALID",
            "拒绝删除受控恢复目录之外的路径。",
        ));
    }
    fs::remove_dir_all(canonical_operation).map_err(|error| {
        CommandError::new(
            "RESTORE_OPERATION_CLEANUP_FAILED",
            format!("无法清理恢复暂存数据：{error}"),
        )
    })
}

fn validate_plan_identity(plan: &RestorePlanFile, operation_id: &str) -> CommandResult<()> {
    if plan.version != RESTORE_STATE_VERSION
        || plan.operation_id != operation_id
        || plan.restore_token != operation_id
    {
        return Err(CommandError::new(
            "RESTORE_PLAN_INVALID",
            "恢复计划身份或版本无效。",
        ));
    }
    Ok(())
}

fn canonical_token(value: &str) -> CommandResult<String> {
    let parsed = Uuid::parse_str(value)
        .map_err(|_| CommandError::validation("恢复令牌不是有效的规范 UUID。"))?;
    let canonical = parsed.to_string();
    if canonical != value {
        return Err(CommandError::validation(
            "恢复令牌必须使用规范小写 UUID 格式。",
        ));
    }
    Ok(canonical)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn display_filename(path: &Path) -> CommandResult<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CommandError::validation("恢复备份文件名无效。"))
}

fn require_real_directory(path: &Path, label: &str) -> CommandResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CommandError::new(
            "RESTORE_DIRECTORY_INVALID",
            format!("无法检查{label}：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommandError::new(
            "RESTORE_DIRECTORY_INVALID",
            format!("{label}不是安全的真实目录。"),
        ));
    }
    Ok(())
}

fn direct_real_child(parent: &Path, name: &str, label: &str) -> CommandResult<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(CommandError::new(
            "RESTORE_DIRECTORY_INVALID",
            format!("{label}名称无效。"),
        ));
    }
    require_real_directory(parent, "受控父目录")?;
    let child = parent.join(name);
    require_real_directory(&child, label)?;
    let canonical_parent = fs::canonicalize(parent).map_err(io_error("无法规范化受控父目录"))?;
    let canonical_child = fs::canonicalize(&child).map_err(io_error("无法规范化受控子目录"))?;
    if canonical_child.parent() != Some(canonical_parent.as_path()) {
        return Err(CommandError::new(
            "RESTORE_DIRECTORY_OUTSIDE_PARENT",
            format!("{label}不是受控父目录的真实直接子目录。"),
        ));
    }
    Ok(canonical_child)
}

fn rename_with_retry(source: &Path, destination: &Path) -> CommandResult<()> {
    if destination.exists() {
        return Err(CommandError::new(
            "RESTORE_RENAME_TARGET_EXISTS",
            format!("恢复切换目标已存在：{}", destination.display()),
        ));
    }
    let mut last_error = None;
    for attempt in 0..8 {
        match fs::rename(source, destination) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if attempt < 7 {
                    thread::sleep(Duration::from_millis(125));
                }
            }
        }
    }
    Err(CommandError::new(
        "RESTORE_RENAME_FAILED",
        format!(
            "无法切换“{}”到“{}”：{}",
            source.display(),
            destination.display(),
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "未知文件系统错误".to_owned())
        ),
    ))
}

fn write_json_new<T: Serialize>(path: &Path, value: &T) -> CommandResult<()> {
    if path.exists() {
        return Err(CommandError::new(
            "RESTORE_STATE_EXISTS",
            format!("恢复状态文件已存在：{}", path.display()),
        ));
    }
    write_json_atomic(path, value, false)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T, replace: bool) -> CommandResult<()> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        CommandError::new(
            "RESTORE_STATE_SERIALIZE_FAILED",
            format!("无法生成恢复状态：{error}"),
        )
    })?;
    if bytes.len() as u64 > MAX_RESTORE_STATE_BYTES {
        return Err(CommandError::new(
            "RESTORE_STATE_TOO_LARGE",
            "恢复状态超过 2 MB 安全上限。",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        CommandError::new("RESTORE_STATE_PATH_INVALID", "恢复状态路径缺少父目录。")
    })?;
    require_real_directory(parent, "恢复状态父目录")?;
    let temporary = parent.join(format!(".restore-state-{}.tmp", Uuid::now_v7().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(io_error("无法创建恢复状态临时文件"))?;
    let write_result = file
        .write_all(&bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(CommandError::new(
            "RESTORE_STATE_WRITE_FAILED",
            format!("无法写入恢复状态：{error}"),
        ));
    }
    drop(file);
    if replace && path.exists() {
        fs::remove_file(path).map_err(io_error("无法替换旧恢复状态"))?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        CommandError::new(
            "RESTORE_STATE_COMMIT_FAILED",
            format!("无法提交恢复状态：{error}"),
        )
    })
}

fn read_json<T: DeserializeOwned>(path: &Path) -> CommandResult<T> {
    let metadata = fs::symlink_metadata(path).map_err(io_error("无法检查恢复状态"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_RESTORE_STATE_BYTES
    {
        return Err(CommandError::new(
            "RESTORE_STATE_INVALID",
            "恢复状态不是安全的普通文件或超过大小上限。",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(io_error("无法读取恢复状态"))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        CommandError::new(
            "RESTORE_STATE_INVALID",
            format!("恢复状态无法解析：{error}"),
        )
    })
}

fn sync_file(path: &Path) -> CommandResult<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(io_error("无法将恢复文件安全写入磁盘"))
}

fn summary_api(summary: RestoreCandidateSummary) -> RestoreDataSummaryApi {
    RestoreDataSummaryApi {
        database_uuid: summary.database_uuid,
        database_schema_version: summary.database_schema_version,
        question_count: summary.question_count,
        paper_count: summary.paper_count,
        template_count: summary.template_count,
        resource_count: summary.resource_count,
    }
}

fn inspection_api(inspection: &BackupInspection) -> BackupInspectionApi {
    BackupInspectionApi {
        format_version: inspection.manifest.version,
        created_at: i64::try_from(inspection.manifest.created_at_unix_ms).unwrap_or(i64::MAX),
        source_app_version: inspection.manifest.application_version.clone(),
        database_schema_version: inspection.manifest.database_schema_version,
        source_database_uuid: inspection.manifest.database_uuid.clone(),
        archive_sha256_hex: inspection.archive_sha256_hex.clone(),
        manifest_sha256_hex: inspection.manifest_sha256_hex.clone(),
        archive_byte_size: inspection.archive_size_bytes,
        payload_byte_size: inspection.payload_size_bytes,
        payload_file_count: inspection.payload_file_count as u64,
    }
}

fn backup_error(error: backup::BackupError) -> CommandError {
    CommandError::new("RESTORE_ARCHIVE_REJECTED", error.to_string())
}

fn io_error(context: &'static str) -> impl FnOnce(std::io::Error) -> CommandError {
    move |error| CommandError::new("RESTORE_IO_ERROR", format!("{context}：{error}"))
}

fn now_millis() -> CommandResult<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            CommandError::new(
                "RESTORE_CLOCK_INVALID",
                format!("系统时间无法用于恢复流程：{error}"),
            )
        })?;
    Ok(elapsed.as_millis().min(i64::MAX as u128) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_token_requires_canonical_uuid() {
        assert!(canonical_token("018f4c1a-1234-7abc-8def-0123456789ab").is_ok());
        assert!(canonical_token("018F4C1A-1234-7ABC-8DEF-0123456789AB").is_err());
        assert!(canonical_token("../active.json").is_err());
    }

    #[test]
    fn sha256_confirmation_is_lowercase_and_fixed_length() {
        assert!(valid_sha256(&"ab".repeat(32)));
        assert!(!valid_sha256(&"AB".repeat(32)));
        assert!(!valid_sha256("abcd"));
    }
}
