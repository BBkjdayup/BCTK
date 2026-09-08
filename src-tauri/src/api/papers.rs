use std::collections::HashSet;

use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::models::{
    AutomaticGenerationConfigDto, CommandError, CommandResult, PageResultApi, PaperApi,
    PaperDeleteRequestApi, PaperFiltersApi, PaperItemApi, PaperSummaryApi, QuestionApi,
};
use super::resources;

const TITLE_MAX: usize = 200;
const MAX_ITEMS: usize = 1_000;
const MAX_GENERATION_CHAPTERS: usize = 10_000;
const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
const MAX_LAYOUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_DELETE_BATCH: usize = 500;
const EXPORT_CONTENT_MODES: &[&str] = &[
    "paper_only",
    "answers_only",
    "paper_and_answers",
    "paper_answers_explanations",
];

#[derive(Debug, FromRow)]
struct PaperRow {
    id: String,
    title: String,
    composition_mode: String,
    generation_config_json: Option<String>,
    paper_status: String,
    subject_summary_text: String,
    preferred_template_id: Option<String>,
    export_content_mode: String,
    layout_json: Option<String>,
    layout_updated_at_ms: Option<i64>,
    row_version: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    saved_at_ms: Option<i64>,
    last_saved_at_ms: Option<i64>,
}

#[derive(Debug, FromRow)]
struct PaperItemRow {
    id: String,
    source_question_id: Option<String>,
    position: i64,
    snapshot_json: String,
}

#[derive(Debug, FromRow)]
struct PaperSummaryRow {
    id: String,
    title: String,
    composition_mode: String,
    paper_status: String,
    subject_summary_text: String,
    question_count: i64,
    row_version: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    saved_at_ms: Option<i64>,
    last_saved_at_ms: Option<i64>,
}

#[derive(Debug, FromRow)]
struct ExistingPaperRow {
    paper_status: String,
    row_version: i64,
    saved_at_ms: Option<i64>,
}

#[derive(Debug, FromRow)]
struct SourceQuestionRow {
    content_version: i64,
    exact_fingerprint: Vec<u8>,
}

#[derive(Debug, FromRow)]
struct CopyPaperRow {
    title: String,
    composition_mode: String,
    generation_config_json: Option<String>,
    subject_summary_text: String,
    preferred_template_id: Option<String>,
    export_content_mode: String,
    layout_json: Option<String>,
    layout_updated_at_ms: Option<i64>,
    row_version: i64,
}

#[derive(Debug, FromRow)]
struct CopyItemRow {
    source_question_id: Option<String>,
    position: i64,
    question_type: String,
    subject_id_snapshot: String,
    chapter_id_snapshot: String,
    tag_ids_snapshot_json: String,
    snapshot_schema_version: i64,
    snapshot_json: String,
    source_content_version: Option<i64>,
    source_exact_fingerprint: Option<Vec<u8>>,
}

pub async fn list_papers(
    pool: &SqlitePool,
    filters: &PaperFiltersApi,
) -> CommandResult<PageResultApi<PaperSummaryApi>> {
    validate_status_filter(&filters.status)?;
    let keyword = filters.keyword.trim().nfkc().collect::<String>();
    let pattern = format!("%{keyword}%");
    let page = filters.page.max(1);
    let page_size = filters.page_size.clamp(1, 500);
    let offset = i64::from(page.saturating_sub(1)) * i64::from(page_size);

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM papers \
         WHERE (? = 'all' OR paper_status = ?) AND (? = '' OR title LIKE ?)",
    )
    .bind(&filters.status)
    .bind(&filters.status)
    .bind(&keyword)
    .bind(&pattern)
    .fetch_one(pool)
    .await
    .map_err(CommandError::database)?;

    let rows = sqlx::query_as::<_, PaperSummaryRow>(
        "SELECT p.id, p.title, p.composition_mode, p.paper_status, p.subject_summary_text, \
                (SELECT COUNT(*) FROM paper_items pi WHERE pi.paper_id = p.id) AS question_count, \
                p.row_version, p.created_at_ms, p.updated_at_ms, p.saved_at_ms, p.last_saved_at_ms \
         FROM papers p \
         WHERE (? = 'all' OR p.paper_status = ?) AND (? = '' OR p.title LIKE ?) \
         ORDER BY CASE WHEN p.paper_status = 'saved' \
                  THEN COALESCE(p.saved_at_ms, p.updated_at_ms) ELSE p.updated_at_ms END DESC, p.id \
         LIMIT ? OFFSET ?",
    )
    .bind(&filters.status)
    .bind(&filters.status)
    .bind(&keyword)
    .bind(&pattern)
    .bind(i64::from(page_size))
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;

    Ok(PageResultApi {
        items: rows.into_iter().map(summary_from_row).collect(),
        total,
        page,
        page_size,
    })
}

