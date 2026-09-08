use std::collections::{BTreeMap, HashSet};

use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use super::{
    models::{
        CommandError, CommandResult, QuestionApi, RandomDrawAnalysisApi, RandomDrawRequestApi,
        RandomDrawScopeApi,
    },
    question_types,
    question_usage::{
        UsageConstraint, push_usage_constraint, resolve_usage_constraint, validate_usage_filter,
    },
    questions::{canonical_uuid, load_questions_by_ids},
};

const MAX_TAGS: usize = 100;
const MAX_SCOPE_IDS: usize = 1_000;
const MAX_QUESTIONS: u32 = 1_000;

fn validate_ids(values: &[String], label: &str, maximum: usize) -> CommandResult<()> {
    if values.len() > maximum {
        return Err(CommandError::validation(format!(
            "{label}数量超过安全上限（{maximum}）。"
        )));
    }
    let mut unique = HashSet::with_capacity(values.len());
    for value in values {
        let id = canonical_uuid(value, label)?;
        if !unique.insert(id) {
            return Err(CommandError::validation(format!("{label}不能重复。")));
        }
    }
    Ok(())
}

fn validate_scope(scope: &RandomDrawScopeApi) -> CommandResult<()> {
    validate_ids(&scope.subject_ids, "学科标识", MAX_SCOPE_IDS)?;
    validate_ids(&scope.chapter_ids, "章节标识", MAX_SCOPE_IDS)?;
    validate_ids(&scope.tag_ids, "标签标识", MAX_TAGS)?;
    validate_ids(&scope.excluded_question_ids, "已选题目标识", MAX_SCOPE_IDS)?;
    if !matches!(scope.tag_match_mode.as_str(), "any" | "all") {
        return Err(CommandError::validation("标签匹配方式无效。"));
    }
    validate_usage_filter(&scope.usage)?;
    Ok(())
}

fn push_scope(
    builder: &mut QueryBuilder<'_, Sqlite>,
    scope: &RandomDrawScopeApi,
    usage_constraint: UsageConstraint,
) {
    builder.push(" AND q.deleted_at_ms IS NULL");
    if !scope.subject_ids.is_empty() {
        builder.push(" AND q.subject_id IN (");
        let mut separated = builder.separated(", ");
        for subject_id in &scope.subject_ids {
            separated.push_bind(subject_id.clone());
        }
        separated.push_unseparated(")");
    }
    if !scope.chapter_ids.is_empty() {
        builder.push(" AND q.chapter_id IN (");
        let mut separated = builder.separated(", ");
        for chapter_id in &scope.chapter_ids {
            separated.push_bind(chapter_id.clone());
        }
        separated.push_unseparated(")");
    }
    if scope.tag_match_mode == "any" && !scope.tag_ids.is_empty() {
        builder.push(" AND (");
        for (index, tag_id) in scope.tag_ids.iter().enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            builder
                .push("EXISTS (SELECT 1 FROM question_tags draw_qt WHERE draw_qt.question_id = q.id AND draw_qt.tag_id = ")
                .push_bind(tag_id.clone())
                .push(")");
        }
        builder.push(")");
    } else {
        for tag_id in &scope.tag_ids {
            builder
                .push(" AND EXISTS (SELECT 1 FROM question_tags draw_qt WHERE draw_qt.question_id = q.id AND draw_qt.tag_id = ")
                .push_bind(tag_id.clone())
                .push(")");
        }
    }
    if !scope.excluded_question_ids.is_empty() {
        builder.push(" AND q.id NOT IN (");
        let mut separated = builder.separated(", ");
        for question_id in &scope.excluded_question_ids {
            separated.push_bind(question_id.clone());
        }
        separated.push_unseparated(")");
    }
    push_usage_constraint(builder, usage_constraint);
}

