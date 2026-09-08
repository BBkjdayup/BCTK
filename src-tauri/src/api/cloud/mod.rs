mod sync;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use reqwest::{Method, StatusCode, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tauri::State;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::licensing::{protect_for_current_user, unprotect_for_current_user};

use super::{
    AppState,
    models::{CommandError, CommandResult},
};

const CONFIG_FILENAME: &str = "cloud-api.json";
const SESSION_FILENAME: &str = "cloud-session.dpapi";
const MAX_SESSION_BYTES: u64 = 128 * 1024;
const DEFAULT_API_BASE_URL: &str = match option_env!("ZHITIKU_CLOUD_API_URL") {
    Some(value) => value,
    None => "https://api.tktiku.cn",
};

#[derive(Clone)]
pub struct CloudService {
    inner: Arc<CloudInner>,
}

struct CloudInner {
    root: PathBuf,
    client: reqwest::Client,
    device_name: String,
    runtime: Mutex<CloudRuntime>,
    sync_guard: Mutex<()>,
}

#[derive(Default)]
struct CloudRuntime {
    api_base_url: String,
    session: Option<StoredSession>,
    access_token: Option<String>,
    identity_generation: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudConfigFile {
    api_base_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredSession {
    refresh_token: String,
    refresh_expires_at: String,
    user: CloudUserApi,
    cloud_entitlement: CloudEntitlementApi,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CloudUserApi {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CloudEntitlementApi {
    pub plan_code: String,
    pub status: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
    refresh_expires_at: String,
    user: CloudUserApi,
    cloud_entitlement: CloudEntitlementApi,
}

#[derive(Debug, Deserialize)]
struct CurrentAccountResponse {
    user: CloudUserApi,
    cloud_entitlement: CloudEntitlementApi,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRegisterRequestApi {
    pub username: String,
    pub email: Option<String>,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudLoginRequestApi {
    pub account: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAccountStatusApi {
    pub configured: bool,
    pub api_base_url: String,
    pub logged_in: bool,
    pub user: Option<CloudUserApi>,
    pub entitlement: Option<CloudEntitlementApi>,
    pub can_sync: bool,
    pub database_bound: bool,
    pub last_sync_at_ms: Option<i64>,
    pub conflict_count: i64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncResultApi {
    pub pulled_count: usize,
    pub uploaded_count: usize,
    pub merged_count: usize,
    pub conflict_count: usize,
    pub skipped_count: usize,
    pub completed_at_ms: i64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncPreflightApi {
    pub local_entity_count: i64,
    pub local_question_count: i64,
    pub cloud_entity_count: i64,
    pub cloud_question_count: i64,
    pub local_has_data: bool,
    pub cloud_has_data: bool,
    pub both_non_empty: bool,
    pub recommended_mode: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncConflictApi {
    pub id: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub detected_at_ms: i64,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCloudSyncConflictRequestApi {
    pub id: String,
    pub resolution: String,
}

impl CloudService {
    pub fn initialize(root: PathBuf, app_version: &str) -> CommandResult<Self> {
        fs::create_dir_all(&root).map_err(|error| {
            CommandError::new(
                "CLOUD_STORAGE_ERROR",
                format!("无法创建云账号安全存储目录：{error}"),
            )
        })?;
        let api_base_url = load_api_base_url(&root)?;
        let session = load_session(&root)?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .user_agent(format!("TKQuestionBank/{app_version} Windows"))
            .build()
            .map_err(|error| {
                CommandError::new(
                    "CLOUD_CLIENT_INITIALIZATION_FAILED",
                    format!("云服务网络组件初始化失败：{error}"),
                )
            })?;
        Ok(Self {
            inner: Arc::new(CloudInner {
                root,
                client,
                device_name: format!("TK试题题库 Windows {app_version}"),
                runtime: Mutex::new(CloudRuntime {
                    api_base_url,
                    session,
                    access_token: None,
                    identity_generation: 1,
                }),
                sync_guard: Mutex::new(()),
            }),
        })
    }

    pub fn unavailable(root: PathBuf, app_version: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("default reqwest client configuration should be valid");
        Self {
            inner: Arc::new(CloudInner {
                root,
                client,
                device_name: format!("TK试题题库 Windows {app_version}"),
                runtime: Mutex::new(CloudRuntime::default()),
                sync_guard: Mutex::new(()),
            }),
        }
    }

    async fn set_api_base_url(&self, value: String) -> CommandResult<()> {
        let _sync_guard = self.inner.sync_guard.lock().await;
        let normalized = normalize_api_base_url(&value)?;
        let mut runtime = self.inner.runtime.lock().await;
        if runtime.api_base_url == normalized {
            return Ok(());
        }
        persist_json_atomic(
            &self.inner.root.join(CONFIG_FILENAME),
            &CloudConfigFile {
                api_base_url: normalized.clone(),
            },
            false,
        )?;
        remove_session_file(&self.inner.root)?;
        runtime.api_base_url = normalized;
        runtime.session = None;
        runtime.access_token = None;
        runtime.identity_generation = runtime.identity_generation.saturating_add(1);
        Ok(())
    }

    async fn authenticate(&self, path: &str, body: Value) -> CommandResult<()> {
        let _sync_guard = self.inner.sync_guard.lock().await;
        let mut runtime = self.inner.runtime.lock().await;
        let base = require_configured_url(&runtime.api_base_url)?;
        let response = self
            .inner
            .client
            .post(join_api_url(&base, path)?)
            .json(&body)
            .send()
            .await
            .map_err(network_error)?;
        let auth: AuthResponse = decode_response(response).await?;
        let stored = StoredSession {
            refresh_token: auth.refresh_token,
            refresh_expires_at: auth.refresh_expires_at,
            user: auth.user,
            cloud_entitlement: auth.cloud_entitlement,
        };
        persist_session(&self.inner.root, &stored)?;
        runtime.session = Some(stored);
        runtime.access_token = Some(auth.access_token);
        runtime.identity_generation = runtime.identity_generation.saturating_add(1);
        Ok(())
    }

    async fn logout(&self) -> CommandResult<()> {
        let _sync_guard = self.inner.sync_guard.lock().await;
        let mut runtime = self.inner.runtime.lock().await;
        let remote_result = if let (Ok(base), Some(session)) = (
            require_configured_url(&runtime.api_base_url),
            runtime.session.as_ref(),
        ) {
            self.inner
                .client
                .post(join_api_url(&base, "/api/v1/auth/logout")?)
                .json(&json!({ "refresh_token": session.refresh_token }))
                .send()
                .await
                .map_err(network_error)
                .and_then(|response| {
                    if response.status().is_success() {
                        Ok(())
                    } else {
                        Err(CommandError::new(
                            "CLOUD_LOGOUT_FAILED",
                            "服务器未能撤销登录，但本机登录信息已清除。",
                        ))
                    }
                })
        } else {
            Ok(())
        };
        remove_session_file(&self.inner.root)?;
        runtime.session = None;
        runtime.access_token = None;
        runtime.identity_generation = runtime.identity_generation.saturating_add(1);
        remote_result
    }

    async fn authenticated_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> CommandResult<T> {
        self.authenticated_json_with_context(None, method, path, body)
            .await
    }

    async fn authenticated_json_for<T: DeserializeOwned>(
        &self,
        context: &CloudRuntimeSnapshot,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> CommandResult<T> {
        self.authenticated_json_with_context(Some(context), method, path, body)
            .await
    }

    async fn authenticated_json_with_context<T: DeserializeOwned>(
        &self,
        expected: Option<&CloudRuntimeSnapshot>,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> CommandResult<T> {
        let mut runtime = self.inner.runtime.lock().await;
        if let Some(expected) = expected {
            ensure_runtime_identity(&runtime, expected)?;
        }
        let base = require_configured_url(&runtime.api_base_url)?;
        if runtime.session.is_none() {
            return Err(CommandError::new("CLOUD_NOT_LOGGED_IN", "请先登录云账号。"));
        }
        if runtime.access_token.is_none() {
            refresh_locked(&self.inner, &base, &mut runtime).await?;
        }

        let mut response = send_authenticated(
            &self.inner.client,
            method.clone(),
            join_api_url(&base, path)?,
            runtime.access_token.as_deref().unwrap_or_default(),
            body,
        )
        .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            refresh_locked(&self.inner, &base, &mut runtime).await?;
            response = send_authenticated(
                &self.inner.client,
                method,
                join_api_url(&base, path)?,
                runtime.access_token.as_deref().unwrap_or_default(),
                body,
            )
            .await?;
        }
        decode_response(response).await
    }

    async fn snapshot(&self) -> CloudRuntimeSnapshot {
        let runtime = self.inner.runtime.lock().await;
        CloudRuntimeSnapshot {
            api_base_url: runtime.api_base_url.clone(),
            session: runtime.session.clone(),
            identity_generation: runtime.identity_generation,
        }
    }

    async fn refresh_account_profile(&self) -> CommandResult<()> {
        let profile: CurrentAccountResponse = self
            .authenticated_json(Method::GET, "/api/v1/auth/me", None)
            .await?;
        let mut runtime = self.inner.runtime.lock().await;
        let session = runtime
            .session
            .as_mut()
            .ok_or_else(|| CommandError::new("CLOUD_NOT_LOGGED_IN", "请先登录云账号。"))?;
        if session.user.id != profile.user.id {
            return Err(CommandError::new(
                "CLOUD_RESPONSE_INVALID",
                "云服务返回了与当前登录账号不一致的资料，已停止更新。",
            ));
        }
        session.user = profile.user;
        session.cloud_entitlement = profile.cloud_entitlement;
        persist_session(&self.inner.root, session)
    }
}

#[derive(Clone)]
struct CloudRuntimeSnapshot {
    api_base_url: String,
    session: Option<StoredSession>,
    identity_generation: u64,
}

fn ensure_runtime_identity(
    runtime: &CloudRuntime,
    expected: &CloudRuntimeSnapshot,
) -> CommandResult<()> {
    let current_account = runtime
        .session
        .as_ref()
        .map(|session| session.user.id.as_str());
    let expected_account = expected
        .session
        .as_ref()
        .map(|session| session.user.id.as_str());
    if runtime.identity_generation == expected.identity_generation
        && runtime.api_base_url == expected.api_base_url
        && current_account == expected_account
    {
        Ok(())
    } else {
        Err(CommandError::new(
            "CLOUD_SESSION_CHANGED",
            "云账号或服务地址已发生变化，本次同步已安全停止，请重新同步。",
        ))
    }
}

async fn refresh_locked(
    inner: &CloudInner,
    base: &Url,
    runtime: &mut CloudRuntime,
) -> CommandResult<()> {
    let session = runtime
        .session
        .as_ref()
        .ok_or_else(|| CommandError::new("CLOUD_NOT_LOGGED_IN", "请先登录云账号。"))?;
    let refresh_token = session.refresh_token.clone();
    let expected_account_id = session.user.id.clone();
    let response = inner
        .client
        .post(join_api_url(base, "/api/v1/auth/refresh")?)
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .map_err(network_error)?;
    if response.status() == StatusCode::UNAUTHORIZED {
        remove_session_file(&inner.root)?;
        runtime.session = None;
        runtime.access_token = None;
        runtime.identity_generation = runtime.identity_generation.saturating_add(1);
        return Err(CommandError::new(
            "CLOUD_SESSION_EXPIRED",
            "云账号登录已过期，请重新登录。",
        ));
    }
    let auth: AuthResponse = decode_response(response).await?;
    if auth.user.id != expected_account_id {
        return Err(CommandError::new(
            "CLOUD_RESPONSE_INVALID",
            "云服务刷新结果与当前登录账号不一致，已停止同步。",
        ));
    }
    let stored = StoredSession {
        refresh_token: auth.refresh_token,
        refresh_expires_at: auth.refresh_expires_at,
        user: auth.user,
        cloud_entitlement: auth.cloud_entitlement,
    };
    persist_session(&inner.root, &stored)?;
    runtime.session = Some(stored);
    runtime.access_token = Some(auth.access_token);
    Ok(())
}

async fn send_authenticated(
    client: &reqwest::Client,
    method: Method,
    url: Url,
    token: &str,
    body: Option<&Value>,
) -> CommandResult<reqwest::Response> {
    let mut request = client.request(method, url).bearer_auth(token);
    if let Some(body) = body {
        request = request.json(body);
    }
    request.send().await.map_err(network_error)
}

async fn decode_response<T: DeserializeOwned>(response: reqwest::Response) -> CommandResult<T> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(network_error)?;
    if status.is_success() {
        return serde_json::from_slice(&bytes).map_err(|error| {
            CommandError::new(
                "CLOUD_RESPONSE_INVALID",
                format!("云服务返回了无法识别的数据：{error}"),
            )
        });
    }
    if let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(&bytes) {
        return Err(CommandError::new(
            format!("CLOUD_{}", envelope.error.code),
            envelope.error.message,
        ));
    }
    Err(CommandError::new(
        format!("CLOUD_HTTP_{}", status.as_u16()),
        format!("云服务请求失败（HTTP {}）。", status.as_u16()),
    ))
}

fn network_error(error: reqwest::Error) -> CommandError {
    let message = if error.is_timeout() {
        "连接云服务超时，请检查网络或稍后重试。".to_owned()
    } else if error.is_connect() {
        "无法连接云服务，请检查服务器地址和网络。".to_owned()
    } else {
        format!("云服务网络请求失败：{error}")
    };
    CommandError::new("CLOUD_NETWORK_ERROR", message)
}

fn load_api_base_url(root: &Path) -> CommandResult<String> {
    let path = root.join(CONFIG_FILENAME);
    if !path.exists() {
        return normalize_api_base_url(DEFAULT_API_BASE_URL);
    }
    let bytes = fs::read(&path).map_err(|error| {
        CommandError::new(
            "CLOUD_STORAGE_ERROR",
            format!("无法读取云服务配置：{error}"),
        )
    })?;
    let config: CloudConfigFile = serde_json::from_slice(&bytes).map_err(|error| {
        CommandError::new(
            "CLOUD_CONFIG_INVALID",
            format!("云服务配置文件损坏：{error}"),
        )
    })?;
    normalize_api_base_url(&config.api_base_url)
}

fn normalize_api_base_url(value: &str) -> CommandResult<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return Ok(String::new());
    }
    let url = Url::parse(value).map_err(|_| {
        CommandError::validation("云服务地址格式无效，应类似 https://api.example.cn。")
    })?;
    if url.query().is_some() || url.fragment().is_some() || url.username() != "" {
        return Err(CommandError::validation(
            "云服务地址不能包含账号、查询参数或片段。",
        ));
    }
    let is_loopback_http =
        url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if url.scheme() != "https" && !is_loopback_http {
        return Err(CommandError::validation(
            "正式云服务必须使用 HTTPS；仅本机联调允许 HTTP。",
        ));
    }
    if !url.path().is_empty() && url.path() != "/" {
        return Err(CommandError::validation(
            "云服务地址只填写域名和端口，不要附加接口路径。",
        ));
    }
    Ok(value.to_owned())
}

fn require_configured_url(value: &str) -> CommandResult<Url> {
    if value.is_empty() {
        return Err(CommandError::new(
            "CLOUD_NOT_CONFIGURED",
            "云服务地址尚未配置；备案完成后填写正式 HTTPS 域名即可启用。",
        ));
    }
    Url::parse(value).map_err(|_| CommandError::new("CLOUD_CONFIG_INVALID", "云服务地址无效。"))
}

fn join_api_url(base: &Url, path: &str) -> CommandResult<Url> {
    base.join(path)
        .map_err(|_| CommandError::new("CLOUD_CONFIG_INVALID", "无法组合云服务接口地址。"))
}

fn load_session(root: &Path) -> CommandResult<Option<StoredSession>> {
    let path = root.join(SESSION_FILENAME);
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(&path).map_err(|error| {
        CommandError::new(
            "CLOUD_STORAGE_ERROR",
            format!("无法读取云账号登录信息：{error}"),
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SESSION_BYTES {
        return Err(CommandError::new(
            "CLOUD_SESSION_INVALID",
            "云账号登录信息文件无效。",
        ));
    }
    let protected = fs::read(path).map_err(|error| {
        CommandError::new(
            "CLOUD_STORAGE_ERROR",
            format!("无法读取云账号登录信息：{error}"),
        )
    })?;
    let plaintext = unprotect_for_current_user(&protected).map_err(CommandError::from)?;
    serde_json::from_slice(&plaintext).map(Some).map_err(|_| {
        CommandError::new(
            "CLOUD_SESSION_INVALID",
            "云账号登录信息无法解析，请重新登录。",
        )
    })
}

fn persist_session(root: &Path, session: &StoredSession) -> CommandResult<()> {
    let plaintext = serde_json::to_vec(session).map_err(|error| {
        CommandError::new(
            "CLOUD_STORAGE_ERROR",
            format!("云账号登录信息无法编码：{error}"),
        )
    })?;
    let protected = protect_for_current_user(&plaintext).map_err(CommandError::from)?;
    persist_bytes_atomic(&root.join(SESSION_FILENAME), &protected)
}

fn persist_json_atomic<T: Serialize>(path: &Path, value: &T, pretty: bool) -> CommandResult<()> {
    let bytes = if pretty {
        serde_json::to_vec_pretty(value)
    } else {
        serde_json::to_vec(value)
    }
    .map_err(|error| {
        CommandError::new(
            "CLOUD_STORAGE_ERROR",
            format!("云服务配置无法编码：{error}"),
        )
    })?;
    persist_bytes_atomic(path, &bytes)
}

fn persist_bytes_atomic(path: &Path, bytes: &[u8]) -> CommandResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| CommandError::new("CLOUD_STORAGE_ERROR", "云服务存储路径无效。"))?;
    fs::create_dir_all(parent).map_err(|error| {
        CommandError::new(
            "CLOUD_STORAGE_ERROR",
            format!("无法创建云服务存储目录：{error}"),
        )
    })?;
    let temporary = parent.join(format!(".cloud-write-{}.tmp", Uuid::now_v7()));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result.map_err(|error| {
        CommandError::new(
            "CLOUD_STORAGE_ERROR",
            format!("云服务配置无法安全保存：{error}"),
        )
    })
}

fn remove_session_file(root: &Path) -> CommandResult<()> {
    let path = root.join(SESSION_FILENAME);
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CommandError::new(
            "CLOUD_STORAGE_ERROR",
            format!("无法清除本机云账号登录信息：{error}"),
        )),
    }
}

async fn build_status(
    cloud: &CloudService,
    app: &AppState,
) -> CommandResult<CloudAccountStatusApi> {
    let initial = cloud.snapshot().await;
    if !initial.api_base_url.is_empty() && initial.session.is_some() {
        let _ = cloud.refresh_account_profile().await;
    }
    let runtime = cloud.snapshot().await;
    let Some(session) = runtime.session else {
        return Ok(CloudAccountStatusApi {
            configured: !runtime.api_base_url.is_empty(),
            api_base_url: runtime.api_base_url,
            logged_in: false,
            user: None,
            entitlement: None,
            can_sync: false,
            database_bound: false,
            last_sync_at_ms: None,
            conflict_count: 0,
            message: "尚未登录云账号。桌面离线功能不受影响。".to_owned(),
        });
    };
    let database = app.database().await?;
    let row = sqlx::query_as::<_, (Option<String>, Option<i64>, i64, i64)>(
        r#"
        SELECT
            (SELECT account_id FROM cloud_database_binding WHERE singleton_id = 1),
            (SELECT last_sync_at_ms FROM cloud_sync_meta WHERE account_id = ?),
            (SELECT COUNT(*) FROM cloud_sync_conflicts
             WHERE account_id = ? AND conflict_kind = 'content'
               AND resolved_at_ms IS NULL),
            (SELECT COUNT(*) FROM cloud_sync_conflicts
             WHERE account_id = ? AND conflict_kind = 'deferred_apply'
               AND resolved_at_ms IS NULL)
        "#,
    )
    .bind(&session.user.id)
    .bind(&session.user.id)
    .bind(&session.user.id)
    .fetch_one(database.pool())
    .await
    .map_err(CommandError::database)?;
    let can_sync = matches!(
        session.cloud_entitlement.status.as_str(),
        "active" | "grace"
    );
    let database_bound = row.0.as_deref() == Some(session.user.id.as_str());
    let message = if can_sync {
        if row.0.is_some() && !database_bound {
            "当前题库已绑定另一个云账号，为避免教师数据串号，已停止同步。".to_owned()
        } else if row.2 > 0 {
            format!("云同步可用；有 {} 项双方修改需要确认。", row.2)
        } else if row.3 > 0 {
            format!(
                "云同步可用；有 {} 项云端内容正在等待依赖数据，点击立即同步后会自动补全。",
                row.3
            )
        } else {
            "云同步已开通。只会同步当前登录教师自己的题目数据。".to_owned()
        }
    } else {
        "账号已登录，但题目云同步授权尚未开通或已经到期。".to_owned()
    };
    Ok(CloudAccountStatusApi {
        configured: !runtime.api_base_url.is_empty(),
        api_base_url: runtime.api_base_url,
        logged_in: true,
        user: Some(session.user),
        entitlement: Some(session.cloud_entitlement),
        can_sync: can_sync && (row.0.is_none() || database_bound),
        database_bound,
        last_sync_at_ms: row.1,
        conflict_count: row.2,
        message,
    })
}

#[tauri::command]
pub async fn get_cloud_account_status(
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<CloudAccountStatusApi> {
    build_status(&cloud, &app).await
}

#[tauri::command]
pub async fn set_cloud_api_url(
    api_base_url: String,
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<CloudAccountStatusApi> {
    cloud.set_api_base_url(api_base_url).await?;
    build_status(&cloud, &app).await
}

#[tauri::command]
pub async fn cloud_register(
    request: CloudRegisterRequestApi,
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<CloudAccountStatusApi> {
    cloud
        .authenticate(
            "/api/v1/auth/register",
            json!({
                "username": request.username,
                "email": request.email,
                "password": request.password,
                "device_name": cloud.inner.device_name,
            }),
        )
        .await?;
    build_status(&cloud, &app).await
}

#[tauri::command]
pub async fn cloud_login(
    request: CloudLoginRequestApi,
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<CloudAccountStatusApi> {
    cloud
        .authenticate(
            "/api/v1/auth/login",
            json!({
                "account": request.account,
                "password": request.password,
                "device_name": cloud.inner.device_name,
            }),
        )
        .await?;
    build_status(&cloud, &app).await
}

#[tauri::command]
pub async fn cloud_logout(
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<CloudAccountStatusApi> {
    let logout_result = cloud.logout().await;
    let status = build_status(&cloud, &app).await?;
    logout_result.map(|()| status)
}

#[tauri::command]
pub async fn cloud_sync_now(
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<CloudSyncResultApi> {
    let _sync_guard = cloud.inner.sync_guard.lock().await;
    let database = app.database().await?;
    sync::run_sync(&cloud, &database).await
}

#[tauri::command]
pub async fn get_cloud_sync_preflight(
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<CloudSyncPreflightApi> {
    let _sync_guard = cloud.inner.sync_guard.lock().await;
    let database = app.database().await?;
    sync::preflight(&cloud, &database).await
}

#[tauri::command]
pub async fn list_cloud_sync_conflicts(
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<Vec<CloudSyncConflictApi>> {
    let database = app.database().await?;
    sync::list_conflicts(&cloud, &database).await
}

#[tauri::command]
pub async fn resolve_cloud_sync_conflict(
    request: ResolveCloudSyncConflictRequestApi,
    cloud: State<'_, CloudService>,
    app: State<'_, AppState>,
) -> CommandResult<Vec<CloudSyncConflictApi>> {
    let _sync_guard = cloud.inner.sync_guard.lock().await;
    let database = app.database().await?;
    sync::resolve_conflict(&cloud, &database, request).await
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::*;

    #[test]
    fn api_url_requires_https_except_loopback() {
        assert_eq!(
            normalize_api_base_url(" https://api.example.cn/ ").unwrap(),
            "https://api.example.cn"
        );
        assert_eq!(
            normalize_api_base_url("https://api.tktiku.cn").unwrap(),
            "https://api.tktiku.cn"
        );
        assert!(normalize_api_base_url("http://api.example.cn").is_err());
        assert!(normalize_api_base_url("http://127.0.0.1:8080").is_ok());
        assert!(normalize_api_base_url("https://api.example.cn/api/v1").is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cloud_identity_change_waits_until_the_active_sync_guard_is_released() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-cloud-account-lock-test-{}",
            uuid::Uuid::now_v7().simple()
        ));
        let cloud = CloudService::initialize(root.clone(), "test").unwrap();
        let sync_guard = cloud.inner.sync_guard.lock().await;
        let changing_cloud = cloud.clone();
        let identity_change = tokio::spawn(async move {
            changing_cloud
                .set_api_base_url("https://sync.example.test".to_owned())
                .await
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !identity_change.is_finished(),
            "cloud identity changes must wait for the active sync"
        );
        drop(sync_guard);
        identity_change.await.unwrap().unwrap();
        assert_eq!(
            cloud.snapshot().await.api_base_url,
            "https://sync.example.test"
        );

        drop(cloud);
        for attempt in 0..80 {
            match fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(error) if error.raw_os_error() == Some(32) && attempt < 79 => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean account lock test root: {error}"),
            }
        }
    }
}