pub async fn get_paper(pool: &SqlitePool, id: &str) -> CommandResult<Option<PaperApi>> {
    let row = sqlx::query_as::<_, PaperRow>(
        "SELECT id, title, composition_mode, generation_config_json, paper_status, subject_summary_text, \
                preferred_template_id, export_content_mode, layout_json, layout_updated_at_ms, row_version, created_at_ms, updated_at_ms, saved_at_ms, \
                last_saved_at_ms FROM papers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?;
    let Some(row) = row else {
        return Ok(None);
    };

    let item_rows = sqlx::query_as::<_, PaperItemRow>(
        "SELECT id, source_question_id, position, snapshot_json \
         FROM paper_items WHERE paper_id = ? ORDER BY position, id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;
    let items = item_rows
        .into_iter()
        .map(|item| {
            let snapshot =
                serde_json::from_str::<QuestionApi>(&item.snapshot_json).map_err(|error| {
                    CommandError::new(
                        "PAPER_SNAPSHOT_CORRUPTED",
                        format!("试卷题目快照无法读取：{error}"),
                    )
                })?;
            Ok(PaperItemApi {
                id: item.id,
                source_question_id: item.source_question_id,
                position: u32::try_from(item.position).unwrap_or_default(),
                snapshot,
            })
        })
        .collect::<CommandResult<Vec<_>>>()?;

    let generation_config = if row.composition_mode == "automatic" {
        parse_generation_config(row.generation_config_json.as_deref())?
    } else {
        None
    };
    let layout = row
        .layout_json
        .as_deref()
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()
        .map_err(|error| {
            CommandError::new(
                "PAPER_LAYOUT_CORRUPTED",
                format!("试卷排版 JSON 无法读取：{error}"),
            )
        })?;

    Ok(Some(PaperApi {
        id: row.id,
        title: row.title,
        composition_mode: row.composition_mode,
        generation_config,
        status: row.paper_status,
        items,
        export_content_mode: row.export_content_mode,
        subject_summary_text: row.subject_summary_text,
        preferred_template_id: row.preferred_template_id,
        layout,
        layout_updated_at: row.layout_updated_at_ms,
        row_version: row.row_version,
        created_at: row.created_at_ms,
        updated_at: row.updated_at_ms,
        saved_at: row.saved_at_ms,
        last_saved_at: row.last_saved_at_ms,
    }))
}

pub async fn save_paper(
    pool: &SqlitePool,
    paper: &PaperApi,
    app_version: &str,
) -> CommandResult<PaperApi> {
    let title = validate_paper(paper)?;
    let generation_config_json = generation_config_json(paper)?;
    let layout_json = layout_json(paper)?;
    let paper_id = if paper.id.trim().is_empty() {
        Uuid::now_v7().to_string()
    } else {
        validate_uuid(&paper.id, "试卷 ID")?;
        paper.id.clone()
    };
    let subject_summary = subject_summary(&paper.items);
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let existing = sqlx::query_as::<_, ExistingPaperRow>(
        "SELECT paper_status, row_version, saved_at_ms FROM papers WHERE id = ?",
    )
    .bind(&paper_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    let next_version;
    let saved_at;
    match existing {
        None => {
            if paper.row_version != 0 {
                return Err(CommandError::new(
                    "PAPER_NOT_FOUND",
                    "这份试卷已不存在，请返回试卷列表后重试。",
                ));
            }
            next_version = 1;
            saved_at = (paper.status == "saved").then_some(now);
            sqlx::query(
                "INSERT INTO papers (id, title, composition_mode, paper_status, generation_config_json, \
                 subject_summary_text, preferred_template_id, export_content_mode, proposed_export_filename, \
                 proposed_export_directory, layout_json, layout_updated_at_ms, row_version, created_at_ms, updated_at_ms, saved_at_ms, last_saved_at_ms) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, ?, 1, ?, ?, ?, ?)",
            )
            .bind(&paper_id)
            .bind(&title)
            .bind(&paper.composition_mode)
            .bind(&paper.status)
            .bind(generation_config_json.as_deref())
            .bind(&subject_summary)
            .bind(paper.preferred_template_id.as_deref())
            .bind(&paper.export_content_mode)
            .bind(layout_json.as_deref())
            .bind(layout_json.as_ref().map(|_| now))
            .bind(now)
            .bind(now)
            .bind(saved_at)
            .bind(now)
            .execute(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
        }
        Some(existing) => {
            if paper.row_version != existing.row_version {
                return Err(version_conflict());
            }
            if existing.paper_status == "saved" && paper.status == "draft" {
                return Err(CommandError::validation(
                    "已保存的历史试卷不能改回草稿；请使用“再次编辑”创建副本。",
                ));
            }
            next_version = existing.row_version.saturating_add(1);
            saved_at = if paper.status == "saved" {
                existing.saved_at_ms.or(Some(now))
            } else {
                None
            };
            let result = sqlx::query(
                "UPDATE papers SET title = ?, composition_mode = ?, paper_status = ?, \
                 generation_config_json = ?, subject_summary_text = ?, preferred_template_id = ?, export_content_mode = ?, layout_json = ?, \
                 layout_updated_at_ms = ?, row_version = row_version + 1, \
                 updated_at_ms = ?, saved_at_ms = ?, last_saved_at_ms = ? \
                 WHERE id = ? AND row_version = ?",
            )
            .bind(&title)
            .bind(&paper.composition_mode)
            .bind(&paper.status)
            .bind(generation_config_json.as_deref())
            .bind(&subject_summary)
            .bind(paper.preferred_template_id.as_deref())
            .bind(&paper.export_content_mode)
            .bind(layout_json.as_deref())
            .bind(layout_json.as_ref().map(|_| now))
            .bind(now)
            .bind(saved_at)
            .bind(now)
            .bind(&paper_id)
            .bind(paper.row_version)
            .execute(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
            if result.rows_affected() != 1 {
                return Err(version_conflict());
            }
            sqlx::query("DELETE FROM paper_items WHERE paper_id = ?")
                .bind(&paper_id)
                .execute(&mut *transaction)
                .await
                .map_err(CommandError::database)?;
        }
    }

    insert_items(&mut transaction, &paper_id, paper, now).await?;
    let details = serde_json::json!({
        "questionCount": paper.items.len(),
        "status": paper.status,
        "rowVersion": next_version,
    })
    .to_string();
    write_log(
        &mut transaction,
        now,
        "paper.save",
        &paper_id,
        "保存试卷及题目快照",
        &details,
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;

    get_paper(pool, &paper_id)
        .await?
        .ok_or_else(|| CommandError::database("保存后未能读取试卷"))
}

pub async fn copy_paper(
    pool: &SqlitePool,
    id: &str,
    base_row_version: i64,
    app_version: &str,
) -> CommandResult<PaperApi> {
    let now = now_millis();
    let new_id = Uuid::now_v7().to_string();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let source = sqlx::query_as::<_, CopyPaperRow>(
        "SELECT title, composition_mode, generation_config_json, subject_summary_text, \
                preferred_template_id, export_content_mode, layout_json, layout_updated_at_ms, row_version FROM papers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::new("PAPER_NOT_FOUND", "找不到要复制的试卷。"))?;
    if source.row_version != base_row_version {
        return Err(version_conflict());
    }
    let title = copy_title(&source.title);

    sqlx::query(
        "INSERT INTO papers (id, title, composition_mode, paper_status, generation_config_json, \
         subject_summary_text, preferred_template_id, export_content_mode, proposed_export_filename, \
         proposed_export_directory, layout_json, layout_updated_at_ms, row_version, created_at_ms, updated_at_ms, saved_at_ms, last_saved_at_ms) \
         VALUES (?, ?, ?, 'draft', ?, ?, ?, ?, NULL, NULL, ?, ?, 1, ?, ?, NULL, ?)",
    )
    .bind(&new_id)
    .bind(title)
    .bind(source.composition_mode)
    .bind(source.generation_config_json)
    .bind(source.subject_summary_text)
    .bind(source.preferred_template_id)
    .bind(source.export_content_mode)
    .bind(source.layout_json)
    .bind(source.layout_updated_at_ms)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    let items = sqlx::query_as::<_, CopyItemRow>(
        "SELECT source_question_id, position, question_type, subject_id_snapshot, \
                chapter_id_snapshot, tag_ids_snapshot_json, snapshot_schema_version, snapshot_json, \
                source_content_version, source_exact_fingerprint \
         FROM paper_items WHERE paper_id = ? ORDER BY position, id",
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    for item in items {
        let snapshot =
            serde_json::from_str::<QuestionApi>(&item.snapshot_json).map_err(|error| {
                CommandError::new(
                    "PAPER_SNAPSHOT_CORRUPTED",
                    format!("复制来源中的题目快照无法读取：{error}"),
                )
            })?;
        validate_snapshot_resource_refs(&mut transaction, &snapshot).await?;
        let new_item_id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO paper_items (id, paper_id, source_question_id, position, question_type, \
             subject_id_snapshot, chapter_id_snapshot, tag_ids_snapshot_json, snapshot_schema_version, \
             snapshot_json, source_content_version, source_exact_fingerprint, usage_recorded_at_ms, \
             created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)",
        )
        .bind(&new_item_id)
        .bind(&new_id)
        .bind(item.source_question_id)
        .bind(item.position)
        .bind(item.question_type)
        .bind(item.subject_id_snapshot)
        .bind(item.chapter_id_snapshot)
        .bind(item.tag_ids_snapshot_json)
        .bind(item.snapshot_schema_version)
        .bind(item.snapshot_json)
        .bind(item.source_content_version)
        .bind(item.source_exact_fingerprint)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
        insert_paper_item_resource_refs(
            &mut transaction,
            &new_item_id,
            &snapshot.resource_refs,
            now,
        )
        .await?;
    }
    let details = serde_json::json!({ "sourcePaperId": id }).to_string();
    write_log(
        &mut transaction,
        now,
        "paper.copy",
        &new_id,
        "复制试卷为可编辑草稿",
        &details,
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;

    get_paper(pool, &new_id)
        .await?
        .ok_or_else(|| CommandError::database("复制后未能读取试卷"))
}

pub async fn delete_paper(
    pool: &SqlitePool,
    id: &str,
    base_row_version: i64,
    app_version: &str,
) -> CommandResult<()> {
    delete_papers(
        pool,
        &[PaperDeleteRequestApi {
            id: id.to_owned(),
            base_row_version,
        }],
        app_version,
    )
    .await
}

pub async fn delete_papers(
    pool: &SqlitePool,
    requests: &[PaperDeleteRequestApi],
    app_version: &str,
) -> CommandResult<()> {
    if requests.is_empty() {
        return Ok(());
    }
    if requests.len() > MAX_DELETE_BATCH {
        return Err(CommandError::validation(format!(
            "一次最多删除 {MAX_DELETE_BATCH} 份试卷。"
        )));
    }

    let mut seen_ids = HashSet::with_capacity(requests.len());
    for request in requests {
        validate_uuid(&request.id, "试卷 ID")?;
        if !seen_ids.insert(request.id.as_str()) {
            return Err(CommandError::validation("批量删除中包含重复的试卷。"));
        }
    }

    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    for request in requests {
        let current_version =
            sqlx::query_scalar::<_, i64>("SELECT row_version FROM papers WHERE id = ?")
                .bind(&request.id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(CommandError::database)?
                .ok_or_else(|| CommandError::new("PAPER_NOT_FOUND", "找不到要删除的试卷。"))?;
        if current_version != request.base_row_version {
            return Err(version_conflict());
        }
    }

    let details = serde_json::json!({ "batchSize": requests.len() }).to_string();
    for request in requests {
        let result = sqlx::query("DELETE FROM papers WHERE id = ? AND row_version = ?")
            .bind(&request.id)
            .bind(request.base_row_version)
            .execute(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
        if result.rows_affected() != 1 {
            return Err(version_conflict());
        }
        write_log(
            &mut transaction,
            now,
            "paper.delete",
            &request.id,
            "永久删除试卷及其快照",
            &details,
            app_version,
        )
        .await?;
    }
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

async fn insert_items(
    transaction: &mut Transaction<'_, Sqlite>,
    paper_id: &str,
    paper: &PaperApi,
    now: i64,
) -> CommandResult<()> {
    for (position, item) in paper.items.iter().enumerate() {
        validate_snapshot_resource_refs(transaction, &item.snapshot).await?;
        let snapshot_json =
            serde_json::to_string(&item.snapshot).map_err(CommandError::database)?;
        let tag_ids = item
            .snapshot
            .tags
            .iter()
            .map(|tag| tag.id.clone())
            .collect::<Vec<_>>();
        let tag_ids_json = serde_json::to_string(&tag_ids).map_err(CommandError::database)?;
        let source = if let Some(source_id) = item.source_question_id.as_deref() {
            sqlx::query_as::<_, SourceQuestionRow>(
                "SELECT content_version, exact_fingerprint FROM questions WHERE id = ?",
            )
            .bind(source_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(CommandError::database)?
        } else {
            None
        };
        let source_id = source.as_ref().and(item.source_question_id.as_deref());
        let source_fingerprint = source.as_ref().and_then(|source| {
            (source.content_version == item.snapshot.content_version)
                .then_some(source.exact_fingerprint.as_slice())
        });
        let usage_recorded_at = (paper.status == "saved" && source_id.is_some()).then_some(now);
        let snapshot_schema_version = item
            .snapshot
            .options
            .iter()
            .map(|option| option.content.schema_version)
            .chain([
                item.snapshot.stem.schema_version,
                item.snapshot.answer.schema_version,
                item.snapshot.explanation.schema_version,
            ])
            .max()
            .unwrap_or(1);

        sqlx::query(
            "INSERT INTO paper_items (id, paper_id, source_question_id, position, question_type, \
             subject_id_snapshot, chapter_id_snapshot, tag_ids_snapshot_json, snapshot_schema_version, \
             snapshot_json, source_content_version, source_exact_fingerprint, usage_recorded_at_ms, \
             created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&item.id)
        .bind(paper_id)
        .bind(source_id)
        .bind(i64::try_from(position).unwrap_or(i64::MAX))
        .bind(&item.snapshot.question_type)
        .bind(&item.snapshot.subject_id)
        .bind(&item.snapshot.chapter_id)
        .bind(tag_ids_json)
        .bind(i64::from(snapshot_schema_version))
        .bind(snapshot_json)
        .bind(source.as_ref().map(|_| item.snapshot.content_version))
        .bind(source_fingerprint)
        .bind(usage_recorded_at)
        .bind(now)
        .bind(now)
        .execute(&mut **transaction)
        .await
        .map_err(CommandError::database)?;

        insert_paper_item_resource_refs(transaction, &item.id, &item.snapshot.resource_refs, now)
            .await?;

        if paper.status == "saved"
            && let Some(source_id) = source_id
        {
            sqlx::query("UPDATE questions SET last_used_at_ms = ? WHERE id = ?")
                .bind(now)
                .bind(source_id)
                .execute(&mut **transaction)
                .await
                .map_err(CommandError::database)?;
        }
    }
    Ok(())
}

async fn validate_snapshot_resource_refs(
    transaction: &mut Transaction<'_, Sqlite>,
    snapshot: &QuestionApi,
) -> CommandResult<()> {
    let option_ids = snapshot
        .options
        .iter()
        .map(|option| option.id.clone())
        .collect::<HashSet<_>>();
    resources::validate_resource_refs(transaction, &snapshot.resource_refs, &option_ids).await
}

async fn insert_paper_item_resource_refs(
    transaction: &mut Transaction<'_, Sqlite>,
    paper_item_id: &str,
    refs: &[super::models::QuestionResourceRefApi],
    now: i64,
) -> CommandResult<()> {
    for resource_ref in refs {
        sqlx::query(
            "INSERT INTO paper_item_resource_refs \
                 (id, paper_item_id, resource_id, node_id, created_at_ms) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(paper_item_id)
        .bind(&resource_ref.resource_id)
        .bind(&resource_ref.node_id)
        .bind(now)
        .execute(&mut **transaction)
        .await
        .map_err(CommandError::database)?;
    }
    Ok(())
}

fn validate_paper(paper: &PaperApi) -> CommandResult<String> {
    let title = paper.title.trim().nfkc().collect::<String>();
    let title = title.trim().to_owned();
    if title.is_empty() {
        return Err(CommandError::validation("试卷名称不能为空。"));
    }
    if title.chars().count() > TITLE_MAX {
        return Err(CommandError::validation(format!(
            "试卷名称不能超过 {TITLE_MAX} 个字符。"
        )));
    }
    if !matches!(paper.composition_mode.as_str(), "manual" | "automatic") {
        return Err(CommandError::validation("组卷方式不受支持。"));
    }
    validate_generation_config(paper)?;
    if !matches!(paper.status.as_str(), "draft" | "saved") {
        return Err(CommandError::validation("试卷状态不受支持。"));
    }
    if !EXPORT_CONTENT_MODES.contains(&paper.export_content_mode.as_str()) {
        return Err(CommandError::validation("试卷内容模式不受支持。"));
    }
    if let Some(template_id) = paper.preferred_template_id.as_deref() {
        validate_uuid(template_id, "试卷模板 ID")?;
    }
    if paper.row_version < 0 {
        return Err(CommandError::validation("试卷版本号无效。"));
    }
    if paper.items.len() > MAX_ITEMS {
        return Err(CommandError::validation(format!(
            "一份试卷最多包含 {MAX_ITEMS} 道题。"
        )));
    }
    validate_layout(paper.layout.as_ref())?;
    if paper.status == "saved" && paper.items.is_empty() {
        return Err(CommandError::validation(
            "保存到历史试卷前，请至少加入一道题。",
        ));
    }

    let mut item_ids = HashSet::with_capacity(paper.items.len());
    let mut source_ids = HashSet::with_capacity(paper.items.len());
    let mut total_bytes = 0usize;
    for (position, item) in paper.items.iter().enumerate() {
        validate_uuid(&item.id, "试卷题目 ID")?;
        if !item_ids.insert(item.id.as_str()) {
            return Err(CommandError::validation("试卷中存在重复的题目快照 ID。"));
        }
        if usize::try_from(item.position).unwrap_or(usize::MAX) != position {
            return Err(CommandError::validation(
                "试卷题目顺序不连续，请重新排序后保存。",
            ));
        }
        if item.snapshot.question_type.trim().is_empty() || item.snapshot.question_type.len() > 64 {
            return Err(CommandError::validation("试卷中包含不受支持的题型。"));
        }
        validate_uuid(&item.snapshot.subject_id, "快照学科 ID")?;
        validate_uuid(&item.snapshot.chapter_id, "快照章节 ID")?;
        if let Some(source_id) = item.source_question_id.as_deref() {
            validate_uuid(source_id, "来源题目 ID")?;
            if source_id != item.snapshot.id {
                return Err(CommandError::validation("题目快照与来源题目不匹配。"));
            }
            if !source_ids.insert(source_id) {
                return Err(CommandError::validation(
                    "同一道来源题目不能重复加入一份试卷。",
                ));
            }
        }
        let snapshot_bytes = serde_json::to_vec(&item.snapshot)
            .map_err(CommandError::database)?
            .len();
        if snapshot_bytes > MAX_SNAPSHOT_BYTES {
            return Err(CommandError::validation("单道题目的快照内容过大。"));
        }
        total_bytes = total_bytes.saturating_add(snapshot_bytes);
        if total_bytes > MAX_TOTAL_SNAPSHOT_BYTES {
            return Err(CommandError::validation("试卷题目快照总大小超过安全限制。"));
        }
    }
    Ok(title)
}

fn validate_layout(layout: Option<&serde_json::Value>) -> CommandResult<()> {
    let Some(layout) = layout else {
        return Ok(());
    };
    let serialized = serde_json::to_vec(layout).map_err(CommandError::database)?;
    if serialized.len() > MAX_LAYOUT_BYTES {
        return Err(CommandError::validation(
            "试卷排版数据超过 32 MB 的安全上限。",
        ));
    }
    if layout
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
        || layout.get("editor").and_then(serde_json::Value::as_str) != Some("canvas-editor")
    {
        return Err(CommandError::validation("试卷排版格式或版本不受支持。"));
    }
    let editor_version = layout
        .get("editorVersion")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim();
    if editor_version.is_empty() || editor_version.chars().count() > 64 {
        return Err(CommandError::validation("试卷排版编辑器版本无效。"));
    }
    let source_signature = layout
        .get("sourceSignature")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if source_signature.is_empty() || source_signature.chars().count() > 4_096 {
        return Err(CommandError::validation("试卷排版来源签名无效。"));
    }
    let data = layout.get("data").and_then(serde_json::Value::as_object);
    if data
        .and_then(|data| data.get("main"))
        .and_then(serde_json::Value::as_array)
        .is_none()
    {
        return Err(CommandError::validation("试卷排版缺少正文数据。"));
    }
    if !layout
        .get("options")
        .is_some_and(serde_json::Value::is_object)
    {
        return Err(CommandError::validation("试卷排版选项无效。"));
    }
    if let Some(page_setup) = layout.get("pageSetup").filter(|value| !value.is_null()) {
        let width = page_setup
            .get("widthMm")
            .and_then(serde_json::Value::as_f64);
        let height = page_setup
            .get("heightMm")
            .and_then(serde_json::Value::as_f64);
        if !width.is_some_and(|value| value.is_finite() && (90.0..=600.0).contains(&value))
            || !height.is_some_and(|value| value.is_finite() && (90.0..=600.0).contains(&value))
        {
            return Err(CommandError::validation(
                "试卷排版纸张宽度和高度必须在 90～600 毫米之间。",
            ));
        }
    }
    Ok(())
}

fn validate_generation_config(paper: &PaperApi) -> CommandResult<()> {
    let Some(config) = paper.generation_config.as_ref() else {
        // 兼容旧自动试卷：早期版本没有把已预留的组卷条件字段写入数据库。
        return Ok(());
    };
    if paper.composition_mode != "automatic" {
        return Err(CommandError::validation("手动组卷不能保留自动组卷条件。"));
    }
    if let Some(subject_id) = config.subject_id.as_ref() {
        validate_uuid(subject_id.as_str(), "自动组卷学科 ID")?;
    }
    if config.chapter_ids.len() > MAX_GENERATION_CHAPTERS {
        return Err(CommandError::validation("自动组卷章节数量超过安全上限。"));
    }
    let mut chapter_ids = HashSet::with_capacity(config.chapter_ids.len());
    for chapter_id in &config.chapter_ids {
        validate_uuid(chapter_id.as_str(), "自动组卷章节 ID")?;
        if !chapter_ids.insert(chapter_id.as_str()) {
            return Err(CommandError::validation("自动组卷章节不能重复。"));
        }
    }
    let total = config
        .question_type_counts
        .values()
        .try_fold(0usize, |sum, count| {
            let count = usize::try_from(*count).unwrap_or(usize::MAX);
            if count > MAX_ITEMS {
                return Err(CommandError::validation(
                    "单个题型的自动组卷题量不能超过 1000。",
                ));
            }
            Ok(sum.saturating_add(count))
        })?;
    if total > MAX_ITEMS {
        return Err(CommandError::validation("自动组卷总题量不能超过 1000。"));
    }
    Ok(())
}

fn generation_config_json(paper: &PaperApi) -> CommandResult<Option<String>> {
    if paper.composition_mode != "automatic" {
        return Ok(None);
    }
    paper
        .generation_config
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(CommandError::database)
}

fn layout_json(paper: &PaperApi) -> CommandResult<Option<String>> {
    paper
        .layout
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(CommandError::database)
}

fn parse_generation_config(
    value: Option<&str>,
) -> CommandResult<Option<AutomaticGenerationConfigDto>> {
    let Some(value) = value else {
        return Ok(None);
    };
    serde_json::from_str::<Option<AutomaticGenerationConfigDto>>(value).map_err(|error| {
        CommandError::new(
            "PAPER_GENERATION_CONFIG_CORRUPTED",
            format!("自动组卷条件无法读取：{error}"),
        )
    })
}

fn subject_summary(items: &[PaperItemApi]) -> String {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for item in items {
        let name = item.snapshot.subject_name.trim();
        if !name.is_empty() && seen.insert(name.to_owned()) {
            names.push(name.to_owned());
        }
    }
    names.join("、")
}

fn validate_status_filter(status: &str) -> CommandResult<()> {
    if matches!(status, "all" | "draft" | "saved") {
        Ok(())
    } else {
        Err(CommandError::validation("试卷列表状态筛选无效。"))
    }
}

fn validate_uuid(value: &str, label: &str) -> CommandResult<()> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| CommandError::validation(format!("{label} 无效。")))
}

fn copy_title(title: &str) -> String {
    let suffix = "（副本）";
    let keep = TITLE_MAX.saturating_sub(suffix.chars().count());
    let mut copied = title.chars().take(keep).collect::<String>();
    copied.push_str(suffix);
    copied
}

fn summary_from_row(row: PaperSummaryRow) -> PaperSummaryApi {
    PaperSummaryApi {
        id: row.id,
        title: row.title,
        composition_mode: row.composition_mode,
        status: row.paper_status,
        subject_summary_text: row.subject_summary_text,
        question_count: row.question_count,
        row_version: row.row_version,
        created_at: row.created_at_ms,
        updated_at: row.updated_at_ms,
        saved_at: row.saved_at_ms,
        last_saved_at: row.last_saved_at_ms,
    }
}

fn version_conflict() -> CommandError {
    CommandError::new(
        "PAPER_VERSION_CONFLICT",
        "这份试卷已在其他窗口中更新，请重新打开后再操作。",
    )
}

async fn write_log(
    transaction: &mut Transaction<'_, Sqlite>,
    now: i64,
    action: &str,
    entity_id: &str,
    summary: &str,
    details_json: &str,
    app_version: &str,
) -> CommandResult<()> {
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, entity_id, outcome, \
         summary, details_json, app_version) VALUES (?, ?, 'info', ?, 'paper', ?, 'success', ?, ?, ?)",
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
    use std::fs;

    use crate::{
        api::models::{PaperApi, PaperDeleteRequestApi},
        db::Database,
    };

    use super::{delete_papers, get_paper, parse_generation_config, save_paper, validate_layout};

    fn empty_paper(title: &str) -> PaperApi {
        PaperApi {
            id: uuid::Uuid::now_v7().to_string(),
            title: title.to_owned(),
            composition_mode: "manual".to_owned(),
            generation_config: None,
            status: "draft".to_owned(),
            items: Vec::new(),
            export_content_mode: "paper_only".to_owned(),
            subject_summary_text: String::new(),
            preferred_template_id: None,
            layout: None,
            layout_updated_at: None,
            row_version: 0,
            created_at: 0,
            updated_at: 0,
            saved_at: None,
            last_saved_at: None,
        }
    }

    #[test]
    fn generation_config_accepts_camel_case_and_legacy_snake_case() {
        let subject_id = "019f7d00-0000-7000-8000-000000000001";
        let chapter_id = "019f7d00-0000-7000-8000-000000000002";
        for json in [
            format!(
                r#"{{"subjectId":"{subject_id}","chapterIds":["{chapter_id}"],"questionTypeCounts":{{"single_choice":2}}}}"#
            ),
            format!(
                r#"{{"subject_id":"{subject_id}","chapter_ids":["{chapter_id}"],"question_type_counts":{{"single_choice":2}}}}"#
            ),
        ] {
            let config = parse_generation_config(Some(&json))
                .expect("configuration should parse")
                .expect("configuration should be present");
            assert_eq!(config.subject_id.as_deref(), Some(subject_id));
            assert_eq!(config.chapter_ids.len(), 1);
            assert_eq!(config.chapter_ids[0].as_str(), chapter_id);
            assert_eq!(
                config.question_type_counts.values().copied().sum::<u32>(),
                2
            );
        }
    }

    #[test]
    fn generation_config_keeps_legacy_null_values_compatible() {
        assert!(parse_generation_config(None).unwrap().is_none());
        assert!(parse_generation_config(Some("null")).unwrap().is_none());
        assert!(parse_generation_config(Some("{")).is_err());
    }

    #[test]
    fn canvas_editor_layout_requires_versioned_native_data() {
        let valid = serde_json::json!({
            "schemaVersion": 1,
            "editor": "canvas-editor",
            "editorVersion": "0.9.137",
            "sourceSignature": "paper-v1:1:12345678",
            "data": { "main": [{ "value": "试卷" }] },
            "options": { "width": 794, "height": 1123 },
            "pageSetup": { "widthMm": 420, "heightMm": 297 }
        });
        assert!(validate_layout(Some(&valid)).is_ok());

        let invalid = serde_json::json!({
            "schemaVersion": 1,
            "editor": "unknown",
            "editorVersion": "1",
            "sourceSignature": "x",
            "data": { "main": [] },
            "options": {}
        });
        assert!(validate_layout(Some(&invalid)).is_err());

        let invalid_page = serde_json::json!({
            "schemaVersion": 1,
            "editor": "canvas-editor",
            "editorVersion": "0.9.137",
            "sourceSignature": "paper-v1:1:12345678",
            "data": { "main": [] },
            "options": {},
            "pageSetup": { "widthMm": 10, "heightMm": 297 }
        });
        assert!(validate_layout(Some(&invalid_page)).is_err());
    }

    #[tokio::test]
    async fn sqlite_round_trip_preserves_canvas_editor_layout() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-paper-layout-{}",
            uuid::Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let template_id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO word_templates (
                id, name, name_key, file_rel_path, file_sha256, file_byte_size,
                analysis_status, analysis_schema_version, analysis_json, parser_version,
                row_version, created_at_ms, updated_at_ms, last_verified_at_ms
             ) VALUES (?, '测试模板', '测试模板', '00000000-0000-7000-8000-000000000000.docx',
                       zeroblob(32), 0, 'ready', 1, '{}', 'test', 1, 0, 0, NULL)",
        )
        .bind(&template_id)
        .execute(database.pool())
        .await
        .unwrap();
        let layout = serde_json::json!({
            "schemaVersion": 1,
            "editor": "canvas-editor",
            "editorVersion": "0.9.137",
            "sourceSignature": "paper-v1:0:12345678",
            "data": { "main": [{ "value": "测试排版" }] },
            "options": { "width": 794, "height": 1123 }
        });
        let paper = PaperApi {
            id: uuid::Uuid::now_v7().to_string(),
            title: "排版持久化测试".to_owned(),
            composition_mode: "manual".to_owned(),
            generation_config: None,
            status: "draft".to_owned(),
            items: Vec::new(),
            export_content_mode: "paper_only".to_owned(),
            subject_summary_text: String::new(),
            preferred_template_id: Some(template_id.clone()),
            layout: Some(layout.clone()),
            layout_updated_at: None,
            row_version: 0,
            created_at: 0,
            updated_at: 0,
            saved_at: None,
            last_saved_at: None,
        };

        let saved = save_paper(database.pool(), &paper, "test").await.unwrap();
        assert_eq!(saved.layout, Some(layout));
        assert_eq!(saved.preferred_template_id, Some(template_id));
        assert!(saved.layout_updated_at.is_some());
        database.close().await;
        drop(database);
        let retry_delays_ms = [20, 50, 100, 200, 400, 800, 1_000, 1_500];
        for (attempt, delay_ms) in retry_delays_ms.into_iter().enumerate() {
            match fs::remove_dir_all(&root) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(error)
                    if matches!(error.raw_os_error(), Some(32 | 33))
                        && attempt + 1 < retry_delays_ms.len() =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
                Err(error) => panic!("failed to clean paper test directory: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn batch_delete_is_atomic_when_a_version_conflicts() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-paper-batch-delete-{}",
            uuid::Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let first = save_paper(database.pool(), &empty_paper("第一份"), "test")
            .await
            .unwrap();
        let second = save_paper(database.pool(), &empty_paper("第二份"), "test")
            .await
            .unwrap();

        let conflicted = vec![
            PaperDeleteRequestApi {
                id: first.id.clone(),
                base_row_version: first.row_version,
            },
            PaperDeleteRequestApi {
                id: second.id.clone(),
                base_row_version: second.row_version + 1,
            },
        ];
        assert!(
            delete_papers(database.pool(), &conflicted, "test")
                .await
                .is_err()
        );
        assert!(
            get_paper(database.pool(), &first.id)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            get_paper(database.pool(), &second.id)
                .await
                .unwrap()
                .is_some()
        );

        let valid = vec![
            PaperDeleteRequestApi {
                id: first.id.clone(),
                base_row_version: first.row_version,
            },
            PaperDeleteRequestApi {
                id: second.id.clone(),
                base_row_version: second.row_version,
            },
        ];
        delete_papers(database.pool(), &valid, "test")
            .await
            .unwrap();
        assert!(
            get_paper(database.pool(), &first.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            get_paper(database.pool(), &second.id)
                .await
                .unwrap()
                .is_none()
        );

        database.close().await;
        drop(database);
        for attempt in 0..20 {
            match fs::remove_dir_all(&root) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(_) if attempt < 19 => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(error) => panic!("failed to clean paper batch delete test directory: {error}"),
            }
        }
    }
}
