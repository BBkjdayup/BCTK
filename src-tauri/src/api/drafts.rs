use std::{collections::HashSet, path::Path};

use sqlx::{FromRow, SqlitePool};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::resources;
use super::{
    document_entry_templates::{
        DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID, validate_document_entry_template_config,
        validate_template_id,
    },
    models::{
        CommandError, CommandResult, DocumentQuestionDraftPayloadApi,
        DocumentQuestionDraftRecordApi, QuestionDraftApi, QuestionDraftRecordApi,
        QuestionDuplicateCandidateApi, RichContentApi, WordImportDraftItemApi,
        WordImportDraftPayloadApi, WordImportDraftRecordApi,
    },
};

const PAYLOAD_SCHEMA_VERSION: u32 = 1;
const CREATE_DRAFT_KEY: &str = "question:create";
const EDIT_DRAFT_KEY_PREFIX: &str = "question:";
const QUESTION_CREATE_KIND: &str = "question_create";
const QUESTION_EDIT_KIND: &str = "question_edit";
const WORD_IMPORT_DRAFT_KEY: &str = "word_import:active";
const WORD_IMPORT_DRAFT_KIND: &str = "word_import_preview";
const DOCUMENT_QUESTION_DRAFT_KEY: &str = "question_document:active";
const LEGACY_FREE_DOCUMENT_DRAFT_SCHEMA_VERSION: u32 = 2;
const FREE_DOCUMENT_DRAFT_SCHEMA_VERSION: u32 = 3;

// Keep autosave at least as defensive as the final question write path while
// still allowing an unfinished question to be recovered.
const MAX_RICH_HTML_BYTES: usize = 2 * 1024 * 1024;
const MAX_RICH_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_RICH_PLAIN_BYTES: usize = 512 * 1024;
const MAX_SOURCE_OOXML_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOTAL_CONTENT_BYTES: usize = 12 * 1024 * 1024;
const MAX_DRAFT_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORD_IMPORT_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const MAX_WORD_IMPORT_ITEMS: usize = 5_000;
const MAX_DOCUMENT_QUESTION_ITEMS: usize = 100;
const MAX_DOCUMENT_QUESTION_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
const MAX_WORD_IMPORT_SOURCE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_SOURCE_FILE_NAME_CHARS: usize = 255;
const MAX_PARSER_VERSION_CHARS: usize = 64;
const MAX_DIAGNOSTIC_CHARS: usize = 4_000;
const MAX_CANDIDATE_PREVIEW_CHARS: usize = 1_000;
const MAX_CANDIDATE_TAXONOMY_CHARS: usize = 255;
const MAX_OPTIONS: usize = 26;
const MAX_TAGS: usize = 100;

#[derive(Debug, FromRow)]
struct DraftRow {
    id: String,
    draft_key: String,
    draft_kind: String,
    target_question_id: Option<String>,
    base_content_version: Option<i64>,
    payload_schema_version: i64,
    payload_json: String,
    autosaved_at_ms: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    current_content_version: Option<i64>,
}

#[derive(Debug, FromRow)]
struct ExistingDraftRow {
    id: String,
    draft_kind: String,
}

