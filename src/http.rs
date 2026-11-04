//! リクエストの読み取りと、共通の応答 (JSON、CORS、リダイレクト)

use axum::body::{Body, Bytes};
use axum::http::header::{self, HeaderMap, HeaderName, HeaderValue};
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::response::Response;
use serde_json::{Value, json};
use url::form_urlencoded;

use crate::config::Config;

/// 各機能に渡すリクエスト
pub struct Ctx {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: HeaderMap,
    /// 生のクエリ文字列
    pub raw_query: String,
    /// Origin が、Cookie 付きのリクエストを受けてよい送り元か
    pub origin_trusted: bool,
    pub body: Bytes,
}

impl Ctx {
    pub fn new(parts: &Parts, body: Bytes, cfg: &Config) -> Self {
        let query = parts.uri.query().unwrap_or("");
        let origin = parts
            .headers
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        Self {
            origin_trusted: cfg.is_trusted_origin(origin),
            raw_query: query.to_string(),
            method: parts.method.clone(),
            path: parts.uri.path().to_string(),
            query: form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect(),
            headers: parts.headers.clone(),
            body,
        }
    }

    /// クエリの値 (同じ名前が複数あれば先頭)
    pub fn q(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// ヘッダーの値 (無ければ空文字)
    pub fn header(&self, name: HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }
}

pub fn cors_headers(ctx: &Ctx) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    match HeaderValue::from_str(ctx.header(header::ORIGIN)) {
        Ok(value) if ctx.origin_trusted => {
            headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
        _ => {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
        }
    }
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, Authorization, X-Requested-With"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    headers
}

/// どの Origin からでも読める応答のヘッダー
pub fn public_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers
}

pub fn respond(
    status: u16,
    mut headers: HeaderMap,
    content_type: &'static str,
    body: impl Into<Body>,
) -> Response {
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    let mut res = Response::new(body.into());
    *res.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    *res.headers_mut() = headers;
    res
}

pub const JSON_UTF8: &str = "application/json; charset=utf-8";

pub fn json(ctx: &Ctx, data: &Value, status: u16) -> Response {
    respond(status, cors_headers(ctx), JSON_UTF8, data.to_string())
}

/// 整形した JSON (発見用のメタデータなど)
pub fn pretty_json(mut headers: HeaderMap, data: &Value, status: u16) -> Response {
    let body = serde_json::to_string_pretty(data).expect("JSON value serializes");
    headers.remove(header::CONTENT_TYPE);
    respond(status, headers, JSON_UTF8, body)
}

/// 同時に受けられる数を超えたとき
pub fn busy() -> Response {
    let mut headers = public_headers();
    headers.insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
    respond(
        503,
        headers,
        JSON_UTF8,
        json!({ "error": "Service Unavailable" }).to_string(),
    )
}

/// 受け取っただけで返すものが無いとき (MCP の通知)
pub fn accepted(headers: HeaderMap) -> Response {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::ACCEPTED;
    *res.headers_mut() = headers;
    res
}

pub fn no_content(headers: HeaderMap) -> Response {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::NO_CONTENT;
    *res.headers_mut() = headers;
    res
}

/// 302。`location` は URL として正しいものだけを渡す
pub fn redirect(location: &str) -> Response {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::FOUND;
    match HeaderValue::from_str(location) {
        Ok(value) => {
            res.headers_mut().insert(header::LOCATION, value);
        }
        Err(_) => *res.status_mut() = StatusCode::BAD_REQUEST,
    }
    res
}
