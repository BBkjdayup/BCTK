//! SQLite service for managed Word-template catalogue records.
//!
//! Filesystem work intentionally lives in `templates_core` and the Tauri
//! command layer. This module only validates catalogue metadata, performs
//! transactional database changes, and writes operation logs.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::{
    models::{CommandError, CommandResult, DocxDiagnosticApi, TemplateAnchorApi, WordTemplateApi},
    templates_core::is_managed_template_file_name,
};

const TEMPLATE_NAME_MAX_CHARS: usize = 120;
const ORIGINAL_FILE_NAME_MAX_CHARS: usize = 255;

/// Metadata produced by the filesystem import core and committed only after
/// the managed copy has been durably created.
#[derive(Clone, Debug)]
pub(crate) struct NewTemplateRecord {
    pub name: String,
    /// Base name only. An external absolute path must never enter SQLite.
    pub original_file_name: String,
    /// A backend-generated, single-component name relative to templates/.
    pub file_rel_path: String,
    pub file_sha256: [u8; 32],
    pub file_byte_size: u64,
    /// Import accepts `ready`, `warning`, or the API-only
    /// `needs_configuration` value. The latter is persisted as `warning`.
    pub analysis_status: String,
    pub analysis_schema_version: u32,
    pub analysis: Value,
    pub parser_version: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ConfiguredTemplateRecord {
    pub original_file_name: String,
    pub file_rel_path: String,
    pub file_sha256: [u8; 32],
    pub file_byte_size: u64,
    pub analysis_status: String,
    pub analysis_schema_version: u32,
    pub analysis: Value,
    pub parser_version: String,
}

/// A catalogue row paired with its private managed file name. Commands use
/// the private value to check file availability, then return only `template`.
#[derive(Clone, Debug)]
pub(crate) struct TemplateCatalogRecord {
    pub template: WordTemplateApi,
    pub managed_file_name: String,
    pub file_sha256: [u8; 32],
    pub analysis: Value,
}

/// The immutable information needed to stage a file deletion before the
/// matching optimistic-lock database delete is committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TemplateDeleteCandidate {
    pub id: String,
    pub managed_file_name: String,
    pub row_version: i64,
}

#[derive(Debug, FromRow)]
struct TemplateRow {
    id: String,
    name: String,
    file_rel_path: String,
    file_sha256: Vec<u8>,
    file_byte_size: i64,
    analysis_status: String,
    analysis_schema_version: i64,
    analysis_json: String,
    parser_version: String,
    row_version: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    last_verified_at_ms: Option<i64>,
    is_default: i64,
}

#[derive(Debug, FromRow)]
struct DeleteRow {
    name: String,
    file_rel_path: String,
    row_version: i64,
    is_default: i64,
}

pub(crate) async fn list_templates(pool: &SqlitePool) -> CommandResult<Vec<TemplateCatalogRecord>> {
    let rows = sqlx::query_as::<_, TemplateRow>(
        "SELECT wt.id, wt.name, wt.file_rel_path, wt.file_sha256, wt.file_byte_size, \
                wt.analysis_status, wt.analysis_schema_version, wt.analysis_json, \
                wt.parser_version, wt.row_version, wt.created_at_ms, wt.updated_at_ms, \
                wt.last_verified_at_ms, \
                CASE WHEN settings.default_template_id = wt.id THEN 1 ELSE 0 END AS is_default \
         FROM word_templates wt CROSS JOIN app_settings settings \
         WHERE settings.singleton_id = 1 \
         ORDER BY is_default DESC, wt.updated_at_ms DESC, wt.id",
    )
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;

    rows.into_iter().map(catalog_record_from_row).collect()
}

pub(crate) async fn get_template(
    pool: &SqlitePool,
    id: &str,
) -> CommandResult<Option<TemplateCatalogRecord>> {
    validate_id(id)?;
    let row = fetch_template_row(pool, id)
        .await
        .map_err(CommandError::database)?;
    row.map(catalog_record_from_row).transpose()
}

