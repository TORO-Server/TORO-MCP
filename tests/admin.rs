//! 管理用のツール (荒らし対策と Discord): Admin 以上の人にだけ見え、窓口に取り次ぐ

mod common;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::*;
use serde_json::{Value, json};

async fn tool_names(server: &TestServer, bearer: Option<&str>) -> Vec<String> {
    let body: Value = server
        .rpc("tools/list", json!({}), bearer)
        .await
        .json()
        .await
        .unwrap();
    body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn admin_tools_are_listed_only_for_admins() {
    let server = start().await;
    let editor = server.editor_token();
    let admin = server.admin_token();
    assert_eq!(tool_names(&server, None).await.len(), 7);
    assert_eq!(tool_names(&server, Some(&editor)).await.len(), 7);
    let names = tool_names(&server, Some(&admin)).await;
    assert_eq!(names.len(), 22);
    assert!(names.contains(&"guard_rollback".to_string()));
    assert!(names.contains(&"discord_call_tool".to_string()));

    // 窓口が設定されていないときは、Admin にも出さない
    let server = start_with(|cfg| cfg.guard_url = None).await;
    let admin = server.admin_token();
    assert_eq!(tool_names(&server, Some(&admin)).await.len(), 7);

    // 発見用の JSON も、同じ
    let server = start().await;
    let admin = server.admin_token();
    let body: Value = server
        .client
        .get(server.url("/"))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["tools"].as_array().unwrap().len(), 22);
    let body: Value = server.get("/").await.json().await.unwrap();
    assert_eq!(body["tools"].as_array().unwrap().len(), 7);
}

