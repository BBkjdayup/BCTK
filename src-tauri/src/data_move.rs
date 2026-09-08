use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    api::{
        AppState, backups,
        models::{
            CommandError, CommandResult, DataMovePlanApi, DataMoveResultApi, DataMoveScheduledApi,
            ScheduleDataMoveRequestApi,
        },
    },
    backup::{self, BackupLimits},
    restore::{inspect_prepared_candidate, prepare_restore_candidate},
};

const STATE_VERSION: u32 = 1;
const PLAN_LIFETIME_MS: i64 = 30 * 60 * 1_000;
const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const PERSISTENT_COMPONENTS: [&str; 5] = ["database", "resources", "templates", "backup", "export"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DataMovePlanFile {
    version: u32,
    operation_id: String,
    move_token: String,
    created_at: i64,
    expires_at: i64,
    source_data_root: String,
    target_data_root: String,
    source_database_uuid: String,
    estimated_file_count: u64,
    estimated_byte_size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocationRecord {
    version: u32,
    record_id: String,
    created_at: i64,
    operation_id: String,
    state: String,
    active_data_root: String,
    fallback_data_root: Option<String>,
    source_database_uuid: String,
    target_tree_sha256_hex: String,
    copied_file_count: u64,
    copied_byte_size: u64,
    duration_ms: i64,
    error_message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StartupDataRoot {
    pub selected_data_root: PathBuf,
    pending: Option<LocationRecord>,
}

#[derive(Clone, Copy, Debug, Default)]
struct TreeStats {
    file_count: u64,
    byte_size: u64,
}

struct OperationCleanup {
    path: PathBuf,
    active: bool,
}

impl Drop for OperationCleanup {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub async fn prepare_data_move(
    state: &AppState,
    target_data_root: PathBuf,
) -> CommandResult<DataMovePlanApi> {
    let database = state.exclusive_database().await?;
    let source = canonical_real_directory(database.paths().data_root(), "当前数据目录")?;
    let target = validate_empty_target(&source, &target_data_root)?;
    let source_database_uuid = database
        .database_uuid()
        .await
        .map_err(CommandError::database)?;
    let stats = scan_persistent_components(&source, None)?;
    let operation_id = Uuid::now_v7().to_string();
    let operation_dir = create_operation_dir(&source, &operation_id)?;
    let mut cleanup = OperationCleanup {
        path: operation_dir.clone(),
        active: true,
    };
    let now = now_millis()?;
    let plan = DataMovePlanFile {
        version: STATE_VERSION,
        operation_id: operation_id.clone(),
        move_token: operation_id.clone(),
        created_at: now,
        expires_at: now.saturating_add(PLAN_LIFETIME_MS),
        source_data_root: path_text(&source, "当前数据目录")?,
        target_data_root: path_text(&target, "目标数据目录")?,
        source_database_uuid,
        estimated_file_count: stats.file_count,
        estimated_byte_size: stats.byte_size,
    };
    write_json_new(&operation_dir.join("plan.json"), &plan)?;
    cleanup.active = false;
    Ok(plan_api(&plan))
}

pub async fn cancel_data_move(
    state: &AppState,
    bootstrap_dir: &Path,
    move_token: &str,
) -> CommandResult<()> {
    let operation_id = canonical_token(move_token)?;
    let _maintenance = state.exclusive_maintenance_lease().await;
    let source = canonical_real_directory(state.data_root(), "当前数据目录")?;
    let root = existing_move_root(&source)?.ok_or_else(|| {
        CommandError::new("DATA_MOVE_PLAN_NOT_FOUND", "找不到待取消的数据迁移计划。")
    })?;
    let operation_dir = checked_operation_dir(&root, &operation_id)?;
    let committed = location_records(bootstrap_dir)?
        .iter()
        .any(|record| record.operation_id == operation_id);
    if committed {
        return Err(CommandError::new(
            "DATA_MOVE_ALREADY_SCHEDULED",
            "数据迁移已经进入重启切换阶段，不能再取消。",
        ));
    }
    if operation_dir.join("scheduled.json").exists() {
        // The process or bootstrap write stopped before the location switch was
        // committed. The old root is still active, so only the source-side plan
        // is removed; the verified target copy is deliberately retained.
    }
    let plan: DataMovePlanFile = read_json(&operation_dir.join("plan.json"))?;
    validate_plan(&plan, &operation_id)?;
    remove_owned_operation_dir(&root, &operation_dir)
}

pub async fn schedule_data_move(
    state: &AppState,
    bootstrap_dir: &Path,
    request: &ScheduleDataMoveRequestApi,
) -> CommandResult<DataMoveScheduledApi> {
    if !request.confirmed_keep_old_data {
        return Err(CommandError::validation(
            "必须明确确认迁移完成后保留旧数据目录。",
        ));
    }
    let operation_id = canonical_token(&request.move_token)?;
    let maintenance = state.restore_maintenance_lease().await;
    let database = maintenance.database().ok_or_else(|| {
        CommandError::new(
            "DATABASE_UNAVAILABLE",
            "当前数据库不可用，不能执行数据目录迁移。请先用备份救援或修复数据库。",
        )
    })?;
    let source = canonical_real_directory(database.paths().data_root(), "当前数据目录")?;
    let root = existing_move_root(&source)?
        .ok_or_else(|| CommandError::new("DATA_MOVE_PLAN_NOT_FOUND", "找不到数据迁移计划。"))?;
    let operation_dir = checked_operation_dir(&root, &operation_id)?;
    let plan: DataMovePlanFile = read_json(&operation_dir.join("plan.json"))?;
    validate_plan(&plan, &operation_id)?;
    if now_millis()? > plan.expires_at {
        return Err(CommandError::new(
            "DATA_MOVE_PLAN_EXPIRED",
            "数据迁移确认已过期，请重新选择目标目录。",
        ));
    }
    if request.expected_source_data_root != plan.source_data_root
        || request.expected_target_data_root != plan.target_data_root
    {
        return Err(CommandError::new(
            "DATA_MOVE_CONFIRMATION_MISMATCH",
            "数据迁移确认的源目录或目标目录与预检结果不一致。",
        ));
    }
    let current_uuid = database
        .database_uuid()
        .await
        .map_err(CommandError::database)?;
    if current_uuid != plan.source_database_uuid {
        return Err(CommandError::new(
            "DATA_MOVE_SOURCE_CHANGED",
            "当前题库身份与预检时不一致，已停止迁移。",
        ));
    }
    let target = validate_empty_target(&source, Path::new(&plan.target_data_root))?;
    let started_at = now_millis()?;
    let transfer_archive_name = format!(".data-move-{operation_id}.tqb");
    let transfer_archive = database.paths().backup_dir().join(&transfer_archive_name);
    backups::create_data_move_backup(database, transfer_archive.clone(), state.app_version())
        .await?;

    let incoming = target.join(format!(".move-incoming-{operation_id}"));
    fs::create_dir(&incoming).map_err(|error| {
        CommandError::new(
            "DATA_MOVE_TARGET_CREATE_FAILED",
            format!("无法创建目标暂存目录：{error}"),
        )
    })?;
    let archive_for_extract = transfer_archive.clone();
    let incoming_for_extract = incoming.clone();
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
            "DATA_MOVE_EXTRACT_TASK_FAILED",
            format!("数据迁移解包任务未能正常完成：{error}"),
        )
    })?
    .map_err(backup_error)?;
    let summary = prepare_restore_candidate(&extracted, state.app_version()).await?;
    if summary.database_uuid != plan.source_database_uuid {
        return Err(CommandError::new(
            "DATA_MOVE_DATABASE_IDENTITY_MISMATCH",
            "目标暂存数据库身份与当前题库不一致。",
        ));
    }

    copy_tree_verified(
        &source.join("backup"),
        &incoming.join("backup"),
        Some(&transfer_archive_name),
    )?;
    copy_tree_verified(&source.join("export"), &incoming.join("export"), None)?;
    require_only_named_children(
        &target,
        &[incoming
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                CommandError::new("DATA_MOVE_PATH_INVALID", "目标暂存目录名称无效。")
            })?],
    )?;
    for name in PERSISTENT_COMPONENTS {
        let candidate = incoming.join(name);
        require_real_directory(&candidate, "目标候选目录")?;
        let destination = target.join(name);
        fs::rename(&candidate, &destination).map_err(|error| {
            CommandError::new(
                "DATA_MOVE_TARGET_COMMIT_FAILED",
                format!("无法提交目标目录组件“{name}”：{error}"),
            )
        })?;
    }
    fs::remove_dir(&incoming).map_err(|error| {
        CommandError::new(
            "DATA_MOVE_STAGING_CLEANUP_FAILED",
            format!("无法清理已完成的目标暂存目录：{error}"),
        )
    })?;
    require_only_named_children(&target, &PERSISTENT_COMPONENTS)?;
    inspect_prepared_candidate(&target, &plan.source_database_uuid).await?;
    let (target_tree_sha256_hex, copied) = hash_persistent_components(&target, None)?;
    let duration_ms = now_millis()?.saturating_sub(started_at);
    let pending = LocationRecord {
        version: STATE_VERSION,
        record_id: Uuid::now_v7().to_string(),
        created_at: next_location_timestamp(bootstrap_dir)?,
        operation_id: operation_id.clone(),
        state: "pending".to_owned(),
        active_data_root: path_text(&target, "目标数据目录")?,
        fallback_data_root: Some(path_text(&source, "原数据目录")?),
        source_database_uuid: plan.source_database_uuid.clone(),
        target_tree_sha256_hex,
        copied_file_count: copied.file_count,
        copied_byte_size: copied.byte_size,
        duration_ms,
        error_message: None,
    };
    write_json_new(&operation_dir.join("scheduled.json"), &pending)?;
    if let Err(error) = write_location_record(bootstrap_dir, &pending) {
        let _ = fs::remove_file(operation_dir.join("scheduled.json"));
        return Err(error);
    }
    state
        .hold_restore_maintenance_until_restart(maintenance)
        .await;
    Ok(DataMoveScheduledApi {
        operation_id,
        source_data_root: plan.source_data_root,
        target_data_root: plan.target_data_root,
        copied_file_count: copied.file_count,
        copied_byte_size: copied.byte_size,
        requires_restart: true,
    })
}

