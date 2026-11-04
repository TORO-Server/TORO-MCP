//! MCP のツール。定義は assets/mcp/tools.json、実行は機能ごとのファイル
//!
//! - `wiki`: wiki_list_pages / wiki_get_page / wiki_save_page / wiki_search
//! - `server`: server_get_status / server_get_info
//! - `identity`: auth_get_identity
//! - `admin`: 荒らし対策 (guard_*) と Discord (discord_*)。Admin 以上のロールを持つ人にだけ見せる。
//!   定義は assets/mcp/tools_admin.json

mod admin;
mod identity;
mod server;
mod wiki;

use std::sync::LazyLock;

use serde_json::{Value, json};

use crate::App;
use crate::identity::Session;

/// ツールを呼んだ利用者。トークンは、api に保存を頼むときに渡す
pub struct Caller<'a> {
    pub token: &'a str,
    pub session: &'a Session,
}

static DEFINITIONS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../assets/mcp/tools.json")).expect("tools.json")
});

static ADMIN_DEFINITIONS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../assets/mcp/tools_admin.json"))
        .expect("tools_admin.json")
});

pub fn definitions() -> &'static Value {
    &DEFINITIONS
}

/// 管理用のツールを見せてよいか。窓口が設定されていて、呼び出し元が Admin 以上のとき
fn admin_allowed(app: &App, session: Option<&Session>) -> bool {
    app.cfg.guard_url.is_some() && session.is_some_and(|s| s.is_admin(&app.cfg.admin_roles))
}

/// 呼び出し元に見せるツールの一覧。管理用のツールは、Admin 以上の人にだけ足す
pub fn definitions_for(app: &App, session: Option<&Session>) -> Value {
    let mut tools = definitions().as_array().cloned().unwrap_or_default();
    if admin_allowed(app, session) {
        tools.extend(ADMIN_DEFINITIONS.as_array().cloned().unwrap_or_default());
    }
    Value::Array(tools)
}

pub async fn call(app: &App, name: &str, args: &Value, caller: Option<&Caller<'_>>) -> Value {
    if admin::is_admin_tool(name) {
        // 一覧に出さないだけでなく、呼ばれても、ここで断る
        let Some(caller) = caller.filter(|c| admin_allowed(app, Some(c.session))) else {
            return failure("このツールは、Admin 以上のロールを持つ人でつないだときだけ使えます");
        };
        return admin::call(app, name, args, caller).await;
    }
    match name {
        "wiki_list_pages" => wiki::list_pages(app, args).await,
        "wiki_get_page" => wiki::get_page(app, args).await,
        "wiki_save_page" => wiki::save_page(app, args, caller).await,
        "wiki_search" => wiki::search(app, args).await,
        "server_get_status" => server::status(app).await,
        "server_get_info" => server::info(app),
        "auth_get_identity" => identity::get(caller.map(|c| c.session)),
        _ => failure(&format!("不明なツールです: {name}")),
    }
}

fn success(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn failure(text: &str) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

fn arg<'a>(args: &'a Value, name: &str) -> &'a str {
    args[name].as_str().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_list_every_tool() {
        let names: Vec<&str> = definitions()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "wiki_list_pages",
                "wiki_get_page",
                "wiki_save_page",
                "wiki_search",
                "server_get_status",
                "server_get_info",
                "auth_get_identity",
            ]
        );
    }

    #[test]
    fn admin_definitions_match_the_handlers() {
        let names: Vec<&str> = ADMIN_DEFINITIONS
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.iter().all(|name| admin::is_admin_tool(name)));
        assert_eq!(names.len(), admin::TOOLS.len());
        let public: Vec<&str> = definitions()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.iter().all(|name| !public.contains(name)));
    }
}
