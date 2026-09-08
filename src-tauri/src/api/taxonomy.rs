use std::collections::HashSet;

use sqlx::{Sqlite, SqlitePool, Transaction};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::models::{CommandError, CommandResult, TaxonomyOrderItemApi};

const SUBJECT_NAME_MAX: usize = 50;
const CHAPTER_NAME_MAX: usize = 100;
const TAG_NAME_MAX: usize = 50;
const SORT_STEP: i64 = 1024;

pub async fn create_initial_subject(
    pool: &SqlitePool,
    name: &str,
    app_version: &str,
) -> CommandResult<()> {
    let (name, name_key) = validated_name(name, "学科", SUBJECT_NAME_MAX)?;
    let now = now_millis();
    let subject_id = Uuid::now_v7().to_string();
    let chapter_id = Uuid::now_v7().to_string();
    let default_chapter = "未分类";
    let default_chapter_key = normalize_key(default_chapter);
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    ensure_unique_subject(&mut transaction, &name_key, None).await?;
    sqlx::query(
        "INSERT INTO subjects (id, name, name_key, sort_order, last_accessed_at_ms, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, ?, NULL, ?, ?)",
    )
    .bind(&subject_id)
    .bind(&name)
    .bind(&name_key)
    .bind(next_subject_sort_order(&mut transaction).await?)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    sqlx::query(
        "INSERT INTO chapters (id, subject_id, name, name_key, sort_order, last_accessed_at_ms, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, ?, ?, NULL, ?, ?)",
    )
    .bind(&chapter_id)
    .bind(&subject_id)
    .bind(default_chapter)
    .bind(default_chapter_key)
    .bind(SORT_STEP)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    write_log(
        &mut transaction,
        now,
        "subject.create_initial",
        "subject",
        Some(&subject_id),
        "创建首个学科及默认章节",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn create_subject(
    pool: &SqlitePool,
    name: &str,
    app_version: &str,
) -> CommandResult<String> {
    let (name, name_key) = validated_name(name, "学科", SUBJECT_NAME_MAX)?;
    let id = Uuid::now_v7().to_string();
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    ensure_unique_subject(&mut transaction, &name_key, None).await?;
    let sort_order = next_subject_sort_order(&mut transaction).await?;

    sqlx::query(
        "INSERT INTO subjects (id, name, name_key, sort_order, last_accessed_at_ms, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, ?, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&name_key)
    .bind(sort_order)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    write_log(
        &mut transaction,
        now,
        "subject.create",
        "subject",
        Some(&id),
        "创建学科",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(id)
}

pub async fn update_subject(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    app_version: &str,
) -> CommandResult<()> {
    let (name, name_key) = validated_name(name, "学科", SUBJECT_NAME_MAX)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    require_subject(&mut transaction, id).await?;
    ensure_unique_subject(&mut transaction, &name_key, Some(id)).await?;

    sqlx::query("UPDATE subjects SET name = ?, name_key = ?, updated_at_ms = ? WHERE id = ?")
        .bind(&name)
        .bind(&name_key)
        .bind(now)
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    write_log(
        &mut transaction,
        now,
        "subject.update",
        "subject",
        Some(id),
        "更新学科名称",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn delete_subject(pool: &SqlitePool, id: &str, app_version: &str) -> CommandResult<()> {
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    require_subject(&mut transaction, id).await?;
    let chapter_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chapters WHERE subject_id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    let question_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM questions WHERE subject_id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    if chapter_count > 0 || question_count > 0 {
        return Err(CommandError::validation(format!(
            "该学科下还有 {chapter_count} 个章节、{question_count} 道题，请先移动或删除关联内容。"
        )));
    }

    sqlx::query("DELETE FROM subjects WHERE id = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    write_log(
        &mut transaction,
        now,
        "subject.delete",
        "subject",
        Some(id),
        "删除学科",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn create_chapter(
    pool: &SqlitePool,
    subject_id: &str,
    name: &str,
    app_version: &str,
) -> CommandResult<String> {
    let (name, name_key) = validated_name(name, "章节", CHAPTER_NAME_MAX)?;
    let id = Uuid::now_v7().to_string();
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    require_subject(&mut transaction, subject_id).await?;
    ensure_unique_chapter(&mut transaction, subject_id, &name_key, None).await?;
    let sort_order = next_chapter_sort_order(&mut transaction, subject_id).await?;

    sqlx::query(
        "INSERT INTO chapters (id, subject_id, name, name_key, sort_order, last_accessed_at_ms, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, ?, ?, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(subject_id)
    .bind(&name)
    .bind(&name_key)
    .bind(sort_order)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    write_log(
        &mut transaction,
        now,
        "chapter.create",
        "chapter",
        Some(&id),
        "创建章节",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(id)
}

pub async fn update_chapter(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    app_version: &str,
) -> CommandResult<()> {
    let (name, name_key) = validated_name(name, "章节", CHAPTER_NAME_MAX)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let subject_id = require_chapter(&mut transaction, id).await?;
    ensure_unique_chapter(&mut transaction, &subject_id, &name_key, Some(id)).await?;

    sqlx::query("UPDATE chapters SET name = ?, name_key = ?, updated_at_ms = ? WHERE id = ?")
        .bind(&name)
        .bind(&name_key)
        .bind(now)
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    write_log(
        &mut transaction,
        now,
        "chapter.update",
        "chapter",
        Some(id),
        "更新章节名称",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn delete_chapter(pool: &SqlitePool, id: &str, app_version: &str) -> CommandResult<()> {
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    require_chapter(&mut transaction, id).await?;
    let question_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM questions WHERE chapter_id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    if question_count > 0 {
        return Err(CommandError::validation(format!(
            "该章节中还有 {question_count} 道题，请先将题目移动到其他章节。"
        )));
    }

    sqlx::query("DELETE FROM chapters WHERE id = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    write_log(
        &mut transaction,
        now,
        "chapter.delete",
        "chapter",
        Some(id),
        "删除章节",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn create_tag(pool: &SqlitePool, name: &str, app_version: &str) -> CommandResult<String> {
    let (name, name_key) = validated_name(name, "标签", TAG_NAME_MAX)?;
    let id = Uuid::now_v7().to_string();
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    ensure_unique_tag(&mut transaction, &name_key, None).await?;

    sqlx::query(
        "INSERT INTO tags (id, name, name_key, created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&name_key)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    write_log(
        &mut transaction,
        now,
        "tag.create",
        "tag",
        Some(&id),
        "创建标签",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(id)
}

pub async fn update_tag(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    app_version: &str,
) -> CommandResult<()> {
    let (name, name_key) = validated_name(name, "标签", TAG_NAME_MAX)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    require_tag(&mut transaction, id).await?;
    ensure_unique_tag(&mut transaction, &name_key, Some(id)).await?;
    let question_ids = question_ids_for_tag(&mut transaction, id).await?;

    sqlx::query("UPDATE tags SET name = ?, name_key = ?, updated_at_ms = ? WHERE id = ?")
        .bind(&name)
        .bind(&name_key)
        .bind(now)
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    refresh_tags_plain(&mut transaction, &question_ids).await?;
    write_log(
        &mut transaction,
        now,
        "tag.update",
        "tag",
        Some(id),
        "更新标签名称",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn delete_tag(pool: &SqlitePool, id: &str, app_version: &str) -> CommandResult<()> {
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    require_tag(&mut transaction, id).await?;
    let question_ids = question_ids_for_tag(&mut transaction, id).await?;

    // The question_tags foreign key cascades, so only relationships are removed.
    sqlx::query("DELETE FROM tags WHERE id = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    refresh_tags_plain(&mut transaction, &question_ids).await?;
    write_log(
        &mut transaction,
        now,
        "tag.delete",
        "tag",
        Some(id),
        "删除标签并解除题目关联",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn save_subject_order(
    pool: &SqlitePool,
    items: &[TaxonomyOrderItemApi],
    app_version: &str,
) -> CommandResult<()> {
    validate_order_items(items)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let database_ids = sqlx::query_scalar::<_, String>("SELECT id FROM subjects")
        .fetch_all(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    ensure_exact_order_scope(items, &database_ids, "学科")?;

    for item in items {
        sqlx::query("UPDATE subjects SET sort_order = ?, updated_at_ms = ? WHERE id = ?")
            .bind(item.sort_order)
            .bind(now)
            .bind(&item.id)
            .execute(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    }
    write_log(
        &mut transaction,
        now,
        "subject.reorder",
        "subject",
        None,
        "批量保存学科排序",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

pub async fn save_chapter_order(
    pool: &SqlitePool,
    subject_id: &str,
    items: &[TaxonomyOrderItemApi],
    app_version: &str,
) -> CommandResult<()> {
    validate_order_items(items)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    require_subject(&mut transaction, subject_id).await?;
    let database_ids =
        sqlx::query_scalar::<_, String>("SELECT id FROM chapters WHERE subject_id = ?")
            .bind(subject_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    ensure_exact_order_scope(items, &database_ids, "章节")?;

    for item in items {
        sqlx::query(
            "UPDATE chapters SET sort_order = ?, updated_at_ms = ? WHERE id = ? AND subject_id = ?",
        )
        .bind(item.sort_order)
        .bind(now)
        .bind(&item.id)
        .bind(subject_id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    }
    write_log(
        &mut transaction,
        now,
        "chapter.reorder",
        "subject",
        Some(subject_id),
        "批量保存章节排序",
        app_version,
    )
    .await?;
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

fn validated_name(value: &str, label: &str, maximum: usize) -> CommandResult<(String, String)> {
    let normalized = value.trim().nfkc().collect::<String>();
    let normalized = normalized.trim().to_owned();
    if normalized.is_empty() {
        return Err(CommandError::validation(format!("{label}名称不能为空。")));
    }
    if normalized.chars().count() > maximum {
        return Err(CommandError::validation(format!(
            "{label}名称不能超过 {maximum} 个字符。"
        )));
    }
    let key = normalize_key(&normalized);
    Ok((normalized, key))
}

fn normalize_key(value: &str) -> String {
    value.trim().nfkc().flat_map(char::to_lowercase).collect()
}

fn validate_order_items(items: &[TaxonomyOrderItemApi]) -> CommandResult<()> {
    let mut ids = HashSet::with_capacity(items.len());
    let mut orders = HashSet::with_capacity(items.len());
    for item in items {
        if item.id.trim().is_empty() || !ids.insert(item.id.as_str()) {
            return Err(CommandError::validation("排序列表包含空 ID 或重复 ID。"));
        }
        if item.sort_order < 0 || !orders.insert(item.sort_order) {
            return Err(CommandError::validation("排序值必须是互不重复的非负整数。"));
        }
    }
    Ok(())
}

fn ensure_exact_order_scope(
    items: &[TaxonomyOrderItemApi],
    database_ids: &[String],
    label: &str,
) -> CommandResult<()> {
    let requested = items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let existing = database_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if requested != existing {
        return Err(CommandError::validation(format!(
            "{label}列表已发生变化，请刷新页面后重新排序。"
        )));
    }
    Ok(())
}

async fn ensure_unique_subject(
    transaction: &mut Transaction<'_, Sqlite>,
    name_key: &str,
    except_id: Option<&str>,
) -> CommandResult<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subjects WHERE name_key = ? AND (? IS NULL OR id <> ?)",
    )
    .bind(name_key)
    .bind(except_id)
    .bind(except_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(CommandError::database)?;
    if count > 0 {
        return Err(CommandError::validation("已经存在同名学科。"));
    }
    Ok(())
}

async fn ensure_unique_chapter(
    transaction: &mut Transaction<'_, Sqlite>,
    subject_id: &str,
    name_key: &str,
    except_id: Option<&str>,
) -> CommandResult<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chapters WHERE subject_id = ? AND name_key = ? AND (? IS NULL OR id <> ?)",
    )
    .bind(subject_id)
    .bind(name_key)
    .bind(except_id)
    .bind(except_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(CommandError::database)?;
    if count > 0 {
        return Err(CommandError::validation("这个学科中已经存在同名章节。"));
    }
    Ok(())
}

async fn ensure_unique_tag(
    transaction: &mut Transaction<'_, Sqlite>,
    name_key: &str,
    except_id: Option<&str>,
) -> CommandResult<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tags WHERE name_key = ? AND (? IS NULL OR id <> ?)",
    )
    .bind(name_key)
    .bind(except_id)
    .bind(except_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(CommandError::database)?;
    if count > 0 {
        return Err(CommandError::validation("已经存在同名标签。"));
    }
    Ok(())
}

async fn require_subject(transaction: &mut Transaction<'_, Sqlite>, id: &str) -> CommandResult<()> {
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subjects WHERE id = ?")
        .bind(id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(CommandError::database)?;
    if exists == 0 {
        return Err(CommandError::new(
            "NOT_FOUND",
            "找不到指定学科，请刷新后重试。",
        ));
    }
    Ok(())
}

async fn require_chapter(
    transaction: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> CommandResult<String> {
    sqlx::query_scalar::<_, String>("SELECT subject_id FROM chapters WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(CommandError::database)?
        .ok_or_else(|| CommandError::new("NOT_FOUND", "找不到指定章节，请刷新后重试。"))
}

async fn require_tag(transaction: &mut Transaction<'_, Sqlite>, id: &str) -> CommandResult<()> {
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tags WHERE id = ?")
        .bind(id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(CommandError::database)?;
    if exists == 0 {
        return Err(CommandError::new(
            "NOT_FOUND",
            "找不到指定标签，请刷新后重试。",
        ));
    }
    Ok(())
}

async fn next_subject_sort_order(transaction: &mut Transaction<'_, Sqlite>) -> CommandResult<i64> {
    let maximum = sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(sort_order) FROM subjects")
        .fetch_one(&mut **transaction)
        .await
        .map_err(CommandError::database)?
        .unwrap_or(0);
    Ok(maximum.saturating_add(SORT_STEP))
}

async fn next_chapter_sort_order(
    transaction: &mut Transaction<'_, Sqlite>,
    subject_id: &str,
) -> CommandResult<i64> {
    let maximum = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(sort_order) FROM chapters WHERE subject_id = ?",
    )
    .bind(subject_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(CommandError::database)?
    .unwrap_or(0);
    Ok(maximum.saturating_add(SORT_STEP))
}

async fn question_ids_for_tag(
    transaction: &mut Transaction<'_, Sqlite>,
    tag_id: &str,
) -> CommandResult<Vec<String>> {
    sqlx::query_scalar::<_, String>("SELECT question_id FROM question_tags WHERE tag_id = ?")
        .bind(tag_id)
        .fetch_all(&mut **transaction)
        .await
        .map_err(CommandError::database)
}

async fn refresh_tags_plain(
    transaction: &mut Transaction<'_, Sqlite>,
    question_ids: &[String],
) -> CommandResult<()> {
    for question_id in question_ids {
        let tags_plain = sqlx::query_scalar::<_, Option<String>>(
            "SELECT group_concat(name, ' ') FROM (\
                SELECT t.name AS name FROM tags t \
                JOIN question_tags qt ON qt.tag_id = t.id \
                WHERE qt.question_id = ? ORDER BY t.name\
            )",
        )
        .bind(question_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(CommandError::database)?
        .unwrap_or_default();
        sqlx::query("UPDATE questions SET tags_plain = ? WHERE id = ?")
            .bind(tags_plain)
            .bind(question_id)
            .execute(&mut **transaction)
            .await
            .map_err(CommandError::database)?;
    }
    Ok(())
}

async fn write_log(
    transaction: &mut Transaction<'_, Sqlite>,
    now: i64,
    action: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    summary: &str,
    app_version: &str,
) -> CommandResult<()> {
    sqlx::query(
        "INSERT INTO operation_logs (id, occurred_at_ms, level, action, entity_type, entity_id, outcome, \
         summary, details_json, app_version) VALUES (?, ?, 'info', ?, ?, ?, 'success', ?, '{}', ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(now)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(summary)
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
    use super::{normalize_key, validated_name};

    #[test]
    fn names_are_trimmed_and_nfkc_normalized() {
        let (name, key) = validated_name("  ＡＢＣ  ", "标签", 50).expect("valid name");
        assert_eq!(name, "ABC");
        assert_eq!(key, "abc");
        assert_eq!(normalize_key("Ａbc"), "abc");
    }
}