pub fn resolve_startup_data_root(
    bootstrap_dir: &Path,
    default_data_root: PathBuf,
) -> CommandResult<StartupDataRoot> {
    let Some(record) = latest_location_record(bootstrap_dir)? else {
        return Ok(StartupDataRoot {
            selected_data_root: default_data_root,
            pending: None,
        });
    };
    let selected = PathBuf::from(&record.active_data_root);
    if record.state == "pending" {
        return Ok(StartupDataRoot {
            selected_data_root: selected,
            pending: Some(record),
        });
    }
    if !matches!(record.state.as_str(), "completed" | "rolled_back") {
        return Err(CommandError::new(
            "DATA_LOCATION_STATE_INVALID",
            "数据目录引导状态无效。",
        ));
    }
    validate_existing_data_root(&selected)?;
    Ok(StartupDataRoot {
        selected_data_root: selected,
        pending: None,
    })
}

pub fn validate_existing_data_root(root: &Path) -> CommandResult<()> {
    let root = canonical_real_directory(root, "已配置的数据目录")?;
    for component in PERSISTENT_COMPONENTS {
        require_real_directory(&root.join(component), "已配置的数据子目录")?;
    }
    let database = root.join("database").join("zhitiku.sqlite3");
    let metadata = fs::symlink_metadata(&database).map_err(|error| {
        CommandError::new(
            "DATA_LOCATION_DATABASE_MISSING",
            format!("已配置的数据目录缺少数据库文件：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(CommandError::new(
            "DATA_LOCATION_DATABASE_INVALID",
            "已配置的数据目录没有安全且非空的数据库文件；软件不会创建空题库覆盖该状态。",
        ));
    }
    Ok(())
}

pub async fn validate_pending_target(startup: &StartupDataRoot) -> CommandResult<()> {
    let Some(record) = startup.pending.as_ref() else {
        return Ok(());
    };
    let target = canonical_real_directory(&startup.selected_data_root, "待启用的目标数据目录")?;
    let (tree_hash, stats) = hash_persistent_components(&target, None)?;
    if tree_hash != record.target_tree_sha256_hex
        || stats.file_count != record.copied_file_count
        || stats.byte_size != record.copied_byte_size
    {
        return Err(CommandError::new(
            "DATA_MOVE_TARGET_CHANGED",
            "目标数据目录在重启前发生变化，已拒绝切换。",
        ));
    }
    inspect_prepared_candidate(&target, &record.source_database_uuid).await?;
    Ok(())
}

pub fn finalize_pending_startup(
    bootstrap_dir: &Path,
    startup: &StartupDataRoot,
    target_error: Option<String>,
) -> CommandResult<Option<DataMoveResultApi>> {
    let Some(pending) = startup.pending.as_ref() else {
        return Ok(None);
    };
    let (state, active, retained, error_message, warnings) = if let Some(error) = target_error {
        let fallback = pending.fallback_data_root.clone().ok_or_else(|| {
            CommandError::new(
                "DATA_MOVE_FALLBACK_MISSING",
                "数据迁移失败，但引导记录缺少原数据目录。",
            )
        })?;
        (
            "rolled_back",
            fallback,
            pending.active_data_root.clone(),
            Some(error),
            vec!["目标目录未被启用；软件继续使用原数据目录，目标副本原样保留。".to_owned()],
        )
    } else {
        (
            "completed",
            pending.active_data_root.clone(),
            pending.fallback_data_root.clone().unwrap_or_default(),
            None,
            vec!["原数据目录已完整保留，不会自动删除。".to_owned()],
        )
    };
    let terminal = LocationRecord {
        version: STATE_VERSION,
        record_id: Uuid::now_v7().to_string(),
        created_at: next_location_timestamp(bootstrap_dir)?,
        operation_id: pending.operation_id.clone(),
        state: state.to_owned(),
        active_data_root: active.clone(),
        fallback_data_root: None,
        source_database_uuid: pending.source_database_uuid.clone(),
        target_tree_sha256_hex: pending.target_tree_sha256_hex.clone(),
        copied_file_count: pending.copied_file_count,
        copied_byte_size: pending.copied_byte_size,
        duration_ms: pending.duration_ms,
        error_message: error_message.clone(),
    };
    write_location_record(bootstrap_dir, &terminal)?;
    Ok(Some(DataMoveResultApi {
        operation_id: pending.operation_id.clone(),
        outcome: if state == "completed" {
            "success".to_owned()
        } else {
            "rolled_back".to_owned()
        },
        active_data_root: active,
        retained_data_root: retained,
        copied_file_count: pending.copied_file_count,
        copied_byte_size: pending.copied_byte_size,
        duration_ms: pending.duration_ms,
        warnings,
        error_message,
    }))
}

pub fn get_last_data_move_result(bootstrap_dir: &Path) -> CommandResult<Option<DataMoveResultApi>> {
    let Some(record) = latest_location_record(bootstrap_dir)? else {
        return Ok(None);
    };
    if !matches!(record.state.as_str(), "completed" | "rolled_back")
        || bootstrap_dir
            .join(format!("ack-data-move-{}", record.operation_id))
            .exists()
    {
        return Ok(None);
    }
    let retained = if record.state == "completed" {
        // The preceding pending record contains the original root.
        location_records(bootstrap_dir)?
            .into_iter()
            .rev()
            .find(|candidate| {
                candidate.operation_id == record.operation_id && candidate.state == "pending"
            })
            .and_then(|candidate| candidate.fallback_data_root)
            .unwrap_or_default()
    } else {
        location_records(bootstrap_dir)?
            .into_iter()
            .rev()
            .find(|candidate| {
                candidate.operation_id == record.operation_id && candidate.state == "pending"
            })
            .map(|candidate| candidate.active_data_root)
            .unwrap_or_default()
    };
    Ok(Some(DataMoveResultApi {
        operation_id: record.operation_id,
        outcome: if record.state == "completed" {
            "success".to_owned()
        } else {
            "rolled_back".to_owned()
        },
        active_data_root: record.active_data_root,
        retained_data_root: retained,
        copied_file_count: record.copied_file_count,
        copied_byte_size: record.copied_byte_size,
        duration_ms: record.duration_ms,
        warnings: if record.state == "completed" {
            vec!["原数据目录已完整保留，不会自动删除。".to_owned()]
        } else {
            vec!["目标目录未被启用，目标副本仍原样保留。".to_owned()]
        },
        error_message: record.error_message,
    }))
}

pub fn acknowledge_data_move_result(bootstrap_dir: &Path, operation_id: &str) -> CommandResult<()> {
    let operation_id = canonical_token(operation_id)?;
    let latest = latest_location_record(bootstrap_dir)?.ok_or_else(|| {
        CommandError::new("DATA_MOVE_RESULT_NOT_FOUND", "找不到可确认的数据迁移结果。")
    })?;
    if latest.operation_id != operation_id
        || !matches!(latest.state.as_str(), "completed" | "rolled_back")
    {
        return Err(CommandError::validation(
            "数据迁移结果身份与确认请求不一致。",
        ));
    }
    fs::create_dir_all(bootstrap_dir).map_err(io_error("无法创建数据目录引导位置"))?;
    let path = bootstrap_dir.join(format!("ack-data-move-{operation_id}"));
    if path.exists() {
        return Ok(());
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error("无法确认数据迁移结果"))?;
    file.sync_all().map_err(io_error("无法保存数据迁移确认"))
}

fn plan_api(plan: &DataMovePlanFile) -> DataMovePlanApi {
    DataMovePlanApi {
        move_token: plan.move_token.clone(),
        expires_at: plan.expires_at,
        source_data_root: plan.source_data_root.clone(),
        target_data_root: plan.target_data_root.clone(),
        estimated_file_count: plan.estimated_file_count,
        estimated_byte_size: plan.estimated_byte_size,
        warnings: vec![
            "执行时会冻结写入，创建一致的数据库快照并逐文件校验。".to_owned(),
            "迁移成功后软件会自动重启；旧数据目录会原样保留。".to_owned(),
            "目标目录必须保持为空；复制失败时不会切换当前数据位置。".to_owned(),
        ],
        requires_restart: true,
    }
}

fn validate_empty_target(source: &Path, target: &Path) -> CommandResult<PathBuf> {
    if !target.is_absolute() {
        return Err(CommandError::validation("目标数据目录必须是绝对路径。"));
    }
    let target = canonical_real_directory(target, "目标数据目录")?;
    if target == source || target.starts_with(source) || source.starts_with(&target) {
        return Err(CommandError::validation(
            "目标目录不能与当前数据目录相同，也不能互为父子目录。",
        ));
    }
    let target_text = target.to_string_lossy();
    if target_text.starts_with(r"\\?\UNC\")
        || (target_text.starts_with(r"\\") && !target_text.starts_with(r"\\?\"))
    {
        return Err(CommandError::validation(
            "第一版暂不支持把数据目录迁移到网络共享路径。",
        ));
    }
    let mut entries = fs::read_dir(&target).map_err(io_error("无法读取目标数据目录"))?;
    if entries
        .next()
        .transpose()
        .map_err(io_error("无法检查目标数据目录"))?
        .is_some()
    {
        return Err(CommandError::validation(
            "目标目录必须为空，请新建一个空文件夹后再选择。",
        ));
    }
    let probe = target.join(format!(".zhitiku-write-probe-{}", Uuid::now_v7().simple()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(io_error("目标数据目录不可写"))?;
    file.sync_all()
        .map_err(io_error("目标数据目录写入测试失败"))?;
    drop(file);
    fs::remove_file(probe).map_err(io_error("无法清理目标目录写入测试文件"))?;
    Ok(target)
}

fn require_only_named_children(root: &Path, allowed: &[&str]) -> CommandResult<()> {
    require_real_directory(root, "目标数据目录")?;
    for entry in fs::read_dir(root).map_err(io_error("无法复检目标数据目录"))? {
        let entry = entry.map_err(io_error("无法读取目标数据目录条目"))?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            CommandError::new(
                "DATA_MOVE_PATH_INVALID",
                "目标数据目录包含无法安全记录的文件名。",
            )
        })?;
        if !allowed.contains(&name) {
            return Err(CommandError::new(
                "DATA_MOVE_TARGET_CHANGED",
                format!("目标目录在复制期间出现未预期条目“{name}”。"),
            ));
        }
    }
    Ok(())
}

fn create_operation_dir(source: &Path, operation_id: &str) -> CommandResult<PathBuf> {
    let root = source.join(".data-move");
    fs::create_dir_all(&root).map_err(io_error("无法创建数据迁移维护目录"))?;
    require_real_directory(&root, "数据迁移维护目录")?;
    let operation = root.join(operation_id);
    fs::create_dir(&operation).map_err(io_error("无法创建数据迁移工作目录"))?;
    Ok(operation)
}

fn existing_move_root(source: &Path) -> CommandResult<Option<PathBuf>> {
    let root = source.join(".data-move");
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(Some(root)),
        Ok(_) => Err(CommandError::new(
            "DATA_MOVE_ROOT_INVALID",
            "数据迁移维护路径不是安全的真实目录。",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CommandError::new(
            "DATA_MOVE_ROOT_INVALID",
            format!("无法检查数据迁移维护目录：{error}"),
        )),
    }
}

fn checked_operation_dir(root: &Path, operation_id: &str) -> CommandResult<PathBuf> {
    let path = root.join(operation_id);
    let root = canonical_real_directory(root, "数据迁移维护目录")?;
    let path = canonical_real_directory(&path, "数据迁移工作目录")?;
    if path.parent() != Some(root.as_path()) {
        return Err(CommandError::new(
            "DATA_MOVE_OPERATION_PATH_INVALID",
            "数据迁移工作目录超出受控维护目录。",
        ));
    }
    Ok(path)
}

fn remove_owned_operation_dir(root: &Path, operation: &Path) -> CommandResult<()> {
    let root = canonical_real_directory(root, "数据迁移维护目录")?;
    let operation = canonical_real_directory(operation, "数据迁移工作目录")?;
    if operation.parent() != Some(root.as_path()) {
        return Err(CommandError::new(
            "DATA_MOVE_OPERATION_PATH_INVALID",
            "拒绝删除受控数据迁移目录之外的路径。",
        ));
    }
    fs::remove_dir_all(operation).map_err(io_error("无法清理数据迁移计划"))
}

fn validate_plan(plan: &DataMovePlanFile, operation_id: &str) -> CommandResult<()> {
    if plan.version != STATE_VERSION
        || plan.operation_id != operation_id
        || plan.move_token != operation_id
    {
        return Err(CommandError::new(
            "DATA_MOVE_PLAN_INVALID",
            "数据迁移计划身份或版本无效。",
        ));
    }
    Ok(())
}

fn scan_persistent_components(
    root: &Path,
    skip_backup_file: Option<&str>,
) -> CommandResult<TreeStats> {
    let mut stats = TreeStats::default();
    for component in PERSISTENT_COMPONENTS {
        scan_tree(
            &root.join(component),
            if component == "backup" {
                skip_backup_file
            } else {
                None
            },
            &mut stats,
        )?;
    }
    Ok(stats)
}

fn scan_tree(root: &Path, skip_file: Option<&str>, stats: &mut TreeStats) -> CommandResult<()> {
    require_real_directory(root, "持久化数据目录")?;
    for entry in fs::read_dir(root).map_err(io_error("无法扫描持久化数据目录"))? {
        let entry = entry.map_err(io_error("无法读取持久化数据条目"))?;
        let name = entry.file_name();
        let name_text = name.to_str().ok_or_else(|| {
            CommandError::new(
                "DATA_MOVE_PATH_INVALID",
                "数据目录包含无法安全记录的文件名。",
            )
        })?;
        if skip_file == Some(name_text) || should_skip_transient(name_text) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(io_error("无法检查持久化数据"))?;
        if metadata.file_type().is_symlink() {
            return Err(CommandError::new(
                "DATA_MOVE_SYMLINK_REJECTED",
                "数据目录包含符号链接或重解析路径，已停止迁移。",
            ));
        }
        if metadata.is_dir() {
            scan_tree(&path, skip_file, stats)?;
        } else if metadata.is_file() {
            stats.file_count = stats.file_count.saturating_add(1);
            stats.byte_size = stats.byte_size.saturating_add(metadata.len());
        } else {
            return Err(CommandError::new(
                "DATA_MOVE_FILE_TYPE_REJECTED",
                "数据目录包含不支持的文件类型。",
            ));
        }
    }
    Ok(())
}

fn copy_tree_verified(
    source: &Path,
    target: &Path,
    skip_file: Option<&str>,
) -> CommandResult<TreeStats> {
    require_real_directory(source, "待复制目录")?;
    if target.exists() {
        require_real_directory(target, "目标数据目录")?;
        if fs::read_dir(target)
            .map_err(io_error("无法读取目标数据目录"))?
            .next()
            .transpose()
            .map_err(io_error("无法检查目标数据目录"))?
            .is_some()
        {
            return Err(CommandError::new(
                "DATA_MOVE_TARGET_NOT_EMPTY",
                "目标候选子目录不是空目录，已停止复制。",
            ));
        }
    } else {
        fs::create_dir(target).map_err(io_error("无法创建目标数据目录"))?;
    }
    let mut stats = TreeStats::default();
    copy_tree_entries(source, target, skip_file, &mut stats)?;
    Ok(stats)
}

fn copy_tree_entries(
    source: &Path,
    target: &Path,
    skip_file: Option<&str>,
    stats: &mut TreeStats,
) -> CommandResult<()> {
    for entry in fs::read_dir(source).map_err(io_error("无法读取待复制目录"))? {
        let entry = entry.map_err(io_error("无法读取待复制条目"))?;
        let name = entry.file_name();
        let name_text = name.to_str().ok_or_else(|| {
            CommandError::new(
                "DATA_MOVE_PATH_INVALID",
                "数据目录包含无法安全记录的文件名。",
            )
        })?;
        if skip_file == Some(name_text) || should_skip_transient(name_text) {
            continue;
        }
        let source_path = entry.path();
        let target_path = target.join(&name);
        match fs::symlink_metadata(&target_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(CommandError::new(
                    "DATA_MOVE_TARGET_COLLISION",
                    "目标目录在复制期间出现同名条目，已停止迁移。",
                ));
            }
            Err(error) => {
                return Err(CommandError::new(
                    "DATA_MOVE_TARGET_CHECK_FAILED",
                    format!("无法检查目标数据条目：{error}"),
                ));
            }
        }
        let metadata =
            fs::symlink_metadata(&source_path).map_err(io_error("无法检查待复制条目"))?;
        if metadata.file_type().is_symlink() {
            return Err(CommandError::new(
                "DATA_MOVE_SYMLINK_REJECTED",
                "数据目录包含符号链接或重解析路径，已停止迁移。",
            ));
        }
        if metadata.is_dir() {
            fs::create_dir(&target_path).map_err(io_error("无法创建目标子目录"))?;
            copy_tree_entries(&source_path, &target_path, skip_file, stats)?;
        } else if metadata.is_file() {
            let source_hash = hash_file_stable(&source_path)?;
            fs::copy(&source_path, &target_path).map_err(io_error("无法复制数据文件"))?;
            sync_file(&target_path)?;
            let target_hash = hash_file_stable(&target_path)?;
            let source_after = hash_file_stable(&source_path)?;
            if source_hash != target_hash || source_hash != source_after {
                return Err(CommandError::new(
                    "DATA_MOVE_FILE_CHANGED",
                    "数据文件在复制期间发生变化，已停止切换。",
                ));
            }
            stats.file_count = stats.file_count.saturating_add(1);
            stats.byte_size = stats.byte_size.saturating_add(metadata.len());
        } else {
            return Err(CommandError::new(
                "DATA_MOVE_FILE_TYPE_REJECTED",
                "数据目录包含不支持的文件类型。",
            ));
        }
    }
    Ok(())
}

fn hash_persistent_components(
    root: &Path,
    skip_backup_file: Option<&str>,
) -> CommandResult<(String, TreeStats)> {
    let mut hasher = Sha256::new();
    hasher.update(b"zhitiku-data-root-v1\0");
    let mut stats = TreeStats::default();
    for component in PERSISTENT_COMPONENTS {
        hash_tree_entries(
            root,
            &root.join(component),
            if component == "backup" {
                skip_backup_file
            } else {
                None
            },
            &mut hasher,
            &mut stats,
        )?;
    }
    Ok((format!("{:x}", hasher.finalize()), stats))
}

fn hash_tree_entries(
    root: &Path,
    directory: &Path,
    skip_file: Option<&str>,
    hasher: &mut Sha256,
    stats: &mut TreeStats,
) -> CommandResult<()> {
    require_real_directory(directory, "待校验数据目录")?;
    let mut entries = fs::read_dir(directory)
        .map_err(io_error("无法读取待校验数据目录"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io_error("无法枚举待校验数据目录"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name_text = name.to_str().ok_or_else(|| {
            CommandError::new(
                "DATA_MOVE_PATH_INVALID",
                "数据目录包含无法安全记录的文件名。",
            )
        })?;
        if skip_file == Some(name_text) || should_skip_transient(name_text) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(io_error("无法检查待校验条目"))?;
        if metadata.file_type().is_symlink() {
            return Err(CommandError::new(
                "DATA_MOVE_SYMLINK_REJECTED",
                "数据目录包含符号链接或重解析路径。",
            ));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| CommandError::new("DATA_MOVE_PATH_INVALID", "数据文件超出受控目录。"))?;
        let relative_text = relative.to_str().ok_or_else(|| {
            CommandError::new("DATA_MOVE_PATH_INVALID", "数据路径包含无法安全记录的字符。")
        })?;
        hasher.update(if metadata.is_dir() { b"D\0" } else { b"F\0" });
        hasher.update((relative_text.len() as u64).to_le_bytes());
        hasher.update(relative_text.as_bytes());
        if metadata.is_dir() {
            hash_tree_entries(root, &path, skip_file, hasher, stats)?;
        } else if metadata.is_file() {
            hasher.update(metadata.len().to_le_bytes());
            hasher.update(hash_file_stable(&path)?);
            stats.file_count = stats.file_count.saturating_add(1);
            stats.byte_size = stats.byte_size.saturating_add(metadata.len());
        } else {
            return Err(CommandError::new(
                "DATA_MOVE_FILE_TYPE_REJECTED",
                "数据目录包含不支持的文件类型。",
            ));
        }
    }
    Ok(())
}

fn should_skip_transient(name: &str) -> bool {
    name == ".staging"
        || name.ends_with(".partial")
        || name.starts_with(".backup-snapshot-")
        || name.starts_with(".restore-state-")
}

fn hash_file_stable(path: &Path) -> CommandResult<Vec<u8>> {
    let before = fs::symlink_metadata(path).map_err(io_error("无法检查数据文件"))?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(CommandError::new(
            "DATA_MOVE_FILE_INVALID",
            "待校验条目不是安全的普通文件。",
        ));
    }
    let mut file = File::open(path).map_err(io_error("无法读取数据文件"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(io_error("无法读取数据文件"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let after = fs::symlink_metadata(path).map_err(io_error("无法复检数据文件"))?;
    if after.file_type().is_symlink() || !after.is_file() || after.len() != before.len() {
        return Err(CommandError::new(
            "DATA_MOVE_FILE_CHANGED",
            "数据文件在生成摘要时发生变化。",
        ));
    }
    Ok(hasher.finalize().to_vec())
}

fn write_location_record(bootstrap_dir: &Path, record: &LocationRecord) -> CommandResult<()> {
    fs::create_dir_all(bootstrap_dir).map_err(io_error("无法创建数据目录引导位置"))?;
    require_real_directory(bootstrap_dir, "数据目录引导位置")?;
    let filename = format!(
        "location-{:020}-{}.json",
        record.created_at, record.record_id
    );
    write_json_new(&bootstrap_dir.join(filename), record)
}

fn latest_location_record(bootstrap_dir: &Path) -> CommandResult<Option<LocationRecord>> {
    Ok(location_records(bootstrap_dir)?.into_iter().last())
}

fn next_location_timestamp(bootstrap_dir: &Path) -> CommandResult<i64> {
    let now = now_millis()?;
    Ok(latest_location_record(bootstrap_dir)?
        .map(|record| record.created_at.saturating_add(1).max(now))
        .unwrap_or(now))
}

fn location_records(bootstrap_dir: &Path) -> CommandResult<Vec<LocationRecord>> {
    if !bootstrap_dir.exists() {
        return Ok(Vec::new());
    }
    require_real_directory(bootstrap_dir, "数据目录引导位置")?;
    let mut records = Vec::new();
    for entry in fs::read_dir(bootstrap_dir).map_err(io_error("无法读取数据目录引导位置"))?
    {
        let entry = entry.map_err(io_error("无法读取数据目录引导记录"))?;
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        if !name.starts_with("location-") || !name.ends_with(".json") {
            continue;
        }
        let record: LocationRecord = read_json(&entry.path())?;
        validate_location_record(&record)?;
        records.push(record);
    }
    records.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    Ok(records)
}

fn validate_location_record(record: &LocationRecord) -> CommandResult<()> {
    if record.version != STATE_VERSION
        || Uuid::parse_str(&record.record_id).is_err()
        || Uuid::parse_str(&record.operation_id).is_err()
        || !matches!(
            record.state.as_str(),
            "pending" | "completed" | "rolled_back"
        )
        || !Path::new(&record.active_data_root).is_absolute()
        || !valid_sha256(&record.target_tree_sha256_hex)
    {
        return Err(CommandError::new(
            "DATA_LOCATION_STATE_INVALID",
            "数据目录引导记录无效。",
        ));
    }
    Ok(())
}

fn canonical_real_directory(path: &Path, label: &str) -> CommandResult<PathBuf> {
    require_real_directory(path, label)?;
    fs::canonicalize(path).map_err(io_error("无法规范化数据目录"))
}

fn require_real_directory(path: &Path, label: &str) -> CommandResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CommandError::new(
            "DATA_MOVE_DIRECTORY_INVALID",
            format!("无法检查{label}：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommandError::new(
            "DATA_MOVE_DIRECTORY_INVALID",
            format!("{label}不是安全的真实目录。"),
        ));
    }
    Ok(())
}

fn path_text(path: &Path, label: &str) -> CommandResult<String> {
    let value = path
        .to_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            CommandError::new(
                "DATA_MOVE_PATH_INVALID",
                format!("{label}包含当前系统无法安全记录的字符。"),
            )
        })?;
    #[cfg(windows)]
    {
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return Ok(format!(r"\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return Ok(rest.to_owned());
        }
    }
    Ok(value)
}

fn canonical_token(value: &str) -> CommandResult<String> {
    let parsed = Uuid::parse_str(value)
        .map_err(|_| CommandError::validation("数据迁移令牌不是有效 UUID。"))?;
    let canonical = parsed.to_string();
    if canonical != value {
        return Err(CommandError::validation(
            "数据迁移令牌必须使用规范小写 UUID 格式。",
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

fn write_json_new<T: Serialize>(path: &Path, value: &T) -> CommandResult<()> {
    if path.exists() {
        return Err(CommandError::new(
            "DATA_MOVE_STATE_EXISTS",
            format!("数据迁移状态文件已存在：{}", path.display()),
        ));
    }
    let bytes = serde_json::to_vec(value).map_err(|error| {
        CommandError::new(
            "DATA_MOVE_STATE_SERIALIZE_FAILED",
            format!("无法生成数据迁移状态：{error}"),
        )
    })?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(CommandError::new(
            "DATA_MOVE_STATE_TOO_LARGE",
            "数据迁移状态超过 2 MB 安全上限。",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        CommandError::new("DATA_MOVE_STATE_PATH_INVALID", "数据迁移状态缺少父目录。")
    })?;
    require_real_directory(parent, "数据迁移状态父目录")?;
    let temporary = parent.join(format!(".data-move-state-{}.tmp", Uuid::now_v7().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(io_error("无法创建数据迁移状态临时文件"))?;
    file.write_all(&bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(io_error("无法写入数据迁移状态"))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        CommandError::new(
            "DATA_MOVE_STATE_COMMIT_FAILED",
            format!("无法提交数据迁移状态：{error}"),
        )
    })
}

fn read_json<T: DeserializeOwned>(path: &Path) -> CommandResult<T> {
    let metadata = fs::symlink_metadata(path).map_err(io_error("无法检查数据迁移状态"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_STATE_BYTES
    {
        return Err(CommandError::new(
            "DATA_MOVE_STATE_INVALID",
            "数据迁移状态不是安全的普通文件或超过大小上限。",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(io_error("无法读取数据迁移状态"))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        CommandError::new(
            "DATA_MOVE_STATE_INVALID",
            format!("数据迁移状态无法解析：{error}"),
        )
    })
}

fn sync_file(path: &Path) -> CommandResult<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(io_error("无法将迁移文件安全写入磁盘"))
}

fn backup_error(error: backup::BackupError) -> CommandError {
    CommandError::new("DATA_MOVE_ARCHIVE_REJECTED", error.to_string())
}

fn io_error(context: &'static str) -> impl FnOnce(std::io::Error) -> CommandError {
    move |error| CommandError::new("DATA_MOVE_IO_ERROR", format!("{context}：{error}"))
}

fn now_millis() -> CommandResult<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            CommandError::new(
                "DATA_MOVE_CLOCK_INVALID",
                format!("系统时间无法用于数据迁移：{error}"),
            )
        })?;
    Ok(elapsed.as_millis().min(i64::MAX as u128) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_token_rejects_paths_and_uppercase() {
        assert!(canonical_token("018f4c1a-1234-7abc-8def-0123456789ab").is_ok());
        assert!(canonical_token("../data").is_err());
        assert!(canonical_token("018F4C1A-1234-7ABC-8DEF-0123456789AB").is_err());
    }
}
