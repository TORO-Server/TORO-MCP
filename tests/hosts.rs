//! 役割の範囲と、URL の正本

mod common;

use axum::http::header;
use common::*;
use serde_json::{Value, json};

#[tokio::test]
async fn protected_resource_metadata_points_to_auth() {
    let server = start().await;
    let body: Value = server
        .get("/.well-known/oauth-protected-resource")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["resource"], "https://mcp.torosaba.net");
    assert_eq!(body["resource_name"], "TORO Server");
    assert_eq!(body["authorization_servers"], json!([AUTH_URL]));
    assert_eq!(body["scopes_supported"], json!(["read", "write", "mcp"]));
    assert!(
        body["resource_documentation"]
            .as_str()
            .unwrap()
            .starts_with("https://wiki.torosaba.net/wiki/")
    );
}

#[tokio::test]
async fn only_the_root_answers_as_mcp() {
    let server = start().await;
    for path in [
        "/mcp",
        "/sse",
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/oauth-authorization-server",
        "/.well-known/openid-configuration",
        "/oauth/register",
        "/oauth/authorize?client_id=x",
        "/oauth/token",
    ] {
        assert_eq!(server.get(path).await.status(), 404, "{path}");
    }
}

#[tokio::test]
async fn every_host_gets_the_same_answer() {
    let server = start().await;
    for host in ["mcp.torosaba.net", "localhost", "auth.torosaba.net"] {
        let res = server
            .client
            .get(server.url("/"))
            .header(header::HOST, host)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{host}");
        assert_eq!(res.json::<Value>().await.unwrap()["status"], "ok", "{host}");
    }
}

#[tokio::test]
async fn pages_take_their_urls_from_the_config() {
    let server = start_with(|cfg| {
        cfg.issuer = "https://auth.example.test".into();
        cfg.mcp_url = "https://mcp.example.test".into();
        cfg.api_url = "https://api.example.test".into();
        cfg.wiki_url = "https://wiki.example.test".into();
    })
    .await;

    let guide = server
        .client
        .get(server.url("/"))
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .unwrap();
    assert!(location(&guide).starts_with("https://wiki.example.test/wiki/"));
    assert!(location(&guide).ends_with('/'));

    let (text, _) = server.tool("server_get_info", json!({}), None).await;
    assert!(text.contains("https://api.example.test/"));
    assert!(text.contains("https://auth.example.test/"));
    assert!(text.contains("https://mcp.example.test/"));
    assert!(text.contains("https://wiki.example.test/wiki/"));
    assert!(!text.contains("auth.torosaba.net"));

    let meta: Value = server
        .get("/.well-known/oauth-protected-resource")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(meta["resource"], "https://mcp.example.test");
    assert_eq!(
        meta["authorization_servers"],
        json!(["https://auth.example.test"])
    );

    let res = server.rpc("tools/list", json!({}), Some("bad")).await;
    assert!(
        res.headers()[header::WWW_AUTHENTICATE]
            .to_str()
            .unwrap()
            .contains("https://mcp.example.test/.well-known/oauth-protected-resource")
    );
}
