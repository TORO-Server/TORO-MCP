//! TORO-API (Wiki と稼働状態のデータ) への問い合わせ。データはここ以外から取らない
//!
//! 内部の窓口 (`API_INTERNAL_URL`) を呼ぶ。利用者の Bearer を渡すのは、保存のときだけ。

use serde_json::{Value, json};

use crate::App;
use crate::util::encode_uri_component;

/// api が保存に失敗したときの文言の前置き (MCP では前置きなしで見せる)
const COMMIT_ERROR_PREFIX: &str = "Failed to commit to GitHub: ";

pub struct Page {
    pub path: String,
    pub sha: String,
    pub content: String,
}

pub struct Status {
    pub version: String,
    pub online: i64,
    pub max: i64,
    pub players: Vec<String>,
    pub motd: Vec<String>,
}

pub enum Saved {
    Committed {
        commit_sha: String,
    },
    /// api が、使えないパスだと断った
    InvalidPath,
    Failed(String),
}

fn url(app: &App, path_and_query: &str) -> String {
    format!("{}{path_and_query}", app.cfg.api_internal_url)
}

/// 応答の `error` を取り出す。無ければステータスだけ
async fn error_message(res: reqwest::Response) -> String {
    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    match body["error"].as_str() {
        Some(message) => message.to_string(),
        None => format!("API error ({status})"),
    }
}

async fn get_json(app: &App, path_and_query: &str) -> Result<(u16, Value), String> {
    let res = app
        .http
        .get(url(app, path_and_query))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status().as_u16();
    if res.status().is_success() {
        let body = res.json().await.map_err(|e| e.to_string())?;
        Ok((status, body))
    } else {
        Ok((status, json!({ "error": error_message(res).await })))
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// docs/ の下の原稿の一覧 (docs/ を除いたパス)
pub async fn list_pages(app: &App, category: Option<&str>) -> Result<Vec<String>, String> {
    let query = match category {
        Some(category) => format!("?category={}", encode_uri_component(category)),
        None => String::new(),
    };
    let (status, body) = get_json(app, &format!("/docs/list{query}")).await?;
    if status != 200 {
        return Err(body["error"].as_str().unwrap_or("API error").to_string());
    }
    Ok(strings(&body["pages"]))
}

/// 原稿を読む。無いか、使えないパスなら None
pub async fn get_page(app: &App, path: &str) -> Result<Option<Page>, String> {
    let (status, body) = get_json(
        app,
        &format!("/docs/raw?path={}", encode_uri_component(path)),
    )
    .await?;
    match status {
        200 => Ok(Some(Page {
            path: body["path"].as_str().unwrap_or(path).to_string(),
            sha: body["sha"].as_str().unwrap_or("").to_string(),
            content: body["content"].as_str().unwrap_or("").to_string(),
        })),
        400 | 404 => Ok(None),
        _ => Err(body["error"].as_str().unwrap_or("API error").to_string()),
    }
}

pub struct SaveRequest<'a> {
    pub path: &'a str,
    pub content: &'a str,
    pub sha: Option<&'a str>,
    pub message: &'a str,
}

/// 原稿を保存する。`bearer` は、呼び出し元が MCP に示した利用者のトークン
pub async fn save_page(app: &App, bearer: &str, request: &SaveRequest<'_>) -> Saved {
    let mut payload = json!({
        "path": request.path,
        "content": request.content,
        "message": request.message,
    });
    if let Some(sha) = request.sha.filter(|sha| !sha.is_empty()) {
        payload["sha"] = json!(sha);
    }
    let res = match app
        .http
        .post(url(app, "/docs/save"))
        .bearer_auth(bearer)
        .json(&payload)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => return Saved::Failed(e.to_string()),
    };
    match res.status().as_u16() {
        200 => {
            let body: Value = res.json().await.unwrap_or(Value::Null);
            Saved::Committed {
                commit_sha: body["commitSha"].as_str().unwrap_or("").to_string(),
            }
        }
        400 => Saved::InvalidPath,
        _ => {
            let message = error_message(res).await;
            let message = message
                .strip_prefix(COMMIT_ERROR_PREFIX)
                .unwrap_or(&message);
            Saved::Failed(message.to_string())
        }
    }
}

/// 語をすべて含む原稿のパス。検索が使えないときは None
pub async fn search(app: &App, query: &str) -> Result<Option<Vec<String>>, String> {
    let (status, body) = get_json(
        app,
        &format!("/docs/search?q={}", encode_uri_component(query)),
    )
    .await?;
    match status {
        200 => Ok(Some(strings(&body["hits"]))),
        503 => Ok(None),
        _ => Err(body["error"].as_str().unwrap_or("API error").to_string()),
    }
}

/// Minecraft サーバーの稼働状態。落ちているか、尋ねられなければ None
pub async fn server_status(app: &App) -> Option<Status> {
    let (status, body) = get_json(app, "/server/status").await.ok()?;
    if status != 200 || body["online"].as_bool() != Some(true) {
        return None;
    }
    let players = &body["players"];
    let motd = match &body["motd"] {
        Value::String(text) => vec![text.clone()],
        other => strings(other),
    };
    Some(Status {
        version: body["version"].as_str().unwrap_or("").to_string(),
        online: players["online"].as_i64().unwrap_or(0),
        max: players["max"].as_i64().unwrap_or(0),
        players: strings(&players["names"]),
        motd,
    })
}
