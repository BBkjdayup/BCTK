//! The renderer never receives account tokens or chooses an arbitrary network URL.
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MiniCloud(Mutex<Option<Session>>);
struct Session {
    access: String,
    refresh: String,
    expires: Instant,
}
fn base() -> String {
    // Loopback override is only compiled into debug builds for isolated integration tests.
    #[cfg(debug_assertions)]
    if let Ok(value) = std::env::var("TK_MINI_API_BASE")
        && let Ok(url) = reqwest::Url::parse(&value)
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
    {
        return value.trim_end_matches('/').to_owned();
    }
    "https://api.tktiku.cn/api/v1".into()
}
fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建安全连接".into())
}
async fn request(
    method: Method,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> Result<Value, String> {
    let mut request = client()?.request(method.clone(), format!("{}{path}", base()));
    if method != Method::GET {
        request = request.json(&body);
    }
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let mut response = request
        .send()
        .await
        .map_err(|_| "网络连接失败，操作结果尚未确认，请刷新后重试".to_owned())?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "响应中断，请刷新确认操作结果".to_owned())?
    {
        if bytes.len() + chunk.len() > 10 * 1024 * 1024 {
            return Err("服务器响应过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let data: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status == StatusCode::UNAUTHORIZED {
        return Err("登录已过期，请重新登录小程序管理账号".into());
    }
    if !status.is_success() {
        return Err(data
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("小程序服务暂不可用，请稍后刷新重试")
            .chars()
            .take(300)
            .collect());
    }
    if data.is_null() {
        return Err("服务器响应格式无效".into());
    }
    Ok(data)
}
fn session(data: &Value) -> Result<Session, String> {
    let access = data["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("登录响应无效")?;
    let refresh = data["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("登录响应无效")?;
    let seconds = data["access_expires_in"]
        .as_u64()
        .unwrap_or(900)
        .clamp(1, 86400);
    Ok(Session {
        access: access.into(),
        refresh: refresh.into(),
        expires: Instant::now() + Duration::from_secs(seconds.saturating_sub(30)),
    })
}
#[tauri::command]
pub async fn mini_cloud_login(
    state: tauri::State<'_, MiniCloud>,
    account: String,
    password: String,
    register: bool,
) -> Result<Value, String> {
    let mut guard = state.0.lock().await;
    if guard.is_some() {
        return Err("请先退出当前小程序管理账号".into());
    }
    let (path, body) = if register {
        (
            "/auth/register",
            json!({"username":account,"password":password,"device_name":"TK桌面小程序管理"}),
        )
    } else {
        (
            "/auth/login",
            json!({"account":account,"password":password,"device_name":"TK桌面小程序管理"}),
        )
    };
    let data = request(Method::POST, path, body, None).await?;
    *guard = Some(session(&data)?);
    Ok(data["user"].clone())
}
fn allowed(method: &str, path: &str) -> bool {
    match (method, path) {
        ("GET", "/mini-owner/state")
        | ("GET", "/mini-owner/support-contact")
        | ("GET", "/mini-owner/state?activePublications=true")
        | ("POST", "/mini-owner/publications/search")
        | ("PUT", "/mini-owner/bank")
        | ("POST", "/mini-owner/invites")
        | ("POST", "/mini-owner/assets") => true,
        _ => {
            let parts: Vec<_> = path.split('/').collect();
            if parts.len() == 5
                && parts[0].is_empty()
                && parts[1] == "mini-owner"
                && parts[2] == "invites"
                && parts[4] == "rotate"
                && uuid::Uuid::parse_str(parts[3]).is_ok()
            {
                return method == "POST";
            }
            if parts.len() != 4
                || !parts[0].is_empty()
                || parts[1] != "mini-owner"
                || uuid::Uuid::parse_str(parts[3]).is_err()
            {
                return false;
            }
            matches!(
                (method, parts[2]),
                ("DELETE", "invites") | ("PUT", "members") | ("PUT" | "DELETE", "publications")
            )
        }
    }
}
#[tauri::command]
pub async fn mini_cloud_request(
    state: tauri::State<'_, MiniCloud>,
    method: String,
    path: String,
    body: Value,
) -> Result<Value, String> {
    if !allowed(&method, &path) {
        return Err("不支持的小程序管理操作".into());
    }
    if body.to_string().len() > 8 * 1024 * 1024 {
        return Err("试卷内容超过 8 MB，请拆分后发布".into());
    }
    // Only this fixed, read-only route is public, including before account login.
    if method == "GET" && path == "/mini-owner/support-contact" {
        return request(Method::GET, &path, json!({}), None).await;
    }
    let mut guard = state.0.lock().await;
    let credentials = guard.as_mut().ok_or("请先登录小程序管理账号")?;
    if Instant::now() >= credentials.expires {
        match request(
            Method::POST,
            "/auth/refresh",
            json!({"refresh_token":credentials.refresh}),
            None,
        )
        .await
        {
            Ok(data) => *credentials = session(&data)?,
            Err(error) => {
                *guard = None;
                return Err(error);
            }
        }
    }
    request(
        Method::from_bytes(method.as_bytes()).map_err(|_| "请求方法无效")?,
        &path,
        body,
        Some(&credentials.access),
    )
    .await
}
#[tauri::command]
pub async fn mini_cloud_logout(state: tauri::State<'_, MiniCloud>) -> Result<(), String> {
    let mut guard = state.0.lock().await;
    if let Some(credentials) = guard.take() {
        request(
            Method::POST,
            "/auth/logout",
            json!({"refresh_token":credentials.refresh}),
            Some(&credentials.access),
        )
        .await?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_allows_only_owner_routes_and_never_admin_or_arbitrary_urls() {
        let id = "00000000-0000-4000-8000-000000000001";
        assert!(allowed("PUT", &format!("/mini-owner/publications/{id}")));
        assert!(allowed("GET", "/mini-owner/state"));
        assert!(allowed("GET", "/mini-owner/support-contact"));
        assert!(!allowed("PUT", "/mini-owner/support-contact"));
        assert!(!allowed("POST", "/mini-owner/support-contact"));
        assert!(allowed("GET", "/mini-owner/state?activePublications=true"));
        assert!(allowed("POST", "/mini-owner/publications/search"));
        assert!(!allowed("DELETE", "/mini-owner/publications/search"));
        for path in [
            "https://evil.invalid/",
            "/admin-api/users",
            "/mini-owner/state?url=x",
            "/mini-owner/state?activePublications=true&url=x",
            "/mini-owner/../auth/login",
            "/mini-owner/invites/../../",
        ] {
            assert!(!allowed("GET", path));
            assert!(!allowed("PUT", path));
        }
        assert!(!allowed("PUT", "/mini-owner/grant"));
    }
}
