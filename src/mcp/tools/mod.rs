//! MCP のツール。定義は assets/mcp/tools.json、実行は機能ごとのファイル
//!
//! - `wiki`: wiki_list_pages / wiki_get_page / wiki_save_page / wiki_search
//! - `server`: server_get_status / server_get_info
//! - `identity`: auth_get_identity

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

pub fn definitions() -> &'static Value {
    &DEFINITIONS
}

pub async fn call(app: &App, name: &str, args: &Value, caller: Option<&Caller<'_>>) -> Value {
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
}
