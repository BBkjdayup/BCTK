use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use super::content_identity::FINGERPRINT_VERSION;
use super::models::{CommandError, CommandResult, QuestionBatchTargetApi};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeRequest {
    pub keeper_id: String,
    pub members: Vec<QuestionBatchTargetApi>,
    pub duplicate_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeReceipt {
    pub id: String,
    pub keeper_id: String,
    pub recycled_count: usize,
    pub merged_tag_count: usize,
}

#[derive(Serialize, Deserialize)]
struct UndoRecord {
    receipt: MergeReceipt,
    members: Vec<QuestionBatchTargetApi>,
    old_tags: Vec<String>,
    merged_tags: Vec<String>,
    merged_at: i64,
}

#[derive(FromRow)]
struct Member {
    content_version: i64,
    deleted_at_ms: Option<i64>,
    updated_at_ms: i64,
    exact_fingerprint: Vec<u8>,
    fingerprint_version: i64,
    fingerprint_comparable: bool,
}

fn conflict() -> CommandError {
    CommandError::new(
        "CONTENT_CONFLICT",
        "题目已变更或不再可用，本次操作未执行。请重新查重后再处理。",
    )
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
fn validate_id(value: &str) -> CommandResult<()> {
    if Uuid::parse_str(value)
        .map(|id| id.to_string() == value)
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(CommandError::validation("题目或操作标识无效。"))
    }
}
async fn read_member(tx: &mut Transaction<'_, Sqlite>, id: &str) -> CommandResult<Member> {
    sqlx::query_as("SELECT content_version,deleted_at_ms,updated_at_ms,exact_fingerprint,fingerprint_version,fingerprint_comparable FROM questions WHERE id=?")
        .bind(id).fetch_optional(&mut **tx).await.map_err(CommandError::database)?.ok_or_else(conflict)
}
async fn tags(tx: &mut Transaction<'_, Sqlite>, id: &str) -> CommandResult<Vec<String>> {
    sqlx::query_scalar("SELECT tag_id FROM question_tags WHERE question_id=? ORDER BY tag_id")
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(CommandError::database)
}
async fn replace_tags(
    tx: &mut Transaction<'_, Sqlite>,
    keeper: &str,
    ids: &[String],
) -> CommandResult<()> {
    let mut names = Vec::new();
    for id in ids {
        let name: String = sqlx::query_scalar("SELECT name FROM tags WHERE id=?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(CommandError::database)?
            .ok_or_else(conflict)?;
        names.push(name);
    }
    sqlx::query("DELETE FROM question_tags WHERE question_id=?")
        .bind(keeper)
        .execute(&mut **tx)
        .await
        .map_err(CommandError::database)?;
    for id in ids {
        sqlx::query("INSERT INTO question_tags(question_id,tag_id,created_at_ms) VALUES(?,?,?)")
            .bind(keeper)
            .bind(id)
            .bind(now())
            .execute(&mut **tx)
            .await
            .map_err(CommandError::database)?;
    }
    names.sort();
    sqlx::query("UPDATE questions SET tags_plain=? WHERE id=?")
        .bind(names.join(" "))
        .bind(keeper)
        .execute(&mut **tx)
        .await
        .map_err(CommandError::database)?;
    Ok(())
}
async fn log(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    action: &str,
    summary: &str,
    app_version: &str,
) -> CommandResult<()> {
    sqlx::query("INSERT INTO operation_logs(id,occurred_at_ms,level,action,entity_type,entity_id,outcome,summary,details_json,app_version) VALUES(?,?,'info',?,'question',?,'success',?,'{}',?)")
        .bind(Uuid::now_v7().to_string()).bind(now()).bind(action).bind(id).bind(summary).bind(app_version)
        .execute(&mut **tx).await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn merge(
    pool: &SqlitePool,
    input: &MergeRequest,
    app_version: &str,
) -> CommandResult<MergeReceipt> {
    validate_id(&input.keeper_id)?;
    if !(2..=500).contains(&input.members.len())
        || !["exact", "suspected"].contains(&input.duplicate_kind.as_str())
    {
        return Err(CommandError::validation(
            "每次归并需选择 2 至 500 道题，并指定重复类型。",
        ));
    }
    let mut unique = HashSet::new();
    for member in &input.members {
        validate_id(&member.id)?;
        if member.expected_content_version < 1 || !unique.insert(&member.id) {
            return Err(conflict());
        }
    }
    if !unique.contains(&input.keeper_id) {
        return Err(CommandError::validation("保留题必须属于本组重复题。"));
    }
    let mut tx = pool.begin().await.map_err(CommandError::database)?;
    let mut fingerprint = None;
    let mut merged_tags = HashSet::new();
    for target in &input.members {
        let member = read_member(&mut tx, &target.id).await?;
        if member.content_version != target.expected_content_version
            || member.deleted_at_ms.is_some()
        {
            return Err(conflict());
        }
        if input.duplicate_kind == "exact" {
            if !member.fingerprint_comparable
                || member.fingerprint_version != FINGERPRINT_VERSION
                || fingerprint
                    .as_ref()
                    .is_some_and(|value| value != &member.exact_fingerprint)
            {
                return Err(conflict());
            }
            fingerprint = Some(member.exact_fingerprint);
        }
        merged_tags.extend(tags(&mut tx, &target.id).await?);
    }
    if merged_tags.len() > super::questions::MAX_TAGS {
        return Err(CommandError::validation(
            "归并后标签超过 100 个，请先整理标签；本组题目未作改动。",
        ));
    }
    let mut merged_tags: Vec<_> = merged_tags.into_iter().collect();
    merged_tags.sort();
    let old_tags = tags(&mut tx, &input.keeper_id).await?;
    let merged_at = now();
    let receipt = MergeReceipt {
        id: Uuid::now_v7().to_string(),
        keeper_id: input.keeper_id.clone(),
        recycled_count: input.members.len() - 1,
        merged_tag_count: merged_tags.len(),
    };
    replace_tags(&mut tx, &input.keeper_id, &merged_tags).await?;
    for target in &input.members {
        let deleted = if target.id == input.keeper_id {
            None
        } else {
            Some(merged_at)
        };
        let affected = sqlx::query("UPDATE questions SET deleted_at_ms=?,updated_at_ms=?,content_version=content_version+1 WHERE id=? AND content_version=? AND deleted_at_ms IS NULL")
            .bind(deleted).bind(merged_at).bind(&target.id).bind(target.expected_content_version).execute(&mut *tx).await.map_err(CommandError::database)?;
        if affected.rows_affected() != 1 {
            return Err(conflict());
        }
    }
    let record = UndoRecord {
        receipt: receipt.clone(),
        members: input.members.clone(),
        old_tags,
        merged_tags,
        merged_at,
    };
    sqlx::query("INSERT INTO question_duplicate_merges(id,keeper_id,undo_json,created_at_ms) VALUES(?,?,?,?)")
        .bind(&receipt.id).bind(&receipt.keeper_id).bind(serde_json::to_string(&record).map_err(CommandError::database)?).bind(merged_at)
        .execute(&mut *tx).await.map_err(CommandError::database)?;
    log(
        &mut tx,
        &receipt.id,
        "question.duplicate_merge",
        "归并重复题标签并回收重复题",
        app_version,
    )
    .await?;
    tx.commit().await.map_err(CommandError::database)?;
    Ok(receipt)
}

pub async fn latest(pool: &SqlitePool) -> CommandResult<Option<MergeReceipt>> {
    let json: Option<String> = sqlx::query_scalar("SELECT undo_json FROM question_duplicate_merges WHERE undone_at_ms IS NULL ORDER BY created_at_ms DESC,id DESC LIMIT 1")
        .fetch_optional(pool).await.map_err(CommandError::database)?;
    json.map(|value| {
        serde_json::from_str::<UndoRecord>(&value)
            .map(|record| record.receipt)
            .map_err(CommandError::database)
    })
    .transpose()
}

pub async fn undo(pool: &SqlitePool, id: &str, app_version: &str) -> CommandResult<()> {
    validate_id(id)?;
    let mut tx = pool.begin().await.map_err(CommandError::database)?;
    let json: String = sqlx::query_scalar(
        "SELECT undo_json FROM question_duplicate_merges WHERE id=? AND undone_at_ms IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::validation("归并记录不存在或已撤销。"))?;
    let record: UndoRecord = serde_json::from_str(&json).map_err(CommandError::database)?;
    for target in &record.members {
        let member = read_member(&mut tx, &target.id).await?;
        let deleted = if target.id == record.receipt.keeper_id {
            None
        } else {
            Some(record.merged_at)
        };
        if member.content_version != target.expected_content_version + 1
            || member.deleted_at_ms != deleted
            || member.updated_at_ms != record.merged_at
        {
            return Err(CommandError::new(
                "CONTENT_CONFLICT",
                "归并后的题目已被修改、恢复或删除，无法整体撤销。可在回收站查看保留的重复题。",
            ));
        }
    }
    if tags(&mut tx, &record.receipt.keeper_id).await? != record.merged_tags {
        return Err(conflict());
    }
    replace_tags(&mut tx, &record.receipt.keeper_id, &record.old_tags).await?;
    for target in &record.members {
        sqlx::query("UPDATE questions SET deleted_at_ms=NULL,updated_at_ms=?,content_version=content_version+1 WHERE id=?")
            .bind(now()).bind(&target.id).execute(&mut *tx).await.map_err(CommandError::database)?;
    }
    sqlx::query("UPDATE question_duplicate_merges SET undone_at_ms=? WHERE id=?")
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(CommandError::database)?;
    log(
        &mut tx,
        id,
        "question.duplicate_merge.undo",
        "撤销重复题归并",
        app_version,
    )
    .await?;
    tx.commit().await.map_err(CommandError::database)?;
    Ok(())
}
