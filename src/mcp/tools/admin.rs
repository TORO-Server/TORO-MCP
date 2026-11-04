//! 荒らし対策 (toro-guard の窓口) と Discord のツール。Admin 以上の人の頼みを取り次ぐ
//!
//! 窓口 (toro-guard) には、共有の秘密を使わず、**呼び出した利用者の Bearer トークンをそのまま渡す** (OAuth 2.0)。
//! 窓口が auth の `/introspect` に尋ね、いまの Discord のロールが Admin 以上かを、自分で確かめる (ここの判断は、見せるかどうかだけ)。
//! 窓口の audit.jsonl には、トークンの持ち主が残る。Discord のトークンは、このサービスには無い
//! (toro-guard が DiscordSRV の設定から読み、Discord-MCP に渡す)。

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::{Caller, UNTRUSTED, arg, failure, success};
use crate::App;
use crate::util::encode_uri_component;

/// 管理用のツールの名前 (定義は assets/mcp/tools_admin.json)
pub const TOOLS: [&str; 15] = [
    "guard_status",
    "guard_list_incidents",
    "guard_get_incident",
    "guard_get_image",
    "guard_report_incident",
    "guard_rollback",
    "guard_dismiss_incident",
    "guard_player_activity",
    "guard_recent_chat",
    "guard_mute_player",
    "guard_kick_player",
    "guard_map_screenshot",
    "guard_get_screenshot",
    "discord_search_tools",
    "discord_call_tool",
];

pub fn is_admin_tool(name: &str) -> bool {
    TOOLS.contains(&name)
}

/// 窓口の応答の待ち時間。Muse のコネクターの 1 回の呼び出しは 45 秒で切れるので、それより短くする
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

enum Reply {
    Json(Value),
    Png(Vec<u8>),
}

/// 事件の ID として安全か (窓口の URL にそのまま入れるので、区切りや .. を通さない)
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && !id.contains("..")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-')
}

/// Minecraft のプレイヤー名として安全か
fn valid_player(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// 窓口に 1 回頼む。エラーは、窓口の `error` をそのまま利用者に見せられる文にして返す
async fn ask(
    app: &App,
    who: &Caller<'_>,
    method: reqwest::Method,
    path_and_query: &str,
    body: Option<Value>,
    timeout: Duration,
) -> Result<Reply, String> {
    let Some(base) = app.cfg.guard_url.as_deref() else {
        return Err("荒らし対策の窓口が設定されていません".into());
    };
    let mut request = app
        .http
        .request(method, format!("{base}{path_and_query}"))
        .bearer_auth(who.token)
        .timeout(timeout);
    if let Some(body) = body {
        request = request.json(&body);
    }
    // 内部の URL は利用者に見せない。詳しくはログにだけ残す
    let res = request.send().await.map_err(|e| {
        eprintln!("guard: {e}");
        "荒らし対策の窓口につなげません".to_string()
    })?;
    let status = res.status();
    let is_png = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("image/png"));
    if status.is_success() && is_png {
        return res
            .bytes()
            .await
            .map(|b| Reply::Png(b.to_vec()))
            .map_err(|e| {
                eprintln!("guard: {e}");
                "荒らし対策の窓口から画像を受け取れません".to_string()
            });
    }
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(Reply::Json(body));
    }
    let message = body["error"].as_str().unwrap_or("窓口がエラーを返しました");
    Err(format!("{message} (HTTP {})", status.as_u16()))
}

fn image_result(png: &[u8], caption: &str) -> Value {
    json!({ "content": [
        { "type": "image", "data": STANDARD.encode(png), "mimeType": "image/png" },
        { "type": "text", "text": caption },
    ] })
}

fn render(reply: Result<Reply, String>, untrusted: bool) -> Value {
    match reply {
        Ok(Reply::Json(body)) if untrusted => success(&format!("{UNTRUSTED}{}", pretty(&body))),
        Ok(Reply::Json(body)) => success(&pretty(&body)),
        Ok(Reply::Png(png)) => image_result(&png, "画像"),
        Err(message) => failure(&message),
    }
}

