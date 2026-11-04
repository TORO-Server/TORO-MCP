//! MCP の受け口: つなぎ方の記事への案内、SSE、発見用の JSON、JSON-RPC 2.0

use std::convert::Infallible;

use axum::http::Method;
use axum::http::header::{self, HeaderMap, HeaderName, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};

use super::tools;
use crate::App;
use crate::http::{self, Ctx};
use crate::identity::{self, Identity, Session};
use crate::site;

/// コネクタの名前。どのクライアントにもこの表記で見せる
const SERVER_NAME: &str = "TORO Server";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_PROTOCOL_VERSION: &str = "2025-03-26";

fn headers(ctx: &Ctx) -> HeaderMap {
    let mut headers = http::cors_headers(ctx);
    headers.insert(
        HeaderName::from_static("access-control-expose-headers"),
        HeaderValue::from_static("WWW-Authenticate"),
    );
    headers
}

pub async fn handle(app: &App, ctx: &Ctx) -> Response {
    // トークンなしの利用は認めるが、無効なトークンは匿名として扱わない。
    // 401 を返さないと、クライアントは再認可が要ることに気づけない。
    // auth に尋ねられなかったときは 503 にする (401 だと、障害のたびに再認可させてしまう)
    let session = match identity::identify(app, ctx).await {
        Identity::Anonymous => None,
        Identity::User(session) => Some(session),
        Identity::Invalid => return unauthorized(app, ctx),
        Identity::Unavailable => return unavailable(ctx),
    };
    // セッションを持たないので、切断するものが無い (MCP の仕様どおり 405)
    if ctx.method == Method::DELETE {
        let mut headers = headers(ctx);
        headers.insert(
            header::ALLOW,
            HeaderValue::from_static("GET, POST, OPTIONS"),
        );
        return http::respond(
            405,
            headers,
            http::JSON_UTF8,
            json!({ "error": "Method Not Allowed" }).to_string(),
        );
    }
    if ctx.method == Method::GET || ctx.method == Method::HEAD {
        return handle_get(app, ctx, session.as_ref());
    }
    handle_rpc(app, ctx, session.as_ref()).await
}

fn unauthorized(app: &App, ctx: &Ctx) -> Response {
    let mut headers = headers(ctx);
    let challenge = format!(
        "Bearer error=\"invalid_token\", resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
        app.cfg.mcp_url
    );
    if let Ok(value) = HeaderValue::from_str(&challenge) {
        headers.insert(header::WWW_AUTHENTICATE, value);
    }
    http::respond(
        401,
        headers,
        http::JSON_UTF8,
        json!({ "error": "invalid_token", "error_description": "Invalid or expired token" })
            .to_string(),
    )
}

fn unavailable(ctx: &Ctx) -> Response {
    let mut headers = headers(ctx);
    headers.insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
    http::respond(
        503,
        headers,
        http::JSON_UTF8,
        json!({ "error": "Authentication service unavailable" }).to_string(),
    )
}

fn handle_get(app: &App, ctx: &Ctx, session: Option<&Session>) -> Response {
    let accept = ctx.header(header::ACCEPT);
    let path = ctx.path.as_str();
    // 接続先は `/` だけを伝える。以前に `/mcp` で登録したクライアントのために、`/mcp` も同じに答える
    let is_dedicated_path = path == "/mcp" || path.starts_with("/mcp/");
    let wants_json = accept.contains("application/json") || ctx.q("format") == Some("json");

    // ブラウザで開いた人には、つなぎ方の記事を見せる
    if !is_dedicated_path && !wants_json && accept.contains("text/html") {
        return http::redirect(&app.cfg.connect_page());
    }

    if accept.contains("text/event-stream") {
        return sse(app);
    }

    let tool_names: Vec<Value> = tools::definitions_for(app, session)
        .as_array()
        .map(|list| list.iter().map(|tool| tool["name"].clone()).collect())
        .unwrap_or_default();
    let user = session.map_or(
        Value::Null,
        |s| json!({ "username": s.username(), "id": s.id(), "can_edit": s.can_edit() }),
    );
    let mcp_url = &app.cfg.mcp_url;
    http::pretty_json(
        headers(ctx),
        &json!({
            "status": "ok",
            "service": SERVER_NAME,
            "version": SERVER_VERSION,
            "authenticated": session.is_some(),
            "user": user,
            "tools": tool_names,
            "instructions": format!(
                "Connect via Claude / ChatGPT / Gemini by setting URL \"{mcp_url}/\". Setup guide: {}",
                app.cfg.connect_page()
            ),
        }),
        200,
    )
}

/// 接続先を伝えたあとは、客が切るまで開けたままにする。開けておける数には上限がある
fn sse(app: &App) -> Response {
    let Ok(permit) = app.sse_streams.clone().try_acquire_owned() else {
        return http::busy();
    };
    let endpoint = Event::default()
        .event("endpoint")
        .data(format!("{}/", app.cfg.mcp_url));
    let events = stream::once(async move { Ok::<_, Infallible>(endpoint) })
        .chain(stream::pending())
        .map(move |event| {
            let _held_until_disconnect = &permit;
            event
        });
    let mut res = Sse::new(events)
        .keep_alive(KeepAlive::default())
        .into_response();
    res.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    res
}

/// 本体を埋め込んだ data: URI だけを伝える。
/// Gemini は CSP で外部の画像を読めないのに、URL があれば大きい方を選んで壊れた画像を出す
fn server_icons() -> Value {
    json!([
        {
            "src": site::icon_data_uri(),
            "mimeType": "image/png",
            "sizes": ["64x64"],
        },
    ])
}

async fn handle_rpc(app: &App, ctx: &Ctx, session: Option<&Session>) -> Response {
    let Ok(body) = serde_json::from_slice::<Value>(&ctx.body) else {
        let parse_error = json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "Parse error" } });
        return http::respond(
            400,
            http::public_headers(),
            "application/json",
            parse_error.to_string(),
        );
    };

    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body.get("method").and_then(Value::as_str).unwrap_or("");
    let params = body.get("params").cloned().unwrap_or_else(|| json!({}));
    let reply = |result: Value| {
        let body = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        http::respond(200, headers(ctx), http::JSON_UTF8, body.to_string())
    };

    match method {
        "initialize" => {
            let version = params["protocolVersion"]
                .as_str()
                .unwrap_or(DEFAULT_PROTOCOL_VERSION);
            reply(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "title": SERVER_NAME,
                    "version": SERVER_VERSION,
                    "websiteUrl": app.cfg.wiki_url,
                    "icons": server_icons(),
                },
                "instructions": "TORO Server provides tools to interact with the TORO Server Wiki and server status.",
            }))
        }
        "notifications/initialized" => http::accepted(headers(ctx)),
        "tools/list" => reply(json!({ "tools": tools::definitions_for(app, session) })),
        "resources/list" => reply(json!({ "resources": [] })),
        "prompts/list" => reply(json!({ "prompts": [] })),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let empty = json!({});
            let args = params
                .get("arguments")
                .filter(|a| a.is_object())
                .unwrap_or(&empty);
            // 保存は、利用者のトークンをそのまま api に渡す
            let caller = identity::bearer_token(ctx)
                .zip(session)
                .map(|(token, session)| tools::Caller { token, session });
            reply(tools::call(app, name, args, caller.as_ref()).await)
        }
        "ping" => reply(json!({})),
        _ if id.is_null() => http::accepted(headers(ctx)),
        _ => {
            let body = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method not found: {method}") },
            });
            http::respond(404, headers(ctx), http::JSON_UTF8, body.to_string())
        }
    }
}
