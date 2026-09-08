use std::collections::HashSet;

use sqlx::{FromRow, SqlitePool};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use super::models::{
    CommandError, CommandResult, QuestionTypeDefinitionApi, SaveQuestionTypeRequestApi,
    TaxonomyOrderItemApi,
};

const BEHAVIORS: &[&str] = &[
    "single_choice",
    "multiple_choice",
    "fill_blank",
    "open_response",
];
const NAME_MAX: usize = 40;
const ALIAS_MAX: usize = 40;
const MAX_ALIASES: usize = 20;
const MAX_DEFAULT_OPTIONS: usize = 12;

#[derive(Debug, FromRow)]
struct QuestionTypeRow {
    code: String,
    name: String,
    behavior: String,
    default_options_json: String,
    is_builtin: i64,
    is_enabled: i64,
    sort_order: i64,
    question_count: i64,
    paper_item_count: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX)
}

pub(crate) fn normalized_name_key(value: &str) -> String {
    value
        .trim()
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn validate_text(value: &str, label: &str, max_chars: usize) -> CommandResult<String> {
    let value = value.trim().nfkc().collect::<String>();
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(CommandError::validation(format!("{label}不能为空。")));
    }
    if value.chars().count() > max_chars {
        return Err(CommandError::validation(format!(
            "{label}不能超过 {max_chars} 个字符。"
        )));
    }
    Ok(value)
}

fn validate_request(
    request: &SaveQuestionTypeRequestApi,
) -> CommandResult<(String, String, Vec<String>, Vec<String>)> {
    let name = validate_text(&request.name, "题型名称", NAME_MAX)?;
    if !BEHAVIORS.contains(&request.behavior.as_str()) {
        return Err(CommandError::validation("请选择有效的基础答题结构。"));
    }
    if request.aliases.len() > MAX_ALIASES {
        return Err(CommandError::validation(format!(
            "一个题型最多设置 {MAX_ALIASES} 个识别别名。"
        )));
    }
    if request.default_options.len() > MAX_DEFAULT_OPTIONS {
        return Err(CommandError::validation(format!(
            "一个题型最多设置 {MAX_DEFAULT_OPTIONS} 个默认选项。"
        )));
    }

    let name_key = normalized_name_key(&name);
    let mut alias_keys = HashSet::new();
    let mut aliases = Vec::new();
    for alias in &request.aliases {
        let alias = validate_text(alias, "识别别名", ALIAS_MAX)?;
        let key = normalized_name_key(&alias);
        if key == name_key || !alias_keys.insert(key) {
            continue;
        }
        aliases.push(alias);
    }

    let mut option_keys = HashSet::new();
    let mut default_options = Vec::new();
    for option in &request.default_options {
        let option = validate_text(option, "默认选项", 80)?;
        let key = normalized_name_key(&option);
        if option_keys.insert(key) {
            default_options.push(option);
        }
    }
    if matches!(
        request.behavior.as_str(),
        "single_choice" | "multiple_choice"
    ) {
        if !default_options.is_empty() && default_options.len() < 2 {
            return Err(CommandError::validation(
                "选择结构至少需要两个默认选项，或者不设置默认选项。",
            ));
        }
    } else if !default_options.is_empty() {
        return Err(CommandError::validation(
            "只有单选或多选结构可以设置默认选项。",
        ));
    }

    Ok((name, name_key, aliases, default_options))
}

async fn ensure_name_available(
    pool: &SqlitePool,
    keys: &[String],
    excluding_code: Option<&str>,
) -> CommandResult<()> {
    for key in keys {
        let conflict = sqlx::query_scalar::<_, String>(
            "SELECT code FROM question_types WHERE name_key = ? AND (? IS NULL OR code <> ?) \
             UNION ALL \
             SELECT question_type_code FROM question_type_aliases \
             WHERE alias_key = ? AND (? IS NULL OR question_type_code <> ?) LIMIT 1",
        )
        .bind(key)
        .bind(excluding_code)
        .bind(excluding_code)
        .bind(key)
        .bind(excluding_code)
        .bind(excluding_code)
        .fetch_optional(pool)
        .await
        .map_err(CommandError::database)?;
        if conflict.is_some() {
            return Err(CommandError::validation(
                "题型名称或识别别名已经被其他题型使用。",
            ));
        }
    }
    Ok(())
}