fn optional_text(args: &Value, name: &str) -> Option<String> {
    args[name]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn query(pairs: &[(&str, Option<String>)]) -> String {
    let parts: Vec<String> = pairs
        .iter()
        .filter_map(|(k, v)| {
            v.as_ref()
                .map(|v| format!("{k}={}", encode_uri_component(v)))
        })
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// 数のつもりの引数 (整数でも小数でも、文字列で来ても受ける)。数でなければ None
fn number(args: &Value, name: &str) -> Option<f64> {
    match &args[name] {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub async fn call(app: &App, name: &str, args: &Value, who: &Caller<'_>) -> Value {
    use reqwest::Method;
    let get = |path: String| async move {
        ask(app, who, Method::GET, &path, None, DEFAULT_TIMEOUT).await
    };
    let post = |path: String, body: Value| async move {
        ask(app, who, Method::POST, &path, Some(body), DEFAULT_TIMEOUT).await
    };

    let id = arg(args, "id");
    let player = arg(args, "player");
    let id_ok = valid_id(id);
    let player_ok = valid_player(player);
    let bad_id =
        || failure("事件の ID が正しくありません (guard_list_incidents の id を使ってください)");
    let bad_player = || failure("プレイヤー名が正しくありません (英数字と _ . で 32 文字まで)");

    match name {
        "guard_status" => render(get("/v1/status".into()).await, false),
        "guard_list_incidents" => {
            let state = optional_text(args, "state");
            let limit = number(args, "limit").map(|n| (n as i64).to_string());
            render(
                get(format!(
                    "/v1/incidents{}",
                    query(&[("state", state), ("limit", limit)])
                ))
                .await,
                true,
            )
        }
        "guard_get_incident" if id_ok => render(get(format!("/v1/incidents/{id}")).await, true),
        "guard_get_image" if id_ok => {
            let image = arg(args, "name");
            if !valid_id(image) {
                return failure("画像の名前が正しくありません (事件の images にあるもの)");
            }
            match get(format!("/v1/incidents/{id}/images/{image}")).await {
                Ok(Reply::Png(png)) => image_result(&png, &format!("{id} / {image}")),
                Ok(Reply::Json(_)) => failure("画像ではない応答でした"),
                Err(message) => failure(&message),
            }
        }
        "guard_report_incident" if id_ok => render(
            post(
                format!("/v1/incidents/{id}/report"),
                json!({ "note": arg(args, "note") }),
            )
            .await,
            false,
        ),
        "guard_rollback" if id_ok => render(
            post(format!("/v1/incidents/{id}/rollback"), json!({})).await,
            false,
        ),
        "guard_dismiss_incident" if id_ok => render(
            post(
                format!("/v1/incidents/{id}/dismiss"),
                json!({ "reason": arg(args, "reason") }),
            )
            .await,
            false,
        ),
        "guard_player_activity" if player_ok => {
            let hours = number(args, "hours").map(|n| n.to_string());
            render(
                get(format!(
                    "/v1/players/{player}/activity{}",
                    query(&[("hours", hours)])
                ))
                .await,
                true,
            )
        }
        "guard_recent_chat" => {
            let who_filter = optional_text(args, "player");
            if who_filter.as_deref().is_some_and(|p| !valid_player(p)) {
                return bad_player();
            }
            let minutes = number(args, "minutes").map(|n| (n as i64).to_string());
            render(
                get(format!(
                    "/v1/chat{}",
                    query(&[("minutes", minutes), ("player", who_filter)])
                ))
                .await,
                true,
            )
        }
        "guard_mute_player" if player_ok => render(
            post(
                format!("/v1/players/{player}/mute"),
                json!({ "reason": arg(args, "reason") }),
            )
            .await,
            false,
        ),
        "guard_kick_player" if player_ok => render(
            post(
                format!("/v1/players/{player}/kick"),
                json!({ "reason": arg(args, "reason") }),
            )
            .await,
            false,
        ),
        "guard_map_screenshot" => {
            let (Some(x), Some(z)) = (number(args, "x"), number(args, "z")) else {
                return failure("x と z (数) が要ります");
            };
            let mut body = json!({ "x": x, "z": z });
            if let Some(y) = number(args, "y") {
                body["y"] = json!(y);
            }
            if let Some(distance) = number(args, "distance") {
                body["distance"] = json!(distance);
            }
            if let Some(world) = optional_text(args, "world") {
                body["world"] = json!(world);
            }
            // 撮影は 1 分ほどかかる (コネクターの 1 回の呼び出しは 45 秒で切れる) ので、始めるだけにして、job を返す
            render(post("/v1/screenshot".into(), body).await, false)
        }
        "guard_get_screenshot" => {
            let job = arg(args, "job");
            if !valid_id(job) {
                return failure("job が正しくありません (guard_map_screenshot が返した値)");
            }
            match get(format!("/v1/screenshots/{job}")).await {
                Ok(Reply::Png(png)) => image_result(&png, &format!("BlueMap (job {job})")),
                other => render(other, false),
            }
        }
        "discord_search_tools" => {
            let q = optional_text(args, "query");
            render(
                get(format!("/v1/discord/tools{}", query(&[("query", q)]))).await,
                false,
            )
        }
        "discord_call_tool" => {
            let tool = arg(args, "tool");
            if tool.is_empty() {
                return failure("tool が要ります");
            }
            let mut body = json!({ "tool": tool });
            for key in ["pathParams", "queryParams", "body"] {
                if args[key].is_object() {
                    body[key] = args[key].clone();
                }
            }
            for key in ["fields", "reason"] {
                if let Some(text) = optional_text(args, key) {
                    body[key] = json!(text);
                }
            }
            render(post("/v1/discord/call".into(), body).await, true)
        }
        "guard_get_incident"
        | "guard_get_image"
        | "guard_report_incident"
        | "guard_rollback"
        | "guard_dismiss_incident" => bad_id(),
        "guard_player_activity" | "guard_mute_player" | "guard_kick_player" => bad_player(),
        _ => failure(&format!("不明なツールです: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names_are_checked_before_they_reach_a_url() {
        assert!(valid_id("20261104-101500-fuuty119"));
        assert!(valid_id("chat-20261104-a.b_c"));
        for bad in [
            "", "../x", "a/b", "a\\b", "a b", "a..b", "a?x=1", "a%2F", "あ",
        ] {
            assert!(!valid_id(bad), "{bad:?}");
        }
        assert!(valid_player("fuuty119"));
        assert!(valid_player(".Steve_1"));
        for bad in ["", "a b", "a;op", "a\nb", "x".repeat(33).as_str(), "あ"] {
            assert!(!valid_player(bad), "{bad:?}");
        }
    }

    #[test]
    fn query_is_encoded_and_skips_empty_values() {
        assert_eq!(query(&[("a", None), ("b", None)]), "");
        assert_eq!(
            query(&[("player", Some("a b&c".into())), ("limit", None)]),
            "?player=a%20b%26c"
        );
    }

    #[test]
    fn numbers_come_from_numbers_or_strings() {
        let args = json!({ "a": 3, "b": 2.5, "c": " 7 ", "d": "x", "e": true });
        assert_eq!(number(&args, "a"), Some(3.0));
        assert_eq!(number(&args, "b"), Some(2.5));
        assert_eq!(number(&args, "c"), Some(7.0));
        assert_eq!(number(&args, "d"), None);
        assert_eq!(number(&args, "e"), None);
        assert_eq!(number(&args, "missing"), None);
    }
}
