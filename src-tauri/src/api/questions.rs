use sha2::{Digest, Sha256};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteConnection, SqlitePool};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::{Mutex, OnceLock};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::models::{
    CommandError, CommandResult, IgnoreQuestionDuplicateRequestApi,
    ImportedQuestionOverwriteRequestApi, ImportedQuestionOverwriteResultApi,
    ImportedQuestionOverwriteResultItemApi, PageResultApi, QuestionApi,
    QuestionBatchEditRequestApi, QuestionBatchEditResultApi, QuestionBatchVersionApi,
    QuestionDraftApi, QuestionDuplicateBatchItemResultApi, QuestionDuplicateBatchRequestApi,
    QuestionDuplicateBatchResultApi, QuestionDuplicateCandidateApi, QuestionDuplicateCheckApi,
    QuestionDuplicateGroupApi, QuestionDuplicateMemberApi, QuestionDuplicateScanRequestApi,
    QuestionDuplicateScanResultApi, QuestionFiltersApi, QuestionOptionApi, QuestionResourceRefApi,
    RichContentApi, TagApi,
};
use super::{
    question_types,
    question_usage::{
        UsageConstraint, push_usage_constraint, resolve_usage_constraint, validate_usage_filter,
    },
    resources,
};

const MAX_RICH_HTML_BYTES: usize = 2 * 1024 * 1024;
const MAX_RICH_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_RICH_PLAIN_BYTES: usize = 512 * 1024;
const MAX_SOURCE_OOXML_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOTAL_CONTENT_BYTES: usize = 12 * 1024 * 1024;
const MAX_OPTIONS: usize = 26;
const MAX_TAGS: usize = 100;
const MAX_BATCH_EDIT_QUESTIONS: usize = 500;
const MAX_IMPORTED_QUESTION_OVERWRITES: usize = 100;
const MAX_DUPLICATE_CANDIDATES: usize = 256;
const MAX_DUPLICATE_BATCH_ITEMS: usize = 100;
const MAX_IMPORT_DUPLICATE_BATCH_ITEMS: usize = 5_000;
const MAX_DUPLICATE_PROFILE_CACHE: usize = 1_024;
const MAX_SIMILARITY_FIELD_CHARS: usize = 8_192;
const DUPLICATE_STEM_PREVIEW_CHARS: usize = 160;
const SUSPECTED_DUPLICATE_THRESHOLD_PERCENT: u32 = 82;
const SIMILARITY_METHOD: &str = "nfkc_char_bigram_dice_v1";
const MAX_DUPLICATE_SCAN_SCOPE_QUESTIONS: usize = 5_000;
const MAX_REMEMBERED_CANCELLED_DUPLICATE_SCANS: usize = 256;

static CANCELLED_DUPLICATE_SCANS: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

pub fn cancel_question_duplicate_scan(scan_id: &str) -> CommandResult<()> {
    let scan_id = canonical_uuid(scan_id, "重复检查任务标识")?;
    let mut cancelled = CANCELLED_DUPLICATE_SCANS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .map_err(|_| CommandError::database("重复检查取消状态暂时不可用。"))?;
    if !cancelled.contains(&scan_id) {
        cancelled.push_back(scan_id);
        while cancelled.len() > MAX_REMEMBERED_CANCELLED_DUPLICATE_SCANS {
            cancelled.pop_front();
        }
    }
    Ok(())
}

fn ensure_duplicate_scan_active(scan_id: Option<&str>) -> CommandResult<()> {
    let Some(scan_id) = scan_id else {
        return Ok(());
    };
    let cancelled = CANCELLED_DUPLICATE_SCANS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .map_err(|_| CommandError::database("重复检查取消状态暂时不可用。"))?;
    if cancelled.iter().any(|cancelled_id| cancelled_id == scan_id) {
        return Err(CommandError::new(
            "DUPLICATE_SCAN_CANCELLED",
            "重复题检查已取消。",
        ));
    }
    Ok(())
}

#[derive(Debug, FromRow)]
struct QuestionRow {
    id: String,
    question_type: String,
    subject_id: String,
    chapter_id: String,
    subject_name: String,
    chapter_name: String,
    stem_json: String,
    answer_json: String,
    explanation_json: String,
    content_version: i64,
    last_used_at_ms: Option<i64>,
    deleted_at_ms: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, FromRow)]
struct ExportOptionRow {
    question_id: String,
    id: String,
    position: i64,
    content_json: String,
}

#[derive(Debug, FromRow)]
struct ExportTagRow {
    question_id: String,
    id: String,
    name: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    question_count: i64,
}

#[derive(Debug, FromRow)]
struct ExportResourceRefRow {
    question_id: String,
    node_id: String,
    resource_id: String,
    content_slot: String,
    option_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct DuplicateCandidateRow {
    id: String,
    question_type: String,
    stem_plain: String,
    options_plain: String,
    answer_plain: String,
    subject_name: String,
    chapter_name: String,
    content_version: i64,
}

#[derive(Clone, Debug, FromRow)]
struct DuplicateScanMetaRow {
    id: String,
    question_type: String,
    subject_id: String,
    chapter_id: String,
    stem_preview: String,
    subject_name: String,
    chapter_name: String,
    exact_fingerprint: Vec<u8>,
    content_version: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    similarity_length: i64,
}

#[derive(Debug, FromRow)]
struct DuplicateScanTextRow {
    id: String,
    stem_plain: String,
    options_plain: String,
    answer_plain: String,
}

#[derive(Debug, FromRow)]
struct DuplicateIgnoreRow {
    question_id_low: String,
    question_id_high: String,
    content_version_low: i64,
    content_version_high: i64,
}

#[derive(Debug, FromRow)]
struct BatchQuestionRow {
    id: String,
    subject_id: String,
    chapter_id: String,
    content_version: i64,
    last_used_at_ms: Option<i64>,
    deleted_at_ms: Option<i64>,
}

fn parse_rich(json: &str) -> CommandResult<RichContentApi> {
    serde_json::from_str(json).map_err(|error| {
        CommandError::new("CONTENT_CORRUPTED", format!("题目富内容无法读取：{error}"))
    })
}

async fn hydrate_question(pool: &SqlitePool, row: QuestionRow) -> CommandResult<QuestionApi> {
    hydrate_questions(pool, vec![row])
        .await?
        .pop()
        .ok_or_else(|| CommandError::database("题目批量读取结果为空。"))
}

async fn hydrate_questions(
    pool: &SqlitePool,
    rows: Vec<QuestionRow>,
) -> CommandResult<Vec<QuestionApi>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let question_ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();

    let mut option_builder = QueryBuilder::<Sqlite>::new(
        "SELECT qo.question_id, qo.id, qo.position, qo.content_json \
         FROM question_options qo WHERE qo.question_id IN (",
    );
    {
        let mut separated = option_builder.separated(", ");
        for id in &question_ids {
            separated.push_bind(id.clone());
        }
        separated.push_unseparated(") ORDER BY qo.question_id, qo.position, qo.id");
    }
    let option_rows = option_builder
        .build_query_as::<ExportOptionRow>()
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)?;

    let mut tag_builder = QueryBuilder::<Sqlite>::new(
        "WITH active_tag_counts AS ( \
             SELECT qt.tag_id, COUNT(*) AS question_count \
             FROM question_tags qt JOIN questions q ON q.id = qt.question_id \
             WHERE q.deleted_at_ms IS NULL GROUP BY qt.tag_id \
         ) \
         SELECT qt.question_id, t.id, t.name, t.created_at_ms, t.updated_at_ms, \
                COALESCE(atc.question_count, 0) AS question_count \
         FROM question_tags qt JOIN tags t ON t.id = qt.tag_id \
         LEFT JOIN active_tag_counts atc ON atc.tag_id = t.id \
         WHERE qt.question_id IN (",
    );
    {
        let mut separated = tag_builder.separated(", ");
        for id in &question_ids {
            separated.push_bind(id.clone());
        }
        separated.push_unseparated(") ORDER BY qt.question_id, t.name, t.id");
    }
    let tag_rows = tag_builder
        .build_query_as::<ExportTagRow>()
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)?;

    let mut resource_builder = QueryBuilder::<Sqlite>::new(
        "SELECT qrr.question_id, qrr.node_id, qrr.resource_id, qrr.content_slot, qrr.option_id \
         FROM question_resource_refs qrr WHERE qrr.question_id IN (",
    );
    {
        let mut separated = resource_builder.separated(", ");
        for id in &question_ids {
            separated.push_bind(id.clone());
        }
        separated.push_unseparated(") ORDER BY qrr.question_id, qrr.created_at_ms, qrr.id");
    }
    let resource_rows = resource_builder
        .build_query_as::<ExportResourceRefRow>()
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)?;

    let mut options_by_question = HashMap::<String, Vec<QuestionOptionApi>>::new();
    for option in option_rows {
        options_by_question
            .entry(option.question_id)
            .or_default()
            .push(QuestionOptionApi {
                id: option.id,
                position: u32::try_from(option.position).unwrap_or_default(),
                content: parse_rich(&option.content_json)?,
            });
    }
    let mut tags_by_question = HashMap::<String, Vec<TagApi>>::new();
    for tag in tag_rows {
        tags_by_question
            .entry(tag.question_id)
            .or_default()
            .push(TagApi {
                id: tag.id,
                name: tag.name,
                question_count: tag.question_count,
                created_at: tag.created_at_ms,
                updated_at: tag.updated_at_ms,
            });
    }
    let mut resources_by_question = HashMap::<String, Vec<QuestionResourceRefApi>>::new();
    for resource in resource_rows {
        resources_by_question
            .entry(resource.question_id)
            .or_default()
            .push(QuestionResourceRefApi {
                node_id: resource.node_id,
                resource_id: resource.resource_id,
                content_slot: resource.content_slot,
                option_id: resource.option_id,
            });
    }

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row.id.clone();
        items.push(QuestionApi {
            id: row.id,
            question_type: row.question_type,
            question_type_name: None,
            stem: parse_rich(&row.stem_json)?,
            options: options_by_question.remove(&id).unwrap_or_default(),
            answer: parse_rich(&row.answer_json)?,
            explanation: parse_rich(&row.explanation_json)?,
            subject_id: row.subject_id,
            chapter_id: row.chapter_id,
            subject_name: row.subject_name,
            chapter_name: row.chapter_name,
            tags: tags_by_question.remove(&id).unwrap_or_default(),
            resource_refs: resources_by_question.remove(&id).unwrap_or_default(),
            last_used_at: row.last_used_at_ms,
            deleted_at: row.deleted_at_ms,
            created_at: row.created_at_ms,
            updated_at: row.updated_at_ms,
            content_version: row.content_version,
        });
    }
    Ok(items)
}

fn push_filters<'args>(
    builder: &mut QueryBuilder<'args, Sqlite>,
    filters: &'args QuestionFiltersApi,
    usage_constraint: UsageConstraint,
) {
    if filters.deleted {
        builder.push(" AND q.deleted_at_ms IS NOT NULL");
    } else {
        builder.push(" AND q.deleted_at_ms IS NULL");
    }
    if !filters.keyword.trim().is_empty() {
        let pattern = format!("%{}%", filters.keyword.trim());
        builder
            .push(" AND (q.stem_plain LIKE ")
            .push_bind(pattern.clone())
            .push(" OR q.options_plain LIKE ")
            .push_bind(pattern.clone())
            .push(" OR q.answer_plain LIKE ")
            .push_bind(pattern.clone())
            .push(" OR q.explanation_plain LIKE ")
            .push_bind(pattern.clone())
            .push(" OR q.tags_plain LIKE ")
            .push_bind(pattern)
            .push(")");
    }
    if let Some(subject_id) = &filters.subject_id {
        builder
            .push(" AND q.subject_id = ")
            .push_bind(subject_id.clone());
    }
    if let Some(chapter_id) = &filters.chapter_id {
        builder
            .push(" AND q.chapter_id = ")
            .push_bind(chapter_id.clone());
    }
    if let Some(question_type) = &filters.question_type {
        builder
            .push(" AND q.question_type = ")
            .push_bind(question_type.clone());
    }
    push_usage_constraint(builder, usage_constraint);
    if filters.tag_match_mode == "any" && !filters.tag_ids.is_empty() {
        builder.push(" AND (");
        for (index, tag_id) in filters.tag_ids.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            builder
                .push("EXISTS (SELECT 1 FROM question_tags filter_qt WHERE filter_qt.question_id = q.id AND filter_qt.tag_id = ")
                .push_bind(tag_id.clone())
                .push(")");
        }
        builder.push(")");
    } else {
        for tag_id in &filters.tag_ids {
            builder
                .push(" AND EXISTS (SELECT 1 FROM question_tags filter_qt WHERE filter_qt.question_id = q.id AND filter_qt.tag_id = ")
                .push_bind(tag_id.clone())
                .push(")");
        }
    }
}

pub async fn list_questions(
    pool: &SqlitePool,
    filters: &QuestionFiltersApi,
) -> CommandResult<PageResultApi<QuestionApi>> {
    if filters.tag_ids.len() > MAX_TAGS {
        return Err(CommandError::validation(format!(
            "一次最多筛选 {MAX_TAGS} 个标签。"
        )));
    }
    if !matches!(filters.tag_match_mode.as_str(), "any" | "all") {
        return Err(CommandError::validation("标签匹配方式无效。"));
    }
    validate_usage_filter(&filters.usage)?;
    if filters.tag_ids.iter().collect::<HashSet<_>>().len() != filters.tag_ids.len() {
        return Err(CommandError::validation("筛选标签不能重复。"));
    }
    let page = filters.page.max(1);
    let page_size = filters.page_size.clamp(1, 100);
    let offset = (u64::from(page) - 1).saturating_mul(u64::from(page_size));
    let offset = i64::try_from(offset).unwrap_or(i64::MAX);
    let usage_constraint = resolve_usage_constraint(&filters.usage)?;

    let mut count_builder =
        QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM questions q WHERE 1 = 1");
    push_filters(&mut count_builder, filters, usage_constraint);
    let total: i64 = count_builder
        .build_query_scalar()
        .fetch_one(pool)
        .await
        .map_err(CommandError::database)?;

    let mut rows_builder = QueryBuilder::<Sqlite>::new(
        "SELECT q.id, q.question_type, q.subject_id, q.chapter_id, \
                s.name AS subject_name, c.name AS chapter_name, \
                q.stem_json, q.answer_json, q.explanation_json, q.content_version, \
                q.last_used_at_ms, q.deleted_at_ms, q.created_at_ms, q.updated_at_ms \
         FROM questions q \
         JOIN subjects s ON s.id = q.subject_id \
         JOIN chapters c ON c.id = q.chapter_id \
         WHERE 1 = 1",
    );
    push_filters(&mut rows_builder, filters, usage_constraint);
    rows_builder
        .push(" ORDER BY ")
        .push(if filters.deleted {
            "q.deleted_at_ms DESC"
        } else {
            "q.updated_at_ms DESC"
        })
        .push(", q.id LIMIT ")
        .push_bind(i64::from(page_size))
        .push(" OFFSET ")
        .push_bind(offset);

    let rows = rows_builder
        .build_query_as::<QuestionRow>()
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)?;

    let items = hydrate_questions(pool, rows).await?;

    Ok(PageResultApi {
        items,
        total,
        page,
        page_size,
    })
}

pub(crate) async fn load_questions_by_ids(
    pool: &SqlitePool,
    ordered_ids: &[String],
) -> CommandResult<Vec<QuestionApi>> {
    if ordered_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT q.id, q.question_type, q.subject_id, q.chapter_id, \
                s.name AS subject_name, c.name AS chapter_name, \
                q.stem_json, q.answer_json, q.explanation_json, q.content_version, \
                q.last_used_at_ms, q.deleted_at_ms, q.created_at_ms, q.updated_at_ms \
         FROM questions q JOIN subjects s ON s.id = q.subject_id \
         JOIN chapters c ON c.id = q.chapter_id WHERE q.id IN (",
    );
    {
        let mut separated = builder.separated(", ");
        for id in ordered_ids {
            separated.push_bind(id.clone());
        }
        separated.push_unseparated(")");
    }
    let rows = builder
        .build_query_as::<QuestionRow>()
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)?;
    let mut by_id = hydrate_questions(pool, rows)
        .await?
        .into_iter()
        .map(|question| (question.id.clone(), question))
        .collect::<HashMap<_, _>>();
    ordered_ids
        .iter()
        .map(|id| {
            by_id
                .remove(id)
                .ok_or_else(|| CommandError::database("随机抽中的题目无法重新读取。"))
        })
        .collect()
}