#[derive(Debug, FromRow)]
struct WordImportDraftRow {
    id: String,
    draft_key: String,
    draft_kind: String,
    target_question_id: Option<String>,
    base_content_version: Option<i64>,
    payload_schema_version: i64,
    payload_json: String,
    source_title: Option<String>,
    autosaved_at_ms: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    import_session_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct DocumentQuestionDraftRow {
    id: String,
    draft_key: String,
    payload_schema_version: i64,
    payload_json: String,
    autosaved_at_ms: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DraftIdentity {
    draft_key: String,
    draft_kind: &'static str,
    target_question_id: Option<String>,
}

/// Returns the recoverable autosave for the new-question editor (`None`) or a
/// particular existing question (`Some`). The caller never supplies a draft
/// key; this service derives the only accepted key form.
pub(crate) async fn get_question_draft(
    pool: &SqlitePool,
    question_id: Option<&str>,
) -> CommandResult<Option<QuestionDraftRecordApi>> {
    let identity = draft_identity(question_id)?;
    let row = sqlx::query_as::<_, DraftRow>(
        "SELECT d.id, d.draft_key, d.draft_kind, d.target_question_id, \
                d.base_content_version, d.payload_schema_version, d.payload_json, \
                d.autosaved_at_ms, d.created_at_ms, d.updated_at_ms, \
                q.content_version AS current_content_version \
         FROM drafts d LEFT JOIN questions q ON q.id = d.target_question_id \
         WHERE d.draft_key = ? AND d.draft_kind IN ('question_create', 'question_edit')",
    )
    .bind(&identity.draft_key)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?;

    row.map(record_from_row).transpose()
}

/// Lists only question-editor autosaves. Word-import preview drafts remain an
/// independent workflow and are deliberately not exposed here.
pub(crate) async fn list_question_drafts(
    pool: &SqlitePool,
) -> CommandResult<Vec<QuestionDraftRecordApi>> {
    let rows = sqlx::query_as::<_, DraftRow>(
        "SELECT d.id, d.draft_key, d.draft_kind, d.target_question_id, \
                d.base_content_version, d.payload_schema_version, d.payload_json, \
                d.autosaved_at_ms, d.created_at_ms, d.updated_at_ms, \
                q.content_version AS current_content_version \
         FROM drafts d LEFT JOIN questions q ON q.id = d.target_question_id \
         WHERE d.draft_kind IN ('question_create', 'question_edit') \
         ORDER BY d.autosaved_at_ms DESC, d.id",
    )
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;

    rows.into_iter().map(record_from_row).collect()
}

/// Transactionally inserts or replaces the one autosave slot for the editor.
/// Autosaves intentionally do not write operation logs: doing so every minute
/// would drown out meaningful teacher actions and grow the database quickly.
pub(crate) async fn save_question_draft(
    pool: &SqlitePool,
    draft: &QuestionDraftApi,
) -> CommandResult<QuestionDraftRecordApi> {
    let identity = draft_identity(draft.question_id.as_deref())?;
    validate_draft_shape(draft, &identity)?;

    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    let existing = sqlx::query_as::<_, ExistingDraftRow>(
        "SELECT id, draft_kind FROM drafts WHERE draft_key = ?",
    )
    .bind(&identity.draft_key)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    if let Some(existing) = &existing
        && existing.draft_kind != identity.draft_kind
    {
        return Err(CommandError::new(
            "DRAFT_KEY_CONFLICT",
            "草稿保存位置已被其他工作流程占用，请重启软件后重试。",
        ));
    }

    if let Some(question_id) = identity.target_question_id.as_deref() {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT content_version FROM questions WHERE id = ?")
                .bind(question_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(CommandError::database)?;
        if exists.is_none() {
            return Err(CommandError::new(
                "DRAFT_TARGET_NOT_FOUND",
                "要编辑的题目已经不存在，无法继续自动保存该编辑草稿。",
            ));
        }
    }

    let record_id = existing
        .map(|row| row.id)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let mut payload = draft.clone();
    // These identity fields are authoritative server values. Rewriting them
    // also means serialized JSON can never smuggle in a client draft key.
    payload.id = Some(record_id.clone());
    payload.question_id = identity.target_question_id.clone();
    payload.base_content_version = draft.base_content_version;
    let option_ids = option_id_set(&payload)?;
    payload.resource_refs = resources::reconcile_question_resource_refs(
        &payload.stem,
        &payload.options,
        &payload.answer,
        &payload.explanation,
        &payload.resource_refs,
    )?;
    resources::validate_resource_refs(&mut transaction, &payload.resource_refs, &option_ids)
        .await?;
    let payload_json = serialize_payload(&payload)?;

    sqlx::query(
        "INSERT INTO drafts (id, draft_key, draft_kind, target_question_id, \
             base_content_version, payload_schema_version, payload_json, source_title, \
             autosaved_at_ms, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?) \
         ON CONFLICT(draft_key) DO UPDATE SET \
             draft_kind = excluded.draft_kind, \
             target_question_id = excluded.target_question_id, \
             base_content_version = excluded.base_content_version, \
             payload_schema_version = excluded.payload_schema_version, \
             payload_json = excluded.payload_json, \
             source_title = NULL, \
             autosaved_at_ms = excluded.autosaved_at_ms, \
             updated_at_ms = excluded.updated_at_ms",
    )
    .bind(&record_id)
    .bind(&identity.draft_key)
    .bind(identity.draft_kind)
    .bind(identity.target_question_id.as_deref())
    .bind(payload.base_content_version)
    .bind(i64::from(PAYLOAD_SCHEMA_VERSION))
    .bind(payload_json)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    resources::sync_draft_resource_refs(&mut transaction, &record_id, &payload.resource_refs, now)
        .await?;

    transaction.commit().await.map_err(CommandError::database)?;

    get_question_draft(pool, identity.target_question_id.as_deref())
        .await?
        .ok_or_else(|| CommandError::database("自动保存后未能重新读取题目草稿。"))
}

/// Idempotently deletes the autosave slot for the requested editor.
pub(crate) async fn delete_question_draft(
    pool: &SqlitePool,
    question_id: Option<&str>,
) -> CommandResult<()> {
    let identity = draft_identity(question_id)?;
    sqlx::query("DELETE FROM drafts WHERE draft_key = ? AND draft_kind = ?")
        .bind(identity.draft_key)
        .bind(identity.draft_kind)
        .execute(pool)
        .await
        .map_err(CommandError::database)?;
    Ok(())
}

pub(crate) async fn get_document_question_draft(
    pool: &SqlitePool,
) -> CommandResult<Option<DocumentQuestionDraftRecordApi>> {
    let row = sqlx::query_as::<_, DocumentQuestionDraftRow>(
        "SELECT id, draft_key, payload_schema_version, payload_json, autosaved_at_ms, \
                created_at_ms, updated_at_ms \
         FROM document_question_drafts WHERE draft_key = ?",
    )
    .bind(DOCUMENT_QUESTION_DRAFT_KEY)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?;

    row.map(document_question_record_from_row).transpose()
}

pub(crate) async fn save_document_question_draft(
    pool: &SqlitePool,
    payload: &DocumentQuestionDraftPayloadApi,
) -> CommandResult<DocumentQuestionDraftRecordApi> {
    validate_document_question_payload(payload)?;
    let payload_json = serde_json::to_string(payload).map_err(CommandError::database)?;
    if payload_json.len() > MAX_DOCUMENT_QUESTION_PAYLOAD_BYTES {
        return Err(CommandError::validation(
            "文档式录入草稿超过 32 MB，请先保存部分题目后再继续录入。",
        ));
    }

    let now = now_millis();
    let existing_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM document_question_drafts WHERE draft_key = ?",
    )
    .bind(DOCUMENT_QUESTION_DRAFT_KEY)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?;
    let record_id = existing_id.unwrap_or_else(|| Uuid::now_v7().to_string());

    sqlx::query(
        "INSERT INTO document_question_drafts \
             (id, draft_key, payload_schema_version, payload_json, autosaved_at_ms, \
              created_at_ms, updated_at_ms) \
         VALUES (?, ?, 1, ?, ?, ?, ?) \
         ON CONFLICT(draft_key) DO UPDATE SET \
             payload_schema_version = 1, payload_json = excluded.payload_json, \
             autosaved_at_ms = excluded.autosaved_at_ms, \
             updated_at_ms = excluded.updated_at_ms",
    )
    .bind(&record_id)
    .bind(DOCUMENT_QUESTION_DRAFT_KEY)
    .bind(payload_json)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(CommandError::database)?;

    get_document_question_draft(pool)
        .await?
        .ok_or_else(|| CommandError::database("保存后无法重新读取文档式录入草稿。"))
}

pub(crate) async fn delete_document_question_draft(pool: &SqlitePool) -> CommandResult<()> {
    sqlx::query("DELETE FROM document_question_drafts WHERE draft_key = ?")
        .bind(DOCUMENT_QUESTION_DRAFT_KEY)
        .execute(pool)
        .await
        .map_err(CommandError::database)?;
    Ok(())
}

/// Reads the single recoverable Word-import review slot. The key and kind are
/// server-owned so a renderer cannot use this command to inspect arbitrary
/// draft rows.
pub(crate) async fn get_word_import_draft(
    pool: &SqlitePool,
) -> CommandResult<Option<WordImportDraftRecordApi>> {
    let row = sqlx::query_as::<_, WordImportDraftRow>(
        "SELECT id, draft_key, draft_kind, target_question_id, base_content_version, \
                payload_schema_version, payload_json, source_title, autosaved_at_ms, \
                created_at_ms, updated_at_ms, \
                (SELECT id FROM word_import_sessions WHERE draft_id = drafts.id) AS import_session_id \
         FROM drafts WHERE draft_key = ?",
    )
    .bind(WORD_IMPORT_DRAFT_KEY)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?;

    row.map(word_import_record_from_row).transpose()
}

/// Transactionally creates or replaces the fixed Word-import review slot.
/// Autosaves deliberately do not create operation-log rows because this can
/// run frequently while a teacher reviews thousands of parsed questions.
pub(crate) async fn save_word_import_draft(
    pool: &SqlitePool,
    payload: &WordImportDraftPayloadApi,
) -> CommandResult<WordImportDraftRecordApi> {
    let mut payload = normalized_word_import_payload(payload)?;
    for item in &mut payload.items {
        item.payload.resource_refs = resources::reconcile_question_resource_refs(
            &item.payload.stem,
            &item.payload.options,
            &item.payload.answer,
            &item.payload.explanation,
            &item.payload.resource_refs,
        )?;
    }
    validate_word_import_payload(&payload)?;
    let payload_json = serialize_word_import_payload(&payload)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    let existing = sqlx::query_as::<_, ExistingDraftRow>(
        "SELECT id, draft_kind FROM drafts WHERE draft_key = ?",
    )
    .bind(WORD_IMPORT_DRAFT_KEY)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    let record_id = match existing {
        Some(row) => {
            if row.draft_kind != WORD_IMPORT_DRAFT_KIND {
                return Err(CommandError::new(
                    "DRAFT_KEY_CONFLICT",
                    "Word 导入草稿保存位置已被其他工作流程占用，请重启软件后重试。",
                ));
            }
            let canonical = canonical_uuid(&row.id, "Word 导入草稿记录 ID")
                .map_err(|error| corrupted(format!("草稿记录 ID 无效：{}", error.message)))?;
            if canonical != row.id {
                return Err(corrupted("Word 导入草稿记录 ID 不是规范 UUID 格式。"));
            }
            row.id
        }
        None => Uuid::now_v7().to_string(),
    };

    let stored_session: Option<(String, String, i64, String)> =
        sqlx::query_as::<_, (String, String, i64, String)>(
            "SELECT id, source_filename, source_byte_size, parser_version \
         FROM word_import_sessions WHERE draft_id = ?",
        )
        .bind(&record_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    match (&payload.import_session_id, &stored_session) {
        (Some(requested), Some((stored, filename, byte_size, parser_version)))
            if requested == stored
                && filename == &payload.source_file_name
                && *byte_size == i64::try_from(payload.source_file_size).unwrap_or(i64::MAX)
                && parser_version == &payload.parser_version => {}
        (None, None) => {}
        (Some(_), Some(_)) => {
            return Err(CommandError::new(
                "WORD_IMPORT_SESSION_MISMATCH",
                "Word 导入草稿与已建立的图片导入会话不一致，请重新打开导入页。",
            ));
        }
        _ => {
            return Err(CommandError::new(
                "WORD_IMPORT_SESSION_MISMATCH",
                "Word 导入草稿缺少或多出了导入会话，已停止保存。",
            ));
        }
    }

    let mut all_refs = Vec::new();
    for item in &payload.items {
        let option_ids = option_id_set(&item.payload)?;
        resources::validate_resource_refs(
            &mut transaction,
            &item.payload.resource_refs,
            &option_ids,
        )
        .await?;
        all_refs.extend(item.payload.resource_refs.iter().cloned());
    }
    let result = sqlx::query(
        "INSERT INTO drafts (id, draft_key, draft_kind, target_question_id, \
             base_content_version, payload_schema_version, payload_json, source_title, \
             autosaved_at_ms, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, NULL, NULL, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(draft_key) DO UPDATE SET \
             draft_kind = excluded.draft_kind, \
             target_question_id = NULL, \
             base_content_version = NULL, \
             payload_schema_version = excluded.payload_schema_version, \
             payload_json = excluded.payload_json, \
             source_title = excluded.source_title, \
             autosaved_at_ms = excluded.autosaved_at_ms, \
             updated_at_ms = excluded.updated_at_ms \
         WHERE drafts.draft_kind = 'word_import_preview'",
    )
    .bind(&record_id)
    .bind(WORD_IMPORT_DRAFT_KEY)
    .bind(WORD_IMPORT_DRAFT_KIND)
    .bind(i64::from(PAYLOAD_SCHEMA_VERSION))
    .bind(payload_json)
    .bind(&payload.source_file_name)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    if result.rows_affected() != 1 {
        return Err(CommandError::new(
            "DRAFT_KEY_CONFLICT",
            "Word 导入草稿保存位置已被其他工作流程占用，请重启软件后重试。",
        ));
    }
    resources::sync_draft_resource_refs(&mut transaction, &record_id, &all_refs, now).await?;
    if let Some((session_id, _, _, _)) = stored_session {
        sync_word_import_items(&mut transaction, &session_id, &payload, now).await?;
    }
    transaction.commit().await.map_err(CommandError::database)?;

    get_word_import_draft(pool)
        .await?
        .ok_or_else(|| CommandError::database("自动保存后未能重新读取 Word 导入草稿。"))
}

async fn sync_word_import_items(
    connection: &mut sqlx::SqliteConnection,
    session_id: &str,
    payload: &WordImportDraftPayloadApi,
    now: i64,
) -> CommandResult<()> {
    sqlx::query("DELETE FROM word_import_items WHERE session_id = ?")
        .bind(session_id)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    for item in &payload.items {
        let item_payload = serde_json::to_string(&item.payload).map_err(CommandError::database)?;
        let validation_json = serde_json::json!({
            "status": item.status,
            "diagnostic": item.diagnostic,
            "needsCheck": item.needs_check,
            "checkError": item.check_error,
        })
        .to_string();
        let duplicate_question_id = if let Some(candidate) = item.candidate.as_ref() {
            sqlx::query_scalar::<_, String>("SELECT id FROM questions WHERE id = ?")
                .bind(&candidate.id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(CommandError::database)?
        } else {
            None
        };
        let subject_id = if item.payload.subject_id.is_empty() {
            None
        } else {
            sqlx::query_scalar::<_, String>("SELECT id FROM subjects WHERE id = ?")
                .bind(&item.payload.subject_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(CommandError::database)?
        };
        let chapter_id = if subject_id.is_some() && !item.payload.chapter_id.is_empty() {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM chapters WHERE id = ? AND subject_id = ?",
            )
            .bind(&item.payload.chapter_id)
            .bind(subject_id.as_deref())
            .fetch_optional(&mut *connection)
            .await
            .map_err(CommandError::database)?
        } else {
            None
        };
        sqlx::query(
            "INSERT INTO word_import_items (id, session_id, source_ordinal, selected, \
                 item_schema_version, payload_json, subject_id, chapter_id, validation_json, \
                 duplicate_kind, duplicate_question_id, duplicate_action, item_status, \
                 imported_question_id, error_code, error_message, created_at_ms, updated_at_ms) \
             VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, 'review', NULL, NULL, ?, ?, ?)",
        )
        .bind(&item.id)
        .bind(session_id)
        .bind(i64::from(item.ordinal))
        .bind(if item.selected { 1_i64 } else { 0_i64 })
        .bind(item_payload)
        .bind(subject_id.as_deref())
        .bind(chapter_id.as_deref())
        .bind(validation_json)
        .bind(&item.duplicate_kind)
        .bind(duplicate_question_id)
        .bind(item.action.as_deref())
        .bind(item.diagnostic.as_deref().or(item.check_error.as_deref()))
        .bind(now)
        .bind(now)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
        for resource_ref in &item.payload.resource_refs {
            sqlx::query(
                "INSERT INTO word_import_item_resource_refs \
                     (id, import_item_id, resource_id, node_id, created_at_ms) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(&item.id)
            .bind(&resource_ref.resource_id)
            .bind(&resource_ref.node_id)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(CommandError::database)?;
        }
    }
    sqlx::query(
        "UPDATE word_import_sessions SET status = 'reviewing', updated_at_ms = ? WHERE id = ?",
    )
    .bind(now)
    .bind(session_id)
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

/// Idempotently deletes only the fixed Word-import review slot.
pub(crate) async fn delete_word_import_draft(pool: &SqlitePool) -> CommandResult<()> {
    sqlx::query("DELETE FROM drafts WHERE draft_key = ? AND draft_kind = ?")
        .bind(WORD_IMPORT_DRAFT_KEY)
        .bind(WORD_IMPORT_DRAFT_KIND)
        .execute(pool)
        .await
        .map_err(CommandError::database)?;
    Ok(())
}

fn document_question_record_from_row(
    row: DocumentQuestionDraftRow,
) -> CommandResult<DocumentQuestionDraftRecordApi> {
    let record_id = canonical_uuid(&row.id, "文档式录入草稿记录 ID")
        .map_err(|error| corrupted(format!("文档式录入草稿记录 ID 无效：{}", error.message)))?;
    if record_id != row.id || row.draft_key != DOCUMENT_QUESTION_DRAFT_KEY {
        return Err(corrupted("文档式录入草稿使用了无效的记录 ID 或固定键。"));
    }
    if row.payload_schema_version != i64::from(PAYLOAD_SCHEMA_VERSION) {
        return Err(CommandError::new(
            "DRAFT_SCHEMA_UNSUPPORTED",
            "文档式录入草稿的数据版本不受当前软件支持。",
        ));
    }
    if row.payload_json.len() > MAX_DOCUMENT_QUESTION_PAYLOAD_BYTES {
        return Err(corrupted("文档式录入草稿 JSON 超过 32 MB 安全上限。"));
    }
    if row.created_at_ms < 0
        || row.autosaved_at_ms < 0
        || row.updated_at_ms < 0
        || row.created_at_ms > row.updated_at_ms
        || row.autosaved_at_ms != row.updated_at_ms
    {
        return Err(corrupted("文档式录入草稿的保存时间字段不一致。"));
    }

    let payload: DocumentQuestionDraftPayloadApi = serde_json::from_str(&row.payload_json)
        .map_err(|error| corrupted(format!("文档式录入草稿 JSON 无法读取：{error}")))?;
    validate_document_question_payload(&payload)
        .map_err(|error| corrupted(format!("文档式录入草稿内容无效：{}", error.message)))?;

    Ok(DocumentQuestionDraftRecordApi {
        id: record_id,
        payload,
        autosaved_at: row.autosaved_at_ms,
        created_at: row.created_at_ms,
        updated_at: row.updated_at_ms,
    })
}

fn validate_document_question_payload(
    payload: &DocumentQuestionDraftPayloadApi,
) -> CommandResult<()> {
    if payload.schema_version == LEGACY_FREE_DOCUMENT_DRAFT_SCHEMA_VERSION {
        if payload.template_id.as_deref() != Some(DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID) {
            return Err(CommandError::validation(
                "文档式录入草稿使用了不受支持的输入模板。",
            ));
        }
        if payload.template_name_snapshot.is_some() || payload.template_config_snapshot.is_some() {
            return Err(CommandError::validation(
                "旧版自由文档草稿不能携带新版模板快照。",
            ));
        }
        if payload.active_item_id.is_some() || !payload.items.is_empty() {
            return Err(CommandError::validation(
                "自由文档草稿不能混入旧版固定题目块。",
            ));
        }
        let source = payload
            .source
            .as_ref()
            .ok_or_else(|| CommandError::validation("自由文档草稿缺少可恢复的编辑器内容。"))?;
        validate_document_entry_source("自由录题文档", source)?;
        if rich_content_bytes(source) > MAX_DOCUMENT_QUESTION_PAYLOAD_BYTES {
            return Err(CommandError::validation(
                "自由录题文档超过 32 MB，请先识别并分批导入。",
            ));
        }
        return Ok(());
    }

    if payload.schema_version == FREE_DOCUMENT_DRAFT_SCHEMA_VERSION {
        let template_id = payload
            .template_id
            .as_deref()
            .ok_or_else(|| CommandError::validation("自由文档草稿缺少录题模板 ID。"))?;
        validate_template_id(template_id)?;
        let template_name = payload
            .template_name_snapshot
            .as_deref()
            .ok_or_else(|| CommandError::validation("自由文档草稿缺少录题模板名称快照。"))?;
        if template_name.trim().is_empty()
            || template_name.chars().count() > 40
            || template_name.chars().any(char::is_control)
        {
            return Err(CommandError::validation(
                "自由文档草稿的录题模板名称快照无效。",
            ));
        }
        let config = payload
            .template_config_snapshot
            .as_ref()
            .ok_or_else(|| CommandError::validation("自由文档草稿缺少录题模板配置快照。"))?;
        validate_document_entry_template_config(config)?;
        if payload.active_item_id.is_some() || !payload.items.is_empty() {
            return Err(CommandError::validation(
                "自由文档草稿不能混入旧版固定题目块。",
            ));
        }
        let source = payload
            .source
            .as_ref()
            .ok_or_else(|| CommandError::validation("自由文档草稿缺少可恢复的编辑器内容。"))?;
        validate_document_entry_source("自由录题文档", source)?;
        if rich_content_bytes(source) > MAX_DOCUMENT_QUESTION_PAYLOAD_BYTES {
            return Err(CommandError::validation(
                "自由录题文档超过 32 MB，请先识别并分批导入。",
            ));
        }
        return Ok(());
    }

    if payload.schema_version != PAYLOAD_SCHEMA_VERSION
        || payload.template_id.is_some()
        || payload.template_name_snapshot.is_some()
        || payload.template_config_snapshot.is_some()
        || payload.source.is_some()
    {
        return Err(CommandError::validation("文档式录入草稿的数据版本无效。"));
    }
    if payload.items.is_empty() || payload.items.len() > MAX_DOCUMENT_QUESTION_ITEMS {
        return Err(CommandError::validation(format!(
            "文档式录入一次必须保留 1 至 {MAX_DOCUMENT_QUESTION_ITEMS} 道题。"
        )));
    }

    let create_identity = DraftIdentity {
        draft_key: CREATE_DRAFT_KEY.to_owned(),
        draft_kind: QUESTION_CREATE_KIND,
        target_question_id: None,
    };
    let mut ids = HashSet::with_capacity(payload.items.len());
    let mut resource_node_ids = HashSet::new();
    for (index, item) in payload.items.iter().enumerate() {
        let canonical_id = canonical_uuid(&item.id, &format!("第 {} 项的 ID", index + 1))?;
        if canonical_id != item.id || !ids.insert(item.id.as_str()) {
            return Err(CommandError::validation(
                "文档式录入草稿中的题目 ID 必须是唯一的规范 UUID。",
            ));
        }
        if item.ordinal != u32::try_from(index + 1).unwrap_or(u32::MAX) {
            return Err(CommandError::validation(
                "文档式录入草稿中的题目顺序必须从 1 连续排列。",
            ));
        }
        validate_draft_shape(&item.payload, &create_identity).map_err(|error| {
            CommandError::validation(format!(
                "第 {} 题的草稿内容无效：{}",
                index + 1,
                error.message
            ))
        })?;
        for resource_ref in &item.payload.resource_refs {
            if !resource_node_ids.insert(resource_ref.node_id.as_str()) {
                return Err(CommandError::validation(
                    "文档式录入草稿中的图片节点不能跨题目重复。",
                ));
            }
        }
    }
    if let Some(active_item_id) = payload.active_item_id.as_deref() {
        let canonical = canonical_uuid(active_item_id, "当前题目 ID")?;
        if canonical != active_item_id || !ids.contains(active_item_id) {
            return Err(CommandError::validation(
                "当前题目 ID 不属于这份文档式录入草稿。",
            ));
        }
    }
    Ok(())
}

fn word_import_record_from_row(row: WordImportDraftRow) -> CommandResult<WordImportDraftRecordApi> {
    let record_id = canonical_uuid(&row.id, "Word 导入草稿记录 ID")
        .map_err(|error| corrupted(format!("Word 导入草稿记录 ID 无效：{}", error.message)))?;
    if record_id != row.id {
        return Err(corrupted("Word 导入草稿记录 ID 必须使用规范 UUID 格式。"));
    }
    if row.draft_key != WORD_IMPORT_DRAFT_KEY || row.draft_kind != WORD_IMPORT_DRAFT_KIND {
        return Err(corrupted("Word 导入草稿的固定键或类型不正确。"));
    }
    if row.target_question_id.is_some() || row.base_content_version.is_some() {
        return Err(corrupted("Word 导入草稿不应关联单道题目或基础内容版本。"));
    }
    if row.payload_schema_version != i64::from(PAYLOAD_SCHEMA_VERSION) {
        return Err(CommandError::new(
            "DRAFT_SCHEMA_UNSUPPORTED",
            format!(
                "Word 导入草稿使用了当前软件不支持的数据版本 {}。",
                row.payload_schema_version
            ),
        ));
    }
    if row.payload_json.len() > MAX_WORD_IMPORT_PAYLOAD_BYTES {
        return Err(corrupted("Word 导入草稿 JSON 超过 64 MB 安全上限。"));
    }
    if row.created_at_ms < 0
        || row.autosaved_at_ms < 0
        || row.updated_at_ms < 0
        || row.created_at_ms > row.updated_at_ms
        || row.autosaved_at_ms != row.updated_at_ms
    {
        return Err(corrupted("Word 导入草稿的保存时间字段不一致。"));
    }

    let payload: WordImportDraftPayloadApi = serde_json::from_str(&row.payload_json)
        .map_err(|error| corrupted(format!("Word 导入草稿 JSON 无法读取：{error}")))?;
    validate_word_import_payload(&payload)
        .map_err(|error| corrupted(format!("Word 导入草稿内容无效：{}", error.message)))?;
    if i64::from(payload.schema_version) != row.payload_schema_version {
        return Err(corrupted("Word 导入草稿 JSON 与数据库版本字段不一致。"));
    }
    if row.source_title.as_deref() != Some(payload.source_file_name.as_str()) {
        return Err(corrupted(
            "Word 导入草稿 JSON 与数据库中的来源文件名不一致。",
        ));
    }
    if row.import_session_id != payload.import_session_id {
        return Err(corrupted("Word 导入草稿 JSON 与数据库中的导入会话不一致。"));
    }

    Ok(WordImportDraftRecordApi {
        id: record_id,
        payload,
        autosaved_at: row.autosaved_at_ms,
        created_at: row.created_at_ms,
        updated_at: row.updated_at_ms,
    })
}

fn normalized_word_import_payload(
    payload: &WordImportDraftPayloadApi,
) -> CommandResult<WordImportDraftPayloadApi> {
    let mut normalized = payload.clone();
    normalized.source_file_name = normalized_source_file_name(&payload.source_file_name)?;
    normalized.parser_version = normalized_parser_version(&payload.parser_version)?;
    if let Some(session_id) = payload.import_session_id.as_deref() {
        normalized.import_session_id = Some(canonical_uuid(session_id, "Word 导入会话 ID")?);
    }
    Ok(normalized)
}

fn validate_word_import_payload(payload: &WordImportDraftPayloadApi) -> CommandResult<()> {
    if payload.schema_version != PAYLOAD_SCHEMA_VERSION {
        return Err(CommandError::validation(format!(
            "Word 导入草稿仅支持数据版本 {PAYLOAD_SCHEMA_VERSION}。"
        )));
    }
    if normalized_source_file_name(&payload.source_file_name)? != payload.source_file_name {
        return Err(CommandError::validation(
            "批量导入来源文件名必须是已规范化且不含路径的文件名。",
        ));
    }
    if normalized_parser_version(&payload.parser_version)? != payload.parser_version {
        return Err(CommandError::validation(
            "Word 分析器版本必须是已规范化的短字符串。",
        ));
    }
    if let Some(session_id) = payload.import_session_id.as_deref()
        && canonical_uuid(session_id, "Word 导入会话 ID")? != session_id
    {
        return Err(CommandError::validation(
            "Word 导入会话 ID 必须使用规范 UUID 格式。",
        ));
    }
    if payload.source_file_size > MAX_WORD_IMPORT_SOURCE_BYTES {
        return Err(CommandError::validation("来源 Word 文件不能超过 100 MB。"));
    }
    if payload.items.len() > MAX_WORD_IMPORT_ITEMS {
        return Err(CommandError::validation(format!(
            "一次 Word 导入最多恢复 {MAX_WORD_IMPORT_ITEMS} 道题。"
        )));
    }
    match payload.source_item_count {
        Some(source_count) => {
            let retained_and_omitted = u32::try_from(payload.items.len())
                .unwrap_or(u32::MAX)
                .saturating_add(payload.omitted_item_count);
            if payload.omitted_item_count > source_count || retained_and_omitted > source_count {
                return Err(CommandError::validation(
                    "Word 导入草稿的原始识别数量、保留数量与未载入数量不一致。",
                ));
            }
        }
        None if payload.omitted_item_count != 0 => {
            return Err(CommandError::validation(
                "Word 导入草稿记录了未载入题目，但缺少原始识别数量。",
            ));
        }
        None => {}
    }

    let mut ids = HashSet::with_capacity(payload.items.len());
    let mut ordinals = HashSet::with_capacity(payload.items.len());
    let mut resource_node_ids = HashSet::new();
    for (index, item) in payload.items.iter().enumerate() {
        validate_word_import_item(item, index)?;
        if !ids.insert(item.id.as_str()) {
            return Err(CommandError::validation(
                "Word 导入草稿中存在重复的题目 ID。",
            ));
        }
        if !ordinals.insert(item.ordinal) {
            return Err(CommandError::validation(
                "Word 导入草稿中存在重复的来源顺序号。",
            ));
        }
        for resource_ref in &item.payload.resource_refs {
            if !resource_node_ids.insert(resource_ref.node_id.as_str()) {
                return Err(CommandError::validation(
                    "Word 导入草稿中的图片节点 ID 不能跨题目重复。",
                ));
            }
        }
    }
    if let Some(active_item_id) = payload.active_item_id.as_deref() {
        let active_item_id = canonical_uuid(active_item_id, "当前审查题目 ID")?;
        if payload.active_item_id.as_deref() != Some(active_item_id.as_str()) {
            return Err(CommandError::validation(
                "当前审查题目 ID 必须使用规范 UUID 格式。",
            ));
        }
        if !ids.contains(active_item_id.as_str()) {
            return Err(CommandError::validation(
                "当前审查题目 ID 不属于这份 Word 导入草稿。",
            ));
        }
    }
    Ok(())
}

fn validate_word_import_item(item: &WordImportDraftItemApi, index: usize) -> CommandResult<()> {
    let canonical_id = canonical_uuid(&item.id, &format!("第 {} 项的 ID", index + 1))?;
    if canonical_id != item.id {
        return Err(CommandError::validation(format!(
            "第 {} 项的 ID 必须使用规范 UUID 格式。",
            index + 1
        )));
    }
    if item.ordinal == 0 || item.ordinal > MAX_WORD_IMPORT_ITEMS as u32 {
        return Err(CommandError::validation(format!(
            "第 {} 项的来源顺序号必须在 1 到 {MAX_WORD_IMPORT_ITEMS} 之间。",
            index + 1
        )));
    }
    if !matches!(item.status.as_str(), "ok" | "warning" | "error") {
        return Err(CommandError::validation(format!(
            "第 {} 项的审查状态无效。",
            index + 1
        )));
    }
    validate_bounded_message(
        item.diagnostic.as_deref(),
        MAX_DIAGNOSTIC_CHARS,
        &format!("第 {} 项的诊断信息", index + 1),
    )?;
    validate_bounded_message(
        item.check_error.as_deref(),
        MAX_DIAGNOSTIC_CHARS,
        &format!("第 {} 项的重复检查错误", index + 1),
    )?;

    let create_identity = DraftIdentity {
        draft_key: CREATE_DRAFT_KEY.to_owned(),
        draft_kind: QUESTION_CREATE_KIND,
        target_question_id: None,
    };
    validate_draft_shape(&item.payload, &create_identity).map_err(|error| {
        CommandError::validation(format!(
            "第 {} 项的题目草稿无效：{}",
            index + 1,
            error.message
        ))
    })?;
    if let Some(draft_id) = item.payload.id.as_deref()
        && canonical_uuid(draft_id, &format!("第 {} 项的题目草稿 ID", index + 1))? != draft_id
    {
        return Err(CommandError::validation(format!(
            "第 {} 项的题目草稿 ID 必须使用规范 UUID 格式。",
            index + 1
        )));
    }

    if !matches!(item.duplicate_kind.as_str(), "none" | "exact" | "suspected") {
        return Err(CommandError::validation(format!(
            "第 {} 项的重复类型无效。",
            index + 1
        )));
    }
    if let Some(action) = item.action.as_deref() {
        if !matches!(action, "skip" | "overwrite" | "keep") {
            return Err(CommandError::validation(format!(
                "第 {} 项的重复题处理方式无效。",
                index + 1
            )));
        }
        if item.duplicate_kind == "none" || item.candidate.is_none() {
            return Err(CommandError::validation(format!(
                "第 {} 项没有重复候选题，不能记录重复题处理方式。",
                index + 1
            )));
        }
    }
    if item.duplicate_kind == "none" && item.candidate.is_some() {
        return Err(CommandError::validation(format!(
            "第 {} 项标记为不重复，却仍携带重复候选题。",
            index + 1
        )));
    }
    if item.duplicate_kind != "none" && item.candidate.is_none() {
        return Err(CommandError::validation(format!(
            "第 {} 项标记为重复题，却缺少对应的候选题。",
            index + 1
        )));
    }
    if let Some(candidate) = &item.candidate {
        validate_duplicate_candidate(candidate, index)?;
    }
    if item.check_error.is_some() && !item.needs_check {
        return Err(CommandError::validation(format!(
            "第 {} 项记录了重复检查错误，但未标记为需要重新检查。",
            index + 1
        )));
    }
    Ok(())
}

fn validate_duplicate_candidate(
    candidate: &QuestionDuplicateCandidateApi,
    item_index: usize,
) -> CommandResult<()> {
    if canonical_uuid(&candidate.id, "重复候选题 ID")? != candidate.id {
        return Err(CommandError::validation(format!(
            "第 {} 项的重复候选题 ID 必须使用规范 UUID 格式。",
            item_index + 1
        )));
    }
    if candidate.question_type.trim().is_empty() || candidate.question_type.len() > 64 {
        return Err(CommandError::validation(format!(
            "第 {} 项的重复候选题型无效。",
            item_index + 1
        )));
    }
    validate_required_bounded_text(
        &candidate.stem_preview,
        MAX_CANDIDATE_PREVIEW_CHARS,
        "重复候选题预览",
    )?;
    validate_required_bounded_text(
        &candidate.subject_name,
        MAX_CANDIDATE_TAXONOMY_CHARS,
        "重复候选题学科名称",
    )?;
    validate_required_bounded_text(
        &candidate.chapter_name,
        MAX_CANDIDATE_TAXONOMY_CHARS,
        "重复候选题章节名称",
    )?;
    let source_kind = candidate.source_kind.as_deref().unwrap_or("question");
    if !matches!(source_kind, "question" | "import-item") {
        return Err(CommandError::validation(format!(
            "第 {} 项的重复候选来源无效。",
            item_index + 1
        )));
    }
    if source_kind == "import-item"
        && candidate
            .source_ordinal
            .is_none_or(|ordinal| ordinal == 0 || ordinal as usize > MAX_WORD_IMPORT_ITEMS)
    {
        return Err(CommandError::validation(format!(
            "第 {} 项缺少批次内重复候选的题目序号。",
            item_index + 1
        )));
    }
    if source_kind == "question" && candidate.source_ordinal.is_some() {
        return Err(CommandError::validation(format!(
            "第 {} 项的题库重复候选不应包含批次序号。",
            item_index + 1
        )));
    }
    if candidate.similarity_percent > 100
        || (source_kind == "question" && candidate.content_version < 1)
        || (source_kind == "import-item" && candidate.content_version != 0)
    {
        return Err(CommandError::validation(format!(
            "第 {} 项的重复候选题版本或相似度无效。",
            item_index + 1
        )));
    }
    Ok(())
}

pub(crate) fn normalized_source_file_name(value: &str) -> CommandResult<String> {
    let normalized = value.trim().nfkc().collect::<String>();
    let normalized = normalized.trim().to_owned();
    if normalized.is_empty()
        || exceeds_char_limit(&normalized, MAX_SOURCE_FILE_NAME_CHARS)
        || normalized.chars().any(char::is_control)
        || normalized.contains('/')
        || normalized.contains('\\')
        || normalized.contains(':')
        || matches!(normalized.as_str(), "." | "..")
        || Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(normalized.as_str())
    {
        return Err(CommandError::validation(
            "批量导入来源文件名无效；只能保存不含路径且不超过 255 个字符的文件名。",
        ));
    }
    let lowercase = normalized.to_ascii_lowercase();
    if !lowercase.ends_with(".docx") && !lowercase.ends_with(".xlsx") {
        return Err(CommandError::validation(
            "批量导入来源文件名必须以 .docx 或 .xlsx 结尾。",
        ));
    }
    Ok(normalized)
}

fn normalized_parser_version(value: &str) -> CommandResult<String> {
    let normalized = value.trim().nfkc().collect::<String>();
    let normalized = normalized.trim().to_owned();
    if normalized.is_empty()
        || exceeds_char_limit(&normalized, MAX_PARSER_VERSION_CHARS)
        || normalized
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{1e}' | '\u{1f}'))
    {
        return Err(CommandError::validation(
            "Word 分析器版本不能为空、不能含控制字符且不能超过 64 个字符。",
        ));
    }
    Ok(normalized)
}

fn validate_bounded_message(
    value: Option<&str>,
    max_chars: usize,
    label: &str,
) -> CommandResult<()> {
    if let Some(value) = value {
        if exceeds_char_limit(value, max_chars) {
            return Err(CommandError::validation(format!(
                "{label}不能超过 {max_chars} 个字符。"
            )));
        }
        if contains_reserved_separator(value) || contains_disallowed_text_control(value) {
            return Err(CommandError::validation(format!(
                "{label}包含软件保留或不允许的控制字符。"
            )));
        }
    }
    Ok(())
}

fn validate_required_bounded_text(value: &str, max_chars: usize, label: &str) -> CommandResult<()> {
    if value.trim().is_empty() || exceeds_char_limit(value, max_chars) {
        return Err(CommandError::validation(format!(
            "{label}不能为空且不能超过 {max_chars} 个字符。"
        )));
    }
    if contains_reserved_separator(value) || contains_disallowed_text_control(value) {
        return Err(CommandError::validation(format!(
            "{label}包含软件保留或不允许的控制字符。"
        )));
    }
    Ok(())
}

fn canonical_uuid(value: &str, label: &str) -> CommandResult<String> {
    if value.trim() != value {
        return Err(CommandError::validation(format!(
            "{label}前后不能包含空白字符。"
        )));
    }
    Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| CommandError::validation(format!("{label}无效。")))
}

fn exceeds_char_limit(value: &str, max_chars: usize) -> bool {
    value.len() > max_chars.saturating_mul(4) || value.chars().count() > max_chars
}

fn contains_reserved_separator(value: &str) -> bool {
    value
        .chars()
        .any(|character| matches!(character, '\u{1e}' | '\u{1f}'))
}

fn contains_disallowed_text_control(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn serialize_word_import_payload(payload: &WordImportDraftPayloadApi) -> CommandResult<String> {
    let serialized = serde_json::to_string(payload).map_err(|error| {
        CommandError::database(format!("Word 导入草稿 JSON 序列化失败：{error}"))
    })?;
    if serialized.len() > MAX_WORD_IMPORT_PAYLOAD_BYTES {
        return Err(CommandError::validation(
            "Word 导入草稿数据超过 64 MB 安全上限。",
        ));
    }
    Ok(serialized)
}

/// Count helper for badges that specifically describe question-editor work.
/// The existing bootstrap-wide `COUNT(*)` may remain broader when its label
/// means all pending workflows.
#[allow(dead_code)]
pub(crate) async fn pending_question_draft_count(pool: &SqlitePool) -> CommandResult<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM drafts WHERE draft_kind IN ('question_create', 'question_edit')",
    )
    .fetch_one(pool)
    .await
    .map_err(CommandError::database)
}

fn draft_identity(question_id: Option<&str>) -> CommandResult<DraftIdentity> {
    match question_id {
        None => Ok(DraftIdentity {
            draft_key: CREATE_DRAFT_KEY.to_owned(),
            draft_kind: QUESTION_CREATE_KIND,
            target_question_id: None,
        }),
        Some(raw_id) => {
            if raw_id.trim() != raw_id {
                return Err(CommandError::validation("题目 ID 前后不能含有空白字符。"));
            }
            let question_id = Uuid::parse_str(raw_id)
                .map_err(|_| CommandError::validation("题目 ID 无效。"))?
                .to_string();
            Ok(DraftIdentity {
                draft_key: format!("{EDIT_DRAFT_KEY_PREFIX}{question_id}"),
                draft_kind: QUESTION_EDIT_KIND,
                target_question_id: Some(question_id),
            })
        }
    }
}

fn record_from_row(row: DraftRow) -> CommandResult<QuestionDraftRecordApi> {
    let record_id = Uuid::parse_str(&row.id)
        .map_err(|_| corrupted("草稿记录 ID 不是有效的 UUID。"))?
        .to_string();
    if record_id != row.id {
        return Err(corrupted("草稿记录 ID 不是规范格式。"));
    }
    if row.payload_schema_version != i64::from(PAYLOAD_SCHEMA_VERSION) {
        return Err(CommandError::new(
            "DRAFT_SCHEMA_UNSUPPORTED",
            format!(
                "草稿使用了当前软件不支持的数据版本 {}。",
                row.payload_schema_version
            ),
        ));
    }
    if row.payload_json.len() > MAX_DRAFT_PAYLOAD_BYTES {
        return Err(corrupted("草稿 JSON 超过安全大小上限。"));
    }

    let mut payload: QuestionDraftApi = serde_json::from_str(&row.payload_json)
        .map_err(|error| corrupted(format!("草稿 JSON 无法读取：{error}")))?;
    let identity = stored_identity(&row, &payload)?;
    validate_draft_shape(&payload, &identity)
        .map_err(|error| corrupted(format!("草稿内容无效：{}", error.message)))?;

    if payload.base_content_version != row.base_content_version {
        return Err(corrupted("草稿 JSON 与数据库中的基础题目版本不一致。"));
    }
    payload.id = Some(record_id.clone());
    payload.question_id = identity.target_question_id.clone();

    let stale = stale_from_versions(
        identity.draft_kind,
        row.base_content_version,
        row.current_content_version,
    );

    Ok(QuestionDraftRecordApi {
        id: record_id,
        draft_key: row.draft_key,
        draft_kind: row.draft_kind,
        target_question_id: identity.target_question_id,
        base_content_version: row.base_content_version,
        payload_schema_version: PAYLOAD_SCHEMA_VERSION,
        payload,
        autosaved_at: row.autosaved_at_ms,
        created_at: row.created_at_ms,
        updated_at: row.updated_at_ms,
        stale,
    })
}

fn stored_identity(row: &DraftRow, payload: &QuestionDraftApi) -> CommandResult<DraftIdentity> {
    let identity = match row.draft_kind.as_str() {
        QUESTION_CREATE_KIND => {
            if payload.question_id.is_some()
                || payload.base_content_version.is_some()
                || row.target_question_id.is_some()
                || row.base_content_version.is_some()
            {
                return Err(corrupted("新建题目草稿不应关联题目 ID 或基础内容版本。"));
            }
            draft_identity(None).expect("the fixed create identity is valid")
        }
        QUESTION_EDIT_KIND => {
            let question_id = payload
                .question_id
                .as_deref()
                .ok_or_else(|| corrupted("编辑题目草稿缺少目标题目 ID。"))?;
            let identity = draft_identity(Some(question_id))
                .map_err(|_| corrupted("编辑题目草稿中的目标题目 ID 无效。"))?;
            if let Some(stored_target) = row.target_question_id.as_deref() {
                let stored_target = Uuid::parse_str(stored_target)
                    .map_err(|_| corrupted("草稿关联的目标题目 ID 无效。"))?
                    .to_string();
                if identity.target_question_id.as_deref() != Some(stored_target.as_str()) {
                    return Err(corrupted("草稿 JSON 与数据库关联的题目 ID 不一致。"));
                }
            }
            if row.base_content_version.is_none_or(|version| version < 1) {
                return Err(corrupted("编辑题目草稿缺少有效的基础内容版本。"));
            }
            identity
        }
        _ => return Err(corrupted("草稿类型不是题目新建或题目编辑草稿。")),
    };

    if identity.draft_key != row.draft_key {
        return Err(corrupted("草稿键与后端规定的格式不一致。"));
    }
    Ok(identity)
}

fn validate_draft_shape(draft: &QuestionDraftApi, identity: &DraftIdentity) -> CommandResult<()> {
    if draft.question_type.trim().is_empty() || draft.question_type.len() > 64 {
        return Err(CommandError::validation("题型标识无效。"));
    }
    if draft.options.len() > MAX_OPTIONS {
        return Err(CommandError::validation(format!(
            "一道题最多允许 {MAX_OPTIONS} 个选项。"
        )));
    }
    if draft.tag_ids.len() > MAX_TAGS {
        return Err(CommandError::validation(format!(
            "一道题最多允许 {MAX_TAGS} 个标签。"
        )));
    }
    if draft.tag_ids.iter().collect::<HashSet<_>>().len() != draft.tag_ids.len() {
        return Err(CommandError::validation("同一个标签不能在草稿中重复。"));
    }

    match identity.draft_kind {
        QUESTION_CREATE_KIND => {
            if draft.question_id.is_some() || draft.base_content_version.is_some() {
                return Err(CommandError::validation(
                    "新建题目的草稿不能携带题目 ID 或基础内容版本。",
                ));
            }
        }
        QUESTION_EDIT_KIND => {
            let expected_id = identity
                .target_question_id
                .as_deref()
                .expect("edit identity always has a target");
            let payload_id = draft
                .question_id
                .as_deref()
                .ok_or_else(|| CommandError::validation("编辑草稿缺少题目 ID。"))?;
            let payload_id = Uuid::parse_str(payload_id)
                .map_err(|_| CommandError::validation("编辑草稿的题目 ID 无效。"))?
                .to_string();
            if payload_id != expected_id {
                return Err(CommandError::validation(
                    "编辑草稿中的题目 ID 与保存目标不一致。",
                ));
            }
            if draft.base_content_version.is_none_or(|version| version < 1) {
                return Err(CommandError::validation(
                    "编辑草稿必须记录大于零的基础内容版本。",
                ));
            }
        }
        _ => {
            return Err(CommandError::validation(
                "草稿类型不是题目编辑器支持的类型。",
            ));
        }
    }

    validate_optional_uuid(&draft.subject_id, "学科 ID")?;
    validate_optional_uuid(&draft.chapter_id, "章节 ID")?;
    let option_ids = option_id_set(draft)?;
    for (index, option) in draft.options.iter().enumerate() {
        validate_optional_uuid(&option.id, &format!("选项 {} 的 ID", index + 1))?;
        validate_rich_content(&format!("选项 {}", index + 1), &option.content)?;
    }
    resources::validate_resource_ref_shape(&draft.resource_refs, &option_ids)?;
    for tag_id in &draft.tag_ids {
        validate_optional_uuid(tag_id, "标签 ID")?;
    }
    validate_rich_content("题干", &draft.stem)?;
    validate_rich_content("答案", &draft.answer)?;
    validate_rich_content("解析", &draft.explanation)?;

    let total_bytes = rich_content_bytes(&draft.stem)
        .saturating_add(rich_content_bytes(&draft.answer))
        .saturating_add(rich_content_bytes(&draft.explanation))
        .saturating_add(
            draft
                .options
                .iter()
                .map(|option| rich_content_bytes(&option.content))
                .sum::<usize>(),
        );
    if total_bytes > MAX_TOTAL_CONTENT_BYTES {
        return Err(CommandError::validation(format!(
            "草稿内容合计超过 {} MB 的安全上限。",
            MAX_TOTAL_CONTENT_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

fn option_id_set(draft: &QuestionDraftApi) -> CommandResult<HashSet<String>> {
    let mut ids = HashSet::with_capacity(draft.options.len());
    for option in &draft.options {
        if option.id.is_empty() {
            continue;
        }
        let canonical = Uuid::parse_str(&option.id)
            .map(|id| id.to_string())
            .map_err(|_| CommandError::validation("选项 ID 无效。"))?;
        if canonical != option.id || !ids.insert(canonical) {
            return Err(CommandError::validation(
                "选项 ID 必须使用规范 UUID，且同一道题中不能重复。",
            ));
        }
    }
    Ok(ids)
}

fn validate_optional_uuid(value: &str, label: &str) -> CommandResult<()> {
    if value.is_empty() || Uuid::parse_str(value).is_ok() {
        Ok(())
    } else {
        Err(CommandError::validation(format!("{label} 无效。")))
    }
}

fn validate_rich_content(label: &str, content: &RichContentApi) -> CommandResult<()> {
    validate_rich_content_with_field_limits(label, content, true)
}

fn validate_document_entry_source(label: &str, content: &RichContentApi) -> CommandResult<()> {
    // A document-entry draft contains a whole batch rather than one question
    // field. Its size is bounded once by MAX_DOCUMENT_QUESTION_PAYLOAD_BYTES,
    // so applying the per-field HTML/JSON caps here rejects normal image-rich
    // WPS pastes long before the batch-level bound is reached.
    validate_rich_content_with_field_limits(label, content, false)
}

fn validate_rich_content_with_field_limits(
    label: &str,
    content: &RichContentApi,
    enforce_field_limits: bool,
) -> CommandResult<()> {
    validate_rich_document(label, content, enforce_field_limits)?;
    if enforce_field_limits && content.html.len() > MAX_RICH_HTML_BYTES {
        return Err(CommandError::validation(format!(
            "{label}的富文本超过 {} MB 的安全上限。",
            MAX_RICH_HTML_BYTES / 1024 / 1024
        )));
    }
    if enforce_field_limits && content.plain_text.len() > MAX_RICH_PLAIN_BYTES {
        return Err(CommandError::validation(format!(
            "{label}的可搜索文字过长。"
        )));
    }
    if enforce_field_limits
        && content
            .source_ooxml
            .as_ref()
            .is_some_and(|xml| xml.len() > MAX_SOURCE_OOXML_BYTES)
    {
        return Err(CommandError::validation(format!(
            "{label}保留的 Word 原始内容超过安全上限。"
        )));
    }
    if contains_reserved_separator(&content.html)
        || contains_reserved_separator(&content.plain_text)
        || content
            .source_ooxml
            .as_ref()
            .is_some_and(|xml| contains_reserved_separator(xml))
    {
        return Err(CommandError::validation(format!(
            "{label}包含软件保留的控制分隔符，请删除后重试。"
        )));
    }

    let normalized_html = content
        .html
        .to_ascii_lowercase()
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let forbidden = [
        "<script",
        "<iframe",
        "<object",
        "<embed",
        "<link",
        "<meta",
        "<base",
        "javascript:",
        "data:text/html",
        "srcdoc=",
    ];
    if forbidden
        .iter()
        .any(|pattern| normalized_html.contains(pattern))
        || contains_inline_event_handler(&normalized_html)
    {
        return Err(CommandError::new(
            "UNSAFE_RICH_CONTENT",
            format!("{label}中包含不安全的网页代码，已停止自动保存。"),
        ));
    }
    Ok(())
}

fn contains_inline_event_handler(html: &str) -> bool {
    let bytes = html.as_bytes();
    let mut index = 0usize;
    while index + 3 < bytes.len() {
        if bytes[index] == b' ' && bytes[index + 1] == b'o' && bytes[index + 2] == b'n' {
            let mut cursor = index + 3;
            while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
                cursor += 1;
            }
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor > index + 3 && bytes.get(cursor) == Some(&b'=') {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn validate_rich_document(
    label: &str,
    content: &RichContentApi,
    enforce_document_limit: bool,
) -> CommandResult<()> {
    match content.schema_version {
        1 => {
            if content.editor.is_some()
                || content.editor_version.is_some()
                || content.document.is_some()
            {
                return Err(CommandError::validation(format!(
                    "{label}的旧内容版本不能携带编辑器 JSON。"
                )));
            }
        }
        2 => {
            if content.editor.as_deref() != Some("tiptap") {
                return Err(CommandError::validation(format!(
                    "{label}的编辑器类型不受支持。"
                )));
            }
            let version = content.editor_version.as_deref().unwrap_or_default().trim();
            if version.is_empty() || version.chars().count() > 64 {
                return Err(CommandError::validation(format!(
                    "{label}的编辑器版本无效。"
                )));
            }
            let document = content.document.as_ref().ok_or_else(|| {
                CommandError::validation(format!("{label}缺少 Tiptap 文档 JSON。"))
            })?;
            if document.get("type").and_then(serde_json::Value::as_str) != Some("doc") {
                return Err(CommandError::validation(format!(
                    "{label}的 Tiptap 文档根节点无效。"
                )));
            }
            if enforce_document_limit
                && serde_json::to_vec(document)
                    .map_err(CommandError::database)?
                    .len()
                    > MAX_RICH_DOCUMENT_BYTES
            {
                return Err(CommandError::validation(format!(
                    "{label}的 Tiptap 文档 JSON 超过安全上限。"
                )));
            }
        }
        _ => {
            return Err(CommandError::validation(format!(
                "{label}使用了当前软件不支持的内容版本。"
            )));
        }
    }
    Ok(())
}

fn rich_content_bytes(content: &RichContentApi) -> usize {
    content
        .html
        .len()
        .saturating_add(content.plain_text.len())
        .saturating_add(
            content
                .source_ooxml
                .as_ref()
                .map_or(0, std::string::String::len),
        )
        .saturating_add(
            content
                .document
                .as_ref()
                .and_then(|document| serde_json::to_vec(document).ok())
                .map_or(0, |document| document.len()),
        )
}

fn serialize_payload(payload: &QuestionDraftApi) -> CommandResult<String> {
    let serialized = serde_json::to_string(payload)
        .map_err(|error| CommandError::database(format!("草稿 JSON 序列化失败：{error}")))?;
    if serialized.len() > MAX_DRAFT_PAYLOAD_BYTES {
        return Err(CommandError::validation(format!(
            "草稿数据超过 {} MB 的安全上限。",
            MAX_DRAFT_PAYLOAD_BYTES / 1024 / 1024
        )));
    }
    Ok(serialized)
}

fn stale_from_versions(
    draft_kind: &str,
    base_content_version: Option<i64>,
    current_content_version: Option<i64>,
) -> bool {
    draft_kind == QUESTION_EDIT_KIND && base_content_version != current_content_version
}

fn corrupted(message: impl Into<String>) -> CommandError {
    CommandError::new("DRAFT_CORRUPTED", message)
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

    fn rich(html: &str, plain_text: &str) -> RichContentApi {
        RichContentApi {
            schema_version: 1,
            editor: None,
            editor_version: None,
            document: None,
            html: html.to_owned(),
            plain_text: plain_text.to_owned(),
            source_ooxml: None,
        }
    }

    fn word_import_payload() -> WordImportDraftPayloadApi {
        let item_id = "018f4c1a-1234-7abc-8def-0123456789ab".to_owned();
        WordImportDraftPayloadApi {
            schema_version: 1,
            source_file_name: "课堂练习.docx".to_owned(),
            source_file_size: 1_024,
            parser_version: "w1-ooxml-1".to_owned(),
            import_session_id: None,
            source_item_count: Some(1),
            omitted_item_count: 0,
            active_item_id: Some(item_id.clone()),
            items: vec![WordImportDraftItemApi {
                id: item_id,
                ordinal: 1,
                selected: true,
                payload: QuestionDraftApi {
                    id: None,
                    question_id: None,
                    question_type: "short_answer".to_owned(),
                    stem: rich("<p>待审查题干</p>", "待审查题干"),
                    options: Vec::new(),
                    answer: rich("", ""),
                    explanation: rich("", ""),
                    subject_id: String::new(),
                    chapter_id: String::new(),
                    tag_ids: Vec::new(),
                    resource_refs: Vec::new(),
                    base_content_version: None,
                },
                status: "warning".to_owned(),
                diagnostic: Some("请补充学科和答案。".to_owned()),
                duplicate_kind: "none".to_owned(),
                candidate: None,
                action: None,
                needs_check: true,
                check_error: None,
            }],
        }
    }

    fn document_question_payload() -> DocumentQuestionDraftPayloadApi {
        let item_id = "018f4c1a-1234-7abc-8def-0123456789ab".to_owned();
        DocumentQuestionDraftPayloadApi {
            schema_version: 1,
            active_item_id: Some(item_id.clone()),
            items: vec![super::super::models::DocumentQuestionDraftItemApi {
                id: item_id,
                ordinal: 1,
                payload: QuestionDraftApi {
                    id: None,
                    question_id: None,
                    question_type: "short_answer".to_owned(),
                    stem: rich("", ""),
                    options: Vec::new(),
                    answer: rich("", ""),
                    explanation: rich("", ""),
                    subject_id: String::new(),
                    chapter_id: String::new(),
                    tag_ids: Vec::new(),
                    resource_refs: Vec::new(),
                    base_content_version: None,
                },
            }],
            template_id: None,
            template_name_snapshot: None,
            template_config_snapshot: None,
            source: None,
        }
    }

    #[test]
    fn create_draft_has_one_fixed_server_owned_key() {
        let identity = draft_identity(None).unwrap();
        assert_eq!(identity.draft_key, "question:create");
        assert_eq!(identity.draft_kind, "question_create");
        assert_eq!(identity.target_question_id, None);
    }

    #[test]
    fn edit_draft_key_uses_canonical_uuid() {
        let identity = draft_identity(Some("018F4C1A-1234-7ABC-8DEF-0123456789AB")).unwrap();
        assert_eq!(
            identity.draft_key,
            "question:018f4c1a-1234-7abc-8def-0123456789ab"
        );
        assert_eq!(identity.draft_kind, "question_edit");
        assert_eq!(
            identity.target_question_id.as_deref(),
            Some("018f4c1a-1234-7abc-8def-0123456789ab")
        );
    }

    #[test]
    fn malformed_or_padded_question_ids_are_rejected() {
        assert!(draft_identity(Some("not-a-uuid")).is_err());
        assert!(draft_identity(Some(" 018f4c1a-1234-7abc-8def-0123456789ab")).is_err());
    }

    #[test]
    fn document_question_draft_requires_contiguous_unique_items() {
        let payload = document_question_payload();
        validate_document_question_payload(&payload).unwrap();

        let mut invalid = payload.clone();
        invalid.items[0].ordinal = 2;
        assert!(validate_document_question_payload(&invalid).is_err());

        let mut invalid = payload;
        invalid.active_item_id = Some("018f4c1a-1234-7abc-8def-0123456789ac".to_owned());
        assert!(validate_document_question_payload(&invalid).is_err());
    }

    #[test]
    fn free_document_draft_accepts_tiptap_source_without_fixed_items() {
        let payload = DocumentQuestionDraftPayloadApi {
            schema_version: 2,
            active_item_id: None,
            items: Vec::new(),
            template_id: Some("labeled_fields_v1".to_owned()),
            template_name_snapshot: None,
            template_config_snapshot: None,
            source: Some(rich(
                "<p>题型：单选题</p><p>题目：示例</p><p>答案：A</p>",
                "题型：单选题\n题目：示例\n答案：A",
            )),
        };
        validate_document_question_payload(&payload).unwrap();

        let mut invalid = payload;
        invalid.template_id = Some("unknown".to_owned());
        assert!(validate_document_question_payload(&invalid).is_err());
    }

    #[test]
    fn free_document_draft_uses_the_batch_limit_instead_of_single_field_limits() {
        let large_text = "x".repeat(5 * 1024 * 1024);
        let source = RichContentApi {
            schema_version: 2,
            editor: Some("tiptap".to_owned()),
            editor_version: Some("3.28.0".to_owned()),
            document: Some(serde_json::json!({
                "type": "doc",
                "content": [{
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": large_text.clone() }]
                }]
            })),
            html: format!("<p>{large_text}</p>"),
            plain_text: "批量录入内容".to_owned(),
            source_ooxml: None,
        };
        assert!(source.html.len() > MAX_RICH_HTML_BYTES);
        assert!(
            serde_json::to_vec(source.document.as_ref().unwrap())
                .unwrap()
                .len()
                > MAX_RICH_DOCUMENT_BYTES
        );

        let payload = DocumentQuestionDraftPayloadApi {
            schema_version: LEGACY_FREE_DOCUMENT_DRAFT_SCHEMA_VERSION,
            active_item_id: None,
            items: Vec::new(),
            template_id: Some(DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID.to_owned()),
            template_name_snapshot: None,
            template_config_snapshot: None,
            source: Some(source),
        };
        validate_document_question_payload(&payload).unwrap();
    }

    #[test]
    fn free_document_draft_still_rejects_executable_html() {
        let payload = DocumentQuestionDraftPayloadApi {
            schema_version: LEGACY_FREE_DOCUMENT_DRAFT_SCHEMA_VERSION,
            active_item_id: None,
            items: Vec::new(),
            template_id: Some(DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID.to_owned()),
            template_name_snapshot: None,
            template_config_snapshot: None,
            source: Some(RichContentApi {
                schema_version: 2,
                editor: Some("tiptap".to_owned()),
                editor_version: Some("3.28.0".to_owned()),
                document: Some(serde_json::json!({
                    "type": "doc",
                    "content": [{ "type": "paragraph" }]
                })),
                html: "<script>alert('unsafe')</script>".to_owned(),
                plain_text: "unsafe".to_owned(),
                source_ooxml: None,
            }),
        };
        assert!(validate_document_question_payload(&payload).is_err());
    }

    #[test]
    fn current_free_document_draft_requires_a_valid_template_snapshot() {
        use super::super::models::{DocumentEntryOptionStyleApi, DocumentEntryTemplateConfigApi};

        let config = DocumentEntryTemplateConfigApi {
            schema_version: 1,
            type_marker: "【类型】".to_owned(),
            stem_marker: "【题干】".to_owned(),
            answer_marker: "【答案】".to_owned(),
            explanation_marker: "【解析】".to_owned(),
            option_style: DocumentEntryOptionStyleApi::Brackets,
            question_separator: "---".to_owned(),
            compatible_default_markers: true,
            recognize_question_numbers: true,
            strip_recognized_question_numbers: true,
            stem_prompt: "题目".to_owned(),
            option_prompt: "选项".to_owned(),
            answer_prompt: "答案".to_owned(),
            explanation_prompt: "解析".to_owned(),
        };
        let payload = DocumentQuestionDraftPayloadApi {
            schema_version: 3,
            active_item_id: None,
            items: Vec::new(),
            template_id: Some("018f4c1a-1234-7abc-8def-0123456789ab".to_owned()),
            template_name_snapshot: Some("我的模板".to_owned()),
            template_config_snapshot: Some(config),
            source: Some(RichContentApi {
                schema_version: 2,
                editor: Some("tiptap".to_owned()),
                editor_version: Some("3.28.0".to_owned()),
                document: Some(serde_json::json!({
                    "type": "doc",
                    "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "【题干】示例" }] }]
                })),
                html: "<p>【题干】示例</p>".to_owned(),
                plain_text: "【题干】示例".to_owned(),
                source_ooxml: None,
            }),
        };
        validate_document_question_payload(&payload).unwrap();

        let mut invalid = payload;
        invalid
            .template_config_snapshot
            .as_mut()
            .unwrap()
            .stem_marker = "【类型】内容".to_owned();
        assert!(validate_document_question_payload(&invalid).is_err());
    }

    #[test]
    fn edited_draft_is_stale_when_question_changed_or_was_deleted() {
        assert!(!stale_from_versions("question_edit", Some(3), Some(3)));
        assert!(stale_from_versions("question_edit", Some(3), Some(4)));
        assert!(stale_from_versions("question_edit", Some(3), None));
        assert!(!stale_from_versions("question_create", None, None));
    }

    #[test]
    fn inline_event_handler_detection_matches_question_write_safety_rule() {
        assert!(contains_inline_event_handler(
            "<img src=x onerror =alert(1)>"
        ));
        assert!(!contains_inline_event_handler("<strong>on call</strong>"));
    }

    #[test]
    fn batch_import_metadata_accepts_safe_docx_and_xlsx_names_without_paths() {
        let mut payload = word_import_payload();
        payload.source_file_name = "  Ｌｅｓｓｏｎ．ｄｏｃｘ  ".to_owned();
        payload.parser_version = "  ｗ１-v1  ".to_owned();
        let normalized = normalized_word_import_payload(&payload).unwrap();
        assert_eq!(normalized.source_file_name, "Lesson.docx");
        assert_eq!(normalized.parser_version, "w1-v1");
        validate_word_import_payload(&normalized).unwrap();

        payload.source_file_name = "  题库．ＸＬＳＸ  ".to_owned();
        let normalized = normalized_word_import_payload(&payload).unwrap();
        assert_eq!(normalized.source_file_name, "题库.XLSX");
        validate_word_import_payload(&normalized).unwrap();

        payload.source_file_name = "C:\\教师资料\\Lesson.docx".to_owned();
        assert!(normalized_word_import_payload(&payload).is_err());
        payload.source_file_name = "../Lesson.docx".to_owned();
        assert!(normalized_word_import_payload(&payload).is_err());
        payload.source_file_name = "题库.xlsm".to_owned();
        assert!(normalized_word_import_payload(&payload).is_err());
    }

    #[test]
    fn word_import_items_require_unique_canonical_identity() {
        let mut payload = word_import_payload();
        let mut duplicate = payload.items[0].clone();
        duplicate.ordinal = 2;
        payload.items.push(duplicate);
        assert!(validate_word_import_payload(&payload).is_err());

        let mut payload = word_import_payload();
        payload.items[0].id = "018F4C1A-1234-7ABC-8DEF-0123456789AB".to_owned();
        payload.active_item_id = None;
        assert!(validate_word_import_payload(&payload).is_err());

        let mut payload = word_import_payload();
        payload.active_item_id = Some("018f4c1a-1234-7abc-8def-0123456789ac".to_owned());
        assert!(validate_word_import_payload(&payload).is_err());

        let mut payload = word_import_payload();
        payload.items[0].ordinal = 0;
        assert!(validate_word_import_payload(&payload).is_err());
        payload.items[0].ordinal = (MAX_WORD_IMPORT_ITEMS + 1) as u32;
        assert!(validate_word_import_payload(&payload).is_err());
    }

    #[test]
    fn word_import_truncation_metadata_must_cover_retained_items() {
        let mut payload = word_import_payload();
        payload.source_item_count = Some(2);
        payload.omitted_item_count = 1;
        validate_word_import_payload(&payload).unwrap();

        payload.source_item_count = Some(1);
        assert!(validate_word_import_payload(&payload).is_err());

        payload.source_item_count = None;
        assert!(validate_word_import_payload(&payload).is_err());
    }

    #[test]
    fn word_import_question_payload_is_create_shaped_but_may_be_incomplete() {
        let payload = word_import_payload();
        validate_word_import_payload(&payload).unwrap();

        let mut editing_payload = word_import_payload();
        editing_payload.items[0].payload.question_id =
            Some("018f4c1a-1234-7abc-8def-0123456789ac".to_owned());
        editing_payload.items[0].payload.base_content_version = Some(1);
        assert!(validate_word_import_payload(&editing_payload).is_err());
    }

    #[test]
    fn word_import_duplicate_kind_and_candidate_are_consistent() {
        let mut payload = word_import_payload();
        payload.items[0].duplicate_kind = "exact".to_owned();
        assert!(validate_word_import_payload(&payload).is_err());

        let mut payload = word_import_payload();
        payload.items[0].candidate = Some(QuestionDuplicateCandidateApi {
            id: "018f4c1a-1234-7abc-8def-0123456789ac".to_owned(),
            question_type: "short_answer".to_owned(),
            stem_preview: "已有题干".to_owned(),
            subject_name: "语文".to_owned(),
            chapter_name: "第一章".to_owned(),
            similarity_percent: 100,
            content_version: 1,
            source_kind: None,
            source_ordinal: None,
        });
        assert!(validate_word_import_payload(&payload).is_err());
    }

    #[test]
    fn word_import_rejects_unsafe_html_and_reserved_separators() {
        let mut payload = word_import_payload();
        payload.items[0].payload.stem.html = "<img src=x onerror=alert(1)>".to_owned();
        assert!(validate_word_import_payload(&payload).is_err());

        let mut payload = word_import_payload();
        payload.items[0].payload.stem.plain_text = "题干\u{1f}答案".to_owned();
        assert!(validate_word_import_payload(&payload).is_err());
    }

    #[test]
    fn word_import_payload_rejects_unknown_top_level_fields() {
        let mut value = serde_json::to_value(word_import_payload()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("draftKey".to_owned(), serde_json::json!("client-owned"));
        assert!(serde_json::from_value::<WordImportDraftPayloadApi>(value).is_err());
    }
}