/// Commits a successfully analyzed managed file to SQLite. If this returns an
/// error, the command layer must remove the managed file created beforehand.
pub(crate) async fn commit_import_template(
    pool: &SqlitePool,
    record: NewTemplateRecord,
    app_version: &str,
) -> CommandResult<TemplateCatalogRecord> {
    validate_app_version(app_version)?;
    let (name, name_key) = validated_name(&record.name)?;
    validate_original_file_name(&record.original_file_name)?;
    if !is_managed_template_file_name(&record.file_rel_path) {
        return Err(CommandError::new(
            "TEMPLATE_FILE_REFERENCE_UNSAFE",
            "模板内部文件名无效，已取消写入数据库。",
        ));
    }
    if record.analysis_schema_version == 0 {
        return Err(CommandError::validation("模板分析结构版本必须大于零。"));
    }
    if record.parser_version.trim().is_empty() {
        return Err(CommandError::validation("模板解析器版本不能为空。"));
    }
    let file_byte_size = i64::try_from(record.file_byte_size)
        .map_err(|_| CommandError::validation("模板文件过大，无法安全写入本地数据库。"))?;

    let mut analysis = normalized_analysis(record.analysis, &record.original_file_name)?;
    let needs_configuration = analysis_needs_configuration(&analysis);
    set_analysis_flag(&mut analysis, "needsConfiguration", needs_configuration)?;
    let stored_status = stored_analysis_status(&record.analysis_status, needs_configuration)?;
    let analysis_json = serde_json::to_string(&analysis).map_err(|error| {
        CommandError::new(
            "TEMPLATE_ANALYSIS_INVALID",
            format!("模板分析结果无法保存：{error}"),
        )
    })?;
    let id = Uuid::now_v7().to_string();
    let now = now_millis();

    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    if template_name_exists(&mut transaction, &name_key, None).await? {
        return Err(name_conflict());
    }

    sqlx::query(
        "INSERT INTO word_templates (id, name, name_key, file_rel_path, file_sha256, \
         file_byte_size, analysis_status, analysis_schema_version, analysis_json, \
         parser_version, row_version, created_at_ms, updated_at_ms, last_verified_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&name_key)
    .bind(&record.file_rel_path)
    .bind(record.file_sha256.as_slice())
    .bind(file_byte_size)
    .bind(stored_status)
    .bind(i64::from(record.analysis_schema_version))
    .bind(&analysis_json)
    .bind(record.parser_version.trim())
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(map_template_write_error)?;

    write_log(
        &mut transaction,
        now,
        "template.import",
        Some(&id),
        "导入并登记 Word 模板",
        json!({
            "name": name,
            "originalFileName": record.original_file_name,
            "managedFileName": record.file_rel_path,
            "fileByteSize": record.file_byte_size,
            "analysisStatus": stored_status,
            "needsConfiguration": needs_configuration,
        }),
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;

    get_template(pool, &id)
        .await?
        .ok_or_else(|| CommandError::database("模板导入提交后未能重新读取记录。"))
}

pub(crate) async fn rename_template(
    pool: &SqlitePool,
    id: &str,
    requested_name: &str,
    base_row_version: i64,
    app_version: &str,
) -> CommandResult<TemplateCatalogRecord> {
    validate_id(id)?;
    validate_base_version(base_row_version)?;
    validate_app_version(app_version)?;
    let (name, name_key) = validated_name(requested_name)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    let current = sqlx::query_as::<_, (String, i64)>(
        "SELECT name, row_version FROM word_templates WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(not_found)?;
    if current.1 != base_row_version {
        return Err(version_conflict());
    }
    if template_name_exists(&mut transaction, &name_key, Some(id)).await? {
        return Err(name_conflict());
    }

    let result = sqlx::query(
        "UPDATE word_templates SET name = ?, name_key = ?, row_version = row_version + 1, \
         updated_at_ms = ? WHERE id = ? AND row_version = ?",
    )
    .bind(&name)
    .bind(&name_key)
    .bind(now)
    .bind(id)
    .bind(base_row_version)
    .execute(&mut *transaction)
    .await
    .map_err(map_template_write_error)?;
    if result.rows_affected() != 1 {
        return Err(version_conflict());
    }

    write_log(
        &mut transaction,
        now,
        "template.rename",
        Some(id),
        "重命名 Word 模板",
        json!({
            "oldName": current.0,
            "newName": name,
            "baseRowVersion": base_row_version,
            "rowVersion": base_row_version.saturating_add(1),
        }),
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;

    get_template(pool, id)
        .await?
        .ok_or_else(|| CommandError::database("模板重命名后未能重新读取记录。"))
}

pub(crate) async fn commit_template_configuration(
    pool: &SqlitePool,
    id: &str,
    base_row_version: i64,
    record: ConfiguredTemplateRecord,
    app_version: &str,
) -> CommandResult<TemplateCatalogRecord> {
    validate_id(id)?;
    validate_base_version(base_row_version)?;
    validate_app_version(app_version)?;
    validate_original_file_name(&record.original_file_name)?;
    if !is_managed_template_file_name(&record.file_rel_path) {
        return Err(CommandError::new(
            "TEMPLATE_FILE_REFERENCE_UNSAFE",
            "新模板副本的内部文件名无效，已取消配置。",
        ));
    }
    if record.analysis_schema_version == 0 || record.parser_version.trim().is_empty() {
        return Err(CommandError::validation("模板配置分析版本无效。"));
    }
    let file_byte_size = i64::try_from(record.file_byte_size)
        .map_err(|_| CommandError::validation("配置后的模板文件过大。"))?;
    let mut analysis = normalized_analysis(record.analysis, &record.original_file_name)?;
    if analysis_needs_configuration(&analysis) {
        return Err(CommandError::new(
            "TEMPLATE_CONFIGURATION_INCOMPLETE",
            "配置后的模板仍然缺少题目替换区域。",
        ));
    }
    set_analysis_flag(&mut analysis, "needsConfiguration", false)?;
    let stored_status = stored_analysis_status(&record.analysis_status, false)?;
    let analysis_json = serde_json::to_string(&analysis).map_err(|error| {
        CommandError::new(
            "TEMPLATE_ANALYSIS_INVALID",
            format!("配置后的模板分析结果无法保存：{error}"),
        )
    })?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let current = sqlx::query_as::<_, (String, i64)>(
        "SELECT name, row_version FROM word_templates WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(not_found)?;
    if current.1 != base_row_version {
        return Err(version_conflict());
    }

    let result = sqlx::query(
        "UPDATE word_templates SET file_rel_path = ?, file_sha256 = ?, file_byte_size = ?, \
         analysis_status = ?, analysis_schema_version = ?, analysis_json = ?, parser_version = ?, \
         row_version = row_version + 1, updated_at_ms = ?, last_verified_at_ms = ? \
         WHERE id = ? AND row_version = ?",
    )
    .bind(&record.file_rel_path)
    .bind(record.file_sha256.as_slice())
    .bind(file_byte_size)
    .bind(stored_status)
    .bind(i64::from(record.analysis_schema_version))
    .bind(analysis_json)
    .bind(record.parser_version.trim())
    .bind(now)
    .bind(now)
    .bind(id)
    .bind(base_row_version)
    .execute(&mut *transaction)
    .await
    .map_err(map_template_write_error)?;
    if result.rows_affected() != 1 {
        return Err(version_conflict());
    }
    write_log(
        &mut transaction,
        now,
        "template.configure_regions",
        Some(id),
        "配置 Word 模板替换区域",
        json!({
            "name": current.0,
            "managedFileName": record.file_rel_path,
            "baseRowVersion": base_row_version,
            "rowVersion": base_row_version.saturating_add(1),
        }),
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    get_template(pool, id)
        .await?
        .ok_or_else(|| CommandError::database("模板配置后未能重新读取记录。"))
}

pub(crate) async fn get_delete_candidate(
    pool: &SqlitePool,
    id: &str,
    base_row_version: i64,
) -> CommandResult<TemplateDeleteCandidate> {
    validate_id(id)?;
    validate_base_version(base_row_version)?;
    let row = sqlx::query_as::<_, (String, i64)>(
        "SELECT file_rel_path, row_version FROM word_templates WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(not_found)?;
    if row.1 != base_row_version {
        return Err(version_conflict());
    }
    if !is_managed_template_file_name(&row.0) {
        return Err(CommandError::new(
            "TEMPLATE_FILE_REFERENCE_UNSAFE",
            "数据库中的模板文件引用不安全，已停止删除。",
        ));
    }
    Ok(TemplateDeleteCandidate {
        id: id.to_owned(),
        managed_file_name: row.0,
        row_version: row.1,
    })
}

/// Deletes only the catalogue record. The command layer must first stage the
/// corresponding file using `get_delete_candidate`, restore it on error, and
/// finalize the staged deletion only after this transaction succeeds.
pub(crate) async fn commit_delete_template(
    pool: &SqlitePool,
    id: &str,
    base_row_version: i64,
    app_version: &str,
) -> CommandResult<()> {
    validate_id(id)?;
    validate_base_version(base_row_version)?;
    validate_app_version(app_version)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let current = sqlx::query_as::<_, DeleteRow>(
        "SELECT wt.name, wt.file_rel_path, wt.row_version, \
                CASE WHEN settings.default_template_id = wt.id THEN 1 ELSE 0 END AS is_default \
         FROM word_templates wt CROSS JOIN app_settings settings \
         WHERE settings.singleton_id = 1 AND wt.id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(not_found)?;
    if current.row_version != base_row_version {
        return Err(version_conflict());
    }
    if !is_managed_template_file_name(&current.file_rel_path) {
        return Err(CommandError::new(
            "TEMPLATE_FILE_REFERENCE_UNSAFE",
            "数据库中的模板文件引用不安全，已停止删除。",
        ));
    }

    let result = sqlx::query("DELETE FROM word_templates WHERE id = ? AND row_version = ?")
        .bind(id)
        .bind(base_row_version)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    if result.rows_affected() != 1 {
        return Err(version_conflict());
    }

    write_log(
        &mut transaction,
        now,
        "template.delete",
        Some(id),
        "删除 Word 模板目录记录",
        json!({
            "name": current.name,
            "managedFileName": current.file_rel_path,
            "baseRowVersion": base_row_version,
            "wasDefault": current.is_default == 1,
        }),
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub(crate) async fn set_default_template(
    pool: &SqlitePool,
    id: &str,
    base_row_version: i64,
    app_version: &str,
) -> CommandResult<TemplateCatalogRecord> {
    validate_id(id)?;
    validate_base_version(base_row_version)?;
    validate_app_version(app_version)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    let current = sqlx::query_as::<_, (String, i64, String)>(
        "SELECT name, row_version, analysis_json FROM word_templates WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(not_found)?;
    if current.1 != base_row_version {
        return Err(version_conflict());
    }
    let analysis: Value = serde_json::from_str(&current.2)
        .map_err(|error| corrupted_analysis(format!("分析 JSON 无法读取：{error}")))?;
    if !has_unique_questions_anchor(&analysis)? {
        return Err(CommandError::new(
            "TEMPLATE_CONFIGURATION_REQUIRED",
            "请先为模板配置唯一的题目替换区域，再设为默认模板。",
        ));
    }
    let previous: Option<String> =
        sqlx::query_scalar("SELECT default_template_id FROM app_settings WHERE singleton_id = 1")
            .fetch_one(&mut *transaction)
            .await
            .map_err(CommandError::database)?;

    let result = sqlx::query(
        "UPDATE app_settings SET default_template_id = ?, updated_at_ms = ? \
         WHERE singleton_id = 1",
    )
    .bind(id)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    if result.rows_affected() != 1 {
        return Err(CommandError::database("应用设置记录不存在。"));
    }

    write_log(
        &mut transaction,
        now,
        "template.set_default",
        Some(id),
        "设置默认 Word 模板",
        json!({
            "name": current.0,
            "previousTemplateId": previous,
            "baseRowVersion": base_row_version,
        }),
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;

    get_template(pool, id)
        .await?
        .ok_or_else(|| CommandError::database("设置默认模板后未能重新读取记录。"))
}

/// Records a non-fatal filesystem cleanup problem after the database delete
/// has already committed. Callers may deliberately treat an error from this
/// function as best-effort because the primary delete transaction is final.
pub(crate) async fn record_template_cleanup_warning(
    pool: &SqlitePool,
    id: &str,
    error_message: &str,
    app_version: &str,
) -> CommandResult<()> {
    validate_id(id)?;
    validate_app_version(app_version)?;
    let message = error_message.trim();
    if message.is_empty() {
        return Err(CommandError::validation("模板清理错误信息不能为空。"));
    }
    let message = message.chars().take(4_000).collect::<String>();
    let details = json!({ "error": message }).to_string();
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, entity_id, \
         outcome, summary, details_json, error_code, app_version) \
         VALUES (?, ?, 'warning', 'template.delete.cleanup', 'word_template', ?, 'partial', \
         '模板目录记录已删除，但托管文件清理未完成', ?, 'TEMPLATE_FILE_CLEANUP_FAILED', ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(now_millis())
    .bind(id)
    .bind(details)
    .bind(app_version)
    .execute(pool)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

async fn fetch_template_row(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<TemplateRow>, sqlx::Error> {
    sqlx::query_as::<_, TemplateRow>(
        "SELECT wt.id, wt.name, wt.file_rel_path, wt.file_sha256, wt.file_byte_size, \
                wt.analysis_status, wt.analysis_schema_version, wt.analysis_json, \
                wt.parser_version, wt.row_version, wt.created_at_ms, wt.updated_at_ms, \
                wt.last_verified_at_ms, \
                CASE WHEN settings.default_template_id = wt.id THEN 1 ELSE 0 END AS is_default \
         FROM word_templates wt CROSS JOIN app_settings settings \
         WHERE settings.singleton_id = 1 AND wt.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

fn catalog_record_from_row(row: TemplateRow) -> CommandResult<TemplateCatalogRecord> {
    if !is_managed_template_file_name(&row.file_rel_path) {
        return Err(CommandError::new(
            "TEMPLATE_FILE_REFERENCE_UNSAFE",
            format!("模板“{}”的内部文件引用不安全。", row.name),
        ));
    }
    let file_sha256: [u8; 32] = row
        .file_sha256
        .as_slice()
        .try_into()
        .map_err(|_| corrupted_analysis("模板文件哈希长度无效。"))?;
    let analysis: Value = serde_json::from_str(&row.analysis_json)
        .map_err(|error| corrupted_analysis(format!("分析 JSON 无法读取：{error}")))?;
    let anchors = analysis_array::<TemplateAnchorApi>(&analysis, "anchors")?;
    let mut diagnostics = analysis_array::<DocxDiagnosticApi>(&analysis, "diagnostics")?;
    let required_fields = [
        ("originalFileName", "original_file_name"),
        ("packageKind", "package_kind"),
        ("archiveBytes", "archive_bytes"),
        ("partCount", "part_count"),
        ("totalUncompressedBytes", "total_uncompressed_bytes"),
        ("anchors", "anchors"),
        ("diagnostics", "diagnostics"),
    ];
    let missing_fields = required_fields
        .into_iter()
        .filter(|(camel, snake)| analysis.get(*camel).is_none() && analysis.get(*snake).is_none())
        .map(|(camel, _)| camel)
        .collect::<Vec<_>>();
    if !missing_fields.is_empty() {
        diagnostics.push(DocxDiagnosticApi {
            severity: "warning".to_owned(),
            code: "TEMPLATE_ANALYSIS_FIELD_MISSING".to_owned(),
            part_name: None,
            message: format!(
                "旧模板分析记录缺少字段：{}；已使用安全默认值。",
                missing_fields.join(", ")
            ),
            suggested_action: Some("重新导入该模板以刷新分析信息。".to_owned()),
        });
    }
    let region_configured = !anchors.is_empty();
    // The API-only state is always derived from the authoritative anchor
    // collection. SQLite stores this case as `warning` because schema 0005
    // intentionally has no `needs_configuration` value.
    let needs_configuration = !region_configured;
    let analysis_status = if needs_configuration {
        "needs_configuration".to_owned()
    } else {
        row.analysis_status
    };
    let package_kind = analysis
        .get("packageKind")
        .or_else(|| analysis.get("package_kind"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let candidate_file_name = analysis
        .get("originalFileName")
        .or_else(|| analysis.get("original_file_name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned);
    let file_name = match candidate_file_name {
        Some(name) if is_safe_original_file_name(&name) => name,
        Some(_) => {
            diagnostics.push(DocxDiagnosticApi {
                severity: "warning".to_owned(),
                code: "TEMPLATE_ORIGINAL_FILENAME_INVALID".to_owned(),
                part_name: None,
                message: "模板记录中的原文件名无效，已使用安全显示名称。".to_owned(),
                suggested_action: Some("重新导入该模板以刷新分析信息。".to_owned()),
            });
            fallback_display_file_name(&row.name, &row.file_rel_path)
        }
        None => fallback_display_file_name(&row.name, &row.file_rel_path),
    };
    let file_byte_size = u64::try_from(row.file_byte_size)
        .map_err(|_| corrupted_analysis("模板文件大小为负数。"))?;
    let analysis_schema_version = u32::try_from(row.analysis_schema_version)
        .map_err(|_| corrupted_analysis("模板分析结构版本超出支持范围。"))?;

    Ok(TemplateCatalogRecord {
        managed_file_name: row.file_rel_path,
        file_sha256,
        analysis,
        template: WordTemplateApi {
            id: row.id,
            name: row.name,
            file_name,
            file_sha256_hex: bytes_to_lower_hex(&file_sha256),
            file_byte_size,
            analysis_status,
            analysis_schema_version,
            parser_version: row.parser_version,
            row_version: row.row_version,
            created_at: row.created_at_ms,
            updated_at: row.updated_at_ms,
            last_verified_at: row.last_verified_at_ms,
            is_default: row.is_default == 1,
            // The command layer checks the private managed file name and may
            // flip this to false before serializing the API response.
            file_available: true,
            region_configured,
            package_kind,
            anchors,
            diagnostics,
        },
    })
}

fn normalized_analysis(mut analysis: Value, original_file_name: &str) -> CommandResult<Value> {
    let object = analysis.as_object_mut().ok_or_else(|| {
        CommandError::new(
            "TEMPLATE_ANALYSIS_INVALID",
            "模板分析结果必须是一个 JSON 对象。",
        )
    })?;
    object.insert(
        "originalFileName".to_owned(),
        Value::String(original_file_name.to_owned()),
    );
    object.entry("anchors").or_insert_with(|| json!([]));
    object.entry("diagnostics").or_insert_with(|| json!([]));
    if !object.get("anchors").is_some_and(Value::is_array)
        || !object.get("diagnostics").is_some_and(Value::is_array)
    {
        return Err(CommandError::new(
            "TEMPLATE_ANALYSIS_INVALID",
            "模板分析结果中的 anchors 和 diagnostics 必须是数组。",
        ));
    }
    Ok(analysis)
}

fn set_analysis_flag(analysis: &mut Value, key: &str, value: bool) -> CommandResult<()> {
    analysis
        .as_object_mut()
        .ok_or_else(|| corrupted_analysis("模板分析结果不是 JSON 对象。"))?
        .insert(key.to_owned(), Value::Bool(value));
    Ok(())
}

fn analysis_needs_configuration(analysis: &Value) -> bool {
    analysis
        .get("anchors")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
}

fn has_unique_questions_anchor(analysis: &Value) -> CommandResult<bool> {
    Ok(analysis_array::<TemplateAnchorApi>(analysis, "anchors")?
        .into_iter()
        .filter(|anchor| anchor.name.trim().eq_ignore_ascii_case("ZT_QUESTIONS"))
        .count()
        == 1)
}

fn stored_analysis_status(status: &str, needs_configuration: bool) -> CommandResult<&'static str> {
    match status.trim() {
        "ready" if !needs_configuration => Ok("ready"),
        "ready" | "warning" | "needs_configuration" => Ok("warning"),
        _ => Err(CommandError::validation(
            "导入模板的分析状态只能是 ready、warning 或 needs_configuration。",
        )),
    }
}

fn analysis_array<T: DeserializeOwned>(analysis: &Value, key: &str) -> CommandResult<Vec<T>> {
    let value = analysis.get(key).cloned().unwrap_or_else(|| json!([]));
    serde_json::from_value(value)
        .map_err(|error| corrupted_analysis(format!("模板分析字段 {key} 无法读取：{error}")))
}

fn validated_name(value: &str) -> CommandResult<(String, String)> {
    let normalized = value.trim().nfkc().collect::<String>();
    let normalized = normalized.trim().to_owned();
    if normalized.is_empty() {
        return Err(CommandError::validation("模板名称不能为空。"));
    }
    if normalized.chars().count() > TEMPLATE_NAME_MAX_CHARS {
        return Err(CommandError::validation(format!(
            "模板名称不能超过 {TEMPLATE_NAME_MAX_CHARS} 个字符。"
        )));
    }
    if normalized.chars().any(char::is_control) {
        return Err(CommandError::validation("模板名称不能包含控制字符。"));
    }
    let key = normalize_name_key(&normalized);
    Ok((normalized, key))
}

fn normalize_name_key(value: &str) -> String {
    value.trim().nfkc().flat_map(char::to_lowercase).collect()
}

fn validate_original_file_name(value: &str) -> CommandResult<()> {
    if !is_safe_original_file_name(value) {
        return Err(CommandError::validation(
            "模板原文件名无效；只能保存不含路径的文件名。",
        ));
    }
    Ok(())
}

fn is_safe_original_file_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed == value
        && trimmed.chars().count() <= ORIGINAL_FILE_NAME_MAX_CHARS
        && Path::new(trimmed)
            .file_name()
            .and_then(|name| name.to_str())
            == Some(trimmed)
        && !trimmed.contains('/')
        && !trimmed.contains('\\')
        && !trimmed.chars().any(char::is_control)
}

fn validate_id(id: &str) -> CommandResult<()> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| CommandError::validation("模板 ID 无效。"))
}

fn validate_base_version(version: i64) -> CommandResult<()> {
    if version < 1 {
        Err(CommandError::validation("模板行版本必须大于零。"))
    } else {
        Ok(())
    }
}

fn validate_app_version(version: &str) -> CommandResult<()> {
    if version.trim().is_empty() {
        Err(CommandError::validation("应用版本不能为空。"))
    } else {
        Ok(())
    }
}

async fn template_name_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    name_key: &str,
    excluding_id: Option<&str>,
) -> CommandResult<bool> {
    let exists: i64 = if let Some(id) = excluding_id {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM word_templates WHERE name_key = ? AND id <> ?)",
        )
        .bind(name_key)
        .bind(id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(CommandError::database)?
    } else {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM word_templates WHERE name_key = ?)")
            .bind(name_key)
            .fetch_one(&mut **transaction)
            .await
            .map_err(CommandError::database)?
    };
    Ok(exists == 1)
}

async fn write_log(
    transaction: &mut Transaction<'_, Sqlite>,
    now: i64,
    action: &str,
    entity_id: Option<&str>,
    summary: &str,
    details: Value,
    app_version: &str,
) -> CommandResult<()> {
    let details_json = serde_json::to_string(&details).map_err(CommandError::database)?;
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, entity_id, \
         outcome, summary, details_json, app_version) \
         VALUES (?, ?, 'info', ?, 'word_template', ?, 'success', ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(now)
    .bind(action)
    .bind(entity_id)
    .bind(summary)
    .bind(details_json)
    .bind(app_version)
    .execute(&mut **transaction)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

fn map_template_write_error(error: sqlx::Error) -> CommandError {
    if let sqlx::Error::Database(database) = &error {
        let message = database.message();
        if message.contains("word_templates.name_key") {
            return name_conflict();
        }
        if message.contains("word_templates.file_rel_path") {
            return CommandError::new(
                "TEMPLATE_FILE_ALREADY_MANAGED",
                "该内部模板文件已登记，请重新导入。",
            );
        }
    }
    CommandError::database(error)
}

fn fallback_display_file_name(name: &str, managed_file_name: &str) -> String {
    let extension = Path::new(managed_file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("docx");
    format!("{name}.{extension}")
}

fn name_conflict() -> CommandError {
    CommandError::new(
        "TEMPLATE_NAME_CONFLICT",
        "已经存在同名模板。名称比较会忽略全角/半角差异和字母大小写。",
    )
}

fn not_found() -> CommandError {
    CommandError::new("TEMPLATE_NOT_FOUND", "找不到指定的模板。")
}

fn version_conflict() -> CommandError {
    CommandError::new(
        "TEMPLATE_VERSION_CONFLICT",
        "该模板已在其他窗口中更新，请刷新模板列表后重试。",
    )
}

fn corrupted_analysis(message: impl Into<String>) -> CommandError {
    CommandError::new("TEMPLATE_CATALOG_CORRUPTED", message)
}

fn bytes_to_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_names_are_trimmed_nfkc_normalized_and_case_folded_for_uniqueness() {
        let (name, key) = validated_name("  Ａ４ 模板  ").unwrap();
        assert_eq!(name, "A4 模板");
        assert_eq!(key, "a4 模板");
    }

    #[test]
    fn anchorless_analysis_is_stored_as_warning_and_exposed_as_configuration_needed() {
        let mut analysis = normalized_analysis(
            json!({ "anchors": [], "diagnostics": [], "packageKind": "document" }),
            "试卷.docx",
        )
        .unwrap();
        assert!(analysis_needs_configuration(&analysis));
        set_analysis_flag(&mut analysis, "needsConfiguration", true).unwrap();
        assert_eq!(stored_analysis_status("ready", true).unwrap(), "warning");
        assert_eq!(analysis["needsConfiguration"], true);
    }

    #[test]
    fn original_file_name_never_accepts_a_path() {
        assert!(validate_original_file_name("试卷.docx").is_ok());
        assert!(validate_original_file_name("../试卷.docx").is_err());
        assert!(validate_original_file_name(r"C:\\Users\\teacher\\试卷.docx").is_err());
    }

    #[test]
    fn camel_case_anchor_json_preserves_replacement_and_container_spans() {
        let analysis = json!({
            "anchors": [{
                "name": "ZT_QUESTIONS",
                "kind": "paragraph_marker",
                "partName": "word/document.xml",
                "replacementSpan": { "start": 12, "end": 34 },
                "containerSpan": { "start": 10, "end": 40 },
                "paragraphIndex": 2
            }]
        });

        let anchors = analysis_array::<TemplateAnchorApi>(&analysis, "anchors").unwrap();
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].replacement_span.start, 12);
        assert_eq!(anchors[0].replacement_span.end, 34);
        assert_eq!(anchors[0].container_span.start, 10);
        assert_eq!(anchors[0].container_span.end, 40);
        assert_eq!(anchors[0].paragraph_index, Some(2));
    }

    #[test]
    fn default_template_requires_exactly_one_questions_anchor() {
        let anchor = |name: &str| {
            json!({
                "name": name,
                "kind": "paragraph_marker",
                "partName": "word/document.xml",
                "replacementSpan": { "start": 12, "end": 34 },
                "containerSpan": { "start": 10, "end": 40 },
                "paragraphIndex": 2
            })
        };
        assert!(!has_unique_questions_anchor(&json!({ "anchors": [] })).unwrap());
        assert!(!has_unique_questions_anchor(&json!({ "anchors": [anchor("ZT_TITLE")] })).unwrap());
        assert!(
            has_unique_questions_anchor(&json!({ "anchors": [anchor("ZT_QUESTIONS")] })).unwrap()
        );
        assert!(
            !has_unique_questions_anchor(&json!({
                "anchors": [anchor("ZT_QUESTIONS"), anchor("zt_questions")]
            }))
            .unwrap()
        );
    }
}