pub(crate) async fn list_active_questions_for_export(
    connection: &mut SqliteConnection,
) -> CommandResult<Vec<QuestionApi>> {
    let question_type_names =
        sqlx::query_as::<_, (String, String)>("SELECT code, name FROM question_types")
            .fetch_all(&mut *connection)
            .await
            .map_err(CommandError::database)?
            .into_iter()
            .collect::<HashMap<_, _>>();

    let rows = sqlx::query_as::<_, QuestionRow>(
        "SELECT q.id, q.question_type, q.subject_id, q.chapter_id, \
                s.name AS subject_name, c.name AS chapter_name, \
                q.stem_json, q.answer_json, q.explanation_json, q.content_version, \
                q.last_used_at_ms, q.deleted_at_ms, q.created_at_ms, q.updated_at_ms \
         FROM questions q \
         JOIN subjects s ON s.id = q.subject_id \
         JOIN chapters c ON c.id = q.chapter_id \
         JOIN question_types qt ON qt.code = q.question_type \
         WHERE q.deleted_at_ms IS NULL \
         ORDER BY s.sort_order, s.id, c.sort_order, c.id, qt.sort_order, qt.code, \
                  q.created_at_ms, q.id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;

    let option_rows = sqlx::query_as::<_, ExportOptionRow>(
        "SELECT qo.question_id, qo.id, qo.position, qo.content_json \
         FROM question_options qo JOIN questions q ON q.id = qo.question_id \
         WHERE q.deleted_at_ms IS NULL ORDER BY qo.question_id, qo.position, qo.id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    let tag_rows = sqlx::query_as::<_, ExportTagRow>(
        "WITH active_tag_counts AS ( \
             SELECT qt.tag_id, COUNT(*) AS question_count \
             FROM question_tags qt JOIN questions q ON q.id = qt.question_id \
             WHERE q.deleted_at_ms IS NULL GROUP BY qt.tag_id \
         ) \
         SELECT qt.question_id, t.id, t.name, t.created_at_ms, t.updated_at_ms, \
                COALESCE(atc.question_count, 0) AS question_count \
         FROM question_tags qt \
         JOIN questions q ON q.id = qt.question_id \
         JOIN tags t ON t.id = qt.tag_id \
         LEFT JOIN active_tag_counts atc ON atc.tag_id = t.id \
         WHERE q.deleted_at_ms IS NULL \
         ORDER BY qt.question_id, t.name, t.id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    let resource_rows = sqlx::query_as::<_, ExportResourceRefRow>(
        "SELECT qrr.question_id, qrr.node_id, qrr.resource_id, qrr.content_slot, qrr.option_id \
         FROM question_resource_refs qrr JOIN questions q ON q.id = qrr.question_id \
         WHERE q.deleted_at_ms IS NULL \
         ORDER BY qrr.question_id, qrr.created_at_ms, qrr.id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;

    let mut options_by_question = HashMap::<String, Vec<QuestionOptionApi>>::new();
    for option in option_rows {
        options_by_question
            .entry(option.question_id)
            .or_default()
            .push(QuestionOptionApi {
                id: option.id,
                position: u32::try_from(option.position).unwrap_or_default(),
                content: parse_rich(&option.content_json)?,
            });
    }
    let mut tags_by_question = HashMap::<String, Vec<TagApi>>::new();
    for tag in tag_rows {
        tags_by_question
            .entry(tag.question_id)
            .or_default()
            .push(TagApi {
                id: tag.id,
                name: tag.name,
                question_count: tag.question_count,
                created_at: tag.created_at_ms,
                updated_at: tag.updated_at_ms,
            });
    }
    let mut resources_by_question = HashMap::<String, Vec<QuestionResourceRefApi>>::new();
    for resource in resource_rows {
        resources_by_question
            .entry(resource.question_id)
            .or_default()
            .push(QuestionResourceRefApi {
                node_id: resource.node_id,
                resource_id: resource.resource_id,
                content_slot: resource.content_slot,
                option_id: resource.option_id,
            });
    }

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row.id.clone();
        let question_type = row.question_type.clone();
        items.push(QuestionApi {
            id: row.id,
            question_type: row.question_type,
            question_type_name: question_type_names.get(&question_type).cloned(),
            stem: parse_rich(&row.stem_json)?,
            options: options_by_question.remove(&id).unwrap_or_default(),
            answer: parse_rich(&row.answer_json)?,
            explanation: parse_rich(&row.explanation_json)?,
            subject_id: row.subject_id,
            chapter_id: row.chapter_id,
            subject_name: row.subject_name,
            chapter_name: row.chapter_name,
            tags: tags_by_question.remove(&id).unwrap_or_default(),
            resource_refs: resources_by_question.remove(&id).unwrap_or_default(),
            last_used_at: row.last_used_at_ms,
            deleted_at: row.deleted_at_ms,
            created_at: row.created_at_ms,
            updated_at: row.updated_at_ms,
            content_version: row.content_version,
        });
    }
    Ok(items)
}

pub async fn get_question(pool: &SqlitePool, id: &str) -> CommandResult<Option<QuestionApi>> {
    let row = sqlx::query_as::<_, QuestionRow>(
        "SELECT q.id, q.question_type, q.subject_id, q.chapter_id, \
                s.name AS subject_name, c.name AS chapter_name, \
                q.stem_json, q.answer_json, q.explanation_json, q.content_version, \
                q.last_used_at_ms, q.deleted_at_ms, q.created_at_ms, q.updated_at_ms \
         FROM questions q JOIN subjects s ON s.id = q.subject_id \
         JOIN chapters c ON c.id = q.chapter_id WHERE q.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?;

    match row {
        Some(row) => Ok(Some(hydrate_question(pool, row).await?)),
        None => Ok(None),
    }
}

fn validate_draft(draft: &QuestionDraftApi, behavior: &str) -> CommandResult<()> {
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
    validate_rich_content("题干", &draft.stem, false)?;
    validate_rich_content("答案", &draft.answer, false)?;
    validate_rich_content("解析", &draft.explanation, true)?;
    for (index, option) in draft.options.iter().enumerate() {
        validate_rich_content(&format!("选项 {}", index + 1), &option.content, false)?;
    }
    if draft.stem.plain_text.trim().is_empty() {
        return Err(CommandError::validation("题干不能为空。"));
    }
    if draft.answer.plain_text.trim().is_empty() {
        return Err(CommandError::validation("答案不能为空。"));
    }
    if matches!(behavior, "single_choice" | "multiple_choice") {
        if draft.options.len() < 2
            || draft
                .options
                .iter()
                .any(|option| option.content.plain_text.trim().is_empty())
        {
            return Err(CommandError::validation("选择题至少需要两个非空选项。"));
        }
    } else if !draft.options.is_empty() {
        return Err(CommandError::validation("非选择题不能保存选择题选项。"));
    }
    let unique_tag_count = draft.tag_ids.iter().collect::<HashSet<_>>().len();
    if unique_tag_count != draft.tag_ids.len() {
        return Err(CommandError::validation("同一道题不能重复选择同一个标签。"));
    }
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
            "题目内容合计超过 {} MB 的安全上限，请压缩图片或拆分内容。",
            MAX_TOTAL_CONTENT_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

fn validate_rich_content(
    label: &str,
    content: &RichContentApi,
    allow_empty: bool,
) -> CommandResult<()> {
    validate_rich_document(label, content)?;
    if !allow_empty && content.plain_text.trim().is_empty() {
        return Err(CommandError::validation(format!("{label}不能为空。")));
    }
    if content
        .plain_text
        .chars()
        .any(|character| matches!(character, '\u{1e}' | '\u{1f}'))
    {
        return Err(CommandError::validation(format!(
            "{label}包含软件保留的控制字符，请删除后重试。"
        )));
    }
    if content.html.len() > MAX_RICH_HTML_BYTES {
        return Err(CommandError::validation(format!(
            "{label}的富文本超过 {} MB 的安全上限。",
            MAX_RICH_HTML_BYTES / 1024 / 1024
        )));
    }
    if content.plain_text.len() > MAX_RICH_PLAIN_BYTES {
        return Err(CommandError::validation(format!(
            "{label}的可搜索文字过长，请拆分内容。"
        )));
    }
    if content
        .source_ooxml
        .as_ref()
        .is_some_and(|xml| xml.len() > MAX_SOURCE_OOXML_BYTES)
    {
        return Err(CommandError::validation(format!(
            "{label}保留的 Word 原始内容超过安全上限。"
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
            format!("{label}中包含不安全的网页代码，已停止保存。"),
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

fn validate_rich_document(label: &str, content: &RichContentApi) -> CommandResult<()> {
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
            if serde_json::to_vec(document)
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
                "{label}使用了软件暂不支持的内容版本。"
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

fn normalized_fingerprint(draft: &QuestionDraftApi) -> Vec<u8> {
    let combined = format!(
        "{}\u{1f}{}\u{1f}{}",
        draft.stem.plain_text,
        draft
            .options
            .iter()
            .map(|option| option.content.plain_text.as_str())
            .collect::<Vec<_>>()
            .join("\u{1e}"),
        draft.answer.plain_text,
    );
    let normalized = combined
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter(|character| !is_duplicate_whitespace(*character))
        .collect::<String>();
    Sha256::digest(normalized.as_bytes()).to_vec()
}

fn is_duplicate_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

fn normalized_similarity_field<'a>(parts: impl IntoIterator<Item = &'a str>) -> Vec<char> {
    let mut raw = String::with_capacity(MAX_SIMILARITY_FIELD_CHARS.min(1_024));
    let mut raw_character_count = 0usize;
    for part in parts {
        for character in part.chars() {
            if raw_character_count == MAX_SIMILARITY_FIELD_CHARS {
                break;
            }
            raw.push(character);
            raw_character_count += 1;
        }
        if raw_character_count == MAX_SIMILARITY_FIELD_CHARS {
            break;
        }
    }
    raw.nfkc()
        .flat_map(char::to_lowercase)
        .filter(|character| !is_duplicate_whitespace(*character))
        .take(MAX_SIMILARITY_FIELD_CHARS)
        .collect()
}

fn bounded_raw_character_count<'a>(parts: impl IntoIterator<Item = &'a str>) -> usize {
    let mut count = 0usize;
    for part in parts {
        count = count.saturating_add(
            part.chars()
                .take(MAX_SIMILARITY_FIELD_CHARS - count)
                .count(),
        );
        if count == MAX_SIMILARITY_FIELD_CHARS {
            break;
        }
    }
    count
}

fn similarity_prefilter_length_from_draft(draft: &QuestionDraftApi) -> usize {
    bounded_raw_character_count([draft.stem.plain_text.as_str()])
        .saturating_add(bounded_raw_character_count(
            draft
                .options
                .iter()
                .map(|option| option.content.plain_text.as_str()),
        ))
        .saturating_add(bounded_raw_character_count([draft
            .answer
            .plain_text
            .as_str()]))
}

fn similarity_text_from_draft(draft: &QuestionDraftApi) -> Vec<char> {
    join_similarity_fields(
        normalized_similarity_field([draft.stem.plain_text.as_str()]),
        normalized_similarity_field(
            draft
                .options
                .iter()
                .map(|option| option.content.plain_text.as_str()),
        ),
        normalized_similarity_field([draft.answer.plain_text.as_str()]),
    )
}

fn similarity_text_from_candidate(candidate: &DuplicateCandidateRow) -> Vec<char> {
    join_similarity_fields(
        normalized_similarity_field([candidate.stem_plain.as_str()]),
        normalized_similarity_field([candidate.options_plain.as_str()]),
        normalized_similarity_field([candidate.answer_plain.as_str()]),
    )
}

fn join_similarity_fields(stem: Vec<char>, options: Vec<char>, answer: Vec<char>) -> Vec<char> {
    let mut combined = Vec::with_capacity(
        stem.len()
            .saturating_add(options.len())
            .saturating_add(answer.len())
            .saturating_add(2),
    );
    combined.extend(stem);
    combined.push('\u{1f}');
    combined.extend(options);
    combined.push('\u{1f}');
    combined.extend(answer);
    combined
}

/// Returns the rounded Sørensen-Dice overlap of Unicode character bigram
/// multisets. NFKC/case/whitespace normalization and per-field limits are
/// applied before this function, making CPU and memory use deterministic.
fn character_bigram_dice_percent(left: &[char], right: &[char]) -> u32 {
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    character_bigram_profile_dice_percent(
        &character_bigram_profile(left),
        &character_bigram_profile(right),
    )
}

struct CharacterBigramProfile {
    counts: HashMap<(char, char), u32>,
    total: u64,
}

fn character_bigram_profile(text: &[char]) -> CharacterBigramProfile {
    let mut counts =
        HashMap::<(char, char), u32>::with_capacity(text.len().saturating_sub(1).max(1));
    if text.len() == 1 {
        counts.insert((text[0], '\0'), 1);
    } else {
        for pair in text.windows(2) {
            let count = counts.entry((pair[0], pair[1])).or_insert(0);
            *count = count.saturating_add(1);
        }
    }
    let total = counts.values().map(|count| u64::from(*count)).sum();
    CharacterBigramProfile { counts, total }
}

fn character_bigram_profile_dice_percent(
    left: &CharacterBigramProfile,
    right: &CharacterBigramProfile,
) -> u32 {
    if left.total == 0 || right.total == 0 {
        return 0;
    }
    let shared = left
        .counts
        .iter()
        .map(|(gram, left_count)| {
            u64::from((*left_count).min(right.counts.get(gram).copied().unwrap_or(0)))
        })
        .sum::<u64>();
    let denominator = left.total.saturating_add(right.total);
    u32::try_from(shared.saturating_mul(200).saturating_add(denominator / 2) / denominator)
        .unwrap_or(100)
        .min(100)
}

fn stem_preview(stem: &str) -> String {
    let collapsed = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = collapsed.chars();
    let preview = characters
        .by_ref()
        .take(DUPLICATE_STEM_PREVIEW_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn duplicate_candidate_api(
    candidate: DuplicateCandidateRow,
    similarity_percent: u32,
) -> QuestionDuplicateCandidateApi {
    QuestionDuplicateCandidateApi {
        id: candidate.id,
        question_type: candidate.question_type,
        stem_preview: stem_preview(&candidate.stem_plain),
        subject_name: candidate.subject_name,
        chapter_name: candidate.chapter_name,
        similarity_percent,
        content_version: candidate.content_version,
        source_kind: None,
        source_ordinal: None,
    }
}

fn duplicate_candidate_from_scan_meta(
    candidate: &DuplicateScanMetaRow,
    similarity_percent: u32,
) -> QuestionDuplicateCandidateApi {
    QuestionDuplicateCandidateApi {
        id: candidate.id.clone(),
        question_type: candidate.question_type.clone(),
        stem_preview: stem_preview(&candidate.stem_preview),
        subject_name: candidate.subject_name.clone(),
        chapter_name: candidate.chapter_name.clone(),
        similarity_percent,
        content_version: candidate.content_version,
        source_kind: Some("question".to_owned()),
        source_ordinal: None,
    }
}

fn fingerprint_hex(fingerprint: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(fingerprint.len().saturating_mul(2));
    for byte in fingerprint {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn import_request_hash(draft: &QuestionDraftApi) -> CommandResult<String> {
    let serialized = serde_json::to_vec(draft).map_err(CommandError::database)?;
    Ok(fingerprint_hex(&Sha256::digest(serialized)))
}

fn imported_overwrite_request_hash(
    request: &ImportedQuestionOverwriteRequestApi,
) -> CommandResult<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"zhitiku.import-overwrite.v1\0");
    for item in &request.items {
        let client_id = item.client_id.as_bytes();
        hasher.update(
            u64::try_from(client_id.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(client_id);
        let serialized = serde_json::to_vec(&item.draft).map_err(CommandError::database)?;
        hasher.update(
            u64::try_from(serialized.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(serialized);
    }
    Ok(fingerprint_hex(&hasher.finalize()))
}

fn no_duplicate_check(evaluated_candidate_count: u32) -> QuestionDuplicateCheckApi {
    QuestionDuplicateCheckApi {
        status: "none".to_owned(),
        candidate: None,
        evaluated_candidate_count,
        suspected_threshold_percent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
        similarity_method: SIMILARITY_METHOD.to_owned(),
    }
}

fn import_item_duplicate_candidate(
    request: &QuestionDuplicateBatchRequestApi,
    index: usize,
    similarity_percent: u32,
) -> QuestionDuplicateCandidateApi {
    let entry = &request.items[index];
    QuestionDuplicateCandidateApi {
        id: entry.client_id.clone(),
        question_type: entry.draft.question_type.clone(),
        stem_preview: stem_preview(&entry.draft.stem.plain_text),
        subject_name: "本批导入".to_owned(),
        chapter_name: format!("第 {} 题", index + 1),
        similarity_percent,
        content_version: 0,
        source_kind: Some("import-item".to_owned()),
        source_ordinal: Some(u32::try_from(index + 1).unwrap_or(u32::MAX)),
    }
}

fn nearest_prior_import_indices(
    target_length: i64,
    prior_by_length: &BTreeMap<i64, Vec<usize>>,
) -> Vec<usize> {
    let mut lower = prior_by_length.range(..=target_length).rev().peekable();
    let mut upper = prior_by_length
        .range((Excluded(target_length), Unbounded))
        .peekable();
    let mut candidates = Vec::with_capacity(MAX_DUPLICATE_CANDIDATES);

    while candidates.len() < MAX_DUPLICATE_CANDIDATES {
        let take_lower = match (lower.peek(), upper.peek()) {
            (Some((lower_length, _)), Some((upper_length, _))) => {
                (**lower_length).abs_diff(target_length) <= (**upper_length).abs_diff(target_length)
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let bucket = if take_lower {
            lower.next().expect("lower length bucket must exist").1
        } else {
            upper.next().expect("upper length bucket must exist").1
        };
        for candidate_index in bucket.iter().rev() {
            candidates.push(*candidate_index);
            if candidates.len() >= MAX_DUPLICATE_CANDIDATES {
                break;
            }
        }
    }
    candidates
}

fn check_import_duplicates(
    request: &QuestionDuplicateBatchRequestApi,
    scan_id: Option<&str>,
) -> CommandResult<QuestionDuplicateBatchResultApi> {
    let mut earlier_by_fingerprint = HashMap::<Vec<u8>, usize>::new();
    let mut prior_by_length = BTreeMap::<i64, Vec<usize>>::new();
    let mut profile_cache = HashMap::<usize, CharacterBigramProfile>::new();
    let mut results = Vec::with_capacity(request.items.len());
    for (index, entry) in request.items.iter().enumerate() {
        ensure_duplicate_scan_active(scan_id)?;
        let fingerprint = normalized_fingerprint(&entry.draft);
        let length =
            i64::try_from(similarity_prefilter_length_from_draft(&entry.draft)).unwrap_or(i64::MAX);
        ensure_duplicate_scan_active(scan_id)?;
        let exact_previous = earlier_by_fingerprint.get(&fingerprint).copied();
        let check = if let Some(previous_index) = exact_previous {
            QuestionDuplicateCheckApi {
                status: "exact".to_owned(),
                candidate: Some(import_item_duplicate_candidate(
                    request,
                    previous_index,
                    100,
                )),
                evaluated_candidate_count: 1,
                suspected_threshold_percent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
                similarity_method: SIMILARITY_METHOD.to_owned(),
            }
        } else {
            let candidates = nearest_prior_import_indices(length, &prior_by_length);
            let input_text = similarity_text_from_draft(&entry.draft);
            let input_profile = character_bigram_profile(&input_text);
            ensure_duplicate_scan_active(scan_id)?;
            let mut best: Option<(usize, u32)> = None;
            for (candidate_position, candidate_index) in candidates.iter().enumerate() {
                if candidate_position % 32 == 0 {
                    ensure_duplicate_scan_active(scan_id)?;
                }
                if !profile_cache.contains_key(candidate_index) {
                    if profile_cache.len() >= MAX_DUPLICATE_PROFILE_CACHE {
                        profile_cache.clear();
                    }
                    let text = similarity_text_from_draft(&request.items[*candidate_index].draft);
                    profile_cache.insert(*candidate_index, character_bigram_profile(&text));
                }
                let Some(candidate_profile) = profile_cache.get(candidate_index) else {
                    continue;
                };
                let similarity =
                    character_bigram_profile_dice_percent(&input_profile, candidate_profile);
                if best
                    .as_ref()
                    .is_none_or(|(_, best_similarity)| similarity > *best_similarity)
                {
                    best = Some((*candidate_index, similarity));
                }
            }
            if profile_cache.len() >= MAX_DUPLICATE_PROFILE_CACHE {
                profile_cache.clear();
            }
            profile_cache.insert(index, input_profile);
            if let Some((candidate_index, similarity)) =
                best.filter(|(_, similarity)| *similarity >= SUSPECTED_DUPLICATE_THRESHOLD_PERCENT)
            {
                QuestionDuplicateCheckApi {
                    status: "suspected".to_owned(),
                    candidate: Some(import_item_duplicate_candidate(
                        request,
                        candidate_index,
                        similarity,
                    )),
                    evaluated_candidate_count: u32::try_from(candidates.len()).unwrap_or(u32::MAX),
                    suspected_threshold_percent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
                    similarity_method: SIMILARITY_METHOD.to_owned(),
                }
            } else {
                no_duplicate_check(u32::try_from(candidates.len()).unwrap_or(u32::MAX))
            }
        };
        earlier_by_fingerprint
            .entry(fingerprint.clone())
            .or_insert(index);
        prior_by_length.entry(length).or_default().push(index);
        results.push(QuestionDuplicateBatchItemResultApi {
            client_id: entry.client_id.clone(),
            exact_fingerprint: fingerprint_hex(&fingerprint),
            check,
        });
    }
    Ok(QuestionDuplicateBatchResultApi { items: results })
}

pub async fn check_question_duplicate(
    pool: &SqlitePool,
    draft: &QuestionDraftApi,
) -> CommandResult<QuestionDuplicateCheckApi> {
    let behavior = question_types::behavior(pool, &draft.question_type).await?;
    validate_draft(draft, &behavior)?;
    if draft
        .question_id
        .as_ref()
        .is_some_and(|id| Uuid::parse_str(id).is_err())
    {
        return Err(CommandError::validation("题目标识无效，请刷新后重试。"));
    }

    let excluded_id = draft.question_id.as_deref().unwrap_or("");
    let fingerprint = normalized_fingerprint(draft);
    let field_limit = i64::try_from(MAX_SIMILARITY_FIELD_CHARS).unwrap_or(i64::MAX);
    let exact = sqlx::query_as::<_, DuplicateCandidateRow>(
        "SELECT q.id, q.question_type, substr(q.stem_plain, 1, ?) AS stem_plain, \
                '' AS options_plain, '' AS answer_plain, s.name AS subject_name, \
                c.name AS chapter_name, q.content_version \
         FROM questions q \
         JOIN subjects s ON s.id = q.subject_id \
         JOIN chapters c ON c.id = q.chapter_id \
         WHERE q.deleted_at_ms IS NULL AND q.fingerprint_version = 1 \
           AND q.exact_fingerprint = ? AND q.id <> ? \
         ORDER BY q.updated_at_ms DESC, q.id LIMIT 1",
    )
    .bind(field_limit)
    .bind(&fingerprint)
    .bind(excluded_id)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?;
    if let Some(exact) = exact {
        return Ok(QuestionDuplicateCheckApi {
            status: "exact".to_owned(),
            candidate: Some(duplicate_candidate_api(exact, 100)),
            evaluated_candidate_count: 1,
            suspected_threshold_percent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
            similarity_method: SIMILARITY_METHOD.to_owned(),
        });
    }

    let input_text = similarity_text_from_draft(draft);
    let input_length =
        i64::try_from(similarity_prefilter_length_from_draft(draft)).unwrap_or(i64::MAX);
    let candidate_limit = i64::try_from(MAX_DUPLICATE_CANDIDATES).unwrap_or(i64::MAX);
    let candidates = sqlx::query_as::<_, DuplicateCandidateRow>(
        "SELECT q.id, q.question_type, substr(q.stem_plain, 1, ?) AS stem_plain, \
                substr(q.options_plain, 1, ?) AS options_plain, \
                substr(q.answer_plain, 1, ?) AS answer_plain, s.name AS subject_name, \
                c.name AS chapter_name, q.content_version \
         FROM questions q \
         JOIN subjects s ON s.id = q.subject_id \
         JOIN chapters c ON c.id = q.chapter_id \
         WHERE q.deleted_at_ms IS NULL AND q.id <> ? \
         ORDER BY abs((min(length(q.stem_plain), ?) + min(length(q.options_plain), ?) + \
                       min(length(q.answer_plain), ?)) - ?) ASC, \
                  q.updated_at_ms DESC, q.id \
         LIMIT ?",
    )
    .bind(field_limit)
    .bind(field_limit)
    .bind(field_limit)
    .bind(excluded_id)
    .bind(field_limit)
    .bind(field_limit)
    .bind(field_limit)
    .bind(input_length)
    .bind(candidate_limit)
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;

    let evaluated_candidate_count = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
    let mut best: Option<(DuplicateCandidateRow, u32)> = None;
    for candidate in candidates {
        let similarity =
            character_bigram_dice_percent(&input_text, &similarity_text_from_candidate(&candidate));
        if best
            .as_ref()
            .is_none_or(|(_, best_similarity)| similarity > *best_similarity)
        {
            best = Some((candidate, similarity));
        }
    }

    let status = if best
        .as_ref()
        .is_some_and(|(_, similarity)| *similarity >= SUSPECTED_DUPLICATE_THRESHOLD_PERCENT)
    {
        "suspected"
    } else {
        "none"
    };
    Ok(QuestionDuplicateCheckApi {
        status: status.to_owned(),
        candidate: best
            .map(|(candidate, similarity)| duplicate_candidate_api(candidate, similarity)),
        evaluated_candidate_count,
        suspected_threshold_percent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
        similarity_method: SIMILARITY_METHOD.to_owned(),
    })
}

pub async fn check_question_duplicates_batch(
    pool: &SqlitePool,
    request: &QuestionDuplicateBatchRequestApi,
) -> CommandResult<QuestionDuplicateBatchResultApi> {
    let scan_id = request
        .scan_id
        .as_deref()
        .map(|id| canonical_uuid(id, "重复检查任务标识"))
        .transpose()?;
    ensure_duplicate_scan_active(scan_id.as_deref())?;
    if request.items.is_empty() {
        return Err(CommandError::validation("批量重复检查至少需要一道题目。"));
    }
    let mode = request.mode.as_deref().unwrap_or("full");
    let import_only = mode == "import";
    let max_items = if import_only {
        MAX_IMPORT_DUPLICATE_BATCH_ITEMS
    } else {
        MAX_DUPLICATE_BATCH_ITEMS
    };
    if request.items.len() > max_items {
        return Err(CommandError::validation(format!(
            "当前模式单次最多批量检查 {max_items} 道题。"
        )));
    }
    let exact_only = match mode {
        "full" => false,
        "exact" => true,
        "import" => false,
        _ => return Err(CommandError::validation("批量重复检查模式无效。")),
    };

    let mut seen_client_ids = HashSet::new();
    let mut behaviors = HashMap::<String, String>::new();
    for entry in &request.items {
        ensure_duplicate_scan_active(scan_id.as_deref())?;
        let canonical_id = Uuid::parse_str(&entry.client_id)
            .map_err(|_| CommandError::validation("批量重复检查的题目标识无效。"))?
            .to_string();
        if canonical_id != entry.client_id || !seen_client_ids.insert(entry.client_id.clone()) {
            return Err(CommandError::validation(
                "批量重复检查的题目标识必须是互不重复的规范 UUID。",
            ));
        }
        if !behaviors.contains_key(&entry.draft.question_type) {
            let behavior = question_types::behavior(pool, &entry.draft.question_type).await?;
            behaviors.insert(entry.draft.question_type.clone(), behavior);
        }
        let behavior = behaviors
            .get(&entry.draft.question_type)
            .ok_or_else(|| CommandError::database("题型行为缓存读取失败。"))?;
        validate_draft(&entry.draft, behavior)?;
        if entry
            .draft
            .question_id
            .as_ref()
            .is_some_and(|id| Uuid::parse_str(id).is_err())
        {
            return Err(CommandError::validation("题目标识无效，请刷新后重试。"));
        }
    }

    if import_only {
        return check_import_duplicates(request, scan_id.as_deref());
    }

    // One metadata read and one length ordering are shared by every draft in
    // this request. Long content is loaded into a bounded profile cache only
    // for the nearest candidate window currently being evaluated.
    let field_limit = i64::try_from(MAX_SIMILARITY_FIELD_CHARS).unwrap_or(i64::MAX);
    let preview_limit = i64::try_from(DUPLICATE_STEM_PREVIEW_CHARS).unwrap_or(i64::MAX);
    let questions = sqlx::query_as::<_, DuplicateScanMetaRow>(
        "SELECT q.id, q.question_type, q.subject_id, q.chapter_id, \
                substr(q.stem_plain, 1, ?) AS stem_preview, \
                s.name AS subject_name, c.name AS chapter_name, q.exact_fingerprint, \
                q.content_version, q.created_at_ms, q.updated_at_ms, \
                min(length(q.stem_plain), ?) + min(length(q.options_plain), ?) + \
                min(length(q.answer_plain), ?) AS similarity_length \
         FROM questions q \
         JOIN subjects s ON s.id = q.subject_id \
         JOIN chapters c ON c.id = q.chapter_id \
         WHERE q.deleted_at_ms IS NULL \
         ORDER BY similarity_length ASC, q.updated_at_ms DESC, q.id",
    )
    .bind(preview_limit)
    .bind(field_limit)
    .bind(field_limit)
    .bind(field_limit)
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;
    ensure_duplicate_scan_active(scan_id.as_deref())?;
    let mut exact_by_fingerprint = HashMap::<Vec<u8>, Vec<usize>>::new();
    for (index, question) in questions.iter().enumerate() {
        exact_by_fingerprint
            .entry(question.exact_fingerprint.clone())
            .or_default()
            .push(index);
    }

    let mut fingerprints = Vec::with_capacity(request.items.len());
    let mut exact_candidates =
        Vec::<Option<QuestionDuplicateCandidateApi>>::with_capacity(request.items.len());
    let mut earlier_in_batch = HashMap::<Vec<u8>, usize>::new();

    for (entry_index, entry) in request.items.iter().enumerate() {
        ensure_duplicate_scan_active(scan_id.as_deref())?;
        let fingerprint = normalized_fingerprint(&entry.draft);
        let excluded_id = entry.draft.question_id.as_deref().unwrap_or("");
        let database_exact = exact_by_fingerprint.get(&fingerprint).and_then(|indices| {
            indices
                .iter()
                .map(|index| &questions[*index])
                .find(|candidate| candidate.id != excluded_id)
        });
        let exact_candidate = if let Some(candidate) = database_exact {
            Some(duplicate_candidate_from_scan_meta(candidate, 100))
        } else if let Some(previous_index) = earlier_in_batch.get(&fingerprint).copied() {
            let previous = &request.items[previous_index];
            Some(QuestionDuplicateCandidateApi {
                id: previous.client_id.clone(),
                question_type: previous.draft.question_type.clone(),
                stem_preview: stem_preview(&previous.draft.stem.plain_text),
                subject_name: "本批导入".to_owned(),
                chapter_name: format!("第 {} 题", previous_index + 1),
                similarity_percent: 100,
                content_version: 0,
                source_kind: Some("import-item".to_owned()),
                source_ordinal: Some(u32::try_from(previous_index + 1).unwrap_or(u32::MAX)),
            })
        } else {
            None
        };
        earlier_in_batch
            .entry(fingerprint.clone())
            .or_insert(entry_index);

        fingerprints.push(fingerprint);
        exact_candidates.push(exact_candidate);
    }

    let mut results = Vec::with_capacity(request.items.len());
    let mut candidate_profiles = HashMap::<String, CharacterBigramProfile>::new();
    for (index, entry) in request.items.iter().enumerate() {
        ensure_duplicate_scan_active(scan_id.as_deref())?;
        let check = if let Some(candidate) = exact_candidates[index].clone() {
            QuestionDuplicateCheckApi {
                status: "exact".to_owned(),
                candidate: Some(candidate),
                evaluated_candidate_count: 1,
                suspected_threshold_percent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
                similarity_method: SIMILARITY_METHOD.to_owned(),
            }
        } else if exact_only {
            no_duplicate_check(0)
        } else {
            let input_length = i64::try_from(similarity_prefilter_length_from_draft(&entry.draft))
                .unwrap_or(i64::MAX);
            let excluded_id = entry.draft.question_id.as_deref().unwrap_or("");
            let insertion =
                questions.partition_point(|candidate| candidate.similarity_length < input_length);
            let mut left = insertion.checked_sub(1);
            let mut right = insertion;
            let mut candidate_indices = Vec::with_capacity(MAX_DUPLICATE_CANDIDATES);
            while candidate_indices.len() < MAX_DUPLICATE_CANDIDATES
                && (left.is_some() || right < questions.len())
            {
                let take_left = match (left, questions.get(right)) {
                    (Some(left_index), Some(right_candidate)) => {
                        questions[left_index]
                            .similarity_length
                            .abs_diff(input_length)
                            <= right_candidate.similarity_length.abs_diff(input_length)
                    }
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => break,
                };
                let candidate_index = if take_left {
                    let left_index = left.expect("left index must exist");
                    left = left_index.checked_sub(1);
                    left_index
                } else {
                    let right_index = right;
                    right += 1;
                    right_index
                };
                if questions[candidate_index].id != excluded_id {
                    candidate_indices.push(candidate_index);
                }
            }

            let missing_ids = candidate_indices
                .iter()
                .map(|candidate_index| &questions[*candidate_index].id)
                .filter(|id| !candidate_profiles.contains_key(*id))
                .cloned()
                .collect::<Vec<_>>();
            if candidate_profiles.len().saturating_add(missing_ids.len())
                > MAX_DUPLICATE_PROFILE_CACHE
            {
                candidate_profiles.clear();
            }
            let missing_ids = candidate_indices
                .iter()
                .map(|candidate_index| &questions[*candidate_index].id)
                .filter(|id| !candidate_profiles.contains_key(*id))
                .cloned()
                .collect::<Vec<_>>();
            for chunk in missing_ids.chunks(400) {
                ensure_duplicate_scan_active(scan_id.as_deref())?;
                let refs = chunk.iter().map(String::as_str).collect::<Vec<_>>();
                for (id, text) in load_duplicate_scan_texts(pool, &refs).await? {
                    candidate_profiles.insert(
                        id,
                        character_bigram_profile(&similarity_text_from_scan(&text)),
                    );
                }
            }
            let input_text = similarity_text_from_draft(&entry.draft);
            let input_profile = character_bigram_profile(&input_text);
            let mut best: Option<(usize, u32)> = None;
            for (candidate_position, candidate_index) in candidate_indices.iter().enumerate() {
                if candidate_position % 32 == 0 {
                    ensure_duplicate_scan_active(scan_id.as_deref())?;
                }
                let candidate = &questions[*candidate_index];
                let Some(candidate_profile) = candidate_profiles.get(&candidate.id) else {
                    continue;
                };
                let similarity =
                    character_bigram_profile_dice_percent(&input_profile, candidate_profile);
                if best
                    .as_ref()
                    .is_none_or(|(_, best_similarity)| similarity > *best_similarity)
                {
                    best = Some((*candidate_index, similarity));
                }
            }
            let status = if best
                .as_ref()
                .is_some_and(|(_, similarity)| *similarity >= SUSPECTED_DUPLICATE_THRESHOLD_PERCENT)
            {
                "suspected"
            } else {
                "none"
            };
            QuestionDuplicateCheckApi {
                status: status.to_owned(),
                candidate: best.map(|(candidate_index, similarity)| {
                    duplicate_candidate_from_scan_meta(&questions[candidate_index], similarity)
                }),
                evaluated_candidate_count: u32::try_from(candidate_indices.len())
                    .unwrap_or(u32::MAX),
                suspected_threshold_percent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
                similarity_method: SIMILARITY_METHOD.to_owned(),
            }
        };
        results.push(QuestionDuplicateBatchItemResultApi {
            client_id: entry.client_id.clone(),
            exact_fingerprint: fingerprint_hex(&fingerprints[index]),
            check,
        });
    }

    Ok(QuestionDuplicateBatchResultApi { items: results })
}

fn duplicate_scan_member(question: &DuplicateScanMetaRow) -> QuestionDuplicateMemberApi {
    QuestionDuplicateMemberApi {
        id: question.id.clone(),
        question_type: question.question_type.clone(),
        stem_preview: stem_preview(&question.stem_preview),
        subject_id: question.subject_id.clone(),
        chapter_id: question.chapter_id.clone(),
        subject_name: question.subject_name.clone(),
        chapter_name: question.chapter_name.clone(),
        content_version: question.content_version,
        created_at: question.created_at_ms,
        updated_at: question.updated_at_ms,
    }
}

fn similarity_text_from_scan(question: &DuplicateScanTextRow) -> Vec<char> {
    join_similarity_fields(
        normalized_similarity_field([question.stem_plain.as_str()]),
        normalized_similarity_field([question.options_plain.as_str()]),
        normalized_similarity_field([question.answer_plain.as_str()]),
    )
}

fn canonical_scan_pair(
    first: &DuplicateScanMetaRow,
    second: &DuplicateScanMetaRow,
) -> (String, i64, String, i64) {
    if first.id < second.id {
        (
            first.id.clone(),
            first.content_version,
            second.id.clone(),
            second.content_version,
        )
    } else {
        (
            second.id.clone(),
            second.content_version,
            first.id.clone(),
            first.content_version,
        )
    }
}

async fn load_duplicate_scan_texts(
    pool: &SqlitePool,
    question_ids: &[&str],
) -> CommandResult<HashMap<String, DuplicateScanTextRow>> {
    if question_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let field_limit = i64::try_from(MAX_SIMILARITY_FIELD_CHARS).unwrap_or(i64::MAX);
    let mut builder = QueryBuilder::<Sqlite>::new("SELECT id, substr(stem_plain, 1, ");
    builder
        .push_bind(field_limit)
        .push(") AS stem_plain, substr(options_plain, 1, ")
        .push_bind(field_limit)
        .push(") AS options_plain, substr(answer_plain, 1, ")
        .push_bind(field_limit)
        .push(
            ") AS answer_plain FROM questions \
               WHERE deleted_at_ms IS NULL AND id IN (",
        );
    {
        let mut separated = builder.separated(", ");
        for question_id in question_ids {
            separated.push_bind(*question_id);
        }
    }
    builder.push(")");

    let rows = builder
        .build_query_as::<DuplicateScanTextRow>()
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)?;
    Ok(rows.into_iter().map(|row| (row.id.clone(), row)).collect())
}

pub async fn scan_question_duplicates(
    pool: &SqlitePool,
    request: &QuestionDuplicateScanRequestApi,
) -> CommandResult<QuestionDuplicateScanResultApi> {
    let subject_id = canonical_uuid(&request.subject_id, "学科标识")?;
    let chapter_id = request
        .chapter_id
        .as_deref()
        .map(|id| canonical_uuid(id, "章节标识"))
        .transpose()?;

    let scope_exists = if let Some(chapter_id) = chapter_id.as_deref() {
        sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM chapters WHERE id = ? AND subject_id = ?)",
        )
        .bind(chapter_id)
        .bind(&subject_id)
        .fetch_one(pool)
        .await
        .map_err(CommandError::database)?
    } else {
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM subjects WHERE id = ?)")
            .bind(&subject_id)
            .fetch_one(pool)
            .await
            .map_err(CommandError::database)?
    };
    if scope_exists == 0 {
        return Err(CommandError::new(
            "DUPLICATE_SCAN_SCOPE_NOT_FOUND",
            "要查重的学科或章节已经不存在，请刷新后重试。",
        ));
    }

    // Keep the whole-bank scan lightweight. The large text fields are loaded
    // only for the bounded candidate window of one scope question at a time.
    let field_limit = i64::try_from(MAX_SIMILARITY_FIELD_CHARS).unwrap_or(i64::MAX);
    let preview_limit = i64::try_from(DUPLICATE_STEM_PREVIEW_CHARS).unwrap_or(i64::MAX);
    let questions = sqlx::query_as::<_, DuplicateScanMetaRow>(
        "SELECT q.id, q.question_type, q.subject_id, q.chapter_id, \
                substr(q.stem_plain, 1, ?) AS stem_preview, \
                s.name AS subject_name, c.name AS chapter_name, q.exact_fingerprint, \
                q.content_version, q.created_at_ms, q.updated_at_ms, \
                min(length(q.stem_plain), ?) + min(length(q.options_plain), ?) + \
                min(length(q.answer_plain), ?) AS similarity_length \
         FROM questions q \
         JOIN subjects s ON s.id = q.subject_id \
         JOIN chapters c ON c.id = q.chapter_id \
         WHERE q.deleted_at_ms IS NULL",
    )
    .bind(preview_limit)
    .bind(field_limit)
    .bind(field_limit)
    .bind(field_limit)
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;

    let scope_ids = questions
        .iter()
        .filter(|question| {
            question.subject_id == subject_id
                && chapter_id
                    .as_ref()
                    .is_none_or(|chapter_id| question.chapter_id == *chapter_id)
        })
        .map(|question| question.id.clone())
        .collect::<HashSet<_>>();
    if scope_ids.len() > MAX_DUPLICATE_SCAN_SCOPE_QUESTIONS {
        return Err(CommandError::new(
            "DUPLICATE_SCAN_SCOPE_TOO_LARGE",
            format!("一次最多扫描 {MAX_DUPLICATE_SCAN_SCOPE_QUESTIONS} 道题，请按章节分别查重。"),
        ));
    }

    let mut exact_by_fingerprint: HashMap<Vec<u8>, Vec<&DuplicateScanMetaRow>> = HashMap::new();
    for question in &questions {
        exact_by_fingerprint
            .entry(question.exact_fingerprint.clone())
            .or_default()
            .push(question);
    }
    let mut exact_groups = exact_by_fingerprint
        .into_values()
        .filter(|members| {
            members.len() > 1 && members.iter().any(|member| scope_ids.contains(&member.id))
        })
        .map(|mut members| {
            members.sort_by_key(|member| (member.created_at_ms, member.id.as_str()));
            QuestionDuplicateGroupApi {
                id: format!("exact:{}", members[0].id),
                duplicate_kind: "exact".to_owned(),
                similarity_percent: 100,
                members: members.into_iter().map(duplicate_scan_member).collect(),
            }
        })
        .collect::<Vec<_>>();
    exact_groups.sort_by(|left, right| left.id.cmp(&right.id));

    let ignored_pairs = sqlx::query_as::<_, DuplicateIgnoreRow>(
        "SELECT question_id_low, question_id_high, content_version_low, content_version_high \
         FROM question_duplicate_ignores",
    )
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?
    .into_iter()
    .map(|row| {
        (
            row.question_id_low,
            row.content_version_low,
            row.question_id_high,
            row.content_version_high,
        )
    })
    .collect::<HashSet<_>>();

    let mut sorted_indices = (0..questions.len()).collect::<Vec<_>>();
    sorted_indices.sort_by(|left, right| {
        questions[*left]
            .similarity_length
            .cmp(&questions[*right].similarity_length)
            .then_with(|| {
                questions[*right]
                    .updated_at_ms
                    .cmp(&questions[*left].updated_at_ms)
            })
            .then_with(|| questions[*left].id.cmp(&questions[*right].id))
    });
    let positions = sorted_indices
        .iter()
        .enumerate()
        .map(|(position, index)| (questions[*index].id.clone(), position))
        .collect::<HashMap<_, _>>();
    let question_indices = questions
        .iter()
        .enumerate()
        .map(|(index, question)| (question.id.clone(), index))
        .collect::<HashMap<_, _>>();

    let mut seen_pairs = HashSet::<(String, String)>::new();
    let mut suspected_groups = Vec::new();
    for scope_id in &scope_ids {
        let Some(&question_index) = question_indices.get(scope_id) else {
            continue;
        };
        let question = &questions[question_index];
        let Some(&position) = positions.get(scope_id) else {
            continue;
        };
        let start = position.saturating_sub(MAX_DUPLICATE_CANDIDATES);
        let end = position
            .saturating_add(MAX_DUPLICATE_CANDIDATES + 1)
            .min(sorted_indices.len());
        let mut candidate_indices = sorted_indices[start..end]
            .iter()
            .copied()
            .filter(|candidate| *candidate != question_index)
            .collect::<Vec<_>>();
        candidate_indices.sort_by(|left, right| {
            let left_question = &questions[*left];
            let right_question = &questions[*right];
            left_question
                .similarity_length
                .abs_diff(question.similarity_length)
                .cmp(
                    &right_question
                        .similarity_length
                        .abs_diff(question.similarity_length),
                )
                .then_with(|| {
                    right_question
                        .updated_at_ms
                        .cmp(&left_question.updated_at_ms)
                })
                .then_with(|| left_question.id.cmp(&right_question.id))
        });

        candidate_indices.truncate(MAX_DUPLICATE_CANDIDATES);
        let mut text_ids = Vec::with_capacity(candidate_indices.len() + 1);
        text_ids.push(question.id.as_str());
        text_ids.extend(
            candidate_indices
                .iter()
                .map(|candidate_index| questions[*candidate_index].id.as_str()),
        );
        let texts = load_duplicate_scan_texts(pool, &text_ids).await?;
        let Some(question_text) = texts.get(&question.id) else {
            continue;
        };
        let input_text = similarity_text_from_scan(question_text);
        for candidate_index in candidate_indices {
            let candidate = &questions[candidate_index];
            let pair_ids = if question.id < candidate.id {
                (question.id.clone(), candidate.id.clone())
            } else {
                (candidate.id.clone(), question.id.clone())
            };
            if !seen_pairs.insert(pair_ids.clone())
                || question.exact_fingerprint == candidate.exact_fingerprint
            {
                continue;
            }
            let pair_with_versions = canonical_scan_pair(question, candidate);
            if ignored_pairs.contains(&pair_with_versions) {
                continue;
            }
            let Some(candidate_text) = texts.get(&candidate.id) else {
                continue;
            };
            let similarity = character_bigram_dice_percent(
                &input_text,
                &similarity_text_from_scan(candidate_text),
            );
            if similarity < SUSPECTED_DUPLICATE_THRESHOLD_PERCENT {
                continue;
            }
            let mut members = vec![question, candidate];
            members.sort_by_key(|member| (member.created_at_ms, member.id.as_str()));
            suspected_groups.push(QuestionDuplicateGroupApi {
                id: format!("suspected:{}:{}", pair_ids.0, pair_ids.1),
                duplicate_kind: "suspected".to_owned(),
                similarity_percent: similarity,
                members: members.into_iter().map(duplicate_scan_member).collect(),
            });
        }
    }
    suspected_groups.sort_by(|left, right| {
        right
            .similarity_percent
            .cmp(&left.similarity_percent)
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(QuestionDuplicateScanResultApi {
        scanned_question_count: u32::try_from(scope_ids.len()).unwrap_or(u32::MAX),
        compared_question_count: u32::try_from(questions.len()).unwrap_or(u32::MAX),
        suspected_threshold_percent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
        exact_groups,
        suspected_groups,
    })
}

pub async fn ignore_question_duplicate(
    pool: &SqlitePool,
    request: &IgnoreQuestionDuplicateRequestApi,
) -> CommandResult<()> {
    let first_id = canonical_uuid(&request.first_question_id, "第一道题标识")?;
    let second_id = canonical_uuid(&request.second_question_id, "第二道题标识")?;
    if first_id == second_id {
        return Err(CommandError::validation("不能把同一道题标记为非重复。"));
    }
    if request.first_content_version < 1 || request.second_content_version < 1 {
        return Err(CommandError::validation("题目内容版本无效，请刷新后重试。"));
    }
    let (low_id, low_version, high_id, high_version) = if first_id < second_id {
        (
            first_id,
            request.first_content_version,
            second_id,
            request.second_content_version,
        )
    } else {
        (
            second_id,
            request.second_content_version,
            first_id,
            request.first_content_version,
        )
    };

    let current_questions = sqlx::query_as::<_, (String, i64, Vec<u8>)>(
        "SELECT id, content_version, exact_fingerprint FROM questions \
         WHERE deleted_at_ms IS NULL AND id IN (?, ?)",
    )
    .bind(&low_id)
    .bind(&high_id)
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?
    .into_iter()
    .map(|(id, version, fingerprint)| (id, (version, fingerprint)))
    .collect::<HashMap<_, _>>();
    if current_questions.get(&low_id).map(|row| row.0) != Some(low_version)
        || current_questions.get(&high_id).map(|row| row.0) != Some(high_version)
    {
        return Err(CommandError::new(
            "DUPLICATE_PAIR_STALE",
            "其中一道题已被修改，请重新查重后再判断。",
        ));
    }
    if current_questions
        .get(&low_id)
        .zip(current_questions.get(&high_id))
        .is_some_and(|(low, high)| low.1 == high.1)
    {
        return Err(CommandError::new(
            "DUPLICATE_PAIR_EXACT",
            "内容完全相同的题目不能标记为非重复；请选择保留题目或返回题库修改内容。",
        ));
    }

    sqlx::query(
        "INSERT INTO question_duplicate_ignores \
         (question_id_low, question_id_high, content_version_low, content_version_high, created_at_ms) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(question_id_low, question_id_high) DO UPDATE SET \
           content_version_low = excluded.content_version_low, \
           content_version_high = excluded.content_version_high, \
           created_at_ms = excluded.created_at_ms",
    )
    .bind(low_id)
    .bind(high_id)
    .bind(low_version)
    .bind(high_version)
    .bind(now_millis())
    .execute(pool)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

async fn save_question_in_connection(
    transaction: &mut SqliteConnection,
    draft: &QuestionDraftApi,
    app_version: &str,
    preferred_create_id: Option<&str>,
) -> CommandResult<String> {
    let behavior = sqlx::query_scalar::<_, String>(
        "SELECT behavior FROM question_types WHERE code = ? AND is_enabled = 1",
    )
    .bind(&draft.question_type)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::validation("所选题型不存在或已经停用。"))?;
    validate_draft(draft, &behavior)?;
    let classification_exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chapters WHERE id = ? AND subject_id = ?")
            .bind(&draft.chapter_id)
            .bind(&draft.subject_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    if classification_exists != 1 {
        return Err(CommandError::validation(
            "请选择有效且相互匹配的学科和章节。",
        ));
    }

    let mut tag_names = Vec::with_capacity(draft.tag_ids.len());
    for tag_id in &draft.tag_ids {
        let name = sqlx::query_scalar::<_, String>("SELECT name FROM tags WHERE id = ?")
            .bind(tag_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(CommandError::database)?
            .ok_or_else(|| CommandError::validation("所选标签已经不存在，请刷新后重试。"))?;
        tag_names.push(name);
    }

    let stem_json = serde_json::to_string(&draft.stem).map_err(CommandError::database)?;
    let answer_json = serde_json::to_string(&draft.answer).map_err(CommandError::database)?;
    let explanation_json =
        serde_json::to_string(&draft.explanation).map_err(CommandError::database)?;
    let content_schema_version = draft
        .options
        .iter()
        .map(|option| option.content.schema_version)
        .chain([
            draft.stem.schema_version,
            draft.answer.schema_version,
            draft.explanation.schema_version,
        ])
        .max()
        .unwrap_or(1);
    let options_plain = draft
        .options
        .iter()
        .map(|option| option.content.plain_text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let tags_plain = tag_names.join(" ");
    let fingerprint = normalized_fingerprint(draft);
    let now = now_millis();
    if let Some(requested_id) = &draft.question_id {
        if Uuid::parse_str(requested_id).is_err() {
            return Err(CommandError::validation("题目标识无效，请刷新后重试。"));
        }
    } else if draft.base_content_version.is_some() {
        return Err(CommandError::validation(
            "新增题目不能携带旧题目的内容版本。",
        ));
    }
    let preferred_create_id = preferred_create_id
        .map(|id| canonical_uuid(id, "导入题目标识"))
        .transpose()?;
    let stable_import_request_hash = preferred_create_id
        .as_ref()
        .map(|_| import_request_hash(draft))
        .transpose()?;
    let question_id = draft.question_id.clone().unwrap_or_else(|| {
        preferred_create_id
            .clone()
            .unwrap_or_else(|| Uuid::now_v7().to_string())
    });
    let mut option_ids = HashSet::with_capacity(draft.options.len());
    for option in &draft.options {
        let option_id = Uuid::parse_str(&option.id)
            .map(|id| id.to_string())
            .map_err(|_| CommandError::validation("选项 ID 无效，请重新打开题目后重试。"))?;
        if option_id != option.id || !option_ids.insert(option_id) {
            return Err(CommandError::validation(
                "选项 ID 必须使用规范 UUID，且同一道题中不能重复。",
            ));
        }
    }
    let resource_refs = resources::reconcile_question_resource_refs(
        &draft.stem,
        &draft.options,
        &draft.answer,
        &draft.explanation,
        &draft.resource_refs,
    )?;
    resources::validate_resource_refs(&mut *transaction, &resource_refs, &option_ids).await?;
    resources::validate_question_resource_ref_consistency(
        &draft.stem,
        &draft.options,
        &draft.answer,
        &draft.explanation,
        &resource_refs,
    )?;

    let existing = sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT content_version, deleted_at_ms FROM questions WHERE id = ?",
    )
    .bind(&question_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    let next_version = if let Some((current_version, deleted_at)) = existing {
        if deleted_at.is_some() {
            return Err(CommandError::new(
                "QUESTION_IN_RECYCLE",
                "这道题已经进入回收站，不能被编辑或覆盖；请先恢复后再操作。",
            ));
        }
        if draft.question_id.is_none() && preferred_create_id.is_some() {
            let recorded_details = sqlx::query_scalar::<_, String>(
                "SELECT details_json FROM operation_logs \
                 WHERE entity_type = 'question' AND entity_id = ? \
                   AND action = 'question.import.create' AND outcome = 'success' \
                 ORDER BY occurred_at_ms DESC, id DESC LIMIT 1",
            )
            .bind(&question_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
            let recorded_hash = recorded_details.as_deref().and_then(|details| {
                serde_json::from_str::<serde_json::Value>(details)
                    .ok()?
                    .get("requestHash")?
                    .as_str()
                    .map(str::to_owned)
            });
            if recorded_hash.as_deref() == stable_import_request_hash.as_deref() {
                return Ok(question_id);
            }
            return Err(CommandError::new(
                "IMPORT_ID_CONFLICT",
                "导入题目标识已被其他内容占用，请重新选择源文件后再试。",
            ));
        }
        let expected_version = draft.base_content_version.ok_or_else(|| {
            CommandError::new(
                "CONTENT_VERSION_REQUIRED",
                "编辑已有题目时缺少版本信息，请重新打开后再保存。",
            )
        })?;
        if expected_version != current_version {
            return Err(CommandError::new(
                "CONTENT_CONFLICT",
                "这道题已在其他窗口被修改，请重新打开后再保存。",
            ));
        }
        let affected = sqlx::query(
            "UPDATE questions SET question_type = ?, subject_id = ?, chapter_id = ?, content_schema_version = ?, \
             stem_json = ?, answer_json = ?, explanation_json = ?, stem_plain = ?, \
             options_plain = ?, answer_plain = ?, explanation_plain = ?, tags_plain = ?, \
             exact_fingerprint = ?, content_version = content_version + 1, updated_at_ms = ? \
             WHERE id = ? AND content_version = ? AND deleted_at_ms IS NULL",
        )
        .bind(&draft.question_type)
        .bind(&draft.subject_id)
        .bind(&draft.chapter_id)
        .bind(i64::from(content_schema_version))
        .bind(&stem_json)
        .bind(&answer_json)
        .bind(&explanation_json)
        .bind(&draft.stem.plain_text)
        .bind(&options_plain)
        .bind(&draft.answer.plain_text)
        .bind(&draft.explanation.plain_text)
        .bind(&tags_plain)
        .bind(&fingerprint)
        .bind(now)
        .bind(&question_id)
        .bind(current_version)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
        if affected.rows_affected() != 1 {
            return Err(CommandError::new(
                "CONTENT_CONFLICT",
                "保存时检测到内容冲突，请重新打开题目。",
            ));
        }
        current_version + 1
    } else {
        if draft.question_id.is_some() {
            return Err(CommandError::new(
                "QUESTION_NOT_FOUND",
                "要编辑的题目已经不存在，请返回题库刷新后重试。",
            ));
        }
        sqlx::query(
            "INSERT INTO questions (id, question_type, subject_id, chapter_id, content_schema_version, \
             stem_json, answer_json, explanation_json, stem_plain, options_plain, answer_plain, \
             explanation_plain, tags_plain, fingerprint_version, exact_fingerprint, content_version, \
             created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, 1, ?, ?)",
        )
        .bind(&question_id).bind(&draft.question_type).bind(&draft.subject_id).bind(&draft.chapter_id)
        .bind(i64::from(content_schema_version))
        .bind(&stem_json).bind(&answer_json).bind(&explanation_json)
        .bind(&draft.stem.plain_text).bind(&options_plain).bind(&draft.answer.plain_text)
        .bind(&draft.explanation.plain_text).bind(&tags_plain).bind(&fingerprint)
        .bind(now).bind(now)
        .execute(&mut *transaction).await.map_err(CommandError::database)?;
        1
    };

    sqlx::query("DELETE FROM question_options WHERE question_id = ?")
        .bind(&question_id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    for (position, option) in draft.options.iter().enumerate() {
        let content_json =
            serde_json::to_string(&option.content).map_err(CommandError::database)?;
        sqlx::query(
            "INSERT INTO question_options (id, question_id, position, content_json, plain_text, created_at_ms, updated_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&option.id).bind(&question_id).bind(i64::try_from(position).unwrap_or(0))
        .bind(content_json).bind(&option.content.plain_text).bind(now).bind(now)
        .execute(&mut *transaction).await.map_err(CommandError::database)?;
    }

    sqlx::query("DELETE FROM question_tags WHERE question_id = ?")
        .bind(&question_id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    for tag_id in &draft.tag_ids {
        sqlx::query(
            "INSERT INTO question_tags (question_id, tag_id, created_at_ms) VALUES (?, ?, ?)",
        )
        .bind(&question_id)
        .bind(tag_id)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    }

    resources::replace_question_resource_refs(&mut *transaction, &question_id, &resource_refs, now)
        .await?;

    let is_import_create = next_version == 1 && stable_import_request_hash.is_some();
    let action = if is_import_create {
        "question.import.create"
    } else if next_version == 1 {
        "question.create"
    } else {
        "question.update"
    };
    let details_json = stable_import_request_hash
        .as_ref()
        .filter(|_| is_import_create)
        .map(|request_hash| serde_json::json!({ "requestHash": request_hash }).to_string())
        .unwrap_or_else(|| "{}".to_owned());
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, entity_id, \
         outcome, summary, details_json, app_version) VALUES (?, ?, 'info', ?, 'question', ?, 'success', ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string()).bind(now).bind(action).bind(&question_id)
    .bind(if is_import_create { "批量导入题目" } else if next_version == 1 { "新增题目" } else { "编辑题目" })
    .bind(details_json).bind(app_version)
    .execute(&mut *transaction).await.map_err(CommandError::database)?;

    Ok(question_id)
}

pub async fn save_question(
    pool: &SqlitePool,
    draft: &QuestionDraftApi,
    app_version: &str,
) -> CommandResult<QuestionApi> {
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let question_id =
        save_question_in_connection(&mut transaction, draft, app_version, None).await?;
    transaction.commit().await.map_err(CommandError::database)?;
    get_question(pool, &question_id)
        .await?
        .ok_or_else(|| CommandError::database("保存后无法重新读取题目。"))
}

pub async fn save_questions(
    pool: &SqlitePool,
    drafts: &[QuestionDraftApi],
    app_version: &str,
) -> CommandResult<Vec<QuestionApi>> {
    if drafts.is_empty() {
        return Err(CommandError::validation("请至少录入一道题目。"));
    }
    if drafts.len() > 100 {
        return Err(CommandError::validation(
            "文档式录入一次最多保存 100 道题目。",
        ));
    }

    let mut requested_question_ids = HashSet::new();
    for draft in drafts {
        if let Some(question_id) = draft.question_id.as_deref() {
            let canonical = Uuid::parse_str(question_id)
                .map_err(|_| CommandError::validation("批量保存中的题目 ID 无效。"))?
                .to_string();
            if canonical != question_id || !requested_question_ids.insert(canonical) {
                return Err(CommandError::validation("批量保存不能多次覆盖同一道题目。"));
            }
        }
    }

    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let mut question_ids = Vec::with_capacity(drafts.len());
    for draft in drafts {
        question_ids
            .push(save_question_in_connection(&mut transaction, draft, app_version, None).await?);
    }
    transaction.commit().await.map_err(CommandError::database)?;

    let mut saved = Vec::with_capacity(question_ids.len());
    for question_id in question_ids {
        saved.push(
            get_question(pool, &question_id)
                .await?
                .ok_or_else(|| CommandError::database("批量保存后无法重新读取题目。"))?,
        );
    }
    Ok(saved)
}

pub async fn save_imported_questions(
    pool: &SqlitePool,
    drafts: &[QuestionDraftApi],
    app_version: &str,
) -> CommandResult<Vec<String>> {
    if drafts.is_empty() {
        return Err(CommandError::validation("请至少导入一道题目。"));
    }
    if drafts.len() > 100 {
        return Err(CommandError::validation(
            "批量导入一次最多保存 100 道题目。",
        ));
    }

    let mut import_ids = HashSet::with_capacity(drafts.len());
    for draft in drafts {
        if draft.question_id.is_some() || draft.base_content_version.is_some() {
            return Err(CommandError::validation(
                "批量新增接口不能用于覆盖已有题目。",
            ));
        }
        let import_id = draft
            .id
            .as_deref()
            .ok_or_else(|| CommandError::validation("批量导入题目缺少稳定标识。"))?;
        let canonical = canonical_uuid(import_id, "导入题目标识")?;
        if !import_ids.insert(canonical) {
            return Err(CommandError::validation("批量导入包含重复的题目标识。"));
        }
    }

    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let mut question_ids = Vec::with_capacity(drafts.len());
    for draft in drafts {
        question_ids.push(
            save_question_in_connection(&mut transaction, draft, app_version, draft.id.as_deref())
                .await?,
        );
    }
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(question_ids)
}

fn validate_imported_question_overwrite_request(
    request: &ImportedQuestionOverwriteRequestApi,
) -> CommandResult<()> {
    let operation_id = canonical_uuid(&request.operation_id, "批量覆盖操作标识")?;
    if operation_id != request.operation_id {
        return Err(CommandError::validation(
            "批量覆盖操作标识格式无效，请重新发起导入。",
        ));
    }
    if request.items.is_empty() {
        return Err(CommandError::new(
            "IMPORT_OVERWRITE_EMPTY",
            "请至少选择一道要覆盖的题目。",
        ));
    }
    if request.items.len() > MAX_IMPORTED_QUESTION_OVERWRITES {
        return Err(CommandError::new(
            "IMPORT_OVERWRITE_TOO_LARGE",
            format!("一次最多覆盖 {MAX_IMPORTED_QUESTION_OVERWRITES} 道题，请分批导入。"),
        ));
    }

    let mut client_ids = HashSet::with_capacity(request.items.len());
    let mut question_ids = HashSet::with_capacity(request.items.len());
    for item in &request.items {
        let client_id = canonical_uuid(&item.client_id, "导入条目标识")?;
        if client_id != item.client_id || !client_ids.insert(client_id) {
            return Err(CommandError::new(
                "IMPORT_OVERWRITE_DUPLICATE_CLIENT_ID",
                "批量覆盖包含无效或重复的导入条目标识。",
            ));
        }

        let question_id = item.draft.question_id.as_deref().ok_or_else(|| {
            CommandError::new(
                "IMPORT_OVERWRITE_TARGET_REQUIRED",
                "批量覆盖中的题目缺少原题标识，请重新查重后再试。",
            )
        })?;
        let canonical_question_id = canonical_uuid(question_id, "原题标识")?;
        if canonical_question_id != question_id {
            return Err(CommandError::validation(
                "批量覆盖中的原题标识格式无效，请重新查重后再试。",
            ));
        }
        if !question_ids.insert(canonical_question_id) {
            return Err(CommandError::new(
                "IMPORT_OVERWRITE_DUPLICATE_TARGET",
                "同一批次不能多次覆盖同一道原题，请先处理源文件中的重复项。",
            ));
        }

        if item
            .draft
            .base_content_version
            .is_none_or(|version| version < 1)
        {
            return Err(CommandError::new(
                "IMPORT_OVERWRITE_VERSION_REQUIRED",
                "批量覆盖中的题目缺少有效版本，请重新查重后再试。",
            ));
        }
    }
    Ok(())
}

/// Atomically overwrites a set of unique existing questions selected during an
/// import review. A durable operation receipt makes retries after an uncertain
/// frontend response safe: the same operation and payload returns the original
/// lightweight result, while reusing the operation ID for different content is
/// rejected.
pub async fn save_imported_question_overwrites(
    pool: &SqlitePool,
    request: &ImportedQuestionOverwriteRequestApi,
    app_version: &str,
) -> CommandResult<ImportedQuestionOverwriteResultApi> {
    validate_imported_question_overwrite_request(request)?;
    let request_hash = imported_overwrite_request_hash(request)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    let reservation = sqlx::query(
        "INSERT OR IGNORE INTO question_import_overwrite_operations \
         (operation_id, request_sha256_hex, result_json, created_at_ms, completed_at_ms) \
         VALUES (?, ?, NULL, ?, NULL)",
    )
    .bind(&request.operation_id)
    .bind(&request_hash)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    if reservation.rows_affected() == 0 {
        let stored = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT request_sha256_hex, result_json \
             FROM question_import_overwrite_operations WHERE operation_id = ?",
        )
        .bind(&request.operation_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(CommandError::database)?
        .ok_or_else(|| CommandError::database("批量覆盖操作登记在读取时意外消失，请稍后重试。"))?;
        if stored.0 != request_hash {
            return Err(CommandError::new(
                "IMPORT_OVERWRITE_OPERATION_CONFLICT",
                "这个批量覆盖操作标识已经用于其他内容，请重新发起导入。",
            ));
        }
        let result_json = stored.1.ok_or_else(|| {
            CommandError::new(
                "IMPORT_OVERWRITE_IN_PROGRESS",
                "这批覆盖仍在处理中，请稍后使用同一操作标识重试。",
            )
        })?;
        let mut result = serde_json::from_str::<ImportedQuestionOverwriteResultApi>(&result_json)
            .map_err(|error| {
            CommandError::new(
                "IMPORT_OVERWRITE_RECEIPT_INVALID",
                format!("批量覆盖完成记录无法读取：{error}"),
            )
        })?;
        if result.operation_id != request.operation_id || result.items.len() != request.items.len()
        {
            return Err(CommandError::new(
                "IMPORT_OVERWRITE_RECEIPT_INVALID",
                "批量覆盖完成记录与当前请求不一致，请联系维护人员检查数据。",
            ));
        }
        transaction
            .rollback()
            .await
            .map_err(CommandError::database)?;
        result.replayed = true;
        return Ok(result);
    }

    let mut items = Vec::with_capacity(request.items.len());
    for item in &request.items {
        let question_id =
            save_question_in_connection(&mut transaction, &item.draft, app_version, None).await?;
        let content_version = item
            .draft
            .base_content_version
            .unwrap_or_default()
            .saturating_add(1);
        items.push(ImportedQuestionOverwriteResultItemApi {
            client_id: item.client_id.clone(),
            question_id,
            content_version,
        });
    }

    let result = ImportedQuestionOverwriteResultApi {
        operation_id: request.operation_id.clone(),
        updated_count: u32::try_from(items.len()).unwrap_or(u32::MAX),
        items,
        replayed: false,
    };
    let result_json = serde_json::to_string(&result).map_err(CommandError::database)?;
    let completed = sqlx::query(
        "UPDATE question_import_overwrite_operations \
         SET result_json = ?, completed_at_ms = ? \
         WHERE operation_id = ? AND request_sha256_hex = ? AND result_json IS NULL",
    )
    .bind(result_json)
    .bind(now_millis())
    .bind(&request.operation_id)
    .bind(&request_hash)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    if completed.rows_affected() != 1 {
        return Err(CommandError::database(
            "批量覆盖已写入，但无法登记完成状态；本次事务已撤销。",
        ));
    }

    transaction.commit().await.map_err(CommandError::database)?;
    Ok(result)
}

pub(crate) fn canonical_uuid(value: &str, label: &str) -> CommandResult<String> {
    let parsed = Uuid::parse_str(value)
        .map_err(|_| CommandError::validation(format!("{label}无效，请刷新后重试。")))?;
    let canonical = parsed.to_string();
    if canonical != value {
        return Err(CommandError::validation(format!(
            "{label}格式无效，请刷新后重试。"
        )));
    }
    Ok(canonical)
}

fn validate_batch_edit_request(request: &QuestionBatchEditRequestApi) -> CommandResult<()> {
    if request.questions.is_empty() {
        return Err(CommandError::new(
            "BATCH_EDIT_EMPTY",
            "请先选择至少一道要修改的题目。",
        ));
    }
    if request.questions.len() > MAX_BATCH_EDIT_QUESTIONS {
        return Err(CommandError::new(
            "BATCH_EDIT_TOO_LARGE",
            format!("一次最多批量修改 {MAX_BATCH_EDIT_QUESTIONS} 道题，请分批操作。"),
        ));
    }

    let mut question_ids = HashSet::with_capacity(request.questions.len());
    for target in &request.questions {
        let id = canonical_uuid(&target.id, "题目标识")?;
        if !question_ids.insert(id) {
            return Err(CommandError::new(
                "BATCH_EDIT_DUPLICATE_ID",
                "批量修改列表中包含重复题目，请重新选择。",
            ));
        }
        if target.expected_content_version < 1 {
            return Err(CommandError::validation(
                "题目版本无效，请刷新题库后重新选择。",
            ));
        }
    }

    if let Some(classification) = &request.classification {
        canonical_uuid(&classification.subject_id, "学科标识")?;
        canonical_uuid(&classification.chapter_id, "章节标识")?;
    }

    if !matches!(request.usage_operation.as_str(), "keep" | "reset_never") {
        return Err(CommandError::validation(
            "最近使用状态操作无效，请重新打开批量编辑窗口。",
        ));
    }

    if !matches!(
        request.tag_operation.mode.as_str(),
        "keep" | "append" | "replace" | "remove"
    ) {
        return Err(CommandError::validation(
            "标签操作无效，请重新打开批量编辑窗口。",
        ));
    }
    if request.tag_operation.tag_ids.len() > MAX_TAGS {
        return Err(CommandError::validation(format!(
            "一次最多选择 {MAX_TAGS} 个标签。"
        )));
    }
    let mut tag_ids = HashSet::with_capacity(request.tag_operation.tag_ids.len());
    for tag_id in &request.tag_operation.tag_ids {
        let id = canonical_uuid(tag_id, "标签标识")?;
        if !tag_ids.insert(id) {
            return Err(CommandError::validation(
                "标签列表中包含重复项，请重新选择。",
            ));
        }
    }
    match request.tag_operation.mode.as_str() {
        "keep" if !request.tag_operation.tag_ids.is_empty() => {
            return Err(CommandError::validation(
                "保持标签不变时不能同时提交标签列表。",
            ));
        }
        "append" | "remove" if request.tag_operation.tag_ids.is_empty() => {
            return Err(CommandError::validation(
                "追加或删除标签时，请至少选择一个标签。",
            ));
        }
        _ => {}
    }

    if request.classification.is_none()
        && request.usage_operation == "keep"
        && request.tag_operation.mode == "keep"
    {
        return Err(CommandError::new(
            "BATCH_EDIT_NO_CHANGES",
            "尚未选择任何要修改的项目。",
        ));
    }
    Ok(())
}

/// Atomically updates question metadata. Every target and every referenced
/// taxonomy row is validated before the first mutation; dropping the
/// transaction on any later error rolls the entire batch back.
pub async fn batch_edit_questions(
    pool: &SqlitePool,
    request: &QuestionBatchEditRequestApi,
    app_version: &str,
) -> CommandResult<QuestionBatchEditResultApi> {
    validate_batch_edit_request(request)?;
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    if let Some(classification) = &request.classification {
        let classification_exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chapters WHERE id = ? AND subject_id = ?")
                .bind(&classification.chapter_id)
                .bind(&classification.subject_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(CommandError::database)?;
        if classification_exists != 1 {
            return Err(CommandError::new(
                "BATCH_CLASSIFICATION_INVALID",
                "所选学科和章节已经不存在或不匹配，请刷新后重试。",
            ));
        }
    }

    for tag_id in &request.tag_operation.tag_ids {
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tags WHERE id = ?")
            .bind(tag_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
        if exists != 1 {
            return Err(CommandError::new(
                "BATCH_TAG_NOT_FOUND",
                "所选标签已经不存在，请刷新后重试。",
            ));
        }
    }

    // Validate the complete target set before changing any relationship or
    // question row. This is what gives a stale item an all-or-nothing result.
    let mut rows = Vec::with_capacity(request.questions.len());
    for target in &request.questions {
        let row = sqlx::query_as::<_, BatchQuestionRow>(
            "SELECT id, subject_id, chapter_id, content_version, last_used_at_ms, deleted_at_ms \
             FROM questions WHERE id = ?",
        )
        .bind(&target.id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(CommandError::database)?
        .ok_or_else(|| {
            CommandError::new(
                "QUESTION_NOT_FOUND",
                "选中的题目中有题目已经不存在，本次修改未执行。",
            )
        })?;
        if row.deleted_at_ms.is_some() {
            return Err(CommandError::new(
                "QUESTION_IN_RECYCLE",
                "选中的题目中有题目已进入回收站，本次修改未执行。",
            ));
        }
        if row.content_version != target.expected_content_version {
            return Err(CommandError::new(
                "CONTENT_CONFLICT",
                "选中的题目中有题目已在其他窗口被修改，本次修改未执行；请刷新后重试。",
            ));
        }
        rows.push(row);
    }

    let now = now_millis();
    let mut versions = Vec::with_capacity(rows.len());
    for (row, target) in rows.iter().zip(&request.questions) {
        match request.tag_operation.mode.as_str() {
            "replace" => {
                sqlx::query("DELETE FROM question_tags WHERE question_id = ?")
                    .bind(&row.id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(CommandError::database)?;
                for tag_id in &request.tag_operation.tag_ids {
                    sqlx::query(
                        "INSERT INTO question_tags (question_id, tag_id, created_at_ms) VALUES (?, ?, ?)",
                    )
                    .bind(&row.id)
                    .bind(tag_id)
                    .bind(now)
                    .execute(&mut *transaction)
                    .await
                    .map_err(CommandError::database)?;
                }
            }
            "append" => {
                for tag_id in &request.tag_operation.tag_ids {
                    sqlx::query(
                        "INSERT OR IGNORE INTO question_tags (question_id, tag_id, created_at_ms) VALUES (?, ?, ?)",
                    )
                    .bind(&row.id)
                    .bind(tag_id)
                    .bind(now)
                    .execute(&mut *transaction)
                    .await
                    .map_err(CommandError::database)?;
                }
            }
            "remove" => {
                for tag_id in &request.tag_operation.tag_ids {
                    sqlx::query("DELETE FROM question_tags WHERE question_id = ? AND tag_id = ?")
                        .bind(&row.id)
                        .bind(tag_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(CommandError::database)?;
                }
            }
            "keep" => {}
            _ => unreachable!("tag mode was validated"),
        }

        let tag_names = sqlx::query_scalar::<_, String>(
            "SELECT t.name FROM tags t JOIN question_tags qt ON qt.tag_id = t.id \
             WHERE qt.question_id = ? ORDER BY t.name, t.id",
        )
        .bind(&row.id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
        if tag_names.len() > MAX_TAGS {
            return Err(CommandError::validation(format!(
                "题目最多允许 {MAX_TAGS} 个标签，本次修改未执行。"
            )));
        }
        let tags_plain = tag_names.join(" ");
        let (subject_id, chapter_id) = request
            .classification
            .as_ref()
            .map(|classification| {
                (
                    classification.subject_id.as_str(),
                    classification.chapter_id.as_str(),
                )
            })
            .unwrap_or((row.subject_id.as_str(), row.chapter_id.as_str()));
        let last_used_at = if request.usage_operation == "reset_never" {
            None
        } else {
            row.last_used_at_ms
        };
        let affected = sqlx::query(
            "UPDATE questions SET subject_id = ?, chapter_id = ?, last_used_at_ms = ?, \
             tags_plain = ?, content_version = content_version + 1, updated_at_ms = ? \
             WHERE id = ? AND content_version = ? AND deleted_at_ms IS NULL",
        )
        .bind(subject_id)
        .bind(chapter_id)
        .bind(last_used_at)
        .bind(tags_plain)
        .bind(now)
        .bind(&row.id)
        .bind(target.expected_content_version)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
        if affected.rows_affected() != 1 {
            return Err(CommandError::new(
                "CONTENT_CONFLICT",
                "执行批量修改时检测到版本冲突，本次修改已全部撤销。",
            ));
        }
        versions.push(QuestionBatchVersionApi {
            id: row.id.clone(),
            content_version: row.content_version.saturating_add(1),
        });
    }

    let details = serde_json::json!({
        "updatedCount": versions.len(),
        "classificationChanged": request.classification.is_some(),
        "usageOperation": request.usage_operation,
        "tagOperation": request.tag_operation.mode,
        "tagCount": request.tag_operation.tag_ids.len(),
    })
    .to_string();
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, outcome, \
         summary, details_json, app_version) VALUES (?, ?, 'info', 'question.batch_edit', \
         'question', 'success', '批量编辑题目', ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(now)
    .bind(details)
    .bind(app_version)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    transaction.commit().await.map_err(CommandError::database)?;
    Ok(QuestionBatchEditResultApi {
        updated_count: u32::try_from(versions.len()).unwrap_or(u32::MAX),
        questions: versions,
    })
}

pub async fn set_deleted(
    pool: &SqlitePool,
    ids: &[String],
    deleted: bool,
    app_version: &str,
) -> CommandResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let mut builder = QueryBuilder::<Sqlite>::new("UPDATE questions SET deleted_at_ms = ");
    if deleted {
        builder.push_bind(now);
    } else {
        builder.push("NULL");
    }
    builder
        .push(", updated_at_ms = ")
        .push_bind(now)
        .push(" WHERE id IN (");
    let mut separated = builder.separated(", ");
    for id in ids {
        separated.push_bind(id.clone());
    }
    separated.push_unseparated(")");
    let affected = builder
        .build()
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?
        .rows_affected();
    let action = if deleted {
        "question.recycle"
    } else {
        "question.restore"
    };
    let summary = if deleted {
        "题目移入回收站"
    } else {
        "从回收站恢复题目"
    };
    let details = serde_json::json!({
        "requestedCount": ids.len(),
        "affectedCount": affected,
    })
    .to_string();
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, outcome, \
         summary, details_json, app_version) VALUES (?, ?, 'info', ?, 'question', 'success', ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(now)
    .bind(action)
    .bind(summary)
    .bind(details)
    .bind(app_version)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn permanently_delete(
    pool: &SqlitePool,
    ids: &[String],
    app_version: &str,
) -> CommandResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let mut builder = QueryBuilder::<Sqlite>::new(
        "DELETE FROM questions WHERE deleted_at_ms IS NOT NULL AND id IN (",
    );
    let mut separated = builder.separated(", ");
    for id in ids {
        separated.push_bind(id.clone());
    }
    separated.push_unseparated(")");
    let affected = builder
        .build()
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?
        .rows_affected();
    let details = serde_json::json!({
        "requestedCount": ids.len(),
        "deletedCount": affected,
    })
    .to_string();
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, outcome, \
         summary, details_json, app_version) VALUES (?, ?, 'warning', 'question.delete_permanently', \
         'question', 'success', '永久删除回收站题目', ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(now)
    .bind(details)
    .bind(app_version)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)?;
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

#[cfg(test)]
mod tests {
    use std::{fs, thread, time::Duration};

    use crate::db::Database;

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

    #[test]
    fn rich_content_v2_accepts_tiptap_json_and_keeps_compatibility_caches() {
        let content = RichContentApi {
            schema_version: 2,
            editor: Some("tiptap".to_owned()),
            editor_version: Some("3.28.0".to_owned()),
            document: Some(serde_json::json!({
                "type": "doc",
                "content": [{
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": "题干" }]
                }]
            })),
            html: "<p>题干</p>".to_owned(),
            plain_text: "题干".to_owned(),
            source_ooxml: None,
        };

        assert!(validate_rich_content("题干", &content, false).is_ok());
    }

    fn draft(stem: &str, answer: &str) -> QuestionDraftApi {
        QuestionDraftApi {
            id: None,
            question_id: None,
            question_type: "short_answer".to_owned(),
            stem: rich(&format!("<p>{stem}</p>"), stem),
            options: Vec::new(),
            answer: rich(&format!("<p>{answer}</p>"), answer),
            explanation: rich("", ""),
            subject_id: Uuid::nil().to_string(),
            chapter_id: Uuid::nil().to_string(),
            tag_ids: Vec::new(),
            resource_refs: Vec::new(),
            base_content_version: None,
        }
    }

    #[test]
    fn rejects_executable_rich_content() {
        let script = rich("<p>题目</p><script>alert(1)</script>", "题目");
        assert!(validate_rich_content("题干", &script, false).is_err());

        let event = rich("<img src='x'\n onerror = 'alert(1)'>", "图片");
        assert!(validate_rich_content("题干", &event, false).is_err());
    }

    #[test]
    fn accepts_basic_formatting() {
        let content = rich("<p><strong>教育</strong>是什么？</p>", "教育是什么？");
        assert!(validate_rich_content("题干", &content, false).is_ok());
    }

    #[test]
    fn rejects_reserved_duplicate_fingerprint_separators() {
        let content = rich("<p>题目</p>", "题目\u{1f}答案");
        assert!(validate_rich_content("题干", &content, false).is_err());
    }

    #[test]
    fn exact_fingerprint_normalizes_nfkc_case_and_whitespace() {
        let first = draft("ＡＢＣ 教育", "答案 A");
        let second = draft("abc\u{0085}教育", "答案a");
        assert_eq!(
            normalized_fingerprint(&first),
            normalized_fingerprint(&second)
        );

        let changed_answer = draft("abc 教育", "答案 B");
        assert_ne!(
            normalized_fingerprint(&first),
            normalized_fingerprint(&changed_answer)
        );
    }

    #[test]
    fn unicode_bigram_dice_marks_small_edits_as_close_and_unrelated_text_as_low() {
        let original = similarity_text_from_draft(&draft(
            "教育的本质属性是有目的地培养人的社会活动",
            "有目的地培养人的社会活动",
        ));
        let small_edit = similarity_text_from_draft(&draft(
            "教育的本质属性是有计划地培养人的社会活动",
            "有目的地培养人的社会活动",
        ));
        let unrelated = similarity_text_from_draft(&draft(
            "光合作用产生氧气需要哪些条件",
            "光照、水和二氧化碳",
        ));

        assert!(
            character_bigram_dice_percent(&original, &small_edit)
                >= SUSPECTED_DUPLICATE_THRESHOLD_PERCENT
        );
        assert!(
            character_bigram_dice_percent(&original, &unrelated)
                < SUSPECTED_DUPLICATE_THRESHOLD_PERCENT
        );
    }

    #[test]
    fn similarity_normalization_has_a_hard_per_field_character_limit() {
        let long_stem = "题".repeat(MAX_SIMILARITY_FIELD_CHARS + 1_000);
        let normalized = similarity_text_from_draft(&draft(&long_stem, "答案"));
        assert_eq!(
            normalized.len(),
            MAX_SIMILARITY_FIELD_CHARS + 2 + "答案".chars().count()
        );
    }

    #[test]
    fn duplicate_scan_cancellation_is_visible_to_cooperative_checks() {
        let scan_id = Uuid::now_v7().to_string();
        ensure_duplicate_scan_active(Some(&scan_id)).unwrap();
        cancel_question_duplicate_scan(&scan_id).unwrap();
        assert_eq!(
            ensure_duplicate_scan_active(Some(&scan_id))
                .unwrap_err()
                .code,
            "DUPLICATE_SCAN_CANCELLED"
        );
    }

    #[test]
    fn import_duplicate_check_bounds_work_for_twelve_hundred_rows() {
        let items = (0..1_200)
            .map(
                |index| super::super::models::QuestionDuplicateBatchEntryApi {
                    client_id: Uuid::now_v7().to_string(),
                    draft: draft(
                        &format!("第 {index} 道批量导入压力题，唯一校验值 {index}"),
                        &format!("第 {index} 道压力题答案"),
                    ),
                },
            )
            .collect::<Vec<_>>();
        let result = check_import_duplicates(
            &QuestionDuplicateBatchRequestApi {
                items,
                mode: Some("import".to_owned()),
                scan_id: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(result.items.len(), 1_200);
        assert!(result.items.iter().all(|item| {
            item.check.evaluated_candidate_count <= u32::try_from(MAX_DUPLICATE_CANDIDATES).unwrap()
        }));
    }

    #[test]
    fn batch_request_requires_an_operation_and_unique_targets() {
        let question_id = Uuid::now_v7().to_string();
        let no_changes = QuestionBatchEditRequestApi {
            questions: vec![super::super::models::QuestionBatchTargetApi {
                id: question_id.clone(),
                expected_content_version: 1,
            }],
            classification: None,
            usage_operation: "keep".to_owned(),
            tag_operation: super::super::models::QuestionBatchTagOperationApi {
                mode: "keep".to_owned(),
                tag_ids: Vec::new(),
            },
        };
        assert_eq!(
            validate_batch_edit_request(&no_changes).unwrap_err().code,
            "BATCH_EDIT_NO_CHANGES"
        );

        let duplicate = QuestionBatchEditRequestApi {
            questions: vec![
                super::super::models::QuestionBatchTargetApi {
                    id: question_id.clone(),
                    expected_content_version: 1,
                },
                super::super::models::QuestionBatchTargetApi {
                    id: question_id,
                    expected_content_version: 1,
                },
            ],
            usage_operation: "reset_never".to_owned(),
            ..no_changes
        };
        assert_eq!(
            validate_batch_edit_request(&duplicate).unwrap_err().code,
            "BATCH_EDIT_DUPLICATE_ID"
        );
    }

    #[test]
    fn imported_overwrite_request_rejects_duplicate_targets_before_writing() {
        let question_id = Uuid::now_v7().to_string();
        let mut first = draft("第一份导入内容", "答案一");
        first.question_id = Some(question_id.clone());
        first.base_content_version = Some(1);
        let mut second = draft("第二份导入内容", "答案二");
        second.question_id = Some(question_id);
        second.base_content_version = Some(1);
        let request = ImportedQuestionOverwriteRequestApi {
            operation_id: Uuid::now_v7().to_string(),
            items: vec![
                super::super::models::ImportedQuestionOverwriteItemApi {
                    client_id: Uuid::now_v7().to_string(),
                    draft: first,
                },
                super::super::models::ImportedQuestionOverwriteItemApi {
                    client_id: Uuid::now_v7().to_string(),
                    draft: second,
                },
            ],
        };

        assert_eq!(
            validate_imported_question_overwrite_request(&request)
                .unwrap_err()
                .code,
            "IMPORT_OVERWRITE_DUPLICATE_TARGET"
        );
    }

    fn test_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "zhitiku-question-{name}-{}",
            Uuid::now_v7().simple()
        ))
    }

    fn remove_test_root(root: &std::path::Path) {
        let retry_delays_ms = [20, 50, 100, 200, 400, 800, 1_000, 1_500];
        for (attempt, delay_ms) in retry_delays_ms.into_iter().enumerate() {
            match fs::remove_dir_all(root) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(error)
                    if matches!(error.raw_os_error(), Some(32 | 33))
                        && attempt + 1 < retry_delays_ms.len() =>
                {
                    thread::sleep(Duration::from_millis(delay_ms));
                }
                Err(error) => panic!("failed to remove test root {}: {error}", root.display()),
            }
        }
    }

    async fn insert_test_classification(
        pool: &SqlitePool,
        subject_name: &str,
        chapter_name: &str,
    ) -> (String, String) {
        let subject_id = Uuid::now_v7().to_string();
        let chapter_id = Uuid::now_v7().to_string();
        let now = now_millis();
        sqlx::query(
            "INSERT INTO subjects (id, name, name_key, sort_order, created_at_ms, updated_at_ms) \
             VALUES (?, ?, ?, 0, ?, ?)",
        )
        .bind(&subject_id)
        .bind(subject_name)
        .bind(subject_name)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chapters (id, subject_id, name, name_key, sort_order, created_at_ms, updated_at_ms) \
             VALUES (?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(&chapter_id)
        .bind(&subject_id)
        .bind(chapter_name)
        .bind(chapter_name)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        (subject_id, chapter_id)
    }

    async fn insert_test_tag(pool: &SqlitePool, name: &str) -> String {
        let id = Uuid::now_v7().to_string();
        let now = now_millis();
        sqlx::query(
            "INSERT INTO tags (id, name, name_key, created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(name)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    async fn random_draw_analyzes_tags_excludes_selected_and_hydrates_only_drawn_questions() {
        let root = test_root("random-draw");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (subject_id, chapter_id) =
            insert_test_classification(pool, "随机抽题学科", "随机抽题章节").await;
        let first_tag = insert_test_tag(pool, "第一标签").await;
        let second_tag = insert_test_tag(pool, "第二标签").await;

        let mut first_draft = draft("第一道候选题", "答案一");
        first_draft.subject_id = subject_id.clone();
        first_draft.chapter_id = chapter_id.clone();
        first_draft.tag_ids = vec![first_tag.clone()];
        let first = save_question(pool, &first_draft, "test").await.unwrap();

        let mut second_draft = draft("第二道候选题", "答案二");
        second_draft.subject_id = subject_id.clone();
        second_draft.chapter_id = chapter_id.clone();
        second_draft.tag_ids = vec![second_tag.clone()];
        save_question(pool, &second_draft, "test").await.unwrap();

        let mut shared_draft = draft("共同标签候选题", "答案三");
        shared_draft.subject_id = subject_id.clone();
        shared_draft.chapter_id = chapter_id.clone();
        shared_draft.tag_ids = vec![first_tag.clone(), second_tag.clone()];
        save_question(pool, &shared_draft, "test").await.unwrap();

        let scope = super::super::models::RandomDrawScopeApi {
            subject_ids: vec![subject_id],
            chapter_ids: vec![chapter_id],
            tag_ids: vec![first_tag, second_tag],
            tag_match_mode: "any".to_owned(),
            usage: "all".to_owned(),
            excluded_question_ids: vec![first.id],
        };
        let analysis = super::super::random_draw::analyze(pool, &scope)
            .await
            .unwrap();
        assert_eq!(analysis.available_total, 2);
        assert_eq!(analysis.available_by_type.get("short_answer"), Some(&2));

        let request = super::super::models::RandomDrawRequestApi {
            scope,
            count_mode: "by_type".to_owned(),
            total_count: 0,
            question_type_counts: [("short_answer".to_owned(), 2)].into_iter().collect(),
        };
        let selected = super::super::random_draw::draw(pool, &request)
            .await
            .unwrap();
        assert_eq!(selected.len(), 2);
        assert!(
            selected
                .iter()
                .all(|question| question.question_type_name.is_none())
        );
        assert!(
            selected
                .iter()
                .all(|question| !question.stem.plain_text.is_empty())
        );

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn random_draw_combines_multiple_subject_scopes() {
        let root = test_root("random-draw-multiple-subjects");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (first_subject, first_chapter) =
            insert_test_classification(pool, "随机抽题第一学科", "共同章节名").await;
        let (second_subject, second_chapter) =
            insert_test_classification(pool, "随机抽题第二学科", "共同章节名").await;

        let mut first_draft = draft("第一学科候选题", "答案一");
        first_draft.subject_id = first_subject.clone();
        first_draft.chapter_id = first_chapter;
        save_question(pool, &first_draft, "test").await.unwrap();

        let mut second_draft = draft("第二学科候选题", "答案二");
        second_draft.subject_id = second_subject.clone();
        second_draft.chapter_id = second_chapter;
        save_question(pool, &second_draft, "test").await.unwrap();

        let combined_scope = super::super::models::RandomDrawScopeApi {
            subject_ids: vec![first_subject.clone(), second_subject.clone()],
            chapter_ids: vec![],
            tag_ids: vec![],
            tag_match_mode: "any".to_owned(),
            usage: "all".to_owned(),
            excluded_question_ids: vec![],
        };
        let combined = super::super::random_draw::analyze(pool, &combined_scope)
            .await
            .unwrap();
        assert_eq!(combined.available_total, 2);

        let single_scope = super::super::models::RandomDrawScopeApi {
            subject_ids: vec![second_subject],
            ..combined_scope
        };
        let single = super::super::random_draw::analyze(pool, &single_scope)
            .await
            .unwrap();
        assert_eq!(single.available_total, 1);

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn random_draw_filters_candidates_by_usage_status() {
        let root = test_root("random-draw-usage-status");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (subject_id, chapter_id) =
            insert_test_classification(pool, "使用状态学科", "使用状态章节").await;

        let mut never_draft = draft("从未使用候选题", "答案一");
        never_draft.subject_id = subject_id.clone();
        never_draft.chapter_id = chapter_id.clone();
        save_question(pool, &never_draft, "test").await.unwrap();

        let mut old_draft = draft("很久未使用候选题", "答案二");
        old_draft.subject_id = subject_id.clone();
        old_draft.chapter_id = chapter_id.clone();
        let old = save_question(pool, &old_draft, "test").await.unwrap();

        let mut today_draft = draft("今天使用候选题", "答案三");
        today_draft.subject_id = subject_id.clone();
        today_draft.chapter_id = chapter_id.clone();
        let today = save_question(pool, &today_draft, "test").await.unwrap();

        let current_time = now_millis();
        sqlx::query("UPDATE questions SET last_used_at_ms = ? WHERE id = ?")
            .bind(current_time.saturating_sub(400 * 86_400_000))
            .bind(&old.id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("UPDATE questions SET last_used_at_ms = ? WHERE id = ?")
            .bind(current_time)
            .bind(&today.id)
            .execute(pool)
            .await
            .unwrap();

        let mut scope = super::super::models::RandomDrawScopeApi {
            subject_ids: vec![subject_id.clone()],
            chapter_ids: vec![chapter_id.clone()],
            tag_ids: vec![],
            tag_match_mode: "any".to_owned(),
            usage: "all".to_owned(),
            excluded_question_ids: vec![],
        };
        assert_eq!(
            super::super::random_draw::analyze(pool, &scope)
                .await
                .unwrap()
                .available_total,
            3
        );

        scope.usage = "never".to_owned();
        assert_eq!(
            super::super::random_draw::analyze(pool, &scope)
                .await
                .unwrap()
                .available_total,
            1
        );

        for usage in [
            "unused_this_semester",
            "unused_this_month",
            "unused_this_week",
            "unused_today",
        ] {
            scope.usage = usage.to_owned();
            let random_draw_total = super::super::random_draw::analyze(pool, &scope)
                .await
                .unwrap()
                .available_total;
            let question_bank_total = list_questions(
                pool,
                &QuestionFiltersApi {
                    keyword: String::new(),
                    subject_id: Some(subject_id.clone()),
                    chapter_id: Some(chapter_id.clone()),
                    question_type: None,
                    tag_ids: vec![],
                    tag_match_mode: "any".to_owned(),
                    usage: usage.to_owned(),
                    deleted: false,
                    page: 1,
                    page_size: 100,
                },
            )
            .await
            .unwrap()
            .total;
            assert_eq!(
                random_draw_total, 2,
                "unexpected candidate count for {usage}"
            );
            assert_eq!(
                i64::from(random_draw_total),
                question_bank_total,
                "question bank and random draw disagree for {usage}"
            );
        }

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn batch_edit_is_atomic_and_refreshes_searchable_tags() {
        let root = test_root("batch-edit");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (old_subject, old_chapter) = insert_test_classification(pool, "原学科", "原章节").await;
        let (new_subject, new_chapter) = insert_test_classification(pool, "新学科", "新章节").await;
        let old_tag = insert_test_tag(pool, "旧标签").await;
        let new_tag = insert_test_tag(pool, "新标签").await;

        let mut first_draft = draft("第一道题", "第一答案");
        first_draft.subject_id = old_subject.clone();
        first_draft.chapter_id = old_chapter.clone();
        first_draft.tag_ids = vec![old_tag.clone()];
        let first = save_question(pool, &first_draft, "test").await.unwrap();

        let mut second_draft = draft("第二道题", "第二答案");
        second_draft.subject_id = old_subject.clone();
        second_draft.chapter_id = old_chapter.clone();
        second_draft.tag_ids = vec![old_tag];
        let second = save_question(pool, &second_draft, "test").await.unwrap();
        sqlx::query("UPDATE questions SET last_used_at_ms = 123 WHERE id IN (?, ?)")
            .bind(&first.id)
            .bind(&second.id)
            .execute(pool)
            .await
            .unwrap();

        let request = QuestionBatchEditRequestApi {
            questions: vec![
                super::super::models::QuestionBatchTargetApi {
                    id: first.id.clone(),
                    expected_content_version: first.content_version,
                },
                super::super::models::QuestionBatchTargetApi {
                    id: second.id.clone(),
                    expected_content_version: second.content_version + 1,
                },
            ],
            classification: Some(super::super::models::QuestionBatchClassificationApi {
                subject_id: new_subject.clone(),
                chapter_id: new_chapter.clone(),
            }),
            usage_operation: "reset_never".to_owned(),
            tag_operation: super::super::models::QuestionBatchTagOperationApi {
                mode: "replace".to_owned(),
                tag_ids: vec![new_tag.clone()],
            },
        };
        let error = batch_edit_questions(pool, &request, "test")
            .await
            .unwrap_err();
        assert_eq!(error.code, "CONTENT_CONFLICT");
        let unchanged = get_question(pool, &first.id).await.unwrap().unwrap();
        assert_eq!(unchanged.subject_id, old_subject);
        assert_eq!(unchanged.content_version, first.content_version);
        assert_eq!(unchanged.tags[0].name, "旧标签");
        assert_eq!(unchanged.last_used_at, Some(123));

        let mut valid_request = request;
        valid_request.questions[1].expected_content_version = second.content_version;
        let result = batch_edit_questions(pool, &valid_request, "test")
            .await
            .unwrap();
        assert_eq!(result.updated_count, 2);
        let updated = get_question(pool, &first.id).await.unwrap().unwrap();
        assert_eq!(updated.subject_id, new_subject);
        assert_eq!(updated.chapter_id, new_chapter);
        assert_eq!(updated.tags[0].id, new_tag);
        assert_eq!(updated.last_used_at, None);
        assert_eq!(updated.content_version, first.content_version + 1);

        let searchable: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM question_fts \
             WHERE rowid = (SELECT local_id FROM questions WHERE id = ?) \
             AND question_fts MATCH '新标签'",
        )
        .bind(&first.id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(searchable, 1);

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn imported_overwrite_batch_is_atomic_and_idempotent() {
        let root = test_root("import-overwrite");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (subject_id, chapter_id) =
            insert_test_classification(pool, "覆盖导入学科", "覆盖导入章节").await;

        let mut first_original = draft("第一道原题", "第一答案");
        first_original.subject_id = subject_id.clone();
        first_original.chapter_id = chapter_id.clone();
        let first = save_question(pool, &first_original, "test").await.unwrap();

        let mut second_original = draft("第二道原题", "第二答案");
        second_original.subject_id = subject_id.clone();
        second_original.chapter_id = chapter_id.clone();
        let second = save_question(pool, &second_original, "test").await.unwrap();

        let mut first_overwrite = draft("第一道覆盖题", "覆盖答案一");
        first_overwrite.subject_id = subject_id.clone();
        first_overwrite.chapter_id = chapter_id.clone();
        first_overwrite.question_id = Some(first.id.clone());
        first_overwrite.base_content_version = Some(first.content_version);

        let mut second_overwrite = draft("第二道覆盖题", "覆盖答案二");
        second_overwrite.subject_id = subject_id;
        second_overwrite.chapter_id = chapter_id;
        second_overwrite.question_id = Some(second.id.clone());
        second_overwrite.base_content_version = Some(second.content_version + 1);

        let mut request = ImportedQuestionOverwriteRequestApi {
            operation_id: Uuid::now_v7().to_string(),
            items: vec![
                super::super::models::ImportedQuestionOverwriteItemApi {
                    client_id: Uuid::now_v7().to_string(),
                    draft: first_overwrite,
                },
                super::super::models::ImportedQuestionOverwriteItemApi {
                    client_id: Uuid::now_v7().to_string(),
                    draft: second_overwrite,
                },
            ],
        };

        let stale_error = save_imported_question_overwrites(pool, &request, "test")
            .await
            .unwrap_err();
        assert_eq!(stale_error.code, "CONTENT_CONFLICT");
        let unchanged_first = get_question(pool, &first.id).await.unwrap().unwrap();
        assert_eq!(unchanged_first.stem.plain_text, "第一道原题");
        assert_eq!(unchanged_first.content_version, first.content_version);
        let receipt_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM question_import_overwrite_operations WHERE operation_id = ?",
        )
        .bind(&request.operation_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(receipt_count, 0);

        request.items[1].draft.base_content_version = Some(second.content_version);
        let first_result = save_imported_question_overwrites(pool, &request, "test")
            .await
            .unwrap();
        assert!(!first_result.replayed);
        assert_eq!(first_result.updated_count, 2);
        assert_eq!(
            first_result.items[0].content_version,
            first.content_version + 1
        );
        assert_eq!(
            get_question(pool, &first.id)
                .await
                .unwrap()
                .unwrap()
                .stem
                .plain_text,
            "第一道覆盖题"
        );

        let replay = save_imported_question_overwrites(pool, &request, "test")
            .await
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.items, first_result.items);
        assert_eq!(
            get_question(pool, &first.id)
                .await
                .unwrap()
                .unwrap()
                .content_version,
            first.content_version + 1
        );

        let mut reused_operation = request;
        reused_operation.items[0].draft.explanation = rich("<p>不同内容</p>", "不同内容");
        let conflict = save_imported_question_overwrites(pool, &reused_operation, "test")
            .await
            .unwrap_err();
        assert_eq!(conflict.code, "IMPORT_OVERWRITE_OPERATION_CONFLICT");

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn batch_duplicate_check_reuses_bank_data_and_finds_in_batch_exact_matches() {
        let root = test_root("duplicate-batch");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (subject_id, chapter_id) =
            insert_test_classification(pool, "批量查重学科", "批量查重章节").await;

        let mut existing = draft(
            "交换机如何根据地址表转发数据帧",
            "根据目的 MAC 地址查表转发",
        );
        existing.subject_id = subject_id.clone();
        existing.chapter_id = chapter_id.clone();
        let saved = save_question(pool, &existing, "test").await.unwrap();

        let mut new_draft = draft("光合作用需要哪些基本条件", "光照、水和二氧化碳");
        new_draft.subject_id = subject_id.clone();
        new_draft.chapter_id = chapter_id.clone();
        let request = QuestionDuplicateBatchRequestApi {
            items: vec![
                super::super::models::QuestionDuplicateBatchEntryApi {
                    client_id: Uuid::now_v7().to_string(),
                    draft: existing.clone(),
                },
                super::super::models::QuestionDuplicateBatchEntryApi {
                    client_id: Uuid::now_v7().to_string(),
                    draft: new_draft.clone(),
                },
                super::super::models::QuestionDuplicateBatchEntryApi {
                    client_id: Uuid::now_v7().to_string(),
                    draft: new_draft,
                },
            ],
            mode: Some("full".to_owned()),
            scan_id: None,
        };
        let result = check_question_duplicates_batch(pool, &request)
            .await
            .unwrap();
        assert_eq!(result.items.len(), 3);
        assert_eq!(result.items[0].check.status, "exact");
        assert_eq!(
            result.items[0]
                .check
                .candidate
                .as_ref()
                .map(|candidate| candidate.id.as_str()),
            Some(saved.id.as_str())
        );
        assert_eq!(result.items[1].check.status, "none");
        assert_eq!(result.items[2].check.status, "exact");
        let in_batch = result.items[2].check.candidate.as_ref().unwrap();
        assert_eq!(in_batch.source_kind.as_deref(), Some("import-item"));
        assert_eq!(in_batch.id, request.items[1].client_id);
        assert_eq!(in_batch.content_version, 0);

        let mut stress_items = Vec::new();
        for index in 0..MAX_DUPLICATE_BATCH_ITEMS {
            let mut item = draft(
                &format!("批量压力测试题目 {index}"),
                &format!("批量压力测试答案 {index}"),
            );
            item.subject_id = subject_id.clone();
            item.chapter_id = chapter_id.clone();
            stress_items.push(super::super::models::QuestionDuplicateBatchEntryApi {
                client_id: Uuid::now_v7().to_string(),
                draft: item,
            });
        }
        let stress = check_question_duplicates_batch(
            pool,
            &QuestionDuplicateBatchRequestApi {
                items: stress_items,
                mode: Some("exact".to_owned()),
                scan_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(stress.items.len(), MAX_DUPLICATE_BATCH_ITEMS);
        assert!(
            stress
                .items
                .iter()
                .all(|item| item.check.evaluated_candidate_count <= 1)
        );

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn import_duplicate_check_finds_similar_items_across_database_batch_boundaries() {
        let root = test_root("duplicate-import-batch");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (subject_id, chapter_id) =
            insert_test_classification(pool, "导入查重学科", "导入查重章节").await;

        let mut items = Vec::new();
        let mut first = draft(
            "某学校网络管理员需要为教学楼配置访问控制策略并限制访客访问服务器",
            "在边界设备配置访问控制列表并只允许指定网段",
        );
        first.subject_id = subject_id.clone();
        first.chapter_id = chapter_id.clone();
        let first_id = Uuid::now_v7().to_string();
        items.push(super::super::models::QuestionDuplicateBatchEntryApi {
            client_id: first_id.clone(),
            draft: first,
        });
        for index in 0..100 {
            let mut filler = draft(
                &format!("批次边界填充题目 {index}，内容彼此独立"),
                &format!("填充答案 {index}"),
            );
            filler.subject_id = subject_id.clone();
            filler.chapter_id = chapter_id.clone();
            items.push(super::super::models::QuestionDuplicateBatchEntryApi {
                client_id: Uuid::now_v7().to_string(),
                draft: filler,
            });
        }
        let mut similar = draft(
            "某学校网络管理员需要为教学楼配置访问控制策略并禁止访客访问服务器",
            "在边界设备配置访问控制列表并只允许指定网段",
        );
        similar.subject_id = subject_id;
        similar.chapter_id = chapter_id;
        items.push(super::super::models::QuestionDuplicateBatchEntryApi {
            client_id: Uuid::now_v7().to_string(),
            draft: similar,
        });

        let result = check_question_duplicates_batch(
            pool,
            &QuestionDuplicateBatchRequestApi {
                items,
                mode: Some("import".to_owned()),
                scan_id: None,
            },
        )
        .await
        .unwrap();
        let last = result.items.last().unwrap();
        assert_eq!(last.check.status, "suspected");
        let candidate = last.check.candidate.as_ref().unwrap();
        assert_eq!(candidate.id, first_id);
        assert_eq!(candidate.source_kind.as_deref(), Some("import-item"));
        assert!(candidate.similarity_percent >= SUSPECTED_DUPLICATE_THRESHOLD_PERCENT);

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn imported_question_batch_is_idempotent_by_stable_draft_id() {
        let root = test_root("import-save-idempotent");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (subject_id, chapter_id) =
            insert_test_classification(pool, "幂等导入学科", "幂等导入章节").await;

        let stable_id = Uuid::now_v7().to_string();
        let mut imported = draft("幂等导入题干", "幂等导入答案");
        imported.id = Some(stable_id.clone());
        imported.subject_id = subject_id;
        imported.chapter_id = chapter_id;

        let first = save_imported_questions(pool, &[imported.clone()], "test")
            .await
            .unwrap();
        let second = save_imported_questions(pool, &[imported.clone()], "test")
            .await
            .unwrap();
        assert_eq!(first, vec![stable_id.clone()]);
        assert_eq!(second, first);
        let stored_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM questions WHERE id = ?")
            .bind(&stable_id)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(stored_count, 1);

        imported.explanation = rich("<p>修改后的解析</p>", "修改后的解析");
        let conflict = save_imported_questions(pool, &[imported], "test")
            .await
            .unwrap_err();
        assert_eq!(conflict.code, "IMPORT_ID_CONFLICT");
        let stored_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM questions WHERE id = ?")
            .bind(&stable_id)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(stored_count, 1);

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn manual_duplicate_scan_respects_scope_and_expires_ignored_pairs_after_edit() {
        let root = test_root("duplicate-scan");
        let database = Database::open(&root, "test").await.unwrap();
        let pool = database.pool();
        let (scope_subject, scope_chapter) =
            insert_test_classification(pool, "范围学科", "范围章节").await;
        let (other_subject, other_chapter) =
            insert_test_classification(pool, "其他学科", "其他章节").await;

        let mut exact_in_scope = draft("交换机的基本转发原理是什么", "根据MAC地址表转发数据帧");
        exact_in_scope.subject_id = scope_subject.clone();
        exact_in_scope.chapter_id = scope_chapter.clone();
        let exact_first = save_question(pool, &exact_in_scope, "test").await.unwrap();

        let mut exact_outside = exact_in_scope.clone();
        exact_outside.subject_id = other_subject.clone();
        exact_outside.chapter_id = other_chapter.clone();
        let exact_second = save_question(pool, &exact_outside, "test").await.unwrap();

        let mut suspected_in_scope = draft(
            "某学校网络管理员需要为教学楼配置访问控制策略并限制访客访问服务器",
            "在边界设备配置访问控制列表并只允许指定网段",
        );
        suspected_in_scope.subject_id = scope_subject.clone();
        suspected_in_scope.chapter_id = scope_chapter.clone();
        let suspected_first = save_question(pool, &suspected_in_scope, "test")
            .await
            .unwrap();

        let mut suspected_outside = draft(
            "某学校网络管理员需要为教学楼配置访问控制策略并禁止访客访问服务器",
            "在边界设备配置访问控制列表并只允许指定网段",
        );
        suspected_outside.subject_id = other_subject;
        suspected_outside.chapter_id = other_chapter;
        let suspected_second = save_question(pool, &suspected_outside, "test")
            .await
            .unwrap();

        let request = QuestionDuplicateScanRequestApi {
            subject_id: scope_subject,
            chapter_id: Some(scope_chapter),
        };
        let missing_scope_error = scan_question_duplicates(
            pool,
            &QuestionDuplicateScanRequestApi {
                subject_id: Uuid::now_v7().to_string(),
                chapter_id: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(missing_scope_error.code, "DUPLICATE_SCAN_SCOPE_NOT_FOUND");
        let initial = scan_question_duplicates(pool, &request).await.unwrap();
        assert_eq!(initial.scanned_question_count, 2);
        assert_eq!(initial.compared_question_count, 4);
        assert!(initial.exact_groups.iter().any(|group| {
            let ids = group
                .members
                .iter()
                .map(|member| member.id.as_str())
                .collect::<HashSet<_>>();
            ids.contains(exact_first.id.as_str()) && ids.contains(exact_second.id.as_str())
        }));
        let exact_ignore_error = ignore_question_duplicate(
            pool,
            &IgnoreQuestionDuplicateRequestApi {
                first_question_id: exact_first.id.clone(),
                first_content_version: exact_first.content_version,
                second_question_id: exact_second.id.clone(),
                second_content_version: exact_second.content_version,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(exact_ignore_error.code, "DUPLICATE_PAIR_EXACT");
        let suspected_group = initial
            .suspected_groups
            .iter()
            .find(|group| {
                let ids = group
                    .members
                    .iter()
                    .map(|member| member.id.as_str())
                    .collect::<HashSet<_>>();
                ids.contains(suspected_first.id.as_str())
                    && ids.contains(suspected_second.id.as_str())
            })
            .expect("the similar cross-scope pair should be reported");
        assert!(suspected_group.similarity_percent >= SUSPECTED_DUPLICATE_THRESHOLD_PERCENT);

        ignore_question_duplicate(
            pool,
            &IgnoreQuestionDuplicateRequestApi {
                first_question_id: suspected_first.id.clone(),
                first_content_version: suspected_first.content_version,
                second_question_id: suspected_second.id.clone(),
                second_content_version: suspected_second.content_version,
            },
        )
        .await
        .unwrap();
        let ignored = scan_question_duplicates(pool, &request).await.unwrap();
        assert!(!ignored.suspected_groups.iter().any(|group| {
            group
                .members
                .iter()
                .any(|member| member.id == suspected_first.id)
                && group
                    .members
                    .iter()
                    .any(|member| member.id == suspected_second.id)
        }));

        suspected_outside.question_id = Some(suspected_second.id.clone());
        suspected_outside.base_content_version = Some(suspected_second.content_version);
        suspected_outside.stem = rich(
            "某学校网络管理员需要为教学楼配置访问控制策略并阻止访客访问服务器",
            "某学校网络管理员需要为教学楼配置访问控制策略并阻止访客访问服务器",
        );
        save_question(pool, &suspected_outside, "test")
            .await
            .unwrap();
        let stale_ignore_error = ignore_question_duplicate(
            pool,
            &IgnoreQuestionDuplicateRequestApi {
                first_question_id: suspected_first.id.clone(),
                first_content_version: suspected_first.content_version,
                second_question_id: suspected_second.id.clone(),
                second_content_version: suspected_second.content_version,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(stale_ignore_error.code, "DUPLICATE_PAIR_STALE");
        let after_edit = scan_question_duplicates(pool, &request).await.unwrap();
        assert!(after_edit.suspected_groups.iter().any(|group| {
            group
                .members
                .iter()
                .any(|member| member.id == suspected_first.id)
                && group
                    .members
                    .iter()
                    .any(|member| member.id == suspected_second.id)
        }));

        database.close().await;
        drop(database);
        remove_test_root(&root);
    }
}
