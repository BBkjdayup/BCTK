use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqliteConnection};
use uuid::Uuid;

use super::{
    CloudService, CloudSyncConflictApi, CloudSyncPreflightApi, CloudSyncResultApi,
    ResolveCloudSyncConflictRequestApi,
};
use crate::{
    api::{
        models::{CommandError, CommandResult},
        resources::read_managed_resource_file,
    },
    db::Database,
};

const PULL_LIMIT: i64 = 500;
const PUSH_LIMIT: usize = 100;
const MAX_PUSH_BODY_BYTES: usize = 17 * 1024 * 1024;
const CONFLICT_KIND_CONTENT: &str = "content";
const CONFLICT_KIND_DEFERRED_APPLY: &str = "deferred_apply";
const KINDS_IN_APPLY_ORDER: [&str; 6] = [
    "question_type",
    "subject",
    "chapter",
    "tag",
    "resource",
    "question",
];

#[derive(Clone)]
struct LocalEntity {
    kind: String,
    local_id: String,
    id: String,
    payload: Option<Value>,
    hash: String,
}

struct MutationContext {
    key: String,
    local_id: Option<String>,
    deleted: bool,
}

#[derive(Default)]
struct EntityMappings {
    local_to_cloud: HashMap<String, String>,
    cloud_to_local: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteEntity {
    entity_kind: String,
    entity_id: String,
    revision: i64,
    change_cursor: i64,
    deleted: bool,
    payload: Option<Value>,
    content_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullResponse {
    changes: Vec<RemoteEntity>,
    next_cursor: i64,
    has_more: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncStatusResponse {
    active_entity_count: i64,
    active_question_count: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PushChange {
    mutation_id: String,
    entity_kind: String,
    entity_id: String,
    base_revision: i64,
    deleted: bool,
    payload: Option<Value>,
    content_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushResponse {
    results: Vec<PushResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushResult {
    mutation_id: String,
    status: String,
    current: Option<RemoteEntity>,
}

#[derive(Clone, Debug, FromRow)]
struct SyncStateRow {
    entity_kind: String,
    entity_id: String,
    server_revision: i64,
    deleted: i64,
    synced_hash: String,
}

#[derive(Debug, FromRow)]
struct ConflictDetailRow {
    id: String,
    entity_kind: String,
    entity_id: String,
    remote_payload_json: String,
}

#[derive(Debug, FromRow)]
struct DeferredApplyRow {
    entity_kind: String,
    entity_id: String,
    local_payload_json: Option<String>,
    remote_payload_json: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredRemoteEnvelope {
    deleted: bool,
    payload: Option<Value>,
    revision: i64,
    content_hash: String,
}

#[derive(Default)]
struct DeferredRetrySummary {
    recovered_count: usize,
    promoted_count: usize,
    pending_count: usize,
}

#[derive(Debug, FromRow)]
struct QuestionTypeRow {
    code: String,
    name: String,
    name_key: String,
    behavior: String,
    is_builtin: i64,
    is_enabled: i64,
    sort_order: i64,
    default_options_json: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Clone, Debug, FromRow, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionTypeAliasPayload {
    id: String,
    alias: String,
    alias_key: String,
    created_at_ms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionTypePayload {
    code: String,
    name: String,
    name_key: String,
    behavior: String,
    is_builtin: bool,
    is_enabled: bool,
    sort_order: i64,
    default_options: Value,
    created_at_ms: i64,
    updated_at_ms: i64,
    aliases: Vec<QuestionTypeAliasPayload>,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubjectPayload {
    id: String,
    name: String,
    name_key: String,
    sort_order: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterPayload {
    id: String,
    subject_id: String,
    name: String,
    name_key: String,
    sort_order: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TagPayload {
    id: String,
    name: String,
    name_key: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, FromRow)]
struct ResourceRow {
    id: String,
    sha256: Vec<u8>,
    resource_kind: String,
    mime_type: String,
    storage_rel_path: String,
    original_filename: Option<String>,
    byte_size: i64,
    intrinsic_width_px: Option<i64>,
    intrinsic_height_px: Option<i64>,
    availability_status: String,
    created_at_ms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourcePayload {
    id: String,
    sha256_hex: String,
    resource_kind: String,
    mime_type: String,
    original_filename: Option<String>,
    byte_size: i64,
    intrinsic_width_px: Option<i64>,
    intrinsic_height_px: Option<i64>,
    availability_status: String,
    created_at_ms: i64,
    data_base64: Option<String>,
}

#[derive(Debug, FromRow)]
struct QuestionRow {
    id: String,
    question_type: String,
    subject_id: String,
    chapter_id: String,
    content_schema_version: i64,
    stem_json: String,
    answer_json: String,
    explanation_json: String,
    stem_plain: String,
    options_plain: String,
    answer_plain: String,
    explanation_plain: String,
    tags_plain: String,
    fingerprint_version: i64,
    exact_fingerprint: Vec<u8>,
    content_version: i64,
    last_used_at_ms: Option<i64>,
    deleted_at_ms: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionOptionPayload {
    id: String,
    question_id: String,
    position: i64,
    content: Value,
    plain_text: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, FromRow)]
struct QuestionOptionRow {
    id: String,
    question_id: String,
    position: i64,
    content_json: String,
    plain_text: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Clone, Debug, FromRow, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionTagPayload {
    question_id: String,
    tag_id: String,
    created_at_ms: i64,
}

#[derive(Clone, Debug, FromRow, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionResourceRefPayload {
    id: String,
    question_id: String,
    option_id: Option<String>,
    resource_id: String,
    content_slot: String,
    node_id: String,
    created_at_ms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionPayload {
    id: String,
    question_type: String,
    subject_id: String,
    chapter_id: String,
    content_schema_version: i64,
    stem: Value,
    answer: Value,
    explanation: Value,
    stem_plain: String,
    options_plain: String,
    answer_plain: String,
    explanation_plain: String,
    tags_plain: String,
    fingerprint_version: i64,
    exact_fingerprint_hex: String,
    content_version: i64,
    last_used_at_ms: Option<i64>,
    deleted_at_ms: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
    options: Vec<QuestionOptionPayload>,
    tags: Vec<QuestionTagPayload>,
    resource_refs: Vec<QuestionResourceRefPayload>,
}

pub(super) async fn run_sync(
    cloud: &CloudService,
    database: &Database,
) -> CommandResult<CloudSyncResultApi> {
    let runtime = cloud.snapshot().await;
    let session = runtime
        .session
        .as_ref()
        .ok_or_else(|| CommandError::new("CLOUD_NOT_LOGGED_IN", "请先登录云账号。"))?;
    if !matches!(
        session.cloud_entitlement.status.as_str(),
        "active" | "grace"
    ) {
        return Err(CommandError::new(
            "CLOUD_SYNC_NOT_ENTITLED",
            "题目云同步授权尚未开通或已经到期。",
        ));
    }

    let _: Value = cloud
        .authenticated_json_for(&runtime, Method::GET, "/api/v1/sync/status", None)
        .await?;
    ensure_database_binding(database, &session.user.id, &session.user.username).await?;
    let result = run_sync_inner(cloud, database, &session.user.id, &runtime).await;
    if let Err(error) = &result {
        let now = now_ms();
        let _ = sqlx::query(
            r#"
            INSERT INTO cloud_sync_meta (account_id, pull_cursor, last_error, updated_at_ms)
            VALUES (?, 0, ?, ?)
            ON CONFLICT(account_id) DO UPDATE SET
                last_error = excluded.last_error,
                updated_at_ms = excluded.updated_at_ms
            "#,
        )
        .bind(&session.user.id)
        .bind(&error.message)
        .bind(now)
        .execute(database.pool())
        .await;
    }
    result
}

pub(super) async fn preflight(
    cloud: &CloudService,
    database: &Database,
) -> CommandResult<CloudSyncPreflightApi> {
    let runtime = cloud.snapshot().await;
    let session = runtime
        .session
        .as_ref()
        .ok_or_else(|| CommandError::new("CLOUD_NOT_LOGGED_IN", "请先登录云账号。"))?;
    if !matches!(
        session.cloud_entitlement.status.as_str(),
        "active" | "grace"
    ) {
        return Err(CommandError::new(
            "CLOUD_SYNC_NOT_ENTITLED",
            "题目云同步授权尚未开通或已经到期。",
        ));
    }
    let remote: SyncStatusResponse = cloud
        .authenticated_json_for(&runtime, Method::GET, "/api/v1/sync/status", None)
        .await?;
    let (local_question_count, local_other_count) = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
            (SELECT COUNT(*) FROM questions),
            (SELECT COUNT(*) FROM subjects)
              + (SELECT COUNT(*) FROM chapters)
              + (SELECT COUNT(*) FROM tags)
              + (SELECT COUNT(*) FROM resources)
              + (SELECT COUNT(*) FROM question_types WHERE is_builtin = 0)
        "#,
    )
    .fetch_one(database.pool())
    .await
    .map_err(CommandError::database)?;
    let local_entity_count = local_question_count.saturating_add(local_other_count);
    let local_has_data = local_entity_count > 0;
    let cloud_has_data = remote.active_entity_count > 0;
    Ok(CloudSyncPreflightApi {
        local_entity_count,
        local_question_count,
        cloud_entity_count: remote.active_entity_count,
        cloud_question_count: remote.active_question_count,
        local_has_data,
        cloud_has_data,
        both_non_empty: local_has_data && cloud_has_data,
        recommended_mode: "merge".to_owned(),
    })
}

pub(super) async fn list_conflicts(
    cloud: &CloudService,
    database: &Database,
) -> CommandResult<Vec<CloudSyncConflictApi>> {
    let runtime = cloud.snapshot().await;
    let session = runtime
        .session
        .ok_or_else(|| CommandError::new("CLOUD_NOT_LOGGED_IN", "请先登录云账号。"))?;
    sqlx::query_as::<_, (String, String, String, i64, String)>(
        r#"
        SELECT id, entity_kind, entity_id, detected_at_ms, message
        FROM cloud_sync_conflicts
        WHERE account_id = ? AND conflict_kind = 'content'
          AND resolved_at_ms IS NULL
        ORDER BY detected_at_ms DESC, id
        "#,
    )
    .bind(&session.user.id)
    .fetch_all(database.pool())
    .await
    .map(|rows| {
        rows.into_iter()
            .map(
                |(id, entity_kind, entity_id, detected_at_ms, message)| CloudSyncConflictApi {
                    id,
                    entity_kind,
                    entity_id,
                    detected_at_ms,
                    message,
                },
            )
            .collect()
    })
    .map_err(CommandError::database)
}

pub(super) async fn resolve_conflict(
    cloud: &CloudService,
    database: &Database,
    request: ResolveCloudSyncConflictRequestApi,
) -> CommandResult<Vec<CloudSyncConflictApi>> {
    if !matches!(request.resolution.as_str(), "keep_local" | "use_cloud") {
        return Err(CommandError::validation("冲突处理方式无效。"));
    }
    let runtime = cloud.snapshot().await;
    let session = runtime
        .session
        .ok_or_else(|| CommandError::new("CLOUD_NOT_LOGGED_IN", "请先登录云账号。"))?;
    let mut transaction = database
        .pool()
        .begin()
        .await
        .map_err(CommandError::database)?;
    let detail = sqlx::query_as::<_, ConflictDetailRow>(
        r#"
        SELECT id, entity_kind, entity_id, remote_payload_json
        FROM cloud_sync_conflicts
        WHERE id = ? AND account_id = ? AND conflict_kind = 'content'
          AND resolved_at_ms IS NULL
        "#,
    )
    .bind(&request.id)
    .bind(&session.user.id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::validation("待处理的同步冲突不存在或已经处理。"))?;

    if request.resolution == "use_cloud" {
        let stored_remote = stored_remote_entity(
            detail.entity_kind,
            detail.entity_id,
            &detail.remote_payload_json,
        )?;
        // The open conflict row is refreshed on every pull, and the sync-state
        // row is the authoritative latest cloud revision already accepted by
        // this client. Prefer it so a delayed conflict decision can never
        // replay an older cloud snapshot over a newer one.
        let remote = latest_remote_entity_from_state(
            &mut transaction,
            &session.user.id,
            &stored_remote.entity_kind,
            &stored_remote.entity_id,
        )
        .await?
        .unwrap_or(stored_remote);
        let mappings = load_entity_mappings(&mut transaction, &session.user.id).await?;
        let localized = localize_remote_entity(&remote, &mappings)?;
        apply_remote_entity(&mut transaction, database, &localized).await?;
    } else {
        sqlx::query(
            r#"
            UPDATE cloud_sync_state SET deleted = 0, updated_at_ms = ?
            WHERE account_id = ? AND entity_kind = ? AND entity_id = ?
            "#,
        )
        .bind(now_ms())
        .bind(&session.user.id)
        .bind(&detail.entity_kind)
        .bind(&detail.entity_id)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    }
    sqlx::query(
        "UPDATE cloud_sync_conflicts SET resolved_at_ms = ? WHERE id = ? AND account_id = ?",
    )
    .bind(now_ms())
    .bind(detail.id)
    .bind(&session.user.id)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)?;
    list_conflicts(cloud, database).await
}

fn stored_remote_entity(
    entity_kind: String,
    entity_id: String,
    payload_json: &str,
) -> CommandResult<RemoteEntity> {
    let envelope: StoredRemoteEnvelope = serde_json::from_str(payload_json)
        .map_err(|_| CommandError::new("CLOUD_CONFLICT_CORRUPTED", "同步冲突记录无法读取。"))?;
    let remote = RemoteEntity {
        entity_kind,
        entity_id,
        revision: envelope.revision,
        // Conflict records predate the durable retry queue and intentionally do
        // not retain their transport cursor. A positive sentinel is sufficient
        // because replay never advances the server cursor.
        change_cursor: 1,
        deleted: envelope.deleted,
        payload: envelope.payload,
        content_hash: envelope.content_hash,
    };
    verify_remote_entity(&remote)?;
    Ok(remote)
}

async fn run_sync_inner(
    cloud: &CloudService,
    database: &Database,
    account_id: &str,
    runtime: &super::CloudRuntimeSnapshot,
) -> CommandResult<CloudSyncResultApi> {
    let initial_retry = retry_deferred_applies(database, account_id).await?;
    let mut cursor = sqlx::query_scalar::<_, i64>(
        "SELECT pull_cursor FROM cloud_sync_meta WHERE account_id = ?",
    )
    .bind(account_id)
    .fetch_optional(database.pool())
    .await
    .map_err(CommandError::database)?
    .unwrap_or(0);
    let mut pulled_count = 0_usize;
    let mut recovered_count = initial_retry.recovered_count;
    let mut merged_count = 0_usize;
    let mut conflict_count = initial_retry.promoted_count;
    loop {
        let path = format!("/api/v1/sync/pull?cursor={cursor}&limit={PULL_LIMIT}");
        let page: PullResponse = cloud
            .authenticated_json_for(runtime, Method::GET, &path, None)
            .await?;
        verify_pull_page(&page, cursor)?;
        let mut changes = page.changes;
        changes.sort_by_key(apply_order_key);
        let identity_merged = prepare_remote_mappings(database, account_id, &changes).await?;
        merged_count += identity_merged.len();
        let mut state_connection = database
            .pool()
            .acquire()
            .await
            .map_err(CommandError::database)?;
        let refreshed_states = load_sync_state(&mut state_connection, account_id).await?;
        drop(state_connection);
        let mut local_entities = capture_entities(database, account_id, &refreshed_states).await?;
        let mut transaction = database
            .pool()
            .begin()
            .await
            .map_err(CommandError::database)?;
        let mut content_conflicts =
            load_open_conflict_keys_by_kind(&mut transaction, account_id, CONFLICT_KIND_CONTENT)
                .await?;
        let deferred_applies = load_open_conflict_keys_by_kind(
            &mut transaction,
            account_id,
            CONFLICT_KIND_DEFERRED_APPLY,
        )
        .await?;
        let state = load_sync_state(&mut transaction, account_id).await?;
        let mappings = load_entity_mappings(&mut transaction, account_id).await?;
        for remote in changes {
            pulled_count += 1;
            let key = entity_key(&remote.entity_kind, &remote.entity_id);
            let local = local_entities.get(&key).cloned();
            let prior = state.get(&key);
            let was_deferred = deferred_applies.contains(&key);
            let local_changed = !was_deferred
                && !identity_merged.contains(&key)
                && local_differs_from_state(local.as_ref(), prior);
            let remote_matches_local = remote_matches_local(&remote, local.as_ref());
            let already_conflicted = content_conflicts.contains(&key);

            if (!local_changed || remote_matches_local) && !already_conflicted {
                let localized_remote = localize_remote_entity(&remote, &mappings)?;
                if !remote_matches_local
                    && let Err(error) = apply_remote_entity_atomically(
                        &mut transaction,
                        database,
                        &localized_remote,
                    )
                    .await
                {
                    if !is_retryable_dependency_error(&error) {
                        return Err(error);
                    }
                    record_conflict(
                        &mut transaction,
                        account_id,
                        local.as_ref(),
                        &remote,
                        CONFLICT_KIND_DEFERRED_APPLY,
                        format!("云端内容等待依赖数据后自动补全：{}", error.message),
                    )
                    .await?;
                    upsert_sync_state(&mut transaction, account_id, &remote).await?;
                    continue;
                }
                if was_deferred {
                    resolve_deferred_apply(
                        &mut transaction,
                        account_id,
                        &remote.entity_kind,
                        &remote.entity_id,
                    )
                    .await?;
                }
                upsert_sync_state(&mut transaction, account_id, &remote).await?;
                if remote.deleted {
                    local_entities.remove(&key);
                } else if let Some(payload) = &remote.payload {
                    local_entities.insert(
                        key,
                        LocalEntity {
                            kind: remote.entity_kind.clone(),
                            local_id: mappings.local_id(&remote.entity_kind, &remote.entity_id),
                            id: remote.entity_id.clone(),
                            payload: Some(payload.clone()),
                            hash: remote.content_hash.clone(),
                        },
                    );
                }
            } else {
                record_conflict(
                    &mut transaction,
                    account_id,
                    local.as_ref(),
                    &remote,
                    CONFLICT_KIND_CONTENT,
                    "本机和云端都修改了这一项，已停止自动覆盖并保留双方内容。".to_owned(),
                )
                .await?;
                if !already_conflicted {
                    conflict_count += 1;
                    content_conflicts.insert(key);
                }
                upsert_sync_state(&mut transaction, account_id, &remote).await?;
            }
        }
        cursor = page.next_cursor;
        upsert_pull_cursor(&mut transaction, account_id, cursor).await?;
        transaction.commit().await.map_err(CommandError::database)?;
        if !page.has_more {
            break;
        }
    }

    let final_retry = retry_deferred_applies(database, account_id).await?;
    recovered_count += final_retry.recovered_count;
    conflict_count += final_retry.promoted_count;
    let deferred_count = final_retry.pending_count;

    let mut uploaded_count = 0_usize;
    for deleting in [false, true] {
        let kinds: Vec<&str> = if deleting {
            KINDS_IN_APPLY_ORDER.iter().rev().copied().collect()
        } else {
            KINDS_IN_APPLY_ORDER.to_vec()
        };
        for kind in kinds {
            let mut connection = database
                .pool()
                .acquire()
                .await
                .map_err(CommandError::database)?;
            let states = load_sync_state(&mut connection, account_id).await?;
            let open_conflicts = load_open_conflict_keys(&mut connection, account_id).await?;
            drop(connection);
            let local_entities = capture_entities(database, account_id, &states).await?;
            let changes = build_push_changes(&local_entities, &states, &open_conflicts)
                .into_iter()
                .filter(|change| change.deleted == deleting && change.entity_kind == kind)
                .collect();
            let (uploaded, merged, conflicted) = push_changes(
                cloud,
                database,
                account_id,
                runtime,
                &local_entities,
                changes,
            )
            .await?;
            uploaded_count += uploaded;
            merged_count += merged;
            conflict_count += conflicted;
        }
    }
    let mut final_connection = database
        .pool()
        .acquire()
        .await
        .map_err(CommandError::database)?;
    let content_skipped_count =
        load_open_conflict_keys_by_kind(&mut final_connection, account_id, CONFLICT_KIND_CONTENT)
            .await?
            .len();
    let skipped_count = content_skipped_count.saturating_add(deferred_count);
    drop(final_connection);

    let completed_at_ms = now_ms();
    sqlx::query(
        r#"
        INSERT INTO cloud_sync_meta (
            account_id, pull_cursor, last_sync_at_ms, last_error, updated_at_ms
        ) VALUES (?, ?, ?, NULL, ?)
        ON CONFLICT(account_id) DO UPDATE SET
            pull_cursor = excluded.pull_cursor,
            last_sync_at_ms = excluded.last_sync_at_ms,
            last_error = NULL,
            updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(account_id)
    .bind(cursor)
    .bind(completed_at_ms)
    .bind(completed_at_ms)
    .execute(database.pool())
    .await
    .map_err(CommandError::database)?;

    let message = if conflict_count > 0 || content_skipped_count > 0 {
        format!(
            "同步完成：下载 {pulled_count} 项、自动补全 {recovered_count} 项、上传 {uploaded_count} 项、自动归一 {merged_count} 项；{} 项冲突内容已保留，未自动覆盖。",
            conflict_count.max(content_skipped_count)
        )
    } else if deferred_count > 0 {
        format!(
            "同步已安全完成：下载 {pulled_count} 项、自动补全 {recovered_count} 项、上传 {uploaded_count} 项、自动归一 {merged_count} 项；{deferred_count} 项云端内容仍在等待依赖数据，将在后续同步自动补全。"
        )
    } else {
        format!(
            "同步完成：下载 {pulled_count} 项，自动补全 {recovered_count} 项，上传 {uploaded_count} 项，自动归一 {merged_count} 项。"
        )
    };
    Ok(CloudSyncResultApi {
        pulled_count,
        uploaded_count,
        merged_count,
        conflict_count,
        skipped_count,
        completed_at_ms,
        message,
    })
}

async fn retry_deferred_applies(
    database: &Database,
    account_id: &str,
) -> CommandResult<DeferredRetrySummary> {
    let rows = sqlx::query_as::<_, DeferredApplyRow>(
        r#"
        SELECT entity_kind, entity_id, local_payload_json, remote_payload_json
        FROM cloud_sync_conflicts
        WHERE account_id = ? AND conflict_kind = 'deferred_apply'
          AND resolved_at_ms IS NULL
        ORDER BY detected_at_ms, id
        "#,
    )
    .bind(account_id)
    .fetch_all(database.pool())
    .await
    .map_err(CommandError::database)?;
    if rows.is_empty() {
        return Ok(DeferredRetrySummary::default());
    }

    let mut state_connection = database
        .pool()
        .acquire()
        .await
        .map_err(CommandError::database)?;
    let states = load_sync_state(&mut state_connection, account_id).await?;
    let mappings = load_entity_mappings(&mut state_connection, account_id).await?;
    drop(state_connection);
    let local_entities = capture_entities(database, account_id, &states).await?;

    let mut pending = rows
        .into_iter()
        .map(|row| {
            let remote = stored_remote_entity(
                row.entity_kind.clone(),
                row.entity_id.clone(),
                &row.remote_payload_json,
            )?;
            Ok((row, remote))
        })
        .collect::<CommandResult<Vec<_>>>()?;
    pending.sort_by_key(|(_, remote)| apply_order_key(remote));

    let mut summary = DeferredRetrySummary::default();
    for (row, remote) in pending {
        let key = entity_key(&remote.entity_kind, &remote.entity_id);
        let local = local_entities.get(&key);
        let baseline_payload = row
            .local_payload_json
            .as_deref()
            .map(|value| parse_json(value, "等待补全时的本机内容"))
            .transpose()?;
        let local_unchanged = match (
            local.and_then(|item| item.payload.as_ref()),
            baseline_payload.as_ref(),
        ) {
            (None, None) => true,
            (Some(current), Some(baseline)) => current == baseline,
            _ => false,
        };

        let mut transaction = database
            .pool()
            .begin()
            .await
            .map_err(CommandError::database)?;
        if remote_matches_local(&remote, local) {
            upsert_sync_state(&mut transaction, account_id, &remote).await?;
            resolve_deferred_apply(
                &mut transaction,
                account_id,
                &row.entity_kind,
                &row.entity_id,
            )
            .await?;
            transaction.commit().await.map_err(CommandError::database)?;
            summary.recovered_count += 1;
            continue;
        }
        if !local_unchanged {
            promote_deferred_apply_to_content_conflict(
                &mut transaction,
                account_id,
                local,
                &row.entity_kind,
                &row.entity_id,
            )
            .await?;
            transaction.commit().await.map_err(CommandError::database)?;
            summary.promoted_count += 1;
            continue;
        }

        let localized = localize_remote_entity(&remote, &mappings)?;
        match apply_remote_entity(&mut transaction, database, &localized).await {
            Ok(()) => {
                upsert_sync_state(&mut transaction, account_id, &remote).await?;
                resolve_deferred_apply(
                    &mut transaction,
                    account_id,
                    &row.entity_kind,
                    &row.entity_id,
                )
                .await?;
                transaction.commit().await.map_err(CommandError::database)?;
                summary.recovered_count += 1;
            }
            Err(error) if is_retryable_dependency_error(&error) => {
                transaction
                    .rollback()
                    .await
                    .map_err(CommandError::database)?;
            }
            Err(error) => {
                transaction
                    .rollback()
                    .await
                    .map_err(CommandError::database)?;
                return Err(error);
            }
        }
    }

    summary.pending_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM cloud_sync_conflicts
        WHERE account_id = ? AND conflict_kind = 'deferred_apply'
          AND resolved_at_ms IS NULL
        "#,
    )
    .bind(account_id)
    .fetch_one(database.pool())
    .await
    .map_err(CommandError::database)?
    .try_into()
    .unwrap_or(usize::MAX);
    Ok(summary)
}

async fn push_changes(
    cloud: &CloudService,
    database: &Database,
    account_id: &str,
    runtime: &super::CloudRuntimeSnapshot,
    local_entities: &HashMap<String, LocalEntity>,
    changes: Vec<PushChange>,
) -> CommandResult<(usize, usize, usize)> {
    let mut uploaded_count = 0_usize;
    let mut merged_count = 0_usize;
    let mut conflict_count = 0_usize;
    for batch in build_push_batches(changes)? {
        let mutation_lookup: HashMap<_, _> = batch
            .iter()
            .map(|change| {
                let key = entity_key(&change.entity_kind, &change.entity_id);
                (
                    change.mutation_id.clone(),
                    MutationContext {
                        local_id: local_entities
                            .get(&key)
                            .map(|entity| entity.local_id.clone()),
                        key,
                        deleted: change.deleted,
                    },
                )
            })
            .collect();
        let body = json!({ "changes": &batch });
        let response: PushResponse = cloud
            .authenticated_json_for(runtime, Method::POST, "/api/v1/sync/push", Some(&body))
            .await?;
        if response.results.len() != batch.len() {
            return Err(CommandError::new(
                "CLOUD_RESPONSE_INVALID",
                "云服务返回的同步结果数量不一致。",
            ));
        }
        let mut transaction = database
            .pool()
            .begin()
            .await
            .map_err(CommandError::database)?;
        for result in response.results {
            let context = mutation_lookup.get(&result.mutation_id).ok_or_else(|| {
                CommandError::new("CLOUD_RESPONSE_INVALID", "云服务返回了未知的同步请求标识。")
            })?;
            let local = local_entities.get(&context.key);
            match (result.status.as_str(), result.current) {
                ("applied", Some(current)) => {
                    verify_remote_entity(&current)?;
                    upsert_sync_state(&mut transaction, account_id, &current).await?;
                    uploaded_count += 1;
                }
                ("merged", Some(current)) => {
                    verify_remote_entity(&current)?;
                    let Some(local_id) = context.local_id.as_deref() else {
                        if !context.deleted {
                            return Err(CommandError::new(
                                "CLOUD_RESPONSE_INVALID",
                                "云服务合并结果缺少本地实体。",
                            ));
                        }
                        reconcile_merged_tombstone(
                            &mut transaction,
                            account_id,
                            &context.key,
                            &current,
                        )
                        .await?;
                        uploaded_count += 1;
                        merged_count += 1;
                        continue;
                    };
                    upsert_entity_mapping(
                        &mut transaction,
                        account_id,
                        &current.entity_kind,
                        local_id,
                        &current.entity_id,
                    )
                    .await?;
                    let mappings = load_entity_mappings(&mut transaction, account_id).await?;
                    let localized = localize_remote_entity(&current, &mappings)?;
                    apply_remote_entity(&mut transaction, database, &localized).await?;
                    if let Some((kind, requested_id)) = split_entity_key(&context.key)
                        && requested_id != current.entity_id
                    {
                        sqlx::query(
                            "DELETE FROM cloud_sync_state WHERE account_id = ? AND entity_kind = ? AND entity_id = ?",
                        )
                        .bind(account_id)
                        .bind(kind)
                        .bind(requested_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(CommandError::database)?;
                    }
                    upsert_sync_state(&mut transaction, account_id, &current).await?;
                    uploaded_count += 1;
                    merged_count += 1;
                }
                ("conflict", Some(current)) => {
                    verify_remote_entity(&current)?;
                    record_conflict(
                        &mut transaction,
                        account_id,
                        local,
                        &current,
                        CONFLICT_KIND_CONTENT,
                        "上传时发现云端已有更新，已停止覆盖并保留双方内容。".to_owned(),
                    )
                    .await?;
                    upsert_sync_state(&mut transaction, account_id, &current).await?;
                    conflict_count += 1;
                }
                _ => {
                    return Err(CommandError::new(
                        "CLOUD_RESPONSE_INVALID",
                        "云服务返回了无法识别的同步状态。",
                    ));
                }
            }
        }
        transaction.commit().await.map_err(CommandError::database)?;
    }
    Ok((uploaded_count, merged_count, conflict_count))
}

async fn reconcile_merged_tombstone(
    connection: &mut SqliteConnection,
    account_id: &str,
    requested_key: &str,
    current: &RemoteEntity,
) -> CommandResult<()> {
    let (requested_kind, requested_id) = split_entity_key(requested_key)
        .ok_or_else(|| CommandError::new("CLOUD_RESPONSE_INVALID", "本地同步实体标识无效。"))?;
    if requested_kind != current.entity_kind || requested_id == current.entity_id {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云服务返回的合并身份与本地删除记录不一致。",
        ));
    }

    sqlx::query(
        "DELETE FROM cloud_sync_state WHERE account_id = ? AND entity_kind = ? AND entity_id = ?",
    )
    .bind(account_id)
    .bind(requested_kind)
    .bind(requested_id)
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    sqlx::query(
        "DELETE FROM cloud_entity_mappings WHERE account_id = ? AND entity_kind = ? AND cloud_entity_id = ?",
    )
    .bind(account_id)
    .bind(requested_kind)
    .bind(requested_id)
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    upsert_sync_state(connection, account_id, current).await
}

fn build_push_batches(changes: Vec<PushChange>) -> CommandResult<Vec<Vec<PushChange>>> {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_bytes = 32_usize;
    for change in changes {
        let change_bytes = serde_json::to_vec(&change)
            .map_err(json_encoding_error)?
            .len()
            .saturating_add(1);
        if change_bytes > MAX_PUSH_BODY_BYTES {
            return Err(CommandError::new(
                "CLOUD_SYNC_ITEM_TOO_LARGE",
                "单项题目或图片超过云同步传输上限。",
            ));
        }
        if !current.is_empty()
            && (current.len() >= PUSH_LIMIT
                || current_bytes.saturating_add(change_bytes) > MAX_PUSH_BODY_BYTES)
        {
            batches.push(std::mem::take(&mut current));
            current_bytes = 32;
        }
        current_bytes = current_bytes.saturating_add(change_bytes);
        current.push(change);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    Ok(batches)
}

async fn ensure_database_binding(
    database: &Database,
    account_id: &str,
    username: &str,
) -> CommandResult<()> {
    let current = sqlx::query_as::<_, (String, String)>(
        "SELECT account_id, username FROM cloud_database_binding WHERE singleton_id = 1",
    )
    .fetch_optional(database.pool())
    .await
    .map_err(CommandError::database)?;
    if let Some((bound_id, bound_name)) = current {
        if bound_id != account_id {
            return Err(CommandError::new(
                "CLOUD_DATABASE_ACCOUNT_MISMATCH",
                format!(
                    "当前题库已经绑定云账号“{bound_name}”。为避免不同教师的数据混在一起，不能改用另一个账号同步。"
                ),
            ));
        }
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO cloud_database_binding (singleton_id, account_id, username, bound_at_ms) VALUES (1, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(username)
    .bind(now_ms())
    .execute(database.pool())
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

async fn capture_entities(
    database: &Database,
    account_id: &str,
    known_states: &HashMap<String, SyncStateRow>,
) -> CommandResult<HashMap<String, LocalEntity>> {
    let mut connection = database
        .pool()
        .acquire()
        .await
        .map_err(CommandError::database)?;
    let mut result = HashMap::new();
    let mappings = load_entity_mappings(&mut connection, account_id).await?;

    let aliases = sqlx::query_as::<_, (String, String, String, String, i64)>(
        "SELECT question_type_code, id, alias, alias_key, created_at_ms FROM question_type_aliases ORDER BY question_type_code, alias_key, id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    let mut aliases_by_type: HashMap<String, Vec<QuestionTypeAliasPayload>> = HashMap::new();
    for (code, id, alias, alias_key, created_at_ms) in aliases {
        aliases_by_type
            .entry(code)
            .or_default()
            .push(QuestionTypeAliasPayload {
                id,
                alias,
                alias_key,
                created_at_ms,
            });
    }
    let question_types = sqlx::query_as::<_, QuestionTypeRow>(
        r#"
        SELECT code, name, name_key, behavior, is_builtin, is_enabled, sort_order,
               default_options_json, created_at_ms, updated_at_ms
        FROM question_types ORDER BY code
        "#,
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    for row in question_types {
        let payload = QuestionTypePayload {
            code: row.code.clone(),
            name: row.name,
            name_key: row.name_key,
            behavior: row.behavior,
            is_builtin: row.is_builtin != 0,
            is_enabled: row.is_enabled != 0,
            sort_order: row.sort_order,
            default_options: parse_json(&row.default_options_json, "题型默认选项")?,
            created_at_ms: row.created_at_ms,
            updated_at_ms: row.updated_at_ms,
            aliases: aliases_by_type.remove(&row.code).unwrap_or_default(),
        };
        insert_local_entity(&mut result, &mappings, "question_type", row.code, payload)?;
    }

    for payload in sqlx::query_as::<_, SubjectPayload>(
        "SELECT id, name, name_key, sort_order, created_at_ms, updated_at_ms FROM subjects ORDER BY id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?
    {
        let id = payload.id.clone();
        insert_local_entity(&mut result, &mappings, "subject", id, payload)?;
    }
    for payload in sqlx::query_as::<_, ChapterPayload>(
        "SELECT id, subject_id, name, name_key, sort_order, created_at_ms, updated_at_ms FROM chapters ORDER BY id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?
    {
        let id = payload.id.clone();
        insert_local_entity(&mut result, &mappings, "chapter", id, payload)?;
    }
    for payload in sqlx::query_as::<_, TagPayload>(
        "SELECT id, name, name_key, created_at_ms, updated_at_ms FROM tags ORDER BY id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?
    {
        let id = payload.id.clone();
        insert_local_entity(&mut result, &mappings, "tag", id, payload)?;
    }

    let resources = sqlx::query_as::<_, ResourceRow>(
        r#"
        SELECT id, sha256, resource_kind, mime_type, storage_rel_path, original_filename,
               byte_size, intrinsic_width_px, intrinsic_height_px, availability_status,
               created_at_ms
        FROM resources ORDER BY id
        "#,
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    for row in resources {
        let cloud_id = mappings.cloud_id("resource", &row.id);
        let key = entity_key("resource", &cloud_id);
        if let Some(state) = known_states.get(&key).filter(|state| state.deleted == 0) {
            result.insert(
                key,
                LocalEntity {
                    kind: "resource".to_owned(),
                    local_id: row.id,
                    id: cloud_id,
                    payload: None,
                    hash: state.synced_hash.clone(),
                },
            );
            continue;
        }
        let sha256_hex = hex_encode(&row.sha256);
        let data_base64 =
            if row.availability_status == "ready" {
                Some(STANDARD.encode(read_managed_resource_file(
                    database.paths().resources_dir(),
                    &row.storage_rel_path,
                    &row.sha256,
                    u64::try_from(row.byte_size).map_err(|_| {
                        CommandError::new("RESOURCE_CORRUPTED", "图片资源大小无效。")
                    })?,
                )?))
            } else {
                None
            };
        let payload = ResourcePayload {
            id: row.id.clone(),
            sha256_hex,
            resource_kind: row.resource_kind,
            mime_type: row.mime_type,
            original_filename: row.original_filename,
            byte_size: row.byte_size,
            intrinsic_width_px: row.intrinsic_width_px,
            intrinsic_height_px: row.intrinsic_height_px,
            availability_status: row.availability_status,
            created_at_ms: row.created_at_ms,
            data_base64,
        };
        insert_local_entity(&mut result, &mappings, "resource", row.id, payload)?;
    }

    let option_rows = sqlx::query_as::<_, QuestionOptionRow>(
        r#"
        SELECT id, question_id, position, content_json, plain_text, created_at_ms, updated_at_ms
        FROM question_options ORDER BY question_id, position, id
        "#,
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    let mut options: HashMap<String, Vec<QuestionOptionPayload>> = HashMap::new();
    for row in option_rows {
        let question_id = row.question_id.clone();
        options
            .entry(question_id)
            .or_default()
            .push(QuestionOptionPayload {
                id: row.id,
                question_id: row.question_id,
                position: row.position,
                content: parse_json(&row.content_json, "题目选项")?,
                plain_text: row.plain_text,
                created_at_ms: row.created_at_ms,
                updated_at_ms: row.updated_at_ms,
            });
    }
    let mut tags: HashMap<String, Vec<QuestionTagPayload>> = HashMap::new();
    for row in sqlx::query_as::<_, QuestionTagPayload>(
        "SELECT question_id, tag_id, created_at_ms FROM question_tags ORDER BY question_id, tag_id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?
    {
        tags.entry(row.question_id.clone()).or_default().push(row);
    }
    let mut refs: HashMap<String, Vec<QuestionResourceRefPayload>> = HashMap::new();
    for row in sqlx::query_as::<_, QuestionResourceRefPayload>(
        r#"
        SELECT id, question_id, option_id, resource_id, content_slot, node_id, created_at_ms
        FROM question_resource_refs ORDER BY question_id, node_id, id
        "#,
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?
    {
        refs.entry(row.question_id.clone()).or_default().push(row);
    }
    let questions = sqlx::query_as::<_, QuestionRow>(
        r#"
        SELECT id, question_type, subject_id, chapter_id, content_schema_version,
               stem_json, answer_json, explanation_json, stem_plain, options_plain,
               answer_plain, explanation_plain, tags_plain, fingerprint_version,
               exact_fingerprint, content_version, last_used_at_ms, deleted_at_ms,
               created_at_ms, updated_at_ms
        FROM questions ORDER BY id
        "#,
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    for row in questions {
        let payload = QuestionPayload {
            id: row.id.clone(),
            question_type: row.question_type,
            subject_id: row.subject_id,
            chapter_id: row.chapter_id,
            content_schema_version: row.content_schema_version,
            stem: parse_json(&row.stem_json, "题干")?,
            answer: parse_json(&row.answer_json, "答案")?,
            explanation: parse_json(&row.explanation_json, "解析")?,
            stem_plain: row.stem_plain,
            options_plain: row.options_plain,
            answer_plain: row.answer_plain,
            explanation_plain: row.explanation_plain,
            tags_plain: row.tags_plain,
            fingerprint_version: row.fingerprint_version,
            exact_fingerprint_hex: hex_encode(&row.exact_fingerprint),
            content_version: row.content_version,
            last_used_at_ms: row.last_used_at_ms,
            deleted_at_ms: row.deleted_at_ms,
            created_at_ms: row.created_at_ms,
            updated_at_ms: row.updated_at_ms,
            options: options.remove(&row.id).unwrap_or_default(),
            tags: tags.remove(&row.id).unwrap_or_default(),
            resource_refs: refs.remove(&row.id).unwrap_or_default(),
        };
        insert_local_entity(&mut result, &mappings, "question", row.id, payload)?;
    }
    Ok(result)
}

fn insert_local_entity<T: Serialize>(
    target: &mut HashMap<String, LocalEntity>,
    mappings: &EntityMappings,
    kind: &str,
    local_id: String,
    payload: T,
) -> CommandResult<()> {
    let mut payload = serde_json::to_value(payload).map_err(|error| {
        CommandError::new(
            "CLOUD_SYNC_ENCODING_FAILED",
            format!("本地同步内容无法编码：{error}"),
        )
    })?;
    let id = mappings.cloud_id(kind, &local_id);
    canonicalize_local_payload(&mut payload, kind, &id, mappings)?;
    let hash = hash_payload(&payload)?;
    target.insert(
        entity_key(kind, &id),
        LocalEntity {
            kind: kind.to_owned(),
            local_id,
            id,
            payload: Some(payload),
            hash,
        },
    );
    Ok(())
}

impl EntityMappings {
    fn cloud_id(&self, kind: &str, local_id: &str) -> String {
        self.local_to_cloud
            .get(&entity_key(kind, local_id))
            .cloned()
            .unwrap_or_else(|| local_id.to_owned())
    }

    fn local_id(&self, kind: &str, cloud_id: &str) -> String {
        self.cloud_to_local
            .get(&entity_key(kind, cloud_id))
            .cloned()
            .unwrap_or_else(|| cloud_id.to_owned())
    }

    fn insert(&mut self, kind: &str, local_id: &str, cloud_id: &str) {
        self.local_to_cloud
            .insert(entity_key(kind, local_id), cloud_id.to_owned());
        self.cloud_to_local
            .insert(entity_key(kind, cloud_id), local_id.to_owned());
    }
}

async fn load_entity_mappings(
    connection: &mut SqliteConnection,
    account_id: &str,
) -> CommandResult<EntityMappings> {
    let rows = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT entity_kind, local_entity_id, cloud_entity_id
        FROM cloud_entity_mappings
        WHERE account_id = ?
        "#,
    )
    .bind(account_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    let mut mappings = EntityMappings::default();
    for (kind, local_id, cloud_id) in rows {
        mappings.insert(&kind, &local_id, &cloud_id);
    }
    Ok(mappings)
}

async fn upsert_entity_mapping(
    connection: &mut SqliteConnection,
    account_id: &str,
    kind: &str,
    local_id: &str,
    cloud_id: &str,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        INSERT INTO cloud_entity_mappings (
            account_id, entity_kind, local_entity_id, cloud_entity_id,
            created_at_ms, updated_at_ms
        ) VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, entity_kind, local_entity_id) DO UPDATE SET
            cloud_entity_id = excluded.cloud_entity_id,
            updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(account_id)
    .bind(kind)
    .bind(local_id)
    .bind(cloud_id)
    .bind(now_ms())
    .bind(now_ms())
    .execute(&mut *connection)
    .await
    .map_err(|error| {
        CommandError::new(
            "CLOUD_IDENTITY_CONFLICT",
            format!("本地与云端身份映射无法保存：{error}"),
        )
    })?;
    Ok(())
}

async fn prepare_remote_mappings(
    database: &Database,
    account_id: &str,
    changes: &[RemoteEntity],
) -> CommandResult<HashSet<String>> {
    let mut transaction = database
        .pool()
        .begin()
        .await
        .map_err(CommandError::database)?;
    let mut mappings = load_entity_mappings(&mut transaction, account_id).await?;
    let mut merged = HashSet::new();
    for remote in changes {
        if mappings
            .cloud_to_local
            .contains_key(&entity_key(&remote.entity_kind, &remote.entity_id))
        {
            continue;
        }
        let local_id = if remote.deleted {
            remote.entity_id.clone()
        } else {
            match remote.entity_kind.as_str() {
                "question_type" | "question" => remote.entity_id.clone(),
                "subject" => {
                    let key = payload_string(remote, "nameKey")?;
                    sqlx::query_scalar::<_, String>("SELECT id FROM subjects WHERE name_key = ?")
                        .bind(key)
                        .fetch_optional(&mut *transaction)
                        .await
                        .map_err(CommandError::database)?
                        .unwrap_or_else(|| remote.entity_id.clone())
                }
                "tag" => {
                    let key = payload_string(remote, "nameKey")?;
                    sqlx::query_scalar::<_, String>("SELECT id FROM tags WHERE name_key = ?")
                        .bind(key)
                        .fetch_optional(&mut *transaction)
                        .await
                        .map_err(CommandError::database)?
                        .unwrap_or_else(|| remote.entity_id.clone())
                }
                "chapter" => {
                    let subject_cloud_id = payload_string(remote, "subjectId")?;
                    let subject_local_id = mappings.local_id("subject", subject_cloud_id);
                    let key = payload_string(remote, "nameKey")?;
                    sqlx::query_scalar::<_, String>(
                        "SELECT id FROM chapters WHERE subject_id = ? AND name_key = ?",
                    )
                    .bind(subject_local_id)
                    .bind(key)
                    .fetch_optional(&mut *transaction)
                    .await
                    .map_err(CommandError::database)?
                    .unwrap_or_else(|| remote.entity_id.clone())
                }
                "resource" => {
                    let sha256 = payload_string(remote, "sha256Hex")?;
                    sqlx::query_scalar::<_, String>(
                        "SELECT id FROM resources WHERE lower(hex(sha256)) = lower(?)",
                    )
                    .bind(sha256)
                    .fetch_optional(&mut *transaction)
                    .await
                    .map_err(CommandError::database)?
                    .unwrap_or_else(|| remote.entity_id.clone())
                }
                _ => remote.entity_id.clone(),
            }
        };
        if local_id != remote.entity_id {
            merged.insert(entity_key(&remote.entity_kind, &remote.entity_id));
        }
        upsert_entity_mapping(
            &mut transaction,
            account_id,
            &remote.entity_kind,
            &local_id,
            &remote.entity_id,
        )
        .await?;
        mappings.insert(&remote.entity_kind, &local_id, &remote.entity_id);
    }
    transaction.commit().await.map_err(CommandError::database)?;
    Ok(merged)
}

fn payload_string<'a>(remote: &'a RemoteEntity, field: &str) -> CommandResult<&'a str> {
    remote
        .payload
        .as_ref()
        .and_then(|payload| payload.get(field))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CommandError::new(
                "CLOUD_RESPONSE_INVALID",
                format!("云端同步内容缺少 {field}。"),
            )
        })
}

fn canonicalize_local_payload(
    payload: &mut Value,
    kind: &str,
    cloud_id: &str,
    mappings: &EntityMappings,
) -> CommandResult<()> {
    if kind == "question_type" {
        set_payload_string(payload, "code", cloud_id)?;
    } else {
        set_payload_string(payload, "id", cloud_id)?;
    }
    match kind {
        "chapter" => map_payload_reference(payload, "subjectId", "subject", mappings, true)?,
        "question" => {
            map_payload_reference(payload, "subjectId", "subject", mappings, true)?;
            map_payload_reference(payload, "chapterId", "chapter", mappings, true)?;
            map_question_rich_content_resources(payload, mappings, true);
            if let Some(options) = payload.get_mut("options").and_then(Value::as_array_mut) {
                for option in options {
                    set_payload_string(option, "questionId", cloud_id)?;
                }
            }
            if let Some(tags) = payload.get_mut("tags").and_then(Value::as_array_mut) {
                for tag in tags {
                    set_payload_string(tag, "questionId", cloud_id)?;
                    map_payload_reference(tag, "tagId", "tag", mappings, true)?;
                }
            }
            if let Some(refs) = payload
                .get_mut("resourceRefs")
                .and_then(Value::as_array_mut)
            {
                for resource_ref in refs {
                    set_payload_string(resource_ref, "questionId", cloud_id)?;
                    map_payload_reference(resource_ref, "resourceId", "resource", mappings, true)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn localize_remote_entity(
    remote: &RemoteEntity,
    mappings: &EntityMappings,
) -> CommandResult<RemoteEntity> {
    let mut localized = remote.clone();
    localized.entity_id = mappings.local_id(&remote.entity_kind, &remote.entity_id);
    let Some(payload) = localized.payload.as_mut() else {
        return Ok(localized);
    };
    if remote.entity_kind == "question_type" {
        set_payload_string(payload, "code", &localized.entity_id)?;
    } else {
        set_payload_string(payload, "id", &localized.entity_id)?;
    }
    match remote.entity_kind.as_str() {
        "chapter" => map_payload_reference(payload, "subjectId", "subject", mappings, false)?,
        "question" => {
            map_payload_reference(payload, "subjectId", "subject", mappings, false)?;
            map_payload_reference(payload, "chapterId", "chapter", mappings, false)?;
            map_question_rich_content_resources(payload, mappings, false);
            if let Some(options) = payload.get_mut("options").and_then(Value::as_array_mut) {
                for option in options {
                    set_payload_string(option, "questionId", &localized.entity_id)?;
                }
            }
            if let Some(tags) = payload.get_mut("tags").and_then(Value::as_array_mut) {
                for tag in tags {
                    set_payload_string(tag, "questionId", &localized.entity_id)?;
                    map_payload_reference(tag, "tagId", "tag", mappings, false)?;
                }
            }
            if let Some(refs) = payload
                .get_mut("resourceRefs")
                .and_then(Value::as_array_mut)
            {
                for resource_ref in refs {
                    set_payload_string(resource_ref, "questionId", &localized.entity_id)?;
                    map_payload_reference(resource_ref, "resourceId", "resource", mappings, false)?;
                }
            }
        }
        _ => {}
    }
    Ok(localized)
}

fn map_payload_reference(
    payload: &mut Value,
    field: &str,
    kind: &str,
    mappings: &EntityMappings,
    to_cloud: bool,
) -> CommandResult<()> {
    let current = payload
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CommandError::new("CLOUD_RESPONSE_INVALID", format!("同步内容缺少 {field}。"))
        })?
        .to_owned();
    let mapped = if to_cloud {
        mappings.cloud_id(kind, &current)
    } else {
        mappings.local_id(kind, &current)
    };
    set_payload_string(payload, field, &mapped)
}

fn map_question_rich_content_resources(
    payload: &mut Value,
    mappings: &EntityMappings,
    to_cloud: bool,
) {
    for field in ["stem", "answer", "explanation"] {
        if let Some(content) = payload.get_mut(field) {
            map_rich_content_resource_ids(content, mappings, to_cloud);
        }
    }
    if let Some(options) = payload.get_mut("options").and_then(Value::as_array_mut) {
        for option in options {
            if let Some(content) = option.get_mut("content") {
                map_rich_content_resource_ids(content, mappings, to_cloud);
            }
        }
    }
}

fn map_rich_content_resource_ids(content: &mut Value, mappings: &EntityMappings, to_cloud: bool) {
    if let Some(html) = content.get("html").and_then(Value::as_str) {
        let rewritten = rewrite_html_resource_ids(html, mappings, to_cloud);
        if rewritten != html
            && let Some(object) = content.as_object_mut()
        {
            object.insert("html".to_owned(), Value::String(rewritten));
        }
    }
    map_document_resource_ids(content, mappings, to_cloud);
}

fn map_document_resource_ids(value: &mut Value, mappings: &EntityMappings, to_cloud: bool) {
    match value {
        Value::Object(object) => {
            for (field, nested) in object {
                if field == "resourceId" {
                    if let Some(current) = nested.as_str() {
                        let mapped = if to_cloud {
                            mappings.cloud_id("resource", current)
                        } else {
                            mappings.local_id("resource", current)
                        };
                        *nested = Value::String(mapped);
                    }
                } else {
                    map_document_resource_ids(nested, mappings, to_cloud);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                map_document_resource_ids(item, mappings, to_cloud);
            }
        }
        _ => {}
    }
}

fn rewrite_html_resource_ids(html: &str, mappings: &EntityMappings, to_cloud: bool) -> String {
    const ATTRIBUTE: &str = "data-resource-id";
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut output = String::with_capacity(html.len());
    let mut copied_until = 0_usize;
    let mut search_from = 0_usize;

    while search_from < html.len() {
        let Some(relative_start) = lower[search_from..].find(ATTRIBUTE) else {
            break;
        };
        let attribute_start = search_from + relative_start;
        let attribute_end = attribute_start + ATTRIBUTE.len();
        if attribute_start > 0
            && matches!(bytes[attribute_start - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-')
        {
            search_from = attribute_end;
            continue;
        }

        let mut cursor = attribute_end;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            search_from = attribute_end;
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        let quote = bytes
            .get(cursor)
            .copied()
            .filter(|byte| matches!(byte, b'\'' | b'"'));
        if quote.is_some() {
            cursor += 1;
        }
        let value_start = cursor;
        while cursor < bytes.len()
            && match quote {
                Some(delimiter) => bytes[cursor] != delimiter,
                None => {
                    !bytes[cursor].is_ascii_whitespace() && !matches!(bytes[cursor], b'>' | b'/')
                }
            }
        {
            cursor += 1;
        }
        if cursor == value_start {
            search_from = attribute_end;
            continue;
        }

        let current = &html[value_start..cursor];
        let mapped = if to_cloud {
            mappings.cloud_id("resource", current)
        } else {
            mappings.local_id("resource", current)
        };
        if mapped != current {
            output.push_str(&html[copied_until..value_start]);
            output.push_str(&mapped);
            copied_until = cursor;
        }
        search_from = if quote.is_some() && cursor < bytes.len() {
            cursor + 1
        } else {
            cursor
        };
    }
    output.push_str(&html[copied_until..]);
    output
}

fn set_payload_string(payload: &mut Value, field: &str, value: &str) -> CommandResult<()> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| CommandError::new("CLOUD_RESPONSE_INVALID", "同步内容必须是 JSON 对象。"))?;
    object.insert(field.to_owned(), Value::String(value.to_owned()));
    Ok(())
}

fn build_push_changes(
    local: &HashMap<String, LocalEntity>,
    states: &HashMap<String, SyncStateRow>,
    open_conflicts: &HashSet<String>,
) -> Vec<PushChange> {
    let mut changes = Vec::new();
    for (key, entity) in local {
        if open_conflicts.contains(key) {
            continue;
        }
        let state = states.get(key);
        if state.is_some_and(|value| value.deleted == 0 && value.synced_hash == entity.hash) {
            continue;
        }
        changes.push(PushChange {
            mutation_id: Uuid::now_v7().to_string(),
            entity_kind: entity.kind.clone(),
            entity_id: entity.id.clone(),
            base_revision: state.map_or(0, |value| value.server_revision),
            deleted: false,
            payload: entity.payload.clone(),
            content_hash: entity.hash.clone(),
        });
    }
    for (key, state) in states {
        if local.contains_key(key) || open_conflicts.contains(key) || state.deleted != 0 {
            continue;
        }
        changes.push(PushChange {
            mutation_id: Uuid::now_v7().to_string(),
            entity_kind: state.entity_kind.clone(),
            entity_id: state.entity_id.clone(),
            base_revision: state.server_revision,
            deleted: true,
            payload: None,
            content_hash: empty_hash(),
        });
    }
    changes
}

async fn apply_remote_entity_atomically(
    connection: &mut SqliteConnection,
    database: &Database,
    remote: &RemoteEntity,
) -> CommandResult<()> {
    sqlx::query("SAVEPOINT cloud_remote_entity_apply")
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    match apply_remote_entity(connection, database, remote).await {
        Ok(()) => {
            sqlx::query("RELEASE SAVEPOINT cloud_remote_entity_apply")
                .execute(&mut *connection)
                .await
                .map_err(CommandError::database)?;
            Ok(())
        }
        Err(error) => {
            sqlx::query("ROLLBACK TO SAVEPOINT cloud_remote_entity_apply")
                .execute(&mut *connection)
                .await
                .map_err(CommandError::database)?;
            sqlx::query("RELEASE SAVEPOINT cloud_remote_entity_apply")
                .execute(&mut *connection)
                .await
                .map_err(CommandError::database)?;
            Err(error)
        }
    }
}

async fn apply_remote_entity(
    connection: &mut SqliteConnection,
    database: &Database,
    remote: &RemoteEntity,
) -> CommandResult<()> {
    if remote.deleted {
        return apply_remote_delete(connection, remote).await;
    }
    let payload = remote
        .payload
        .clone()
        .ok_or_else(|| CommandError::new("CLOUD_RESPONSE_INVALID", "云端同步内容缺失。"))?;
    match remote.entity_kind.as_str() {
        "question_type" => {
            let item: QuestionTypePayload = decode_payload(payload, "题型")?;
            if item.code != remote.entity_id {
                return Err(entity_id_mismatch());
            }
            let default_options_json = serde_json::to_string(&item.default_options)
                .map_err(|error| CommandError::validation(format!("题型默认选项无效：{error}")))?;
            sqlx::query(
                r#"
                INSERT INTO question_types (
                    code, name, name_key, behavior, is_builtin, is_enabled, sort_order,
                    default_options_json, created_at_ms, updated_at_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(code) DO UPDATE SET
                    name = excluded.name, name_key = excluded.name_key,
                    behavior = excluded.behavior, is_enabled = excluded.is_enabled,
                    sort_order = excluded.sort_order,
                    default_options_json = excluded.default_options_json,
                    updated_at_ms = excluded.updated_at_ms
                "#,
            )
            .bind(&item.code)
            .bind(&item.name)
            .bind(&item.name_key)
            .bind(&item.behavior)
            .bind(i64::from(item.is_builtin))
            .bind(i64::from(item.is_enabled))
            .bind(item.sort_order)
            .bind(default_options_json)
            .bind(item.created_at_ms)
            .bind(item.updated_at_ms)
            .execute(&mut *connection)
            .await
            .map_err(CommandError::database)?;
            sqlx::query("DELETE FROM question_type_aliases WHERE question_type_code = ?")
                .bind(&item.code)
                .execute(&mut *connection)
                .await
                .map_err(CommandError::database)?;
            for alias in item.aliases {
                sqlx::query(
                    "INSERT INTO question_type_aliases (id, question_type_code, alias, alias_key, created_at_ms) VALUES (?, ?, ?, ?, ?)",
                )
                .bind(alias.id)
                .bind(&item.code)
                .bind(alias.alias)
                .bind(alias.alias_key)
                .bind(alias.created_at_ms)
                .execute(&mut *connection)
                .await
                .map_err(CommandError::database)?;
            }
        }
        "subject" => {
            let item: SubjectPayload = decode_payload(payload, "学科")?;
            ensure_payload_id(&item.id, &remote.entity_id)?;
            sqlx::query(
                r#"
                INSERT INTO subjects (id, name, name_key, sort_order, last_accessed_at_ms, created_at_ms, updated_at_ms)
                VALUES (?, ?, ?, ?, NULL, ?, ?)
                ON CONFLICT(id) DO UPDATE SET name = excluded.name, name_key = excluded.name_key,
                    sort_order = excluded.sort_order, updated_at_ms = excluded.updated_at_ms
                "#,
            )
            .bind(item.id)
            .bind(item.name)
            .bind(item.name_key)
            .bind(item.sort_order)
            .bind(item.created_at_ms)
            .bind(item.updated_at_ms)
            .execute(&mut *connection)
            .await
            .map_err(CommandError::database)?;
        }
        "chapter" => {
            let item: ChapterPayload = decode_payload(payload, "章节")?;
            ensure_payload_id(&item.id, &remote.entity_id)?;
            sqlx::query(
                r#"
                INSERT INTO chapters (id, subject_id, name, name_key, sort_order, last_accessed_at_ms, created_at_ms, updated_at_ms)
                VALUES (?, ?, ?, ?, ?, NULL, ?, ?)
                ON CONFLICT(id) DO UPDATE SET subject_id = excluded.subject_id, name = excluded.name,
                    name_key = excluded.name_key, sort_order = excluded.sort_order,
                    updated_at_ms = excluded.updated_at_ms
                "#,
            )
            .bind(item.id)
            .bind(item.subject_id)
            .bind(item.name)
            .bind(item.name_key)
            .bind(item.sort_order)
            .bind(item.created_at_ms)
            .bind(item.updated_at_ms)
            .execute(&mut *connection)
            .await
            .map_err(CommandError::database)?;
        }
        "tag" => {
            let item: TagPayload = decode_payload(payload, "标签")?;
            ensure_payload_id(&item.id, &remote.entity_id)?;
            sqlx::query(
                r#"
                INSERT INTO tags (id, name, name_key, created_at_ms, updated_at_ms)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET name = excluded.name, name_key = excluded.name_key,
                    updated_at_ms = excluded.updated_at_ms
                "#,
            )
            .bind(item.id)
            .bind(item.name)
            .bind(item.name_key)
            .bind(item.created_at_ms)
            .bind(item.updated_at_ms)
            .execute(&mut *connection)
            .await
            .map_err(CommandError::database)?;
        }
        "resource" => {
            let item: ResourcePayload = decode_payload(payload, "图片资源")?;
            ensure_payload_id(&item.id, &remote.entity_id)?;
            let sha256 = hex_decode_32(&item.sha256_hex)?;
            let storage_rel_path = persist_resource_bytes(database, &item, &sha256)?;
            let availability = if item.data_base64.is_some() {
                "ready"
            } else {
                "missing"
            };
            sqlx::query(
                r#"
                INSERT INTO resources (
                    id, sha256, resource_kind, mime_type, storage_rel_path, original_filename,
                    byte_size, intrinsic_width_px, intrinsic_height_px, availability_status,
                    created_at_ms, last_verified_at_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET sha256 = excluded.sha256,
                    resource_kind = excluded.resource_kind, mime_type = excluded.mime_type,
                    storage_rel_path = excluded.storage_rel_path,
                    original_filename = excluded.original_filename, byte_size = excluded.byte_size,
                    intrinsic_width_px = excluded.intrinsic_width_px,
                    intrinsic_height_px = excluded.intrinsic_height_px,
                    availability_status = excluded.availability_status,
                    last_verified_at_ms = excluded.last_verified_at_ms
                "#,
            )
            .bind(item.id)
            .bind(sha256.to_vec())
            .bind(item.resource_kind)
            .bind(item.mime_type)
            .bind(storage_rel_path)
            .bind(item.original_filename)
            .bind(item.byte_size)
            .bind(item.intrinsic_width_px)
            .bind(item.intrinsic_height_px)
            .bind(availability)
            .bind(item.created_at_ms)
            .bind(Some(now_ms()))
            .execute(&mut *connection)
            .await
            .map_err(CommandError::database)?;
        }
        "question" => apply_question_payload(connection, remote, payload).await?,
        _ => {
            return Err(CommandError::new(
                "CLOUD_RESPONSE_INVALID",
                "云端实体类型无效。",
            ));
        }
    }
    Ok(())
}

async fn apply_question_payload(
    connection: &mut SqliteConnection,
    remote: &RemoteEntity,
    payload: Value,
) -> CommandResult<()> {
    let item: QuestionPayload = decode_payload(payload, "题目")?;
    ensure_payload_id(&item.id, &remote.entity_id)?;
    let stem_json = serde_json::to_string(&item.stem).map_err(json_encoding_error)?;
    let answer_json = serde_json::to_string(&item.answer).map_err(json_encoding_error)?;
    let explanation_json = serde_json::to_string(&item.explanation).map_err(json_encoding_error)?;
    let fingerprint = hex_decode_32(&item.exact_fingerprint_hex)?;
    sqlx::query(
        r#"
        INSERT INTO questions (
            id, question_type, subject_id, chapter_id, content_schema_version,
            stem_json, answer_json, explanation_json, stem_plain, options_plain,
            answer_plain, explanation_plain, tags_plain, fingerprint_version,
            exact_fingerprint, content_version, last_used_at_ms, deleted_at_ms,
            created_at_ms, updated_at_ms
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            question_type = excluded.question_type, subject_id = excluded.subject_id,
            chapter_id = excluded.chapter_id,
            content_schema_version = excluded.content_schema_version,
            stem_json = excluded.stem_json, answer_json = excluded.answer_json,
            explanation_json = excluded.explanation_json, stem_plain = excluded.stem_plain,
            options_plain = excluded.options_plain, answer_plain = excluded.answer_plain,
            explanation_plain = excluded.explanation_plain, tags_plain = excluded.tags_plain,
            fingerprint_version = excluded.fingerprint_version,
            exact_fingerprint = excluded.exact_fingerprint,
            content_version = excluded.content_version,
            last_used_at_ms = excluded.last_used_at_ms,
            deleted_at_ms = excluded.deleted_at_ms,
            updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(&item.id)
    .bind(item.question_type)
    .bind(item.subject_id)
    .bind(item.chapter_id)
    .bind(item.content_schema_version)
    .bind(stem_json)
    .bind(answer_json)
    .bind(explanation_json)
    .bind(item.stem_plain)
    .bind(item.options_plain)
    .bind(item.answer_plain)
    .bind(item.explanation_plain)
    .bind(item.tags_plain)
    .bind(item.fingerprint_version)
    .bind(fingerprint.to_vec())
    .bind(item.content_version)
    .bind(item.last_used_at_ms)
    .bind(item.deleted_at_ms)
    .bind(item.created_at_ms)
    .bind(item.updated_at_ms)
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    sqlx::query("DELETE FROM question_resource_refs WHERE question_id = ?")
        .bind(&item.id)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    sqlx::query("DELETE FROM question_options WHERE question_id = ?")
        .bind(&item.id)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    sqlx::query("DELETE FROM question_tags WHERE question_id = ?")
        .bind(&item.id)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    for option in item.options {
        if option.question_id != item.id {
            return Err(entity_id_mismatch());
        }
        let content_json = serde_json::to_string(&option.content).map_err(json_encoding_error)?;
        sqlx::query(
            "INSERT INTO question_options (id, question_id, position, content_json, plain_text, created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(option.id)
        .bind(option.question_id)
        .bind(option.position)
        .bind(content_json)
        .bind(option.plain_text)
        .bind(option.created_at_ms)
        .bind(option.updated_at_ms)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    }
    for tag in item.tags {
        if tag.question_id != item.id {
            return Err(entity_id_mismatch());
        }
        sqlx::query(
            "INSERT INTO question_tags (question_id, tag_id, created_at_ms) VALUES (?, ?, ?)",
        )
        .bind(tag.question_id)
        .bind(tag.tag_id)
        .bind(tag.created_at_ms)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    }
    for resource_ref in item.resource_refs {
        if resource_ref.question_id != item.id {
            return Err(entity_id_mismatch());
        }
        sqlx::query(
            r#"
            INSERT INTO question_resource_refs (
                id, question_id, option_id, resource_id, content_slot, node_id, created_at_ms
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(resource_ref.id)
        .bind(resource_ref.question_id)
        .bind(resource_ref.option_id)
        .bind(resource_ref.resource_id)
        .bind(resource_ref.content_slot)
        .bind(resource_ref.node_id)
        .bind(resource_ref.created_at_ms)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    }
    Ok(())
}

async fn apply_remote_delete(
    connection: &mut SqliteConnection,
    remote: &RemoteEntity,
) -> CommandResult<()> {
    let (sql, allow_builtin_guard) = match remote.entity_kind.as_str() {
        "question" => ("DELETE FROM questions WHERE id = ?", false),
        "resource" => ("DELETE FROM resources WHERE id = ?", false),
        "tag" => ("DELETE FROM tags WHERE id = ?", false),
        "chapter" => ("DELETE FROM chapters WHERE id = ?", false),
        "subject" => ("DELETE FROM subjects WHERE id = ?", false),
        "question_type" => (
            "DELETE FROM question_types WHERE code = ? AND is_builtin = 0",
            true,
        ),
        _ => {
            return Err(CommandError::new(
                "CLOUD_RESPONSE_INVALID",
                "云端实体类型无效。",
            ));
        }
    };
    let result = sqlx::query(sql)
        .bind(&remote.entity_id)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    if allow_builtin_guard && result.rows_affected() == 0 {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM question_types WHERE code = ? AND is_builtin = 1",
        )
        .bind(&remote.entity_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(CommandError::database)?;
        if exists > 0 {
            return Err(CommandError::validation("内置题型不能被云端删除。"));
        }
    }
    Ok(())
}

fn persist_resource_bytes(
    database: &Database,
    item: &ResourcePayload,
    sha256: &[u8; 32],
) -> CommandResult<String> {
    let extension = match item.mime_type.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        _ => "bin",
    };
    let relative = format!(
        "images/{}/{}.{}",
        &item.sha256_hex[..2],
        item.sha256_hex,
        extension
    );
    let Some(encoded) = &item.data_base64 else {
        return Ok(relative);
    };
    let bytes = STANDARD.decode(encoded).map_err(|_| {
        CommandError::new("CLOUD_RESPONSE_INVALID", "云端图片内容不是有效 Base64。")
    })?;
    if Sha256::digest(&bytes).as_slice() != sha256 {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云端图片摘要校验失败，已拒绝写入。",
        ));
    }
    if i64::try_from(bytes.len()).ok() != Some(item.byte_size) {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云端图片大小校验失败，已拒绝写入。",
        ));
    }
    let target = database
        .paths()
        .resources_dir()
        .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    if target.exists() {
        let existing = fs::read(&target).map_err(|error| {
            CommandError::new(
                "RESOURCE_FILE_UNAVAILABLE",
                format!("本地图片资源无法读取：{error}"),
            )
        })?;
        if Sha256::digest(existing).as_slice() != sha256 {
            return Err(CommandError::new(
                "RESOURCE_FILE_HASH_MISMATCH",
                "本地已有同名图片，但内容摘要不同。",
            ));
        }
        return Ok(relative);
    }
    let parent = target
        .parent()
        .ok_or_else(|| CommandError::new("RESOURCE_PATH_UNSAFE", "图片资源路径无效。"))?;
    fs::create_dir_all(parent).map_err(|error| {
        CommandError::new(
            "RESOURCE_FILE_UNAVAILABLE",
            format!("无法创建图片资源目录：{error}"),
        )
    })?;
    let temporary = parent.join(format!(".cloud-resource-{}.tmp", Uuid::now_v7()));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result.map_err(|error| {
        CommandError::new(
            "RESOURCE_FILE_UNAVAILABLE",
            format!("云端图片无法安全保存：{error}"),
        )
    })?;
    Ok(relative)
}

async fn load_sync_state(
    connection: &mut SqliteConnection,
    account_id: &str,
) -> CommandResult<HashMap<String, SyncStateRow>> {
    let rows = sqlx::query_as::<_, SyncStateRow>(
        r#"
        SELECT entity_kind, entity_id, server_revision, deleted, synced_hash
        FROM cloud_sync_state WHERE account_id = ?
        "#,
    )
    .bind(account_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(rows
        .into_iter()
        .map(|row| (entity_key(&row.entity_kind, &row.entity_id), row))
        .collect())
}

async fn latest_remote_entity_from_state(
    connection: &mut SqliteConnection,
    account_id: &str,
    entity_kind: &str,
    entity_id: &str,
) -> CommandResult<Option<RemoteEntity>> {
    let row = sqlx::query_as::<_, (i64, i64, String, Option<String>)>(
        r#"
        SELECT server_revision, deleted, synced_hash, synced_payload_json
        FROM cloud_sync_state
        WHERE account_id = ? AND entity_kind = ? AND entity_id = ?
        "#,
    )
    .bind(account_id)
    .bind(entity_kind)
    .bind(entity_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    let Some((revision, deleted, content_hash, payload_json)) = row else {
        return Ok(None);
    };
    let payload = payload_json
        .as_deref()
        .map(|value| parse_json(value, "云端同步状态"))
        .transpose()?;
    let remote = RemoteEntity {
        entity_kind: entity_kind.to_owned(),
        entity_id: entity_id.to_owned(),
        revision,
        change_cursor: 1,
        deleted: deleted != 0,
        payload,
        content_hash,
    };
    verify_remote_entity(&remote)?;
    Ok(Some(remote))
}

async fn load_open_conflict_keys(
    connection: &mut SqliteConnection,
    account_id: &str,
) -> CommandResult<HashSet<String>> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT entity_kind, entity_id FROM cloud_sync_conflicts WHERE account_id = ? AND resolved_at_ms IS NULL",
    )
    .bind(account_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(rows
        .into_iter()
        .map(|(kind, id)| entity_key(&kind, &id))
        .collect())
}

async fn load_open_conflict_keys_by_kind(
    connection: &mut SqliteConnection,
    account_id: &str,
    conflict_kind: &str,
) -> CommandResult<HashSet<String>> {
    let rows = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT entity_kind, entity_id FROM cloud_sync_conflicts
        WHERE account_id = ? AND conflict_kind = ? AND resolved_at_ms IS NULL
        "#,
    )
    .bind(account_id)
    .bind(conflict_kind)
    .fetch_all(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(rows
        .into_iter()
        .map(|(kind, id)| entity_key(&kind, &id))
        .collect())
}

async fn upsert_sync_state(
    connection: &mut SqliteConnection,
    account_id: &str,
    remote: &RemoteEntity,
) -> CommandResult<()> {
    let payload_json = remote
        .payload
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(json_encoding_error)?;
    sqlx::query(
        r#"
        INSERT INTO cloud_sync_state (
            account_id, entity_kind, entity_id, server_revision, deleted,
            synced_hash, synced_payload_json, updated_at_ms
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, entity_kind, entity_id) DO UPDATE SET
            server_revision = excluded.server_revision,
            deleted = excluded.deleted,
            synced_hash = excluded.synced_hash,
            synced_payload_json = excluded.synced_payload_json,
            updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(account_id)
    .bind(&remote.entity_kind)
    .bind(&remote.entity_id)
    .bind(remote.revision)
    .bind(i64::from(remote.deleted))
    .bind(&remote.content_hash)
    .bind(payload_json)
    .bind(now_ms())
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

async fn upsert_pull_cursor(
    connection: &mut SqliteConnection,
    account_id: &str,
    cursor: i64,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        INSERT INTO cloud_sync_meta (account_id, pull_cursor, updated_at_ms)
        VALUES (?, ?, ?)
        ON CONFLICT(account_id) DO UPDATE SET
            pull_cursor = excluded.pull_cursor,
            updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(account_id)
    .bind(cursor)
    .bind(now_ms())
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

async fn resolve_deferred_apply(
    connection: &mut SqliteConnection,
    account_id: &str,
    entity_kind: &str,
    entity_id: &str,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        UPDATE cloud_sync_conflicts SET resolved_at_ms = ?
        WHERE account_id = ? AND entity_kind = ? AND entity_id = ?
          AND conflict_kind = 'deferred_apply' AND resolved_at_ms IS NULL
        "#,
    )
    .bind(now_ms())
    .bind(account_id)
    .bind(entity_kind)
    .bind(entity_id)
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

async fn promote_deferred_apply_to_content_conflict(
    connection: &mut SqliteConnection,
    account_id: &str,
    local: Option<&LocalEntity>,
    entity_kind: &str,
    entity_id: &str,
) -> CommandResult<()> {
    let local_payload = local
        .and_then(|entity| entity.payload.as_ref())
        .map(serde_json::to_string)
        .transpose()
        .map_err(json_encoding_error)?;
    sqlx::query(
        r#"
        UPDATE cloud_sync_conflicts
        SET conflict_kind = 'content', local_payload_json = ?,
            detected_at_ms = ?,
            message = '等待自动补全期间本机出现了不同内容，已停止覆盖并保留双方内容。'
        WHERE account_id = ? AND entity_kind = ? AND entity_id = ?
          AND conflict_kind = 'deferred_apply' AND resolved_at_ms IS NULL
        "#,
    )
    .bind(local_payload)
    .bind(now_ms())
    .bind(account_id)
    .bind(entity_kind)
    .bind(entity_id)
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

async fn record_conflict(
    connection: &mut SqliteConnection,
    account_id: &str,
    local: Option<&LocalEntity>,
    remote: &RemoteEntity,
    conflict_kind: &str,
    message: String,
) -> CommandResult<()> {
    let existing = sqlx::query_as::<_, (String, Option<String>)>(
        r#"
        SELECT conflict_kind, local_payload_json FROM cloud_sync_conflicts
        WHERE account_id = ? AND entity_kind = ? AND entity_id = ?
          AND resolved_at_ms IS NULL
        ORDER BY detected_at_ms DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(account_id)
    .bind(&remote.entity_kind)
    .bind(&remote.entity_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    let current_local_payload = local
        .and_then(|entity| entity.payload.as_ref())
        .map(serde_json::to_string)
        .transpose()
        .map_err(json_encoding_error)?;
    // A deferred apply uses the first observed local state as its baseline.
    // Repeated pulls may refresh the cloud revision, but must not move that
    // baseline forward or a genuine local edit would become invisible.
    let local_payload = match existing.as_ref() {
        Some((existing_kind, existing_local))
            if existing_kind == CONFLICT_KIND_DEFERRED_APPLY
                && conflict_kind == CONFLICT_KIND_DEFERRED_APPLY =>
        {
            existing_local.clone()
        }
        _ => current_local_payload,
    };
    let remote_payload = serde_json::to_string(&json!({
        "deleted": remote.deleted,
        "payload": remote.payload,
        "revision": remote.revision,
        "contentHash": remote.content_hash,
    }))
    .map_err(json_encoding_error)?;
    if existing.is_some() {
        sqlx::query(
            r#"
            UPDATE cloud_sync_conflicts
            SET local_payload_json = ?, remote_payload_json = ?,
                detected_at_ms = ?, conflict_kind = ?, message = ?
            WHERE account_id = ? AND entity_kind = ? AND entity_id = ?
              AND resolved_at_ms IS NULL
            "#,
        )
        .bind(local_payload)
        .bind(remote_payload)
        .bind(now_ms())
        .bind(conflict_kind)
        .bind(message)
        .bind(account_id)
        .bind(&remote.entity_kind)
        .bind(&remote.entity_id)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
        return Ok(());
    }
    sqlx::query(
        r#"
        INSERT INTO cloud_sync_conflicts (
            id, account_id, entity_kind, entity_id, local_payload_json,
            remote_payload_json, detected_at_ms, conflict_kind, message
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::now_v7().to_string())
    .bind(account_id)
    .bind(&remote.entity_kind)
    .bind(&remote.entity_id)
    .bind(local_payload)
    .bind(remote_payload)
    .bind(now_ms())
    .bind(conflict_kind)
    .bind(message)
    .execute(&mut *connection)
    .await
    .map_err(CommandError::database)?;
    Ok(())
}

fn verify_pull_page(page: &PullResponse, previous_cursor: i64) -> CommandResult<()> {
    if page.next_cursor < previous_cursor {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云服务返回了倒退的同步游标。",
        ));
    }
    let mut prior = previous_cursor;
    for entity in &page.changes {
        verify_remote_entity(entity)?;
        if entity.change_cursor <= prior {
            return Err(CommandError::new(
                "CLOUD_RESPONSE_INVALID",
                "云服务返回的同步变更顺序无效。",
            ));
        }
        prior = entity.change_cursor;
    }
    if page.changes.last().map(|item| item.change_cursor) != Some(page.next_cursor)
        && !page.changes.is_empty()
    {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云服务返回的同步游标与内容不一致。",
        ));
    }
    if page.has_more && page.changes.is_empty() {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云服务返回了无法继续的同步分页。",
        ));
    }
    Ok(())
}

fn verify_remote_entity(remote: &RemoteEntity) -> CommandResult<()> {
    if !KINDS_IN_APPLY_ORDER.contains(&remote.entity_kind.as_str())
        || remote.revision < 1
        || remote.change_cursor < 1
        || remote.deleted != remote.payload.is_none()
    {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云服务返回的同步实体无效。",
        ));
    }
    let expected = if let Some(payload) = &remote.payload {
        hash_payload(payload)?
    } else {
        empty_hash()
    };
    if expected != remote.content_hash {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云服务同步内容摘要不一致，已停止写入。",
        ));
    }
    Ok(())
}

fn remote_matches_local(remote: &RemoteEntity, local: Option<&LocalEntity>) -> bool {
    if remote.deleted {
        local.is_none()
    } else {
        local.is_some_and(|entity| entity.hash == remote.content_hash)
    }
}

fn local_differs_from_state(local: Option<&LocalEntity>, state: Option<&SyncStateRow>) -> bool {
    match (local, state) {
        (None, None) => false,
        (Some(_), None) => true,
        (None, Some(state)) => state.deleted == 0,
        (Some(entity), Some(state)) => state.deleted != 0 || entity.hash != state.synced_hash,
    }
}

fn is_retryable_dependency_error(error: &CommandError) -> bool {
    error.code == "DATABASE_ERROR"
        && (error.message.contains("FOREIGN KEY constraint failed")
            || error.message.contains("(code: 787)"))
}

fn apply_order_key(remote: &RemoteEntity) -> usize {
    let normal = KINDS_IN_APPLY_ORDER
        .iter()
        .position(|kind| *kind == remote.entity_kind)
        .unwrap_or(usize::MAX / 2);
    if remote.deleted {
        KINDS_IN_APPLY_ORDER.len().saturating_sub(normal)
    } else {
        normal
    }
}

#[cfg(test)]
fn push_order_key(change: &PushChange) -> usize {
    let normal = KINDS_IN_APPLY_ORDER
        .iter()
        .position(|kind| *kind == change.entity_kind)
        .unwrap_or(usize::MAX / 2);
    if change.deleted {
        KINDS_IN_APPLY_ORDER.len().saturating_sub(normal)
    } else {
        normal
    }
}

fn parse_json(value: &str, label: &str) -> CommandResult<Value> {
    serde_json::from_str(value).map_err(|error| {
        CommandError::new(
            "CONTENT_CORRUPTED",
            format!("{label} JSON 无法读取：{error}"),
        )
    })
}

fn decode_payload<T: for<'de> Deserialize<'de>>(value: Value, label: &str) -> CommandResult<T> {
    serde_json::from_value(value).map_err(|error| {
        CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            format!("云端{label}内容无法识别：{error}"),
        )
    })
}

fn hash_payload(payload: &Value) -> CommandResult<String> {
    let bytes = serde_json::to_vec(payload).map_err(json_encoding_error)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn empty_hash() -> String {
    format!("{:x}", Sha256::digest([]))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode_32(value: &str) -> CommandResult<[u8; 32]> {
    if value.len() != 64 {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云端摘要长度无效。",
        ));
    }
    let mut bytes = [0_u8; 32];
    for (index, target) in bytes.iter_mut().enumerate() {
        *target = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| CommandError::new("CLOUD_RESPONSE_INVALID", "云端摘要格式无效。"))?;
    }
    Ok(bytes)
}

fn ensure_payload_id(actual: &str, expected: &str) -> CommandResult<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(entity_id_mismatch())
    }
}

fn entity_id_mismatch() -> CommandError {
    CommandError::new("CLOUD_RESPONSE_INVALID", "云端同步实体标识与内容不一致。")
}

fn json_encoding_error(error: serde_json::Error) -> CommandError {
    CommandError::new(
        "CLOUD_SYNC_ENCODING_FAILED",
        format!("同步 JSON 无法编码：{error}"),
    )
}

fn entity_key(kind: &str, id: &str) -> String {
    format!("{kind}\u{1f}{id}")
}

fn split_entity_key(value: &str) -> Option<(&str, &str)> {
    value.split_once('\u{1f}')
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::{fs, thread, time::Duration};

    use super::*;

    fn regression_remote(kind: &str, id: &str, payload: Value, revision: i64) -> RemoteEntity {
        RemoteEntity {
            entity_kind: kind.to_owned(),
            entity_id: id.to_owned(),
            revision,
            change_cursor: revision,
            deleted: false,
            content_hash: hash_payload(&payload).unwrap(),
            payload: Some(payload),
        }
    }

    fn regression_subject(id: &str, name: &str, revision: i64) -> RemoteEntity {
        regression_remote(
            "subject",
            id,
            json!({
                "id": id,
                "name": name,
                "nameKey": name,
                "sortOrder": 0,
                "createdAtMs": 1,
                "updatedAtMs": revision,
            }),
            revision,
        )
    }

    fn regression_chapter(id: &str, subject_id: &str, name: &str) -> RemoteEntity {
        regression_remote(
            "chapter",
            id,
            json!({
                "id": id,
                "subjectId": subject_id,
                "name": name,
                "nameKey": name,
                "sortOrder": 0,
                "createdAtMs": 1,
                "updatedAtMs": 1,
            }),
            1,
        )
    }

    fn regression_question(
        id: &str,
        subject_id: &str,
        chapter_id: &str,
        revision: i64,
    ) -> RemoteEntity {
        let payload = serde_json::to_value(QuestionPayload {
            id: id.to_owned(),
            question_type: "short_answer".to_owned(),
            subject_id: subject_id.to_owned(),
            chapter_id: chapter_id.to_owned(),
            content_schema_version: 1,
            stem: json!({"schemaVersion": 1, "html": "<p>题干</p>", "plainText": "题干"}),
            answer: json!({"schemaVersion": 1, "html": "<p>答案</p>", "plainText": "答案"}),
            explanation: json!({"schemaVersion": 1, "html": "", "plainText": ""}),
            stem_plain: "题干".to_owned(),
            options_plain: String::new(),
            answer_plain: "答案".to_owned(),
            explanation_plain: String::new(),
            tags_plain: String::new(),
            fingerprint_version: 1,
            exact_fingerprint_hex: "00".repeat(32),
            content_version: revision,
            last_used_at_ms: None,
            deleted_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: revision,
            options: Vec::new(),
            tags: Vec::new(),
            resource_refs: Vec::new(),
        })
        .unwrap();
        regression_remote("question", id, payload, revision)
    }

    async fn apply_regression_remote(database: &Database, remote: &RemoteEntity) {
        let mut transaction = database.pool().begin().await.unwrap();
        apply_remote_entity(&mut transaction, database, remote)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }

    async fn cleanup_regression_database(database: Database, root: &std::path::Path) {
        database.close().await;
        for attempt in 0..80 {
            match fs::remove_dir_all(root) {
                Ok(()) => return,
                Err(error) if error.raw_os_error() == Some(32) && attempt < 79 => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean cloud regression test root: {error}"),
            }
        }
    }

    #[test]
    fn tombstones_are_ordered_after_dependent_questions() {
        let question = PushChange {
            mutation_id: "1".to_owned(),
            entity_kind: "question".to_owned(),
            entity_id: "q".to_owned(),
            base_revision: 1,
            deleted: true,
            payload: None,
            content_hash: empty_hash(),
        };
        let subject = PushChange {
            entity_kind: "subject".to_owned(),
            ..question.clone()
        };
        assert!(push_order_key(&subject) > push_order_key(&question));
    }

    #[test]
    fn local_state_comparison_detects_edits_and_deletes() {
        let entity = LocalEntity {
            kind: "tag".to_owned(),
            local_id: "id".to_owned(),
            id: "id".to_owned(),
            payload: Some(json!({"name": "A"})),
            hash: "a".repeat(64),
        };
        let state = SyncStateRow {
            entity_kind: "tag".to_owned(),
            entity_id: "id".to_owned(),
            server_revision: 1,
            deleted: 0,
            synced_hash: "b".repeat(64),
        };
        assert!(local_differs_from_state(Some(&entity), Some(&state)));
        assert!(local_differs_from_state(None, Some(&state)));
    }

    #[test]
    fn push_batches_respect_count_and_encoded_size_limits() {
        let changes = (0..101)
            .map(|index| PushChange {
                mutation_id: Uuid::now_v7().to_string(),
                entity_kind: "tag".to_owned(),
                entity_id: Uuid::now_v7().to_string(),
                base_revision: 0,
                deleted: false,
                payload: Some(json!({"index": index})),
                content_hash: "a".repeat(64),
            })
            .collect();
        let batches = build_push_batches(changes).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 100);
        assert_eq!(batches[1].len(), 1);
    }

    #[tokio::test]
    async fn merged_alias_tombstone_replaces_stale_state_without_a_local_entity() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-cloud-merged-tombstone-test-{}",
            Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let account_id = Uuid::now_v7().to_string();
        let alias_id = Uuid::now_v7().to_string();
        let canonical_id = Uuid::now_v7().to_string();
        let payload = json!({
            "id": canonical_id.clone(),
            "name": "数学",
            "nameKey": "数学",
            "sortOrder": 0,
            "createdAtMs": 1,
            "updatedAtMs": 2
        });
        let current = RemoteEntity {
            entity_kind: "subject".to_owned(),
            entity_id: canonical_id.clone(),
            revision: 2,
            change_cursor: 10,
            deleted: false,
            content_hash: hash_payload(&payload).unwrap(),
            payload: Some(payload),
        };
        sqlx::query(
            r#"
            INSERT INTO cloud_sync_state (
                account_id, entity_kind, entity_id, server_revision, deleted,
                synced_hash, synced_payload_json, updated_at_ms
            ) VALUES (?, 'subject', ?, 1, 0, ?, '{}', 1)
            "#,
        )
        .bind(&account_id)
        .bind(&alias_id)
        .bind("a".repeat(64))
        .execute(database.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO cloud_entity_mappings (
                account_id, entity_kind, local_entity_id, cloud_entity_id,
                created_at_ms, updated_at_ms
            ) VALUES (?, 'subject', ?, ?, 1, 1)
            "#,
        )
        .bind(&account_id)
        .bind(&alias_id)
        .bind(&alias_id)
        .execute(database.pool())
        .await
        .unwrap();

        let mut transaction = database.pool().begin().await.unwrap();
        reconcile_merged_tombstone(
            &mut transaction,
            &account_id,
            &entity_key("subject", &alias_id),
            &current,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM cloud_sync_state WHERE account_id = ? AND entity_kind = 'subject' AND entity_id = ?",
            )
            .bind(&account_id)
            .bind(&alias_id)
            .fetch_one(database.pool())
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM cloud_entity_mappings WHERE account_id = ? AND entity_kind = 'subject' AND cloud_entity_id = ?",
            )
            .bind(&account_id)
            .bind(&alias_id)
            .fetch_one(database.pool())
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT server_revision FROM cloud_sync_state WHERE account_id = ? AND entity_kind = 'subject' AND entity_id = ?",
            )
            .bind(&account_id)
            .bind(&canonical_id)
            .fetch_one(database.pool())
            .await
            .unwrap(),
            2
        );

        database.close().await;
        for attempt in 0..80 {
            match fs::remove_dir_all(&root) {
                Ok(()) => return,
                Err(error) if error.raw_os_error() == Some(32) && attempt < 79 => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean merged tombstone test root: {error}"),
            }
        }
    }

    #[test]
    fn canonical_payload_translates_relationships_without_rewriting_local_ids() {
        let mut mappings = EntityMappings::default();
        mappings.insert("subject", "local-subject", "cloud-subject");
        mappings.insert("chapter", "local-chapter", "cloud-chapter");
        mappings.insert("tag", "local-tag", "cloud-tag");
        mappings.insert("resource", "local-resource", "cloud-resource");
        mappings.insert("question", "local-question", "cloud-question");
        let mut payload = json!({
            "id": "local-question",
            "subjectId": "local-subject",
            "chapterId": "local-chapter",
            "stem": {
                "html": "<p><img data-resource-id=\"local-resource\"></p>",
                "document": {"type": "doc", "content": [{"type": "image", "attrs": {"resourceId": "local-resource"}}]}
            },
            "answer": {
                "html": "<img data-resource-id = 'local-resource'>",
                "document": {"attrs": {"resourceId": "local-resource"}}
            },
            "explanation": {
                "html": "<img DATA-RESOURCE-ID=local-resource>",
                "document": {"attrs": {"resourceId": "local-resource"}}
            },
            "options": [{
                "questionId": "local-question",
                "content": {
                    "html": "<img data-resource-id=\"local-resource\">",
                    "document": {"attrs": {"resourceId": "local-resource"}}
                }
            }],
            "tags": [{"questionId": "local-question", "tagId": "local-tag"}],
            "resourceRefs": [{
                "questionId": "local-question",
                "resourceId": "local-resource"
            }]
        });

        canonicalize_local_payload(&mut payload, "question", "cloud-question", &mappings).unwrap();
        assert_eq!(payload["id"], "cloud-question");
        assert_eq!(payload["subjectId"], "cloud-subject");
        assert_eq!(payload["chapterId"], "cloud-chapter");
        assert_eq!(payload["tags"][0]["tagId"], "cloud-tag");
        assert_eq!(payload["resourceRefs"][0]["resourceId"], "cloud-resource");
        for content in [
            &payload["stem"],
            &payload["answer"],
            &payload["explanation"],
            &payload["options"][0]["content"],
        ] {
            assert!(content["html"].as_str().unwrap().contains("cloud-resource"));
            assert_eq!(
                content["document"]
                    .get("content")
                    .and_then(|items| items.get(0))
                    .and_then(|item| item.get("attrs"))
                    .unwrap_or(&content["document"]["attrs"])["resourceId"],
                "cloud-resource"
            );
        }

        let remote = RemoteEntity {
            entity_kind: "question".to_owned(),
            entity_id: "cloud-question".to_owned(),
            revision: 1,
            change_cursor: 1,
            deleted: false,
            payload: Some(payload),
            content_hash: "a".repeat(64),
        };
        let localized = localize_remote_entity(&remote, &mappings).unwrap();
        let localized_payload = localized.payload.unwrap();
        assert_eq!(localized.entity_id, "local-question");
        assert_eq!(localized_payload["subjectId"], "local-subject");
        assert_eq!(localized_payload["chapterId"], "local-chapter");
        assert_eq!(localized_payload["tags"][0]["tagId"], "local-tag");
        assert_eq!(
            localized_payload["resourceRefs"][0]["resourceId"],
            "local-resource"
        );
        for content in [
            &localized_payload["stem"],
            &localized_payload["answer"],
            &localized_payload["explanation"],
            &localized_payload["options"][0]["content"],
        ] {
            assert!(content["html"].as_str().unwrap().contains("local-resource"));
            assert_eq!(
                content["document"]
                    .get("content")
                    .and_then(|items| items.get(0))
                    .and_then(|item| item.get("attrs"))
                    .unwrap_or(&content["document"]["attrs"])["resourceId"],
                "local-resource"
            );
        }
    }

    #[tokio::test]
    async fn fresh_database_has_sync_schema_and_binds_only_one_account() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-cloud-sync-test-{}",
            Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let account_id = Uuid::now_v7().to_string();
        let entities = capture_entities(&database, &account_id, &HashMap::new())
            .await
            .unwrap();
        assert!(entities.contains_key(&entity_key("question_type", "true_false")));

        let first_id = Uuid::now_v7().to_string();
        ensure_database_binding(&database, &first_id, "teacher_a")
            .await
            .unwrap();
        ensure_database_binding(&database, &first_id, "teacher_a")
            .await
            .unwrap();
        let error = ensure_database_binding(&database, &Uuid::now_v7().to_string(), "teacher_b")
            .await
            .unwrap_err();
        assert_eq!(error.code, "CLOUD_DATABASE_ACCOUNT_MISMATCH");

        database.close().await;
        for attempt in 0..80 {
            match fs::remove_dir_all(&root) {
                Ok(()) => return,
                Err(error) if error.raw_os_error() == Some(32) && attempt < 79 => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean cloud sync test root: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn same_named_remote_taxonomy_maps_to_local_ids_before_fk_writes() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-cloud-identity-test-{}",
            Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let account_id = Uuid::now_v7().to_string();
        let local_subject = Uuid::now_v7().to_string();
        let local_chapter = Uuid::now_v7().to_string();
        let cloud_subject = Uuid::now_v7().to_string();
        let cloud_chapter = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO subjects (id, name, name_key, sort_order, created_at_ms, updated_at_ms) VALUES (?, '数学', '数学', 0, 1, 1)",
        )
        .bind(&local_subject)
        .execute(database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chapters (id, subject_id, name, name_key, sort_order, created_at_ms, updated_at_ms) VALUES (?, ?, '第一章', '第一章', 0, 1, 1)",
        )
        .bind(&local_chapter)
        .bind(&local_subject)
        .execute(database.pool())
        .await
        .unwrap();

        let subject_payload = serde_json::to_value(SubjectPayload {
            id: cloud_subject.clone(),
            name: "数学".to_owned(),
            name_key: "数学".to_owned(),
            sort_order: 0,
            created_at_ms: 2,
            updated_at_ms: 2,
        })
        .unwrap();
        let chapter_payload = serde_json::to_value(ChapterPayload {
            id: cloud_chapter.clone(),
            subject_id: cloud_subject.clone(),
            name: "第一章".to_owned(),
            name_key: "第一章".to_owned(),
            sort_order: 0,
            created_at_ms: 2,
            updated_at_ms: 2,
        })
        .unwrap();
        let changes = vec![
            RemoteEntity {
                entity_kind: "subject".to_owned(),
                entity_id: cloud_subject.clone(),
                revision: 1,
                change_cursor: 1,
                deleted: false,
                content_hash: hash_payload(&subject_payload).unwrap(),
                payload: Some(subject_payload),
            },
            RemoteEntity {
                entity_kind: "chapter".to_owned(),
                entity_id: cloud_chapter.clone(),
                revision: 1,
                change_cursor: 2,
                deleted: false,
                content_hash: hash_payload(&chapter_payload).unwrap(),
                payload: Some(chapter_payload),
            },
        ];

        let merged = prepare_remote_mappings(&database, &account_id, &changes)
            .await
            .unwrap();
        assert!(merged.contains(&entity_key("subject", &cloud_subject)));
        assert!(merged.contains(&entity_key("chapter", &cloud_chapter)));
        let mut transaction = database.pool().begin().await.unwrap();
        let mappings = load_entity_mappings(&mut transaction, &account_id)
            .await
            .unwrap();
        let local_subject_change = localize_remote_entity(&changes[0], &mappings).unwrap();
        let local_chapter_change = localize_remote_entity(&changes[1], &mappings).unwrap();
        assert_eq!(local_subject_change.entity_id, local_subject);
        assert_eq!(local_chapter_change.entity_id, local_chapter);
        assert_eq!(
            local_chapter_change.payload.as_ref().unwrap()["subjectId"],
            local_subject
        );
        apply_remote_entity(&mut transaction, &database, &local_subject_change)
            .await
            .unwrap();
        apply_remote_entity(&mut transaction, &database, &local_chapter_change)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM subjects")
                .fetch_one(database.pool())
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM chapters")
                .fetch_one(database.pool())
                .await
                .unwrap(),
            1
        );

        database.close().await;
        for attempt in 0..80 {
            match fs::remove_dir_all(&root) {
                Ok(()) => return,
                Err(error) if error.raw_os_error() == Some(32) && attempt < 79 => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean cloud identity test root: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn deferred_question_from_an_earlier_page_retries_after_parents_arrive() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-cloud-deferred-page-test-{}",
            Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let account_id = Uuid::now_v7().to_string();
        let subject_id = Uuid::now_v7().to_string();
        let chapter_id = Uuid::now_v7().to_string();
        let question_id = Uuid::now_v7().to_string();
        let question_payload = serde_json::to_value(QuestionPayload {
            id: question_id.clone(),
            question_type: "short_answer".to_owned(),
            subject_id: subject_id.clone(),
            chapter_id: chapter_id.clone(),
            content_schema_version: 1,
            stem: json!({"schemaVersion": 1, "html": "题干", "plainText": "题干"}),
            answer: json!({"schemaVersion": 1, "html": "答案", "plainText": "答案"}),
            explanation: json!({"schemaVersion": 1, "html": "", "plainText": ""}),
            stem_plain: "题干".to_owned(),
            options_plain: String::new(),
            answer_plain: "答案".to_owned(),
            explanation_plain: String::new(),
            tags_plain: String::new(),
            fingerprint_version: 1,
            exact_fingerprint_hex: "00".repeat(32),
            content_version: 1,
            last_used_at_ms: None,
            deleted_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            options: Vec::new(),
            tags: Vec::new(),
            resource_refs: Vec::new(),
        })
        .unwrap();
        let question = RemoteEntity {
            entity_kind: "question".to_owned(),
            entity_id: question_id.clone(),
            revision: 1,
            change_cursor: 1,
            deleted: false,
            content_hash: hash_payload(&question_payload).unwrap(),
            payload: Some(question_payload),
        };

        prepare_remote_mappings(&database, &account_id, std::slice::from_ref(&question))
            .await
            .unwrap();
        let mut transaction = database.pool().begin().await.unwrap();
        let mappings = load_entity_mappings(&mut transaction, &account_id)
            .await
            .unwrap();
        let localized = localize_remote_entity(&question, &mappings).unwrap();
        let apply_error = apply_remote_entity_atomically(&mut transaction, &database, &localized)
            .await
            .unwrap_err();
        assert!(is_retryable_dependency_error(&apply_error));
        record_conflict(
            &mut transaction,
            &account_id,
            None,
            &question,
            CONFLICT_KIND_DEFERRED_APPLY,
            format!("云端内容等待依赖数据后自动补全：{}", apply_error.message),
        )
        .await
        .unwrap();
        upsert_sync_state(&mut transaction, &account_id, &question)
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        let first_retry = retry_deferred_applies(&database, &account_id)
            .await
            .unwrap();
        assert_eq!(first_retry.recovered_count, 0);
        assert_eq!(first_retry.pending_count, 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM questions WHERE id = ?")
                .bind(&question_id)
                .fetch_one(database.pool())
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM cloud_sync_conflicts WHERE account_id = ? AND conflict_kind = 'content' AND resolved_at_ms IS NULL",
            )
            .bind(&account_id)
            .fetch_one(database.pool())
            .await
            .unwrap(),
            0
        );

        let subject_payload = serde_json::to_value(SubjectPayload {
            id: subject_id.clone(),
            name: "数学".to_owned(),
            name_key: "数学".to_owned(),
            sort_order: 0,
            created_at_ms: 1,
            updated_at_ms: 1,
        })
        .unwrap();
        let chapter_payload = serde_json::to_value(ChapterPayload {
            id: chapter_id.clone(),
            subject_id: subject_id.clone(),
            name: "第一章".to_owned(),
            name_key: "第一章".to_owned(),
            sort_order: 0,
            created_at_ms: 1,
            updated_at_ms: 1,
        })
        .unwrap();
        let parents = vec![
            RemoteEntity {
                entity_kind: "subject".to_owned(),
                entity_id: subject_id.clone(),
                revision: 1,
                change_cursor: PULL_LIMIT + 1,
                deleted: false,
                content_hash: hash_payload(&subject_payload).unwrap(),
                payload: Some(subject_payload),
            },
            RemoteEntity {
                entity_kind: "chapter".to_owned(),
                entity_id: chapter_id.clone(),
                revision: 1,
                change_cursor: PULL_LIMIT + 2,
                deleted: false,
                content_hash: hash_payload(&chapter_payload).unwrap(),
                payload: Some(chapter_payload),
            },
        ];
        prepare_remote_mappings(&database, &account_id, &parents)
            .await
            .unwrap();
        let mut transaction = database.pool().begin().await.unwrap();
        let mappings = load_entity_mappings(&mut transaction, &account_id)
            .await
            .unwrap();
        for parent in &parents {
            let localized = localize_remote_entity(parent, &mappings).unwrap();
            apply_remote_entity_atomically(&mut transaction, &database, &localized)
                .await
                .unwrap();
            upsert_sync_state(&mut transaction, &account_id, parent)
                .await
                .unwrap();
        }
        transaction.commit().await.unwrap();

        let final_retry = retry_deferred_applies(&database, &account_id)
            .await
            .unwrap();
        assert_eq!(final_retry.recovered_count, 1);
        assert_eq!(final_retry.promoted_count, 0);
        assert_eq!(final_retry.pending_count, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM questions WHERE id = ?")
                .bind(&question_id)
                .fetch_one(database.pool())
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM cloud_sync_conflicts WHERE account_id = ? AND conflict_kind = 'deferred_apply' AND resolved_at_ms IS NULL",
            )
            .bind(&account_id)
            .fetch_one(database.pool())
            .await
            .unwrap(),
            0
        );
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(database.pool())
                .await
                .unwrap()
                .is_empty()
        );

        database.close().await;
        for attempt in 0..80 {
            match fs::remove_dir_all(&root) {
                Ok(()) => return,
                Err(error) if error.raw_os_error() == Some(32) && attempt < 79 => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean deferred page test root: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn deferred_existing_question_retries_when_local_content_is_unchanged() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-cloud-deferred-existing-test-{}",
            Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let account_id = Uuid::now_v7().to_string();
        let subject_id = Uuid::now_v7().to_string();
        let old_chapter_id = Uuid::now_v7().to_string();
        let new_chapter_id = Uuid::now_v7().to_string();
        let question_id = Uuid::now_v7().to_string();

        apply_regression_remote(&database, &regression_subject(&subject_id, "测试学科", 1)).await;
        apply_regression_remote(
            &database,
            &regression_chapter(&old_chapter_id, &subject_id, "旧章节"),
        )
        .await;
        let original = regression_question(&question_id, &subject_id, &old_chapter_id, 1);
        apply_regression_remote(&database, &original).await;

        let states = HashMap::new();
        let locals = capture_entities(&database, &account_id, &states)
            .await
            .unwrap();
        let updated = regression_question(&question_id, &subject_id, &new_chapter_id, 2);
        let mut transaction = database.pool().begin().await.unwrap();
        let error = apply_remote_entity_atomically(&mut transaction, &database, &updated)
            .await
            .unwrap_err();
        assert!(is_retryable_dependency_error(&error));
        record_conflict(
            &mut transaction,
            &account_id,
            locals.get(&entity_key("question", &question_id)),
            &updated,
            CONFLICT_KIND_DEFERRED_APPLY,
            "等待章节同步后自动补全".to_owned(),
        )
        .await
        .unwrap();
        upsert_sync_state(&mut transaction, &account_id, &updated)
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        apply_regression_remote(
            &database,
            &regression_chapter(&new_chapter_id, &subject_id, "新章节"),
        )
        .await;
        let result = retry_deferred_applies(&database, &account_id)
            .await
            .unwrap();
        let actual_chapter =
            sqlx::query_scalar::<_, String>("SELECT chapter_id FROM questions WHERE id = ?")
                .bind(&question_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(result.recovered_count, 1);
        assert_eq!(result.promoted_count, 0);
        assert_eq!(result.pending_count, 0);
        assert_eq!(actual_chapter, new_chapter_id);

        cleanup_regression_database(database, &root).await;
    }

    #[tokio::test]
    async fn open_content_conflict_tracks_the_latest_cloud_revision() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-cloud-latest-conflict-test-{}",
            Uuid::now_v7().simple()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let account_id = Uuid::now_v7().to_string();
        let subject_id = Uuid::now_v7().to_string();
        let old = regression_subject(&subject_id, "云端第二版", 2);
        let latest = regression_subject(&subject_id, "云端第三版", 3);

        let mut transaction = database.pool().begin().await.unwrap();
        record_conflict(
            &mut transaction,
            &account_id,
            None,
            &old,
            CONFLICT_KIND_CONTENT,
            "测试冲突".to_owned(),
        )
        .await
        .unwrap();
        upsert_sync_state(&mut transaction, &account_id, &old)
            .await
            .unwrap();
        record_conflict(
            &mut transaction,
            &account_id,
            None,
            &latest,
            CONFLICT_KIND_CONTENT,
            "测试冲突".to_owned(),
        )
        .await
        .unwrap();
        upsert_sync_state(&mut transaction, &account_id, &latest)
            .await
            .unwrap();

        let stored_json = sqlx::query_scalar::<_, String>(
            "SELECT remote_payload_json FROM cloud_sync_conflicts WHERE account_id = ? AND resolved_at_ms IS NULL",
        )
        .bind(&account_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        let stored: StoredRemoteEnvelope = serde_json::from_str(&stored_json).unwrap();
        let authoritative =
            latest_remote_entity_from_state(&mut transaction, &account_id, "subject", &subject_id)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(stored.revision, 3);
        assert_eq!(authoritative.revision, 3);
        assert_eq!(authoritative.payload.unwrap()["name"], "云端第三版");
        transaction.rollback().await.unwrap();

        cleanup_regression_database(database, &root).await;
    }
}
