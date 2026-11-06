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
    request_at(&base(), method, path, body, token).await
}

async fn request_at(
    api_base: &str,
    method: Method,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> Result<Value, String> {
    let mut request = client()?.request(method.clone(), format!("{api_base}{path}"));
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
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60)
        .clamp(1, 120);
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
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(format!(
            "TK_MINI_HTTP_ERROR:{}",
            json!({
                "status": 429, "code": "RATE_LIMITED", "retryAfterSeconds": retry_after,
            "message": "操作过于频繁，请稍后重试",
            })
        ));
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err("登录已过期，请重新登录小程序管理账号".into());
    }
    if status == StatusCode::NOT_FOUND && path == "/mini-owner/management/state" {
        return Err("MINI_PAGED_MANAGEMENT_UNAVAILABLE: 云服务尚未支持分页管理".into());
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
        | ("GET", "/mini-owner/management/state")
        | ("GET", "/mini-owner/management/state?omitPublications=true")
        | ("POST", "/mini-owner/learning")
        | ("POST", "/mini-owner/members/search")
        | ("POST", "/mini-owner/invites/search")
        | ("POST", "/mini-owner/publications/search")
        | ("PUT", "/mini-owner/bank")
        | ("POST", "/mini-owner/invites")
        | ("POST", "/mini-owner/assets/check")
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
                ("DELETE", "invites")
                    | ("PUT", "feedback")
                    | ("PUT" | "DELETE", "members" | "publications")
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
    owner_request(&state, &base(), &method, &path, body).await
}

