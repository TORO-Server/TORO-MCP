//! 結合試験の共通部品: 偽の auth (トークンの確認) と偽の api (Wiki と稼働状態)、試験用に起動した本体
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::to_bytes;
use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use toro_mcp::App;
use toro_mcp::config::Config;

pub const WIKI_ORIGIN: &str = "https://wiki.torosaba.net";
pub const AUTH_URL: &str = "https://auth.torosaba.net";
pub const MCP_URL: &str = "https://mcp.torosaba.net";

async fn spawn(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

fn json_response(status: u16, body: Value) -> Response {
    (
        StatusCode::from_u16(status).unwrap(),
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

/// 登録したトークンだけを有効と答える auth
#[derive(Default)]
pub struct FakeAuth {
    tokens: Mutex<HashMap<String, Value>>,
    /// true の間は、何を聞かれても 500 を返す
    pub broken: AtomicBool,
    pub requests: AtomicUsize,
}

impl FakeAuth {
    pub fn register(&self, token: &str, claims: Value) {
        self.tokens
            .lock()
            .unwrap()
            .insert(token.to_string(), claims);
    }

    pub fn set_broken(&self, broken: bool) {
        self.broken.store(broken, Ordering::SeqCst);
    }
}

async fn start_fake_auth(state: Arc<FakeAuth>) -> String {
    let router = Router::new().fallback(move |req: Request| {
        let state = state.clone();
        async move {
            let (parts, body) = req.into_parts();
            let body = to_bytes(body, usize::MAX).await.unwrap();
            state.requests.fetch_add(1, Ordering::SeqCst);
            if parts.method != Method::POST || parts.uri.path() != "/introspect" {
                return json_response(404, json!({ "error": "not found" }));
            }
            if state.broken.load(Ordering::SeqCst) {
                return json_response(500, json!({ "error": "broken" }));
            }
            let request: Value = serde_json::from_slice(&body).unwrap();
            let token = request["token"].as_str().unwrap_or("");
            match state.tokens.lock().unwrap().get(token) {
                Some(claims) => json_response(200, json!({ "active": true, "claims": claims })),
                None => json_response(200, json!({ "active": false })),
            }
        }
    });
    spawn(router).await
}

/// api に届いた保存の依頼
pub struct SaveCall {
    /// Authorization ヘッダーの値
    pub authorization: String,
    pub body: Value,
}

/// 原稿と稼働状態を覚えておくだけの api
#[derive(Default)]
pub struct FakeApi {
    /// docs/ を除いたパス → (sha, 本文)。一覧と検索の元
    pub docs: Mutex<BTreeMap<String, (String, String)>>,
    /// docs/ から始まるパス → (sha, 本文)。raw の元
    pub files: Mutex<HashMap<String, (String, String)>>,
    pub saves: Mutex<Vec<SaveCall>>,
    pub requests: Mutex<Vec<String>>,
    /// Some の間は、保存がこの文言で失敗する
    pub save_error: Mutex<Option<String>>,
    /// Some の間は、保存をこの文言の 400 で断る
    pub save_rejection: Mutex<Option<String>>,
    /// Some の間は、検索をこの文言の 400 で断る
    pub search_rejection: Mutex<Option<String>>,
    pub search_unavailable: AtomicBool,
    /// Some の間は、Minecraft サーバーが動いている
    pub status: Mutex<Option<Value>>,
}

fn query_of(parts: &axum::http::request::Parts, name: &str) -> Option<String> {
    url::form_urlencoded::parse(parts.uri.query().unwrap_or("").as_bytes())
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// api の `path::normalize` の代わり。docs/ の外に出るものは None
fn normalize(path: &str) -> Option<String> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.contains('\\')
        || trimmed
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return None;
    }
    let mut clean = if trimmed.starts_with("docs/") {
        trimmed.to_string()
    } else {
        format!("docs/{trimmed}")
    };
    if !clean.ends_with(".md") {
        clean.push_str(".md");
    }
    Some(clean)
}

async fn start_fake_api(state: Arc<FakeApi>) -> String {
    let router = Router::new().fallback(move |req: Request| {
        let state = state.clone();
        async move {
            let (parts, body) = req.into_parts();
            let body = to_bytes(body, usize::MAX).await.unwrap();
            state.requests.lock().unwrap().push(parts.uri.to_string());
            let path = parts.uri.path().to_string();

            match (parts.method.clone(), path.as_str()) {
                (Method::GET, "/docs/list") => {
                    let category = query_of(&parts, "category");
                    let pages: Vec<String> = state
                        .docs
                        .lock()
                        .unwrap()
                        .keys()
                        .filter(|p| category.as_deref().is_none_or(|c| p.starts_with(c)))
                        .cloned()
                        .collect();
                    json_response(200, json!({ "count": pages.len(), "pages": pages }))
                }
                (Method::GET, "/docs/raw") => {
                    let Some(requested) = query_of(&parts, "path").filter(|p| !p.is_empty()) else {
                        return json_response(400, json!({ "error": "Missing path" }));
                    };
                    let Some(clean) = normalize(&requested) else {
                        return json_response(400, json!({ "error": "Invalid path" }));
                    };
                    match state.files.lock().unwrap().get(&clean) {
                        Some((sha, content)) => json_response(
                            200,
                            json!({ "path": clean, "sha": sha, "content": content, "size": content.len() }),
                        ),
                        None => json_response(404, json!({ "error": "File not found" })),
                    }
                }
                (Method::POST, "/docs/save") => {
                    let authorization = parts
                        .headers
                        .get(header::AUTHORIZATION)
                        .and_then(|h| h.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    let payload: Value = serde_json::from_slice(&body).unwrap();
                    state.saves.lock().unwrap().push(SaveCall {
                        authorization,
                        body: payload.clone(),
                    });
                    if normalize(payload["path"].as_str().unwrap_or("")).is_none() {
                        return json_response(400, json!({ "error": "Invalid path" }));
                    }
                    if let Some(message) = state.save_rejection.lock().unwrap().clone() {
                        return json_response(400, json!({ "error": message }));
                    }
                    if let Some(message) = state.save_error.lock().unwrap().clone() {
                        return json_response(
                            500,
                            json!({ "error": format!("Failed to commit to GitHub: {message}") }),
                        );
                    }
                    json_response(
                        200,
                        json!({ "ok": true, "commitSha": "commit-1", "contentSha": "sha-new" }),
                    )
                }
                (Method::GET, "/docs/search") => {
                    if state.search_unavailable.load(Ordering::SeqCst) {
                        return json_response(503, json!({ "error": "Search unavailable" }));
                    }
                    if let Some(message) = state.search_rejection.lock().unwrap().clone() {
                        return json_response(400, json!({ "error": message }));
                    }
                    let query = query_of(&parts, "q").unwrap_or_default();
                    let lowercase = query.to_lowercase();
                    let terms: Vec<&str> = lowercase.split_whitespace().collect();
                    let hits: Vec<String> = if terms.is_empty() {
                        Vec::new()
                    } else {
                        state
                            .docs
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|(path, (_, text))| {
                                let haystack = format!("{path}\n{text}").to_lowercase();
                                terms.iter().all(|t| haystack.contains(t))
                            })
                            .map(|(path, _)| path.clone())
                            .collect()
                    };
                    json_response(200, json!({ "query": query, "hits": hits }))
                }
                (Method::GET, "/server/status") => match state.status.lock().unwrap().clone() {
                    Some(status) => json_response(200, status),
                    None => json_response(200, json!({ "online": false })),
                },
                _ => json_response(404, json!({ "error": "not found" })),
            }
        }
    });
    spawn(router).await
}

pub const ADMIN_ROLE: &str = "497387545928138762";
pub const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nfake";

/// crime-buster の窓口に届いた依頼
#[derive(Clone, Debug)]
pub struct CrimeBusterCall {
    pub method: String,
    /// パスと、? 以降
    pub target: String,
    pub authorization: String,
    pub body: Value,
}

/// 届いた依頼を覚えて、決まった答えを返す crime-buster の窓口
#[derive(Default)]
pub struct FakeCrimeBuster {
    pub calls: Mutex<Vec<CrimeBusterCall>>,
}

async fn start_fake_crime_buster(state: Arc<FakeCrimeBuster>) -> String {
    let router = Router::new().fallback(move |req: Request| {
        let state = state.clone();
        async move {
            let (parts, body) = req.into_parts();
            let body = to_bytes(body, usize::MAX).await.unwrap();
            let header_of = |name: &str| {
                parts
                    .headers
                    .get(name)
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("")
                    .to_string()
            };
            let call = CrimeBusterCall {
                method: parts.method.to_string(),
                target: parts
                    .uri
                    .path_and_query()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                authorization: header_of("authorization"),
                body: serde_json::from_slice(&body).unwrap_or(Value::Null),
            };
            state.calls.lock().unwrap().push(call.clone());
            // 本物の窓口は、利用者のトークンを auth に尋ねて、Admin かを確かめる。ここでは、トークンの名前で決める
            match call.authorization.as_str() {
                "Bearer tok-nobody" => {
                    return json_response(
                        401,
                        json!({ "error": "トークンが無効か、期限切れです" }),
                    );
                }
                "Bearer tok-notadmin" => {
                    return json_response(403, json!({ "error": "Admin 以上のロールが要ります" }));
                }
                _ => {}
            }
            let path = parts.uri.path();
            if path.contains("/missing") {
                return json_response(404, json!({ "error": "incident not found" }));
            }
            if path == "/v1/screenshot" {
                return json_response(202, json!({ "job": "abc123", "status": "pending" }));
            }
            if path == "/v1/screenshots/slow1" {
                return json_response(202, json!({ "job": "slow1", "status": "pending" }));
            }
            if path.ends_with("/images/damage.png") || path == "/v1/screenshots/abc123" {
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/png")],
                    PNG_BYTES.to_vec(),
                )
                    .into_response();
            }
            if path == "/v1/status" {
                return json_response(200, json!({ "mode": "review", "pending": 2 }));
            }
            json_response(200, json!({ "ok": true, "target": call.target }))
        }
    });
    spawn(router).await
}

pub struct TestServer {
    pub base: String,
    pub client: reqwest::Client,
    pub auth: Arc<FakeAuth>,
    pub api: Arc<FakeApi>,
    pub crime_buster: Arc<FakeCrimeBuster>,
}

pub async fn start() -> TestServer {
    start_with(|_| {}).await
}

pub async fn start_with(customize: impl FnOnce(&mut Config)) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());

    let auth = Arc::new(FakeAuth::default());
    let api = Arc::new(FakeApi::default());
    let crime_buster = Arc::new(FakeCrimeBuster::default());
    api.docs.lock().unwrap().extend([
        (
            "01_About/a.md".to_string(),
            (
                "sha-a".to_string(),
                "# 概要\nTrainCarts と 電車 の話".to_string(),
            ),
        ),
        (
            "02_Guide/b.md".to_string(),
            (
                "sha-b".to_string(),
                "# ガイド\nmarumasign の使い方".to_string(),
            ),
        ),
    ]);

    let mut cfg = Config {
        port: 0,
        issuer: AUTH_URL.into(),
        api_url: "https://api.torosaba.net".into(),
        mcp_url: MCP_URL.into(),
        wiki_url: WIKI_ORIGIN.into(),
        cors_origins: vec![WIKI_ORIGIN.into()],
        auth_internal_url: start_fake_auth(auth.clone()).await,
        api_internal_url: start_fake_api(api.clone()).await,
        crime_buster_url: Some(start_fake_crime_buster(crime_buster.clone()).await),
        admin_roles: vec![ADMIN_ROLE.into()],
        body_read_timeout_secs: 30,
    };
    customize(&mut cfg);

    let router = toro_mcp::routes::router(Arc::new(App::new(cfg).unwrap()));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    TestServer {
        base,
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        auth,
        api,
        crime_buster,
    }
}