pub async fn list(pool: &SqlitePool) -> CommandResult<Vec<QuestionTypeDefinitionApi>> {
    let rows = sqlx::query_as::<_, QuestionTypeRow>(
        "SELECT qt.code, qt.name, qt.behavior, qt.default_options_json, qt.is_builtin, \
                qt.is_enabled, qt.sort_order, qt.created_at_ms, qt.updated_at_ms, \
                (SELECT COUNT(*) FROM questions q \
                 WHERE q.question_type = qt.code AND q.deleted_at_ms IS NULL) AS question_count, \
                (SELECT COUNT(*) FROM paper_items pi WHERE pi.question_type = qt.code) AS paper_item_count \
         FROM question_types qt ORDER BY qt.sort_order, qt.code",
    )
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;

    let aliases = sqlx::query_as::<_, (String, String)>(
        "SELECT question_type_code, alias FROM question_type_aliases ORDER BY question_type_code, alias_key",
    )
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;

    rows.into_iter()
        .map(|row| {
            let default_options = serde_json::from_str::<Vec<String>>(&row.default_options_json)
                .map_err(CommandError::database)?;
            Ok(QuestionTypeDefinitionApi {
                aliases: aliases
                    .iter()
                    .filter(|(code, _)| code == &row.code)
                    .map(|(_, alias)| alias.clone())
                    .collect(),
                code: row.code,
                name: row.name,
                behavior: row.behavior,
                default_options,
                is_builtin: row.is_builtin != 0,
                is_enabled: row.is_enabled != 0,
                sort_order: row.sort_order,
                question_count: row.question_count,
                paper_item_count: row.paper_item_count,
                created_at: row.created_at_ms,
                updated_at: row.updated_at_ms,
            })
        })
        .collect()
}

pub async fn behavior(pool: &SqlitePool, code: &str) -> CommandResult<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT behavior FROM question_types WHERE code = ? AND is_enabled = 1",
    )
    .bind(code)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::validation("所选题型不存在或已经停用。"))
}