// The URL is chosen only by native code, never by a renderer IPC argument.
// Keeping this operation separate lets contract tests exercise authentication,
// route validation and real HTTP transport without launching a desktop window.
async fn owner_request(
    state: &MiniCloud,
    api_base: &str,
    method: &str,
    path: &str,
    body: Value,
) -> Result<Value, String> {
    if !allowed(method, path) {
        return Err("不支持的小程序管理操作".into());
    }
    if body.to_string().len() > 8 * 1024 * 1024 {
        return Err("试卷内容超过 8 MB，请拆分后发布".into());
    }
    // Only this fixed, read-only route is public, including before account login.
    if method == "GET" && path == "/mini-owner/support-contact" {
        return request_at(api_base, Method::GET, path, json!({}), None).await;
    }
    let mut guard = state.0.lock().await;
    let credentials = guard.as_mut().ok_or("请先登录小程序管理账号")?;
    if Instant::now() >= credentials.expires {
        match request_at(
            api_base,
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
    request_at(
        api_base,
        Method::from_bytes(method.as_bytes()).map_err(|_| "请求方法无效")?,
        path,
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
        assert!(allowed("DELETE", &format!("/mini-owner/members/{id}")));
        for path in [
            "/mini-owner/members/not-a-uuid".to_owned(),
            format!("/mini-owner/members/{id}/extra"),
            format!("/mini-owner/members/{id}?owner=other"),
            "/mini-owner/members/../state".to_owned(),
        ] {
            assert!(!allowed("DELETE", &path));
        }
        assert!(!allowed("POST", &format!("/mini-owner/members/{id}")));
        assert!(allowed("GET", "/mini-owner/state"));
        assert!(allowed("GET", "/mini-owner/support-contact"));
        assert!(allowed("PUT", &format!("/mini-owner/feedback/{id}")));
        assert!(!allowed("PUT", "/mini-owner/feedback/not-a-uuid"));
        assert!(!allowed("GET", &format!("/mini-owner/feedback/{id}")));
        assert!(!allowed(
            "PUT",
            &format!("/mini-owner/feedback/{id}?owner=other")
        ));
        assert!(!allowed("PUT", "/mini-owner/support-contact"));
        assert!(!allowed("POST", "/mini-owner/support-contact"));
        assert!(allowed("GET", "/mini-owner/state?activePublications=true"));
        assert!(allowed("POST", "/mini-owner/publications/search"));
        assert!(!allowed("DELETE", "/mini-owner/publications/search"));
        for path in [
            "/mini-owner/members/search",
            "/mini-owner/invites/search",
            "/mini-owner/assets/check",
            "/mini-owner/learning",
        ] {
            assert!(allowed("POST", path));
            assert!(!allowed("GET", path));
            assert!(!allowed("DELETE", path));
            assert!(!allowed("POST", &format!("{path}?owner=other")));
        }
        assert!(allowed("GET", "/mini-owner/management/state"));
        assert!(allowed(
            "GET",
            "/mini-owner/management/state?omitPublications=true"
        ));
        assert!(!allowed("POST", "/mini-owner/management/state"));
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

    async fn http_fixture(
        replies: Vec<(u16, Value)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(10), async move {
                let mut requests = Vec::new();
                for (status, body) in replies {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut bytes = Vec::new();
                    let header_end = loop {
                        let mut buffer = [0u8; 1024];
                        let read = stream.read(&mut buffer).await.unwrap();
                        assert!(read > 0);
                        bytes.extend_from_slice(&buffer[..read]);
                        assert!(bytes.len() < 64 * 1024);
                        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                            break end + 4;
                        }
                    };
                    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                    let length = headers.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
                    }).unwrap_or(0);
                    assert!(length < 64 * 1024);
                    while bytes.len() < header_end + length {
                        let mut buffer = [0u8; 1024];
                        let read = stream.read(&mut buffer).await.unwrap();
                        assert!(read > 0);
                        bytes.extend_from_slice(&buffer[..read]);
                    }
                    requests.push(String::from_utf8(bytes).unwrap());
                    let body = body.to_string();
                    let retry = if status == 429 { "Retry-After: 7\r\n" } else { "" };
                    let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\n{retry}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
                requests
            }).await.expect("native bridge did not reach the expected HTTP routes")
        });
        (base, server)
    }

    fn signed_in(expires: Instant) -> MiniCloud {
        MiniCloud(Mutex::new(Some(Session {
            access: "synthetic-access-only".into(),
            refresh: "synthetic-refresh-only".into(),
            expires,
        })))
    }

    #[tokio::test]
    async fn rate_limit_metadata_survives_the_native_http_bridge() {
        let (base, server) =
            http_fixture(vec![(429, json!({"error":{"code":"RATE_LIMITED"}}))]).await;
        let error = request_at(
            &base,
            Method::POST,
            "/mini-owner/assets",
            json!({}),
            Some("test"),
        )
        .await
        .unwrap_err();
        let details: Value =
            serde_json::from_str(error.strip_prefix("TK_MINI_HTTP_ERROR:").unwrap()).unwrap();
        assert_eq!(details["status"], 429);
        assert_eq!(details["code"], "RATE_LIMITED");
        assert_eq!(details["retryAfterSeconds"], 7);
        assert_eq!(server.await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn member_removal_reaches_http_with_native_auth_then_refreshes_state() {
        let id = "00000000-0000-4000-8000-000000000001";
        let (base, server) = http_fixture(vec![
            (200, json!({"ok":true})),
            (200, json!({"members":[],"settings":{"usedQuota":0}})),
        ])
        .await;
        let state = signed_in(Instant::now() + Duration::from_secs(60));
        let deleted = owner_request(
            &state,
            &base,
            "DELETE",
            &format!("/mini-owner/members/{id}"),
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(deleted["ok"], true);
        let refreshed = owner_request(
            &state,
            &base,
            "GET",
            "/mini-owner/state?activePublications=true",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(refreshed["members"], json!([]));
        let requests = server.await.unwrap();
        assert!(requests[0].starts_with(&format!(
            "DELETE /api/v1/mini-owner/members/{id} HTTP/1.1\r\n"
        )));
        assert!(
            requests[1]
                .starts_with("GET /api/v1/mini-owner/state?activePublications=true HTTP/1.1\r\n")
        );
        for request in requests {
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: bearer synthetic-access-only\r\n")
            );
            assert!(!request.contains("synthetic-refresh-only"));
        }
    }

    #[tokio::test]
    async fn member_removal_refreshes_expired_token_and_preserves_server_rejection() {
        let (base, server) = http_fixture(vec![
            (200, json!({"access_token":"new-test-access", "refresh_token":"new-test-refresh", "access_expires_in":900})),
            (403, json!({"error":{"message":"无权移除此成员"}})),
        ]).await;
        let state = signed_in(Instant::now());
        let error = owner_request(
            &state,
            &base,
            "DELETE",
            "/mini-owner/members/00000000-0000-4000-8000-000000000001",
            json!({}),
        )
        .await
        .unwrap_err();
        assert_eq!(error, "无权移除此成员");
        let requests = server.await.unwrap();
        assert!(requests[0].starts_with("POST /api/v1/auth/refresh HTTP/1.1\r\n"));
        assert!(requests[0].contains(r#""refresh_token":"synthetic-refresh-only""#));
        assert!(
            requests[1]
                .to_lowercase()
                .contains("authorization: bearer new-test-access\r\n")
        );
    }

    #[tokio::test]
    async fn invalid_or_unauthenticated_member_removal_stops_before_network() {
        let state = MiniCloud::default();
        let base = "http://127.0.0.1:1/api/v1";
        assert_eq!(
            owner_request(
                &state,
                base,
                "DELETE",
                "/mini-owner/members/not-a-uuid",
                json!({})
            )
            .await
            .unwrap_err(),
            "不支持的小程序管理操作"
        );
        assert_eq!(
            owner_request(
                &state,
                base,
                "DELETE",
                "/mini-owner/members/00000000-0000-4000-8000-000000000001",
                json!({})
            )
            .await
            .unwrap_err(),
            "请先登录小程序管理账号"
        );
    }

    #[tokio::test]
    #[ignore = "run through tools/release/check-mini-member-bridge.ps1 with an isolated cloud fixture"]
    async fn live_cloud_member_removal_contract() {
        let fixture_path =
            std::env::var("TK_MINI_BRIDGE_FIXTURE").expect("cloud test fixture required");
        let fixture: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let base = fixture["base"].as_str().unwrap();
        let url = reqwest::Url::parse(base).unwrap();
        assert_eq!(url.scheme(), "http");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.path(), "/api/v1");
        let id = fixture["memberId"].as_str().unwrap();
        assert!(uuid::Uuid::parse_str(id).is_ok());
        let state = MiniCloud(Mutex::new(Some(Session {
            access: fixture["token"].as_str().unwrap().into(),
            refresh: "unused-synthetic-refresh".into(),
            expires: Instant::now() + Duration::from_secs(60),
        })));
        let before = owner_request(
            &state,
            base,
            "GET",
            "/mini-owner/state?activePublications=true",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(before["settings"]["usedQuota"], 1);
        let summary = owner_request(
            &state,
            base,
            "GET",
            "/mini-owner/management/state",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(summary["members"], json!([]));
        assert_eq!(summary["settings"]["usedQuota"], 1);
        let page = owner_request(
            &state,
            base,
            "POST",
            "/mini-owner/members/search",
            json!({"keyword":"","status":"all","page":1,"pageSize":20}),
        )
        .await
        .unwrap();
        assert_eq!(page["items"][0]["id"], id);
        let invites = owner_request(
            &state,
            base,
            "POST",
            "/mini-owner/invites/search",
            json!({"keyword":"","status":"all","page":1,"pageSize":20}),
        )
        .await
        .unwrap();
        assert_eq!(invites["total"], 0);
        let assets = owner_request(
            &state,
            base,
            "POST",
            "/mini-owner/assets/check",
            json!({"ids":[]}),
        )
        .await
        .unwrap();
        assert_eq!(assets, json!([]));
        assert!(
            before["members"]
                .as_array()
                .unwrap()
                .iter()
                .any(|member| member["id"] == id)
        );
        let deleted = owner_request(
            &state,
            base,
            "DELETE",
            &format!("/mini-owner/members/{id}"),
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(deleted["ok"], true);
        let after = owner_request(
            &state,
            base,
            "GET",
            "/mini-owner/state?activePublications=true",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(after["settings"]["usedQuota"], 0);
        assert!(
            !after["members"]
                .as_array()
                .unwrap()
                .iter()
                .any(|member| member["id"] == id)
        );
    }
}