pub fn location(res: &reqwest::Response) -> String {
    res.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

/// 保存できる利用者のクレーム
pub fn editor_claims() -> Value {
    json!({ "id": "1001", "username": "editor", "can_edit": true, "roles": [] })
}

/// Admin ロールを持つ利用者のクレーム (auth が、いまの Discord で確かめたロール)
pub fn admin_claims() -> Value {
    json!({ "id": "42", "username": "admin1", "can_edit": true, "roles": [ADMIN_ROLE, "1"], "roles_source": "live" })
}

impl TestServer {
    pub fn admin_token(&self) -> String {
        self.token(admin_claims())
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.client.get(self.url(path)).send().await.unwrap()
    }

    /// 有効なトークンとして auth に覚えさせ、その文字列を返す
    pub fn token(&self, claims: Value) -> String {
        let token = format!("tok-{}", self.auth.tokens.lock().unwrap().len());
        self.auth.register(&token, claims);
        token
    }

    pub fn editor_token(&self) -> String {
        self.token(editor_claims())
    }

    pub fn put_file(&self, path: &str, sha: &str, content: &str) {
        self.api
            .files
            .lock()
            .unwrap()
            .insert(path.to_string(), (sha.to_string(), content.to_string()));
    }

    /// JSON-RPC を 1 回呼ぶ
    pub async fn rpc(
        &self,
        method: &str,
        params: Value,
        bearer: Option<&str>,
    ) -> reqwest::Response {
        let mut request = self
            .client
            .post(self.url("/"))
            .json(&json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params }));
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        request.send().await.unwrap()
    }

    /// ツールを呼び、結果の本文と isError を返す
    pub async fn tool(&self, name: &str, arguments: Value, bearer: Option<&str>) -> (String, bool) {
        let res = self
            .rpc(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
                bearer,
            )
            .await;
        assert_eq!(res.status(), 200);
        let body: Value = res.json().await.unwrap();
        let result = &body["result"];
        (
            result["content"][0]["text"].as_str().unwrap().to_string(),
            result["isError"] == true,
        )
    }
}
