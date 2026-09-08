use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{FromRow, SqlitePool};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::models::{
    CommandError, CommandResult, DocumentEntryTemplateApi, DocumentEntryTemplateConfigApi,
    SaveDocumentEntryTemplateRequestApi,
};

pub(crate) const DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID: &str = "labeled_fields_v1";
const MAX_TEMPLATE_COUNT: i64 = 50;
const MAX_NAME_CHARS: usize = 40;
const MAX_MARKER_CHARS: usize = 24;
const MAX_PROMPT_CHARS: usize = 100;

#[derive(Debug, FromRow)]
struct DocumentEntryTemplateRow {
    id: String,
    name: String,
    config_json: String,
    is_builtin: i64,
    is_default: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

pub(crate) async fn list(pool: &SqlitePool) -> CommandResult<Vec<DocumentEntryTemplateApi>> {
    let rows = sqlx::query_as::<_, DocumentEntryTemplateRow>(
        "SELECT id, name, config_json, is_builtin, is_default, created_at_ms, updated_at_ms \
         FROM document_entry_templates \
         ORDER BY is_default DESC, is_builtin DESC, updated_at_ms DESC, name",
    )
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;
    rows.into_iter().map(template_from_row).collect()
}

pub(crate) async fn save(
    pool: &SqlitePool,
    request: &SaveDocumentEntryTemplateRequestApi,
) -> CommandResult<DocumentEntryTemplateApi> {
    let name = request.name.trim().nfc().collect::<String>();
    validate_name(&name)?;
    validate_document_entry_template_config(&request.config)?;
    let config_json = serde_json::to_string(&request.config).map_err(CommandError::database)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    let existing = if let Some(id) = request.id.as_deref() {
        let id = canonical_custom_id(id)?;
        let existing = sqlx::query_as::<_, DocumentEntryTemplateRow>(
            "SELECT id, name, config_json, is_builtin, is_default, created_at_ms, updated_at_ms \
             FROM document_entry_templates WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(CommandError::database)?
        .ok_or_else(|| CommandError::validation("要修改的录题模板已经不存在。"))?;
        if existing.is_builtin != 0 {
            return Err(CommandError::validation(
                "系统默认模板不能直接修改，请先复制成自定义模板。",
            ));
        }
        Some(existing)
    } else {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM document_entry_templates")
            .fetch_one(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
        if count >= MAX_TEMPLATE_COUNT {
            return Err(CommandError::validation(
                "录题模板最多保存 50 套，请删除不再使用的模板后重试。",
            ));
        }
        None
    };

    let duplicate: Option<String> = sqlx::query_scalar(
        "SELECT id FROM document_entry_templates WHERE name = ? COLLATE NOCASE AND id <> ?",
    )
    .bind(&name)
    .bind(existing.as_ref().map(|row| row.id.as_str()).unwrap_or(""))
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    if duplicate.is_some() {
        return Err(CommandError::validation("已经存在同名录题模板。"));
    }

    if request.set_as_default {
        sqlx::query("UPDATE document_entry_templates SET is_default = 0 WHERE is_default = 1")
            .execute(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    }

    let id = existing
        .as_ref()
        .map(|row| row.id.clone())
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let created_at = existing
        .as_ref()
        .map(|row| row.created_at_ms)
        .unwrap_or(now);
    let is_default =
        request.set_as_default || existing.as_ref().is_some_and(|row| row.is_default != 0);

    sqlx::query(
        "INSERT INTO document_entry_templates \
             (id, name, config_json, is_builtin, is_default, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, 0, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
             name = excluded.name, config_json = excluded.config_json, \
             is_default = excluded.is_default, updated_at_ms = excluded.updated_at_ms",
    )
    .bind(&id)
    .bind(&name)
    .bind(config_json)
    .bind(i64::from(is_default))
    .bind(created_at)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    transaction.commit().await.map_err(CommandError::database)?;
    get(pool, &id).await
}

pub(crate) async fn set_default(
    pool: &SqlitePool,
    id: &str,
) -> CommandResult<DocumentEntryTemplateApi> {
    validate_template_id(id)?;
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM document_entry_templates WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    if exists != 1 {
        return Err(CommandError::validation("要设为默认的录题模板已经不存在。"));
    }
    sqlx::query("UPDATE document_entry_templates SET is_default = 0 WHERE is_default = 1")
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    sqlx::query(
        "UPDATE document_entry_templates SET is_default = 1, updated_at_ms = ? WHERE id = ?",
    )
    .bind(now_millis())
    .bind(id)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)?;
    get(pool, id).await
}

pub(crate) async fn delete(pool: &SqlitePool, id: &str) -> CommandResult<()> {
    let id = canonical_custom_id(id)?;
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let row = sqlx::query_as::<_, DocumentEntryTemplateRow>(
        "SELECT id, name, config_json, is_builtin, is_default, created_at_ms, updated_at_ms \
         FROM document_entry_templates WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::validation("要删除的录题模板已经不存在。"))?;
    if row.is_builtin != 0 {
        return Err(CommandError::validation("系统默认模板不能删除。"));
    }
    if row.is_default != 0 {
        sqlx::query("UPDATE document_entry_templates SET is_default = 0 WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
        sqlx::query(
            "UPDATE document_entry_templates SET is_default = 1, updated_at_ms = ? WHERE id = ?",
        )
        .bind(now_millis())
        .bind(DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    }
    sqlx::query("DELETE FROM document_entry_templates WHERE id = ? AND is_builtin = 0")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

async fn get(pool: &SqlitePool, id: &str) -> CommandResult<DocumentEntryTemplateApi> {
    let row = sqlx::query_as::<_, DocumentEntryTemplateRow>(
        "SELECT id, name, config_json, is_builtin, is_default, created_at_ms, updated_at_ms \
         FROM document_entry_templates WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::validation("录题模板已经不存在。"))?;
    template_from_row(row)
}

fn template_from_row(row: DocumentEntryTemplateRow) -> CommandResult<DocumentEntryTemplateApi> {
    validate_template_id(&row.id)
        .map_err(|error| corrupted(format!("录题模板 ID 无效：{}", error.message)))?;
    validate_name(&row.name)
        .map_err(|error| corrupted(format!("录题模板名称无效：{}", error.message)))?;
    if !matches!(row.is_builtin, 0 | 1)
        || !matches!(row.is_default, 0 | 1)
        || row.created_at_ms < 0
        || row.updated_at_ms < row.created_at_ms
    {
        return Err(corrupted("录题模板的状态或时间字段无效。"));
    }
    let config: DocumentEntryTemplateConfigApi = serde_json::from_str(&row.config_json)
        .map_err(|error| corrupted(format!("录题模板配置无法读取：{error}")))?;
    validate_document_entry_template_config(&config)
        .map_err(|error| corrupted(format!("录题模板配置无效：{}", error.message)))?;
    Ok(DocumentEntryTemplateApi {
        id: row.id,
        name: row.name,
        config,
        is_built_in: row.is_builtin != 0,
        is_default: row.is_default != 0,
        created_at: row.created_at_ms,
        updated_at: row.updated_at_ms,
    })
}

pub(crate) fn validate_document_entry_template_config(
    config: &DocumentEntryTemplateConfigApi,
) -> CommandResult<()> {
    if config.schema_version != 1 {
        return Err(CommandError::validation("录题模板的数据版本无效。"));
    }
    let markers = [
        ("题型标记", config.type_marker.as_str()),
        ("题目标记", config.stem_marker.as_str()),
        ("答案标记", config.answer_marker.as_str()),
        ("解析标记", config.explanation_marker.as_str()),
    ];
    for (label, marker) in markers {
        validate_single_line(label, marker, MAX_MARKER_CHARS, false)?;
    }
    let comparable = markers.map(|(_, marker)| {
        marker
            .nfkc()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    });
    for left in 0..comparable.len() {
        for right in 0..comparable.len() {
            if left != right && comparable[left].starts_with(&comparable[right]) {
                return Err(CommandError::validation(
                    "字段标记不能相同或互为开头，否则识别时无法区分。",
                ));
            }
        }
    }
    validate_single_line(
        "题目分隔线",
        &config.question_separator,
        MAX_MARKER_CHARS,
        true,
    )?;
    for (label, prompt) in [
        ("题目提示", config.stem_prompt.as_str()),
        ("选项提示", config.option_prompt.as_str()),
        ("答案提示", config.answer_prompt.as_str()),
        ("解析提示", config.explanation_prompt.as_str()),
    ] {
        validate_single_line(label, prompt, MAX_PROMPT_CHARS, true)?;
    }
    Ok(())
}

fn validate_name(name: &str) -> CommandResult<()> {
    validate_single_line("模板名称", name, MAX_NAME_CHARS, false)
}

fn validate_single_line(
    label: &str,
    value: &str,
    maximum: usize,
    allow_empty: bool,
) -> CommandResult<()> {
    if !allow_empty && value.trim().is_empty() {
        return Err(CommandError::validation(format!("{label}不能为空。")));
    }
    if value.chars().count() > maximum {
        return Err(CommandError::validation(format!(
            "{label}不能超过 {maximum} 个字符。"
        )));
    }
    if value
        .chars()
        .any(|character| character == '\r' || character == '\n' || character.is_control())
    {
        return Err(CommandError::validation(format!(
            "{label}不能包含换行或控制字符。"
        )));
    }
    Ok(())
}

pub(crate) fn validate_template_id(id: &str) -> CommandResult<()> {
    if id == DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID {
        return Ok(());
    }
    canonical_custom_id(id).map(|_| ())
}

fn canonical_custom_id(id: &str) -> CommandResult<&str> {
    let parsed = Uuid::parse_str(id).map_err(|_| CommandError::validation("录题模板 ID 无效。"))?;
    if parsed.to_string() != id {
        return Err(CommandError::validation("录题模板 ID 必须使用规范 UUID。"));
    }
    Ok(id)
}

fn corrupted(message: impl Into<String>) -> CommandError {
    CommandError::new("DATABASE_CORRUPTED", message)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::DocumentEntryOptionStyleApi;
    use crate::db::Database;
    use std::fs;

    fn config() -> DocumentEntryTemplateConfigApi {
        DocumentEntryTemplateConfigApi {
            schema_version: 1,
            type_marker: "题型：".to_owned(),
            stem_marker: "题目：".to_owned(),
            answer_marker: "答案：".to_owned(),
            explanation_marker: "解析：".to_owned(),
            option_style: DocumentEntryOptionStyleApi::Dot,
            question_separator: "---".to_owned(),
            compatible_default_markers: true,
            recognize_question_numbers: true,
            strip_recognized_question_numbers: true,
            stem_prompt: "题干".to_owned(),
            option_prompt: "选项".to_owned(),
            answer_prompt: "答案".to_owned(),
            explanation_prompt: "解析".to_owned(),
        }
    }

    #[test]
    fn accepts_a_safe_literal_configuration() {
        assert!(validate_document_entry_template_config(&config()).is_ok());
    }

    #[test]
    fn legacy_configuration_enables_question_numbers_by_default() {
        let value = serde_json::json!({
            "schemaVersion": 1,
            "typeMarker": "题型：",
            "stemMarker": "题目：",
            "answerMarker": "答案：",
            "explanationMarker": "解析：",
            "optionStyle": "letter_dot",
            "questionSeparator": "---",
            "compatibleDefaultMarkers": true,
            "stemPrompt": "题干",
            "optionPrompt": "选项",
            "answerPrompt": "答案",
            "explanationPrompt": "解析"
        });
        let restored: DocumentEntryTemplateConfigApi = serde_json::from_value(value).unwrap();
        assert!(restored.recognize_question_numbers);
        assert!(restored.strip_recognized_question_numbers);
    }

    #[test]
    fn rejects_ambiguous_markers() {
        let mut value = config();
        value.stem_marker = "题型：内容".to_owned();
        assert!(validate_document_entry_template_config(&value).is_err());
    }

    #[tokio::test]
    async fn custom_template_round_trip_and_default_fallback() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-document-entry-template-{}",
            Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let initial = list(database.pool()).await.unwrap();
        assert_eq!(initial.len(), 1);
        assert!(initial[0].is_built_in);
        assert!(initial[0].is_default);

        let mut custom_config = config();
        custom_config.type_marker = "【类型】".to_owned();
        custom_config.stem_marker = "【题干】".to_owned();
        custom_config.answer_marker = "【答案】".to_owned();
        custom_config.explanation_marker = "【解析】".to_owned();
        let saved = save(
            database.pool(),
            &SaveDocumentEntryTemplateRequestApi {
                id: None,
                name: "我的模板".to_owned(),
                config: custom_config,
                set_as_default: true,
            },
        )
        .await
        .unwrap();
        assert!(saved.is_default);
        assert!(!saved.is_built_in);

        delete(database.pool(), &saved.id).await.unwrap();
        let remaining = list(database.pool()).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].is_built_in);
        assert!(remaining[0].is_default);

        database.close().await;
        drop(database);
        for attempt in 0..20 {
            match fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) if attempt < 19 => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean template test directory: {error}"),
            }
        }
    }
}