pub async fn analyze(
    pool: &SqlitePool,
    scope: &RandomDrawScopeApi,
) -> CommandResult<RandomDrawAnalysisApi> {
    validate_scope(scope)?;
    let usage_constraint = resolve_usage_constraint(&scope.usage)?;
    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT q.question_type, COUNT(*) FROM questions q WHERE 1 = 1",
    );
    push_scope(&mut builder, scope, usage_constraint);
    builder.push(" GROUP BY q.question_type ORDER BY q.question_type");
    let rows = builder
        .build_query_as::<(String, i64)>()
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)?;

    let mut available_total = 0_u32;
    let mut available_by_type = BTreeMap::new();
    for (question_type, count) in rows {
        let count = u32::try_from(count)
            .map_err(|_| CommandError::database("随机抽题候选数量超过支持范围。"))?;
        available_total = available_total
            .checked_add(count)
            .ok_or_else(|| CommandError::database("随机抽题候选总数超过支持范围。"))?;
        available_by_type.insert(question_type, count);
    }
    Ok(RandomDrawAnalysisApi {
        available_total,
        available_by_type,
    })
}

async fn candidate_ids(
    pool: &SqlitePool,
    scope: &RandomDrawScopeApi,
    usage_constraint: UsageConstraint,
    question_type: Option<&str>,
    limit: u32,
) -> CommandResult<Vec<String>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut builder = QueryBuilder::<Sqlite>::new("SELECT q.id FROM questions q WHERE 1 = 1");
    push_scope(&mut builder, scope, usage_constraint);
    if let Some(question_type) = question_type {
        builder
            .push(" AND q.question_type = ")
            .push_bind(question_type.to_owned());
    }
    builder
        .push(" ORDER BY RANDOM() LIMIT ")
        .push_bind(i64::from(limit));
    builder
        .build_query_scalar::<String>()
        .fetch_all(pool)
        .await
        .map_err(CommandError::database)
}

pub async fn draw(
    pool: &SqlitePool,
    request: &RandomDrawRequestApi,
) -> CommandResult<Vec<QuestionApi>> {
    validate_scope(&request.scope)?;
    let usage_constraint = resolve_usage_constraint(&request.scope.usage)?;
    let mut ordered_ids = Vec::new();
    match request.count_mode.as_str() {
        "total" => {
            if request.total_count == 0 || request.total_count > MAX_QUESTIONS {
                return Err(CommandError::validation(format!(
                    "随机抽题总数必须是 1 到 {MAX_QUESTIONS}。"
                )));
            }
            if request
                .question_type_counts
                .values()
                .any(|count| *count > 0)
            {
                return Err(CommandError::validation(
                    "按总题量抽取时不能同时提交分题型数量。",
                ));
            }
            ordered_ids = candidate_ids(
                pool,
                &request.scope,
                usage_constraint,
                None,
                request.total_count,
            )
            .await?;
        }
        "by_type" => {
            if request.total_count != 0 {
                return Err(CommandError::validation("按题型抽取时不能同时提交总题量。"));
            }
            let total = request
                .question_type_counts
                .values()
                .try_fold(0_u32, |sum, count| sum.checked_add(*count))
                .ok_or_else(|| CommandError::validation("随机抽题总数超过支持范围。"))?;
            if total == 0 || total > MAX_QUESTIONS {
                return Err(CommandError::validation(format!(
                    "随机抽题总数必须是 1 到 {MAX_QUESTIONS}。"
                )));
            }
            for (question_type, count) in &request.question_type_counts {
                if *count == 0 {
                    continue;
                }
                question_types::behavior(pool, question_type).await?;
                ordered_ids.extend(
                    candidate_ids(
                        pool,
                        &request.scope,
                        usage_constraint,
                        Some(question_type),
                        *count,
                    )
                    .await?,
                );
            }
        }
        _ => {
            return Err(CommandError::validation(
                "随机抽题方式无效，请重新打开窗口。",
            ));
        }
    }
    load_questions_by_ids(pool, &ordered_ids).await
}