pub async fn save(
    pool: &SqlitePool,
    request: &SaveQuestionTypeRequestApi,
) -> CommandResult<QuestionTypeDefinitionApi> {
    let (name, name_key, aliases, default_options) = validate_request(request)?;
    let mut keys = vec![name_key.clone()];
    keys.extend(aliases.iter().map(|alias| normalized_name_key(alias)));
    ensure_name_available(pool, &keys, request.code.as_deref()).await?;

    let now = now_millis();
    let default_options_json =
        serde_json::to_string(&default_options).map_err(CommandError::database)?;
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let code = if let Some(code) = request.code.as_deref() {
        let existing = sqlx::query_as::<_, (i64, String, i64)>(
            "SELECT is_builtin, behavior, \
                    (SELECT COUNT(*) FROM questions q WHERE q.question_type = question_types.code) + \
                    (SELECT COUNT(*) FROM paper_items pi WHERE pi.question_type = question_types.code) \
             FROM question_types WHERE code = ?",
        )
        .bind(code)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(CommandError::database)?
        .ok_or_else(|| CommandError::validation("要修改的题型已经不存在。"))?;
        if existing.0 != 0 && existing.1 != request.behavior {
            return Err(CommandError::validation("内置题型的基础答题结构不能修改。"));
        }
        if existing.2 > 0 && existing.1 != request.behavior {
            return Err(CommandError::validation(
                "该题型已经被题目或试卷使用，不能修改基础答题结构。",
            ));
        }
        sqlx::query(
            "UPDATE question_types SET name = ?, name_key = ?, behavior = ?, is_enabled = ?, \
                    default_options_json = ?, updated_at_ms = ? WHERE code = ?",
        )
        .bind(&name)
        .bind(&name_key)
        .bind(&request.behavior)
        .bind(if request.is_enabled { 1_i64 } else { 0_i64 })
        .bind(&default_options_json)
        .bind(now)
        .bind(code)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
        sqlx::query("DELETE FROM question_type_aliases WHERE question_type_code = ?")
            .bind(code)
            .execute(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
        code.to_owned()
    } else {
        let code = format!("custom_{}", Uuid::now_v7().simple());
        let sort_order: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(sort_order), 0) + 10 FROM question_types")
                .fetch_one(&mut *transaction)
                .await
                .map_err(CommandError::database)?;
        sqlx::query(
            "INSERT INTO question_types (code, name, name_key, behavior, is_builtin, is_enabled, \
                    sort_order, default_options_json, created_at_ms, updated_at_ms) \
             VALUES (?, ?, ?, ?, 0, ?, ?, ?, ?, ?)",
        )
        .bind(&code)
        .bind(&name)
        .bind(&name_key)
        .bind(&request.behavior)
        .bind(if request.is_enabled { 1_i64 } else { 0_i64 })
        .bind(sort_order)
        .bind(&default_options_json)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
        code
    };

    for alias in aliases {
        sqlx::query(
            "INSERT INTO question_type_aliases (id, question_type_code, alias, alias_key, created_at_ms) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(&code)
        .bind(&alias)
        .bind(normalized_name_key(&alias))
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    }
    transaction.commit().await.map_err(CommandError::database)?;
    list(pool)
        .await?
        .into_iter()
        .find(|item| item.code == code)
        .ok_or_else(|| CommandError::database("保存后无法重新读取题型。"))
}

pub async fn delete(pool: &SqlitePool, code: &str) -> CommandResult<()> {
    let row = sqlx::query_as::<_, (i64, i64)>(
        "SELECT is_builtin, \
                (SELECT COUNT(*) FROM questions q WHERE q.question_type = question_types.code) + \
                (SELECT COUNT(*) FROM paper_items pi WHERE pi.question_type = question_types.code) \
         FROM question_types WHERE code = ?",
    )
    .bind(code)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::validation("题型已经不存在。"))?;
    if row.0 != 0 {
        return Err(CommandError::validation("内置题型不能删除，可以将它停用。"));
    }
    if row.1 > 0 {
        return Err(CommandError::validation(
            "该题型已经被题目或试卷使用，不能删除，可以将它停用。",
        ));
    }
    sqlx::query("DELETE FROM question_types WHERE code = ?")
        .bind(code)
        .execute(pool)
        .await
        .map_err(CommandError::database)?;
    Ok(())
}

