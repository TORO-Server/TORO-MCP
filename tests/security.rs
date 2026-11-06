//! トークンの扱い、CORS、キャッシュ、上限

mod common;

use axum::http::header;
use common::*;
use serde_json::{Value, json};

#[tokio::test]
async fn unknown_and_expired_tokens_get_401_so_clients_can_reauthorize() {
    let server = start().await;
    // auth が有効と認めないものは、期限切れも、別の鍵のものも、ただの文字列も同じ
    for token in ["expired", "signed-with-another-key", "garbage", "a.b.c"] {
        let res = server.rpc("tools/list", json!({}), Some(token)).await;
        assert_eq!(res.status(), 401, "{token}");
        let challenge = res.headers()[header::WWW_AUTHENTICATE].to_str().unwrap();
        assert!(challenge.contains("error=\"invalid_token\""));
        assert!(challenge.contains(
            "resource_metadata=\"https://mcp.torosaba.net/.well-known/oauth-protected-resource\""
        ));
        assert_eq!(
            res.headers()["access-control-expose-headers"],
            "WWW-Authenticate"
        );

        let res = server
            .client
            .get(server.url("/"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 401, "{token}");
    }

    // トークンなしと、有効なトークンは今までどおり
    assert_eq!(
        server.rpc("tools/list", json!({}), None).await.status(),
        200
    );
    let valid = server.editor_token();
    assert_eq!(
        server
            .rpc("tools/list", json!({}), Some(&valid))
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn tokens_without_an_id_are_not_accepted() {
    let server = start().await;
    let token = server.token(json!({ "username": "x", "can_edit": true }));
    assert_eq!(
        server
            .rpc("tools/list", json!({}), Some(&token))
            .await
            .status(),
        401
    );
}

#[tokio::test]
async fn an_auth_outage_is_503_not_401() {
    let server = start().await;
    let token = server.editor_token();
    server.auth.set_broken(true);

    for request in [
        server.rpc("tools/list", json!({}), Some(&token)).await,
        server
            .client
            .get(server.url("/"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap(),
    ] {
        assert_eq!(request.status(), 503);
        assert!(!request.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(
            request.json::<Value>().await.unwrap()["error"],
            "Authentication service unavailable"
        );
    }

    // トークンなしの利用は auth に頼らないので、止まらない
    assert_eq!(
        server.rpc("tools/list", json!({}), None).await.status(),
        200
    );

    server.auth.set_broken(false);
    assert_eq!(
        server
            .rpc("tools/list", json!({}), Some(&token))
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn an_unreachable_auth_is_503_too() {
    let server = start_with(|cfg| cfg.auth_internal_url = "http://127.0.0.1:9".into()).await;
    let res = server.rpc("tools/list", json!({}), Some("any")).await;
    assert_eq!(res.status(), 503);
}

#[tokio::test]
async fn mcp_ignores_cookies() {
    let server = start().await;
    // Cookie にトークンを入れても、見ない (auth に尋ねもしない)
    let token = server.editor_token();
    let cookie = format!("__Host-toro_session={token}");

    let res = server
        .client
        .post(server.url("/"))
        .header(header::COOKIE, &cookie)
        .header(header::ORIGIN, WIKI_ORIGIN)
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "wiki_save_page", "arguments": { "path": "x", "content": "y" } },
        }))
        .send()
        .await
        .unwrap();
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["result"]["isError"], true);
    assert!(server.api.saves.lock().unwrap().is_empty());

    let discovery: Value = server
        .client
        .get(server.url("/"))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(discovery["authenticated"], false);
    assert_eq!(
        server
            .auth
            .requests
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn read_scope_cannot_write() {
    let server = start().await;
    let token = |scope: &str| {
        server.token(json!({
            "id": "1", "sub": "1", "username": "u", "can_edit": true, "scope": scope,
            "type": "mcp_access_token",
        }))
    };
    let args = json!({ "path": "x", "content": "y" });

    let (text, is_error) = server
        .tool("wiki_save_page", args.clone(), Some(&token("read")))
        .await;
    assert!(is_error, "{text}");
    assert!(text.starts_with("権限エラー"));
    assert!(server.api.saves.lock().unwrap().is_empty());

    for scope in ["mcp", "read write"] {
        let (text, is_error) = server
            .tool("wiki_save_page", args.clone(), Some(&token(scope)))
            .await;
        assert!(!is_error, "{scope}: {text}");
    }
    assert_eq!(server.api.saves.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn only_exact_origins_get_credentialed_cors() {
    let server = start().await;
    let origin_header = |origin: &'static str| {
        let request = server
            .client
            .get(server.url("/"))
            .header(header::ORIGIN, origin);
        async move { request.send().await.unwrap() }
    };

    for trusted in [WIKI_ORIGIN, AUTH_URL] {
        let res = origin_header(trusted).await;
        assert_eq!(res.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], trusted);
        assert_eq!(
            res.headers()[header::ACCESS_CONTROL_ALLOW_CREDENTIALS],
            "true"
        );
    }
    for other in [
        "https://assets.torosaba.net",
        "https://wiki.torosaba.net.evil.example",
    ] {
        let res = origin_header(other).await;
        assert_eq!(res.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert!(
            !res.headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
        );
    }

    let res = server
        .client
        .request(reqwest::Method::OPTIONS, server.url("/"))
        .header(header::ORIGIN, WIKI_ORIGIN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    assert!(
        res.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
            .to_str()
            .unwrap()
            .contains("Authorization")
    );
}

#[tokio::test]
async fn responses_are_not_cacheable_unless_they_say_so() {
    let server = start().await;
    let res = server.get("/").await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(res.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");

    let res = server.get("/icon-64.png").await;
    assert!(
        res.headers()[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public")
    );
    assert_eq!(res.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
}

#[tokio::test]
async fn health_checks_answer_even_when_every_slot_is_taken() {
    use tokio::io::AsyncWriteExt;
    let server = start().await;
    let address = server.base.strip_prefix("http://").unwrap();
    // ボディを送らないまま待つ接続で、同時処理の枠 (256) をすべて埋める
    let mut streams = Vec::new();
    for _ in 0..256 {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 10\r\n\r\n")
            .await
            .unwrap();
        streams.push(stream);
    }
    let busy = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let res = server.get("/nope").await;
            if res.status() == 503 {
                return res;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(busy.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(busy.headers()[header::CACHE_CONTROL], "no-store");

    let health = server.get("/healthz").await;
    assert_eq!(health.status(), 200);
    assert_eq!(health.headers()[header::CACHE_CONTROL], "no-store");
    drop(streams);
}

#[tokio::test]
async fn slow_bodies_are_cut_off_and_the_slot_comes_back() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let server = start_with(|cfg| cfg.body_read_timeout_secs = 1).await;
    let address = server.base.strip_prefix("http://").unwrap();
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    // 宣言した長さのボディを送らないまま待つ
    stream
        .write_all(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 10\r\n\r\n")
        .await
        .unwrap();
    let mut head = [0u8; 12];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut head),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&head, b"HTTP/1.1 408");
    assert_eq!(server.get("/healthz").await.status(), 200);
}

#[tokio::test]
async fn large_bodies_are_refused() {
    let server = start().await;
    let res = server
        .client
        .post(server.url("/"))
        .body(vec![b'x'; 2 * 1024 * 1024 + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 413);
}
