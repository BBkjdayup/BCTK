use sqlx::{FromRow, SqlitePool};

use super::models::{AppSettingsApi, CommandError, CommandResult};

#[derive(Debug, FromRow)]
struct SettingsRow {
    default_export_directory: Option<String>,
    default_template_id: Option<String>,
    export_filename_pattern: String,
    optional_confirmations_json: String,
    recent_unused_days: Option<i64>,
    recent_added_days: Option<i64>,
    recent_used_days: Option<i64>,
    recycle_retention_days: i64,
    recycle_policy: String,
    automatic_backup_enabled: i64,
    automatic_backup_interval_days: i64,
    automatic_backup_retention_count: i64,
}

pub async fn get_settings(pool: &SqlitePool) -> CommandResult<AppSettingsApi> {
    let row = sqlx::query_as::<_, SettingsRow>(
        "SELECT default_export_directory, default_template_id, export_filename_pattern, \
         optional_confirmations_json, recent_unused_days, recent_added_days, recent_used_days, \
         recycle_retention_days, recycle_policy, automatic_backup_enabled, \
         automatic_backup_interval_days, automatic_backup_retention_count \
         FROM app_settings WHERE singleton_id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(CommandError::database)?;
    let optional_confirmations =
        serde_json::from_str::<serde_json::Value>(&row.optional_confirmations_json)
            .ok()
            .and_then(|value| value.get("enabled").and_then(serde_json::Value::as_bool))
            .unwrap_or(true);
    Ok(AppSettingsApi {
        default_export_directory: row.default_export_directory.unwrap_or_default(),
        default_template_id: row.default_template_id,
        export_filename_pattern: row.export_filename_pattern,
        optional_confirmations,
        recent_unused_days: row
            .recent_unused_days
            .unwrap_or(90)
            .try_into()
            .unwrap_or(90),
        recent_added_days: row.recent_added_days.unwrap_or(30).try_into().unwrap_or(30),
        recent_used_days: row.recent_used_days.unwrap_or(30).try_into().unwrap_or(30),
        recycle_retention_days: row.recycle_retention_days.try_into().unwrap_or(30),
        recycle_policy: row.recycle_policy,
        automatic_backup_enabled: row.automatic_backup_enabled != 0,
        automatic_backup_interval_days: row.automatic_backup_interval_days.try_into().unwrap_or(7),
        automatic_backup_retention_count: row
            .automatic_backup_retention_count
            .try_into()
            .unwrap_or(5),
    })
}

pub async fn save_settings(
    pool: &SqlitePool,
    settings: &AppSettingsApi,
) -> CommandResult<AppSettingsApi> {
    if settings.export_filename_pattern.trim().is_empty() {
        return Err(CommandError::validation("导出文件命名规则不能为空。"));
    }
    if !matches!(
        settings.recycle_policy.as_str(),
        "manual_only" | "remind_only"
    ) {
        return Err(CommandError::validation("回收站清理策略无效。"));
    }
    if !(1..=365).contains(&settings.automatic_backup_interval_days) {
        return Err(CommandError::validation(
            "自动备份间隔必须在 1 到 365 天之间。",
        ));
    }
    if !(1..=50).contains(&settings.automatic_backup_retention_count) {
        return Err(CommandError::validation(
            "自动备份保留份数必须在 1 到 50 份之间。",
        ));
    }
    let now = now_millis();
    let confirmations =
        serde_json::json!({ "enabled": settings.optional_confirmations }).to_string();
    sqlx::query(
        "UPDATE app_settings SET default_export_directory = ?, default_template_id = ?, \
         export_filename_pattern = ?, optional_confirmations_json = ?, recent_unused_days = ?, \
         recent_added_days = ?, recent_used_days = ?, recycle_retention_days = ?, \
         recycle_policy = ?, automatic_backup_enabled = ?, \
         automatic_backup_interval_days = ?, automatic_backup_retention_count = ?, \
         updated_at_ms = ? WHERE singleton_id = 1",
    )
    .bind(settings.default_export_directory.trim())
    .bind(&settings.default_template_id)
    .bind(settings.export_filename_pattern.trim())
    .bind(confirmations)
    .bind(i64::from(settings.recent_unused_days))
    .bind(i64::from(settings.recent_added_days))
    .bind(i64::from(settings.recent_used_days))
    .bind(i64::from(settings.recycle_retention_days))
    .bind(&settings.recycle_policy)
    .bind(i64::from(settings.automatic_backup_enabled))
    .bind(i64::from(settings.automatic_backup_interval_days))
    .bind(i64::from(settings.automatic_backup_retention_count))
    .bind(now)
    .execute(pool)
    .await
    .map_err(CommandError::database)?;
    get_settings(pool).await
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