pub async fn save_order(pool: &SqlitePool, items: &[TaxonomyOrderItemApi]) -> CommandResult<()> {
    let database_codes = sqlx::query_scalar::<_, String>("SELECT code FROM question_types")
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)?;
    let requested = items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    if requested.len() != items.len()
        || database_codes.len() != items.len()
        || database_codes
            .iter()
            .any(|code| !requested.contains(code.as_str()))
    {
        return Err(CommandError::validation(
            "题型排序范围已经变化，请刷新后重试。",
        ));
    }
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    for item in items {
        sqlx::query("UPDATE question_types SET sort_order = ?, updated_at_ms = ? WHERE code = ?")
            .bind(item.sort_order)
            .bind(now)
            .bind(&item.id)
            .execute(&mut *transaction)
            .await
            .map_err(CommandError::database)?;
    }
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::db::Database;

    use super::*;

    fn test_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "zhitiku-question-types-{}",
            Uuid::now_v7().simple()
        ))
    }

    fn remove_test_root(root: &std::path::Path) {
        let mut cleanup_error = None;
        for _ in 0..10 {
            match fs::remove_dir_all(root) {
                Ok(()) => return,
                Err(error) => {
                    cleanup_error = Some(error);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        panic!("failed to remove test database: {}", cleanup_error.unwrap());
    }

    #[tokio::test]
    async fn custom_type_round_trips_reorders_and_deletes_without_changing_builtin_codes() {
        let root = test_root();
        let database = Database::open(&root, "test").await.unwrap();
        let builtins = list(database.pool()).await.unwrap();
        assert_eq!(builtins.len(), 5);
        assert!(builtins.iter().all(|item| item.is_builtin));
        let judgment = builtins
            .iter()
            .find(|item| item.code == "true_false")
            .unwrap();
        assert_eq!(judgment.name, "判断题");
        assert_eq!(judgment.behavior, "single_choice");
        assert_eq!(judgment.default_options, vec!["正确", "错误"]);
        assert!(
            builtins
                .iter()
                .all(|item| !matches!(item.code.as_str(), "application" | "case_analysis"))
        );

        let created = save(
            database.pool(),
            &SaveQuestionTypeRequestApi {
                code: None,
                name: "计算题".to_owned(),
                behavior: "open_response".to_owned(),
                aliases: vec!["计算应用题".to_owned()],
                default_options: Vec::new(),
                is_enabled: true,
            },
        )
        .await
        .unwrap();
        assert!(created.code.starts_with("custom_"));
        assert_eq!(
            behavior(database.pool(), &created.code).await.unwrap(),
            "open_response"
        );
        assert_eq!(created.aliases, vec!["计算应用题"]);

        let mut ordered = list(database.pool()).await.unwrap();
        ordered.rotate_right(1);
        let order = ordered
            .iter()
            .enumerate()
            .map(|(index, item)| TaxonomyOrderItemApi {
                id: item.code.clone(),
                sort_order: ((index + 1) * 10) as i64,
            })
            .collect::<Vec<_>>();
        save_order(database.pool(), &order).await.unwrap();
        assert_eq!(
            list(database.pool()).await.unwrap()[0].code,
            ordered[0].code
        );

        delete(database.pool(), &created.code).await.unwrap();
        assert_eq!(list(database.pool()).await.unwrap().len(), 5);
        assert!(delete(database.pool(), "single_choice").await.is_err());

        database.close().await;
        remove_test_root(&root);
    }

    #[tokio::test]
    async fn usage_counts_exclude_questions_in_recycle_bin() {
        let root = test_root();
        let database = Database::open(&root, "test").await.unwrap();
        let subject_id = Uuid::now_v7().to_string();
        let chapter_id = Uuid::now_v7().to_string();

        sqlx::query(
            "INSERT INTO subjects \
             (id, name, name_key, sort_order, created_at_ms, updated_at_ms) \
             VALUES (?, '测试学科', '测试学科', 10, 0, 0)",
        )
        .bind(&subject_id)
        .execute(database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chapters \
             (id, subject_id, name, name_key, sort_order, created_at_ms, updated_at_ms) \
             VALUES (?, ?, '测试章节', '测试章节', 10, 0, 0)",
        )
        .bind(&chapter_id)
        .bind(&subject_id)
        .execute(database.pool())
        .await
        .unwrap();

        for deleted_at in [None, Some(1_i64)] {
            sqlx::query(
                "INSERT INTO questions (\
                    id, question_type, subject_id, chapter_id, content_schema_version, \
                    stem_json, answer_json, explanation_json, stem_plain, options_plain, \
                    answer_plain, explanation_plain, tags_plain, fingerprint_version, \
                    exact_fingerprint, content_version, last_used_at_ms, deleted_at_ms, \
                    created_at_ms, updated_at_ms\
                 ) VALUES (?, 'single_choice', ?, ?, 1, '{}', '{}', '{}', '题干', '', \
                           '答案', '', '', 1, zeroblob(32), 1, NULL, ?, 0, 0)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(&subject_id)
            .bind(&chapter_id)
            .bind(deleted_at)
            .execute(database.pool())
            .await
            .unwrap();
        }

        let definitions = list(database.pool()).await.unwrap();
        let single_choice = definitions
            .iter()
            .find(|definition| definition.code == "single_choice")
            .unwrap();
        assert_eq!(single_choice.question_count, 1);

        drop(definitions);
        database.close().await;
        remove_test_root(&root);
    }
}
