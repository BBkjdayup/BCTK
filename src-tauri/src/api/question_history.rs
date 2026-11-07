use super::models::{
    CommandError, CommandResult, QuestionApi, QuestionDraftApi, QuestionFiltersApi,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection, SqlitePool};
use uuid::Uuid;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
pub(super) async fn capture(tx: &mut SqliteConnection, id: &str) -> CommandResult<()> {
    let snapshot:Option<String>=sqlx::query_scalar("SELECT json_object('id',q.id,'type',q.question_type,'stem',json(q.stem_json),'answer',json(q.answer_json),'explanation',json(q.explanation_json),
      'subjectId',q.subject_id,'chapterId',q.chapter_id,'subjectName',s.name,'chapterName',c.name,'unitId',q.unit_id,'unitName',(SELECT name FROM units WHERE id=q.unit_id),'contentVersion',q.content_version,
      'lastUsedAt',q.last_used_at_ms,'deletedAt',q.deleted_at_ms,'createdAt',q.created_at_ms,'updatedAt',q.updated_at_ms,
      'options',json((SELECT json_group_array(json(o.obj)) FROM (SELECT json_object('id',id,'position',position,'content',json(content_json)) obj FROM question_options WHERE question_id=q.id ORDER BY position) o)),
      'tags',json((SELECT json_group_array(json_object('id',t.id,'name',t.name,'questionCount',0,'createdAt',t.created_at_ms,'updatedAt',t.updated_at_ms)) FROM question_tags qt JOIN tags t ON t.id=qt.tag_id WHERE qt.question_id=q.id)),
      'resourceRefs',json((SELECT json_group_array(json_object('nodeId',node_id,'resourceId',resource_id,'contentSlot',content_slot,'optionId',option_id)) FROM question_resource_refs WHERE question_id=q.id)))
      FROM questions q JOIN subjects s ON s.id=q.subject_id JOIN chapters c ON c.id=q.chapter_id WHERE q.id=?")
      .bind(id).fetch_optional(&mut *tx).await.map_err(CommandError::database)?;
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    let question: QuestionApi = serde_json::from_str(&snapshot).map_err(CommandError::database)?;
    let history = Uuid::now_v7().to_string();
    let result=sqlx::query("INSERT OR IGNORE INTO question_history(id,question_id,content_version,saved_at_ms,snapshot_json) VALUES(?,?,?,?,?)")
      .bind(&history).bind(id).bind(question.content_version).bind(question.updated_at).bind(snapshot).execute(&mut *tx).await.map_err(CommandError::database)?;
    if result.rows_affected() > 0 {
        sqlx::query("INSERT INTO question_history_resource_refs(history_id,resource_id) SELECT ?,resource_id FROM question_resource_refs WHERE question_id=? GROUP BY resource_id")
          .bind(&history).bind(id).execute(&mut *tx).await.map_err(CommandError::database)?;
        // A bounded local history, retained by version rather than idle time.
        sqlx::query("DELETE FROM question_history WHERE question_id=? AND id NOT IN(SELECT id FROM question_history WHERE question_id=? ORDER BY content_version DESC LIMIT 50)")
          .bind(id).bind(id).execute(&mut *tx).await.map_err(CommandError::database)?;
    }
    Ok(())
}
pub async fn list(pool: &SqlitePool, id: &str) -> CommandResult<Vec<Value>> {
    let rows=sqlx::query("SELECT id,content_version,saved_at_ms FROM question_history WHERE question_id=? ORDER BY content_version DESC LIMIT 50")
      .bind(id).fetch_all(pool).await.map_err(CommandError::database)?;
    Ok(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"contentVersion":r.get::<i64,_>("content_version"),"savedAt":r.get::<i64,_>("saved_at_ms")})).collect())
}
pub async fn detail(pool: &SqlitePool, id: &str) -> CommandResult<QuestionApi> {
    let value: String = sqlx::query_scalar("SELECT snapshot_json FROM question_history WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(CommandError::database)?
        .ok_or_else(|| CommandError::validation("历史版本不存在或已超出保留范围"))?;
    serde_json::from_str(&value).map_err(CommandError::database)
}
pub async fn restore(
    pool: &SqlitePool,
    id: &str,
    expected: i64,
    app_version: &str,
) -> CommandResult<QuestionApi> {
    let old = detail(pool, id).await?;
    let draft = QuestionDraftApi {
        answer_review_required: false,
        id: None,
        question_id: Some(old.id),
        question_type: old.question_type,
        stem: old.stem,
        options: old.options,
        answer: old.answer,
        explanation: old.explanation,
        subject_id: old.subject_id,
        chapter_id: old.chapter_id,
        unit_id: old.unit_id,
        tag_ids: old.tags.into_iter().map(|t| t.id).collect(),
        resource_refs: old.resource_refs,
        base_content_version: Some(expected),
    };
    // The normal save transaction validates the current version, taxonomy and
    // media, and records a new version. Existing paper snapshots are untouched.
    super::questions::save_question(pool, &draft, app_version).await
}

#[derive(Serialize, Deserialize)]
pub(super) struct BatchBefore {
    pub id: String,
    pub version: i64,
    pub subject: String,
    pub chapter: String,
    #[serde(default)]
    pub unit: Option<String>,
    pub tags: Vec<String>,
    pub used: Option<i64>,
    pub after_used: Option<i64>,
}
pub(super) async fn before(
    tx: &mut SqliteConnection,
    id: &str,
    reset_usage: bool,
) -> CommandResult<BatchBefore> {
    let row = sqlx::query(
        "SELECT content_version,subject_id,chapter_id,unit_id,last_used_at_ms FROM questions WHERE id=?",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(CommandError::database)?;
    let used = row.get("last_used_at_ms");
    let tags =
        sqlx::query_scalar("SELECT tag_id FROM question_tags WHERE question_id=? ORDER BY tag_id")
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(CommandError::database)?;
    Ok(BatchBefore {
        id: id.into(),
        version: row.get("content_version"),
        subject: row.get("subject_id"),
        chapter: row.get("chapter_id"),
        unit: row.get("unit_id"),
        tags,
        used,
        after_used: if reset_usage { None } else { used },
    })
}
pub(super) async fn record_batch(
    tx: &mut SqliteConnection,
    items: &[BatchBefore],
) -> CommandResult<String> {
    let id = Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO question_batch_edits(id,created_at_ms,undo_json) VALUES(?,?,?)")
        .bind(&id)
        .bind(now())
        .bind(serde_json::to_string(items).map_err(CommandError::database)?)
        .execute(&mut *tx)
        .await
        .map_err(CommandError::database)?;
    Ok(id)
}
pub async fn batches(pool: &SqlitePool) -> CommandResult<Vec<Value>> {
    let rows=sqlx::query("SELECT id,created_at_ms,json_array_length(undo_json) count FROM question_batch_edits WHERE undone_at_ms IS NULL ORDER BY created_at_ms DESC,id DESC LIMIT 20").fetch_all(pool).await.map_err(CommandError::database)?;
    Ok(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"createdAt":r.get::<i64,_>("created_at_ms"),"count":r.get::<i64,_>("count")})).collect())
}
pub async fn undo(pool: &SqlitePool, id: &str, app_version: &str) -> CommandResult<()> {
    let mut tx = pool.begin().await.map_err(CommandError::database)?;
    let value: Option<String> = sqlx::query_scalar(
        "SELECT undo_json FROM question_batch_edits WHERE id=? AND undone_at_ms IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(CommandError::database)?;
    let items: Vec<BatchBefore> = serde_json::from_str(
        &value.ok_or_else(|| CommandError::validation("该批次不存在或已经撤销"))?,
    )
    .map_err(CommandError::database)?;
    // Validate the entire batch first, including usage changes that do not
    // increment content_version (for example adding a question to a paper).
    for item in &items {
        let current = sqlx::query(
            "SELECT content_version,deleted_at_ms,last_used_at_ms FROM questions WHERE id=?",
        )
        .bind(&item.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(CommandError::database)?;
        let conflict = || {
            CommandError::new(
                "CONTENT_CONFLICT",
                "批次中的题目之后已被修改、使用或删除，不能覆盖后续操作；本次撤销未执行。",
            )
        };
        let row = current.ok_or_else(conflict)?;
        if row.get::<i64, _>("content_version") != item.version + 1
            || row.get::<Option<i64>, _>("deleted_at_ms").is_some()
            || row.get::<Option<i64>, _>("last_used_at_ms") != item.after_used
        {
            return Err(conflict());
        }
        let valid: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chapters WHERE id=? AND subject_id=?")
                .bind(&item.chapter)
                .bind(&item.subject)
                .fetch_one(&mut *tx)
                .await
                .map_err(CommandError::database)?;
        if valid != 1 {
            return Err(CommandError::validation(
                "原学科或章节已删除，不能恢复该批次",
            ));
        }
        super::taxonomy::validate_unit(&mut tx, &item.chapter, item.unit.as_deref()).await?;
        for tag in &item.tags {
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tags WHERE id=?")
                .bind(tag)
                .fetch_one(&mut *tx)
                .await
                .map_err(CommandError::database)?;
            if exists != 1 {
                return Err(CommandError::validation("原标签已删除，不能恢复该批次"));
            }
        }
    }
    for item in &items {
        capture(&mut tx, &item.id).await?;
        sqlx::query("DELETE FROM question_tags WHERE question_id=?")
            .bind(&item.id)
            .execute(&mut *tx)
            .await
            .map_err(CommandError::database)?;
        for tag in &item.tags {
            sqlx::query(
                "INSERT INTO question_tags(question_id,tag_id,created_at_ms) VALUES(?,?,?)",
            )
            .bind(&item.id)
            .bind(tag)
            .bind(now())
            .execute(&mut *tx)
            .await
            .map_err(CommandError::database)?;
        }
        sqlx::query("UPDATE questions SET subject_id=?,chapter_id=?,unit_id=?,last_used_at_ms=?,tags_plain=(SELECT COALESCE(group_concat(name,' '),'') FROM tags WHERE id IN(SELECT tag_id FROM question_tags WHERE question_id=?)),content_version=content_version+1,updated_at_ms=? WHERE id=?")
          .bind(&item.subject).bind(&item.chapter).bind(&item.unit).bind(item.used).bind(&item.id).bind(now()).bind(&item.id).execute(&mut *tx).await.map_err(CommandError::database)?;
        capture(&mut tx, &item.id).await?;
    }
    sqlx::query("UPDATE question_batch_edits SET undone_at_ms=? WHERE id=?")
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(CommandError::database)?;
    sqlx::query("INSERT INTO operation_logs(id,occurred_at_ms,level,action,entity_type,entity_id,outcome,summary,details_json,app_version) VALUES(?,?,'info','question.batch_undo','question',?,'success','撤销批量编辑',?,?)")
      .bind(Uuid::now_v7().to_string()).bind(now()).bind(id).bind(json!({"count":items.len()}).to_string()).bind(app_version).execute(&mut *tx).await.map_err(CommandError::database)?;
    tx.commit().await.map_err(CommandError::database)
}

pub async fn filters(pool: &SqlitePool) -> CommandResult<Vec<Value>> {
    let rows=sqlx::query("SELECT id,name,filters_json FROM saved_question_filters ORDER BY created_at_ms DESC,id DESC LIMIT 100").fetch_all(pool).await.map_err(CommandError::database)?;
    rows.iter().map(|r|Ok(json!({"id":r.get::<String,_>("id"),"name":r.get::<String,_>("name"),"filters":serde_json::from_str::<Value>(r.get::<&str,_>("filters_json")).map_err(CommandError::database)?}))).collect()
}
pub async fn save_filter(
    pool: &SqlitePool,
    name: &str,
    mut filters: QuestionFiltersApi,
) -> CommandResult<()> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > 40
        || filters.keyword.chars().count() > 500
        || filters.tag_ids.len() > 100
    {
        return Err(CommandError::validation(
            "筛选名称需为 1 至 40 字，条件不能过长",
        ));
    }
    filters.page = 1;
    filters.deleted = false;
    let mut tx = pool.begin().await.map_err(CommandError::database)?;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM saved_question_filters WHERE name<>?")
            .bind(name)
            .fetch_one(&mut *tx)
            .await
            .map_err(CommandError::database)?;
    if count >= 100 {
        return Err(CommandError::validation("最多保存 100 组筛选条件"));
    }
    sqlx::query("INSERT INTO saved_question_filters(id,name,filters_json,created_at_ms) VALUES(?,?,?,?) ON CONFLICT(name) DO UPDATE SET filters_json=EXCLUDED.filters_json,created_at_ms=EXCLUDED.created_at_ms")
      .bind(Uuid::now_v7().to_string()).bind(name).bind(serde_json::to_string(&filters).map_err(CommandError::database)?).bind(now()).execute(&mut *tx).await.map_err(CommandError::database)?;
    tx.commit().await.map_err(CommandError::database)
}
pub async fn delete_filter(pool: &SqlitePool, id: &str) -> CommandResult<()> {
    sqlx::query("DELETE FROM saved_question_filters WHERE id=?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(CommandError::database)?;
    Ok(())
}
