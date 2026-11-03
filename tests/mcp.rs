//! MCP サーバー: 受け口とツール

mod common;

use axum::http::header;
use common::*;
use serde_json::{Value, json};

#[tokio::test]
async fn initialize_echoes_the_protocol_version() {
    let server = start().await;
    let res = server
        .rpc(
            "initialize",
            json!({ "protocolVersion": "2025-06-18" }),
            None,
        )
        .await;
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["id"], 7);
    assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    let info = &body["result"]["serverInfo"];
    assert_eq!(info["name"], "TORO Server");
    assert_eq!(info["title"], "TORO Server");
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
    // 外部の画像を読めないクライアントがあるので、本体を埋め込んだものだけを伝える
    let icons = info["icons"].as_array().unwrap();
    assert_eq!(icons.len(), 1);
    assert!(
        icons[0]["src"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,iVBOR")
    );
    assert_eq!(icons[0]["mimeType"], "image/png");
    assert_eq!(icons[0]["sizes"][0], "64x64");
    assert_eq!(
        body["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
}

#[tokio::test]
async fn lists_are_served() {
    let server = start().await;
    let body: Value = server
        .rpc("tools/list", json!({}), None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["result"]["tools"].as_array().unwrap().len(), 7);
    let body: Value = server
        .rpc("resources/list", json!({}), None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["result"]["resources"], json!([]));
    let body: Value = server
        .rpc("prompts/list", json!({}), None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["result"]["prompts"], json!([]));
    let body: Value = server
        .rpc("ping", json!({}), None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["result"], json!({}));
}

#[tokio::test]
async fn notifications_and_errors() {
    let server = start().await;
    assert_eq!(
        server
            .rpc("notifications/initialized", json!({}), None)
            .await
            .status(),
        202
    );

    let res = server.rpc("no/such", json!({}), None).await;
    assert_eq!(res.status(), 404);
    assert_eq!(res.json::<Value>().await.unwrap()["error"]["code"], -32601);

    // id の無い未知の通知は黙って受ける
    let res = server
        .client
        .post(server.url("/mcp"))
        .json(&json!({ "jsonrpc": "2.0", "method": "notifications/other" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 202);

    let res = server
        .client
        .post(server.url("/mcp"))
        .body("{broken")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    assert_eq!(res.json::<Value>().await.unwrap()["error"]["code"], -32700);
}

#[tokio::test]
async fn browsers_go_to_the_wiki_and_clients_get_json() {
    let server = start().await;
    let request = |accept: &'static str| {
        server
            .client
            .get(server.url("/"))
            .header(header::ACCEPT, accept)
    };

    let res = request("text/html").send().await.unwrap();
    assert_eq!(res.status(), 302);
    assert_eq!(
        location(&res),
        format!(
            "https://wiki.torosaba.net/wiki/{}/",
            toro_mcp::util::encode_uri_component("AIクライアントの接続方法")
        )
    );

    let body: Value = request("application/json")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["authenticated"], false);
    assert_eq!(body["tools"].as_array().unwrap().len(), 7);

    // 専用のパスはブラウザでも JSON
    let res = server
        .client
        .get(server.url("/mcp"))
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .unwrap();
    assert_eq!(res.json::<Value>().await.unwrap()["status"], "ok");
}

#[tokio::test]
async fn discovery_names_the_authenticated_user() {
    let server = start().await;
    let token = server.editor_token();
    let body: Value = server
        .client
        .get(server.url("/mcp"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["user"]["username"], "editor");
    assert_eq!(body["user"]["can_edit"], true);
}

#[tokio::test]
async fn icons_are_served_with_cross_origin_headers() {
    let server = start().await;
    for icon in [
        "/favicon.ico",
        "/favicon.png",
        "/_assets/favicon-32.png",
        "/apple-touch-icon.png",
        "/icon-64.png",
        "/icon-192.png",
        "/icon-512.png",
    ] {
        let res = server.get(icon).await;
        assert_eq!(res.status(), 200, "{icon}");
        assert_eq!(res.headers()[header::CONTENT_TYPE], "image/png", "{icon}");
        assert_eq!(res.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert_eq!(
            res.headers()["cross-origin-resource-policy"],
            "cross-origin"
        );
        assert!(res.bytes().await.unwrap().starts_with(b"\x89PNG"), "{icon}");
    }
}

#[tokio::test]
async fn unknown_paths_fall_into_the_mcp_endpoint() {
    let server = start().await;
    let body: Value = server.get("/api/auth/me").await.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn healthz_answers() {
    let server = start().await;
    let res = server.get("/healthz").await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.json::<Value>().await.unwrap()["ok"], true);
}

#[tokio::test]
async fn sse_sends_the_endpoint_and_stays_open() {
    let server = start().await;
    let mut res = server
        .client
        .get(server.url("/sse"))
        .header(header::ACCEPT, "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/event-stream");
    assert_eq!(res.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    let first = res.chunk().await.unwrap().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&first),
        "event: endpoint\ndata: https://mcp.torosaba.net/mcp\n\n"
    );

    // すぐには閉じない
    let next = tokio::time::timeout(std::time::Duration::from_millis(200), res.chunk()).await;
    assert!(next.is_err(), "stream closed early");
}

#[tokio::test]
async fn wiki_list_and_get_tools() {
    let server = start().await;
    server.put_file("docs/01_About/a.md", "sha1", "本文");

    let (text, is_error) = server.tool("wiki_list_pages", json!({}), None).await;
    assert!(!is_error);
    assert_eq!(
        text,
        "取得ドキュメント総数: 2\n\n- 01_About/a.md\n- 02_Guide/b.md"
    );

    let (text, _) = server
        .tool("wiki_list_pages", json!({ "category": "02_Guide" }), None)
        .await;
    assert_eq!(text, "取得ドキュメント総数: 1\n\n- 02_Guide/b.md");

    let (text, is_error) = server
        .tool("wiki_get_page", json!({ "path": "01_About/a" }), None)
        .await;
    assert!(!is_error);
    assert_eq!(text, "=== Path: docs/01_About/a.md (SHA: sha1) ===\n\n本文");

    let (text, is_error) = server
        .tool("wiki_get_page", json!({ "path": "missing" }), None)
        .await;
    assert!(is_error);
    assert!(text.contains("\"missing\" にドキュメントが見つかりません"));
    let (_, is_error) = server.tool("wiki_get_page", json!({}), None).await;
    assert!(is_error);
}

#[tokio::test]
async fn paths_are_left_to_the_api_to_judge() {
    let server = start().await;
    let token = server.editor_token();

    // 使えないパスを api が 400 で断ると、見つからない / 使えないという案内になる
    let (text, is_error) = server
        .tool("wiki_get_page", json!({ "path": "../../secret" }), None)
        .await;
    assert!(is_error);
    assert!(text.contains("見つかりません"));
    let (text, is_error) = server
        .tool(
            "wiki_save_page",
            json!({ "path": "../.github/x", "content": "x" }),
            Some(&token),
        )
        .await;
    assert!(is_error);
    assert_eq!(text, "Error: パス \"../.github/x\" は使えません。");
}

#[tokio::test]
async fn wiki_save_tool_checks_permission_and_commits() {
    let server = start().await;
    let args = json!({ "path": "a", "content": "本文", "message": "更新" });

    let (text, is_error) = server.tool("wiki_save_page", args.clone(), None).await;
    assert!(is_error);
    assert!(text.starts_with("認証エラー"));

    let readonly = server.token(json!({ "id": "9", "username": "ro", "can_edit": false }));
    let (text, is_error) = server
        .tool("wiki_save_page", args.clone(), Some(&readonly))
        .await;
    assert!(is_error);
    assert!(text.starts_with("権限エラー"));
    // 権限が無い利用者の依頼は、api まで届かない
    assert!(server.api.saves.lock().unwrap().is_empty());

    let editor = server.editor_token();
    let (text, is_error) = server.tool("wiki_save_page", args, Some(&editor)).await;
    assert!(!is_error, "{text}");
    assert_eq!(
        text,
        "コミット成功！\n- コミット SHA: commit-1\n- ファイル: a\n- コミット者: editor"
    );
}

#[tokio::test]
async fn wiki_save_forwards_the_bearer_and_the_commit_message() {
    let server = start().await;
    let editor = server.editor_token();
    let (_, is_error) = server
        .tool(
            "wiki_save_page",
            json!({ "path": "a", "content": "本文", "message": "更新", "sha": "old-sha" }),
            Some(&editor),
        )
        .await;
    assert!(!is_error);
    // メッセージを省いたときの既定と、sha を省いたときの形
    let (_, is_error) = server
        .tool(
            "wiki_save_page",
            json!({ "path": "b", "content": "本文" }),
            Some(&editor),
        )
        .await;
    assert!(!is_error);

    let saves = server.api.saves.lock().unwrap();
    assert_eq!(saves[0].authorization, format!("Bearer {editor}"));
    assert_eq!(saves[0].body["path"], "a");
    assert_eq!(saves[0].body["content"], "本文");
    assert_eq!(saves[0].body["sha"], "old-sha");
    assert_eq!(
        saves[0].body["message"],
        "docs: 更新 (by editor via TORO Server)"
    );
    assert_eq!(
        saves[1].body["message"],
        "docs: Update via AI Connector (by editor via TORO Server)"
    );
    assert!(saves[1].body.get("sha").is_none());
}

#[tokio::test]
async fn wiki_save_failure_keeps_the_old_text() {
    let server = start().await;
    *server.api.save_error.lock().unwrap() =
        Some("GitHub Commit error (409): docs/a.md does not match current".into());
    let (text, is_error) = server
        .tool(
            "wiki_save_page",
            json!({ "path": "a", "content": "x" }),
            Some(&server.editor_token()),
        )
        .await;
    assert!(is_error);
    assert_eq!(
        text,
        "コミット失敗: GitHub Commit error (409): docs/a.md does not match current"
    );
}

#[tokio::test]
async fn wiki_search_finds_text_in_documents() {
    let server = start().await;
    let search = |query: &'static str| server.tool("wiki_search", json!({ "query": query }), None);

    assert_eq!(
        search("traincarts").await.0,
        "検索結果 \"traincarts\":\n\n- 01_About/a.md"
    );
    assert_eq!(
        search("電車 TrainCarts").await.0,
        "検索結果 \"電車 TrainCarts\":\n\n- 01_About/a.md"
    );
    assert_eq!(
        search("02_guide").await.0,
        "検索結果 \"02_guide\":\n\n- 02_Guide/b.md"
    );
    assert!(search("無い語").await.0.contains("見つかりませんでした"));
    assert!(search("   ").await.0.contains("見つかりませんでした"));
}

#[tokio::test]
async fn wiki_search_cuts_long_results() {
    let server = start().await;
    {
        let mut docs = server.api.docs.lock().unwrap();
        for i in 0..40 {
            docs.insert(format!("03_Many/p{i:02}.md"), ("s".into(), "共通語".into()));
        }
    }
    let (text, is_error) = server
        .tool("wiki_search", json!({ "query": "共通語" }), None)
        .await;
    assert!(!is_error);
    assert_eq!(text.matches("\n- ").count(), 30);
    assert!(text.ends_with("(ほかに 10 件)"));
}

#[tokio::test]
async fn wiki_search_failure_is_reported() {
    let server = start().await;
    server
        .api
        .search_unavailable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (text, is_error) = server
        .tool("wiki_search", json!({ "query": "x" }), None)
        .await;
    assert!(is_error);
    assert!(text.contains("検索に失敗しました"));
}

#[tokio::test]
async fn tools_that_read_work_without_a_token() {
    let server = start().await;
    for (name, args) in [
        ("wiki_list_pages", json!({})),
        ("wiki_get_page", json!({ "path": "01_About/a" })),
        ("wiki_search", json!({ "query": "電車" })),
        ("server_get_status", json!({})),
        ("server_get_info", json!({})),
        ("auth_get_identity", json!({})),
    ] {
        let (text, _) = server.tool(name, args, None).await;
        assert!(!text.is_empty(), "{name}");
    }
    // トークンが無ければ auth には尋ねない
    assert_eq!(
        server
            .auth
            .requests
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn data_tools_report_errors_when_the_api_is_down() {
    // 誰も待っていないポート
    let server = start_with(|cfg| cfg.api_internal_url = "http://127.0.0.1:9".into()).await;

    for (name, args) in [
        ("wiki_list_pages", json!({})),
        ("wiki_get_page", json!({ "path": "01_About/a" })),
        ("wiki_search", json!({ "query": "x" })),
    ] {
        let (text, is_error) = server.tool(name, args, None).await;
        assert!(is_error, "{name}: {text}");
    }
    let (text, is_error) = server
        .tool(
            "wiki_save_page",
            json!({ "path": "a", "content": "x" }),
            Some(&server.editor_token()),
        )
        .await;
    assert!(is_error);
    assert!(text.starts_with("コミット失敗"));

    let (text, is_error) = server.tool("server_get_status", json!({}), None).await;
    assert!(!is_error);
    assert!(text.contains("[OFFLINE] オフライン"));
}

#[tokio::test]
async fn server_status_comes_from_the_api() {
    let server = start().await;
    let (text, is_error) = server.tool("server_get_status", json!({}), None).await;
    assert!(!is_error);
    assert!(text.contains("[OFFLINE] オフライン"));

    *server.api.status.lock().unwrap() = Some(json!({
        "online": true,
        "version": "Paper 1.20.4",
        "players": { "online": 2, "max": 50, "names": ["alice", "bob"] },
        "motd": ["TORO", "Server"],
    }));
    let (text, _) = server.tool("server_get_status", json!({}), None).await;
    assert_eq!(
        text,
        "[TORO Server (Minecraft) 稼働状態]\n- 状態: [ONLINE] オンライン\n- バージョン: Paper 1.20.4\n- 接続アドレス: torosaba.net\n- プレイヤー数: 2 / 50\n- ログイン中: alice, bob\n- MOTD: TORO / Server"
    );
}

#[tokio::test]
async fn server_info_and_identity() {
    let server = start().await;
    let (text, _) = server.tool("server_get_info", json!({}), None).await;
    assert!(text.contains("https://wiki.torosaba.net/"));

    let (text, _) = server.tool("auth_get_identity", json!({}), None).await;
    assert!(text.starts_with("未認証状態です"));

    let (text, _) = server
        .tool("auth_get_identity", json!({}), Some(&server.editor_token()))
        .await;
    assert!(text.contains("- USER: editor"));
    assert!(text.contains("- DISCORD ID: 1001"));
    assert!(text.contains("[YES] (TORO SERVER MEMBER)"));

    let (text, is_error) = server.tool("no_such_tool", json!({}), None).await;
    assert!(is_error);
    assert!(text.contains("no_such_tool"));
}

#[tokio::test]
async fn delete_is_not_supported() {
    let server = start().await;
    let res = server
        .client
        .delete(server.url("/mcp"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 405);
}