#[tokio::test]
async fn non_admins_are_refused_and_nothing_reaches_the_guard() {
    let server = start().await;
    let editor = server.editor_token();
    // OAuth のアクセストークンのように、ロールが空のもの
    let oauth =
        server.token(json!({ "id": "7", "username": "oauth", "can_edit": true, "roles": [] }));
    let other_role =
        server.token(json!({ "id": "8", "username": "mod", "roles": ["503206005434482698"] }));
    for bearer in [
        None,
        Some(editor.as_str()),
        Some(oauth.as_str()),
        Some(other_role.as_str()),
    ] {
        for name in [
            "guard_status",
            "guard_rollback",
            "discord_call_tool",
            "guard_kick_player",
        ] {
            let (text, is_error) = server
                .tool(
                    name,
                    json!({ "id": "a1", "player": "x", "tool": "discord_create_message" }),
                    bearer,
                )
                .await;
            assert!(is_error, "{name} {bearer:?}: {text}");
            assert!(text.contains("Admin"), "{text}");
        }
    }
    assert!(server.guard.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn oauth_tokens_need_the_mcp_scope_for_admin_tools() {
    let server = start().await;
    let token_with_scope = |scope: &str| {
        server.token(json!({
            "id": "42", "username": "admin1", "can_edit": true, "scope": scope,
            "roles": [ADMIN_ROLE], "roles_source": "live", "type": "mcp_access_token",
        }))
    };

    // Admin の人でも、読むだけなどに同意したトークンでは、一覧に出ず、呼んでも窓口に届かない
    for scope in ["read", "read write", "write", "", "mcpx"] {
        let token = token_with_scope(scope);
        assert_eq!(
            tool_names(&server, Some(&token)).await.len(),
            7,
            "{scope:?}"
        );
        for name in ["guard_status", "guard_rollback", "discord_call_tool"] {
            let (text, is_error) = server
                .tool(
                    name,
                    json!({ "id": "a1", "tool": "discord_create_message" }),
                    Some(&token),
                )
                .await;
            assert!(is_error, "{name} {scope:?}: {text}");
            assert!(text.contains("mcp"), "{text}");
        }
    }
    assert!(server.guard.calls.lock().unwrap().is_empty());

    // mcp に同意したものは使える
    for scope in ["mcp", "read write mcp"] {
        let token = token_with_scope(scope);
        assert_eq!(
            tool_names(&server, Some(&token)).await.len(),
            22,
            "{scope:?}"
        );
        let (text, is_error) = server.tool("guard_status", json!({}), Some(&token)).await;
        assert!(!is_error, "{scope:?}: {text}");
    }

    // 本人が発行したトークン (scope が無い) は、ロールだけで決まる
    let personal = server.token(json!({
        "id": "42", "username": "admin1", "can_edit": true,
        "purpose": "ai_connector", "roles": [ADMIN_ROLE], "roles_source": "live",
    }));
    assert_eq!(tool_names(&server, Some(&personal)).await.len(), 22);
}

#[tokio::test]
async fn status_goes_to_the_guard_with_the_callers_own_token() {
    let server = start().await;
    let admin = server.admin_token();
    let (text, is_error) = server.tool("guard_status", json!({}), Some(&admin)).await;
    assert!(!is_error, "{text}");
    assert!(text.contains("review") && text.contains("pending"));
    let calls = server.guard.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "GET");
    assert_eq!(calls[0].target, "/v1/status");
    // 共有の秘密ではなく、呼び出した利用者の Bearer トークンをそのまま渡す (窓口が auth に尋ねて、Admin かを確かめる)
    assert_eq!(calls[0].authorization, format!("Bearer {admin}"));
}

#[tokio::test]
async fn list_incidents_encodes_the_query() {
    let server = start().await;
    let admin = server.admin_token();
    let (text, is_error) = server
        .tool(
            "guard_list_incidents",
            json!({ "state": "reported", "limit": 5 }),
            Some(&admin),
        )
        .await;
    assert!(!is_error, "{text}");
    assert!(text.contains("信頼できない"), "{text}");
    assert_eq!(
        server.guard.calls.lock().unwrap()[0].target,
        "/v1/incidents?state=reported&limit=5"
    );
    server.guard.calls.lock().unwrap().clear();
    server
        .tool("guard_list_incidents", json!({}), Some(&admin))
        .await;
    assert_eq!(
        server.guard.calls.lock().unwrap()[0].target,
        "/v1/incidents"
    );
}

#[tokio::test]
async fn unsafe_ids_and_names_never_become_urls() {
    let server = start().await;
    let admin = server.admin_token();
    for id in ["../etc", "a/b", "a?x=1", "a b", "", "a..b"] {
        for name in [
            "guard_get_incident",
            "guard_rollback",
            "guard_report_incident",
            "guard_dismiss_incident",
        ] {
            let (_, is_error) = server.tool(name, json!({ "id": id }), Some(&admin)).await;
            assert!(is_error, "{name} {id:?}");
        }
    }
    for player in ["a b", "x;op", "a\nb", "", "あ"] {
        for name in [
            "guard_mute_player",
            "guard_kick_player",
            "guard_player_activity",
        ] {
            let (_, is_error) = server
                .tool(name, json!({ "player": player }), Some(&admin))
                .await;
            assert!(is_error, "{name} {player:?}");
        }
    }
    let (_, is_error) = server
        .tool(
            "guard_recent_chat",
            json!({ "player": "a b" }),
            Some(&admin),
        )
        .await;
    assert!(is_error);
    let (_, is_error) = server
        .tool(
            "guard_get_image",
            json!({ "id": "a1", "name": "../record.json" }),
            Some(&admin),
        )
        .await;
    assert!(is_error);
    assert!(server.guard.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn actions_are_forwarded_with_their_bodies() {
    let server = start().await;
    let admin = server.admin_token();
    let cases = [
        (
            "guard_report_incident",
            json!({ "id": "a1", "note": "画像を確認" }),
            "/v1/incidents/a1/report",
            json!({ "note": "画像を確認" }),
        ),
        (
            "guard_rollback",
            json!({ "id": "a1" }),
            "/v1/incidents/a1/rollback",
            json!({}),
        ),
        (
            "guard_dismiss_incident",
            json!({ "id": "a1", "reason": "整地" }),
            "/v1/incidents/a1/dismiss",
            json!({ "reason": "整地" }),
        ),
        (
            "guard_mute_player",
            json!({ "player": "spam1", "reason": "連投" }),
            "/v1/players/spam1/mute",
            json!({ "reason": "連投" }),
        ),
        (
            "guard_kick_player",
            json!({ "player": "spam1" }),
            "/v1/players/spam1/kick",
            json!({ "reason": "" }),
        ),
    ];
    for (tool, args, target, body) in cases {
        let (text, is_error) = server.tool(tool, args, Some(&admin)).await;
        assert!(!is_error, "{tool}: {text}");
        let calls = server.guard.calls.lock().unwrap();
        let last = calls.last().unwrap();
        assert_eq!(
            (last.method.as_str(), last.target.as_str()),
            ("POST", target),
            "{tool}"
        );
        assert_eq!(last.body, body, "{tool}");
    }
}

#[tokio::test]
async fn activity_and_chat_are_marked_untrusted() {
    let server = start().await;
    let admin = server.admin_token();
    let (text, is_error) = server
        .tool(
            "guard_player_activity",
            json!({ "player": "fuuty119", "hours": 12 }),
            Some(&admin),
        )
        .await;
    assert!(!is_error);
    assert!(text.starts_with("[注意]"), "{text}");
    assert_eq!(
        server.guard.calls.lock().unwrap().last().unwrap().target,
        "/v1/players/fuuty119/activity?hours=12"
    );
    let (text, _) = server
        .tool(
            "guard_recent_chat",
            json!({ "minutes": "45", "player": "a.b" }),
            Some(&admin),
        )
        .await;
    assert!(text.starts_with("[注意]"));
    assert_eq!(
        server.guard.calls.lock().unwrap().last().unwrap().target,
        "/v1/chat?minutes=45&player=a.b"
    );
}
#[tokio::test]
async fn images_come_back_as_mcp_image_content() {
    let server = start().await;
    let admin = server.admin_token();
    for (tool, args) in [
        (
            "guard_get_image",
            json!({ "id": "a1", "name": "damage.png" }),
        ),
        ("guard_get_screenshot", json!({ "job": "abc123" })),
    ] {
        let res = server
            .rpc(
                "tools/call",
                json!({ "name": tool, "arguments": args }),
                Some(&admin),
            )
            .await;
        let body: Value = res.json().await.unwrap();
        let content = &body["result"]["content"];
        assert_eq!(content[0]["type"], "image", "{tool}: {body}");
        assert_eq!(content[0]["mimeType"], "image/png");
        assert_eq!(
            STANDARD
                .decode(content[0]["data"].as_str().unwrap())
                .unwrap(),
            PNG_BYTES
        );
        assert_eq!(content[1]["type"], "text");
        assert!(body["result"]["isError"].is_null());
    }
}

#[tokio::test]
async fn screenshots_are_started_then_fetched() {
    let server = start().await;
    let admin = server.admin_token();
    // 始めるだけ。撮れるのは 1 分ほど後なので、job を返す (コネクターの 1 回の呼び出しは 45 秒で切れる)
    let (text, is_error) = server
        .tool(
            "guard_map_screenshot",
            json!({ "x": -5650, "z": "-10100", "distance": 150 }),
            Some(&admin),
        )
        .await;
    assert!(!is_error, "{text}");
    assert!(text.contains("abc123"), "{text}");
    {
        let calls = server.guard.calls.lock().unwrap();
        assert_eq!(calls[0].method, "POST");
        assert_eq!(calls[0].target, "/v1/screenshot");
        assert_eq!(
            calls[0].body,
            json!({ "x": -5650.0, "z": -10100.0, "distance": 150.0 })
        );
    }
    let (_, is_error) = server
        .tool("guard_map_screenshot", json!({ "x": 1 }), Some(&admin))
        .await;
    assert!(is_error, "z is required");

    // まだ撮れていないときは、エラーではなく pending の文字
    let (text, is_error) = server
        .tool(
            "guard_get_screenshot",
            json!({ "job": "slow1" }),
            Some(&admin),
        )
        .await;
    assert!(!is_error, "{text}");
    assert!(text.contains("pending"), "{text}");
    for bad in ["", "../x", "a/b"] {
        let (_, is_error) = server
            .tool("guard_get_screenshot", json!({ "job": bad }), Some(&admin))
            .await;
        assert!(is_error, "{bad:?}");
    }
}

#[tokio::test]
async fn discord_calls_are_forwarded() {
    let server = start().await;
    let admin = server.admin_token();
    let (text, is_error) = server
        .tool(
            "discord_call_tool",
            json!({
                "tool": "discord_create_message",
                "pathParams": { "channel_id": "5" },
                "queryParams": { "limit": 3 },
                "body": { "content": "確認しました" },
                "reason": "荒らし対応",
                "ignored": "x",
            }),
            Some(&admin),
        )
        .await;
    assert!(!is_error, "{text}");
    assert!(text.starts_with("[注意]"));
    {
        let calls = server.guard.calls.lock().unwrap();
        assert_eq!(calls[0].target, "/v1/discord/call");
        assert_eq!(
            calls[0].body,
            json!({
                "tool": "discord_create_message",
                "pathParams": { "channel_id": "5" },
                "queryParams": { "limit": 3 },
                "body": { "content": "確認しました" },
                "reason": "荒らし対応",
            })
        );
    }
    let (_, is_error) = server
        .tool("discord_call_tool", json!({}), Some(&admin))
        .await;
    assert!(is_error);
    let (_, is_error) = server
        .tool(
            "discord_search_tools",
            json!({ "query": "mess age" }),
            Some(&admin),
        )
        .await;
    assert!(!is_error);
    assert_eq!(
        server.guard.calls.lock().unwrap().last().unwrap().target,
        "/v1/discord/tools?query=mess%20age"
    );
}

#[tokio::test]
async fn guard_errors_are_shown_as_text() {
    let server = start().await;
    let admin = server.admin_token();
    let (text, is_error) = server
        .tool(
            "guard_get_incident",
            json!({ "id": "missing" }),
            Some(&admin),
        )
        .await;
    assert!(is_error);
    assert!(
        text.contains("incident not found") && text.contains("404"),
        "{text}"
    );

    // 窓口が落ちている
    let server = start_with(|cfg| cfg.guard_url = Some("http://127.0.0.1:9".into())).await;
    let admin = server.admin_token();
    let (text, is_error) = server.tool("guard_status", json!({}), Some(&admin)).await;
    assert!(is_error);
    assert!(text.contains("つなげません"), "{text}");

    // 窓口が、トークンを断る (MCP が Admin と見ても、窓口が auth に尋ねた結果が正しい)
    let server = start().await;
    server.auth.register("tok-nobody", admin_claims());
    let (text, is_error) = server
        .tool("guard_status", json!({}), Some("tok-nobody"))
        .await;
    assert!(is_error);
    assert!(text.contains("期限切れ") && text.contains("401"), "{text}");
    server.auth.register("tok-notadmin", admin_claims());
    let (text, is_error) = server
        .tool(
            "guard_rollback",
            json!({ "id": "a1" }),
            Some("tok-notadmin"),
        )
        .await;
    assert!(is_error);
    assert!(text.contains("Admin") && text.contains("403"), "{text}");
}

#[tokio::test]
async fn roles_that_auth_did_not_confirm_are_not_trusted() {
    let server = start().await;
    // トークンに入っていた古いロール (roles_source が live でない) では、Admin にならない
    let stale = server.token(json!({
        "id": "50", "username": "stale", "roles": [ADMIN_ROLE], "roles_source": "token",
    }));
    let unavailable = server.token(json!({
        "id": "51", "username": "unavail", "roles": [ADMIN_ROLE], "roles_source": "unavailable",
    }));
    let old_auth = server.token(json!({ "id": "52", "username": "old", "roles": [ADMIN_ROLE] }));
    for token in [&stale, &unavailable, &old_auth] {
        assert_eq!(tool_names(&server, Some(token)).await.len(), 7);
        let (_, is_error) = server.tool("guard_status", json!({}), Some(token)).await;
        assert!(is_error);
    }
    assert!(server.guard.calls.lock().unwrap().is_empty());
}
