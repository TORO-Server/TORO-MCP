//! 呼び出し元の確認
//!
//! このサービスは JWT を自分では検証しない (署名の鍵を持たない)。
//! Bearer を auth の内部の窓口 (`POST /introspect`) に渡し、有効かどうかと中身を教えてもらう。
//! Cookie は見ない。MCP のクライアントは Bearer を使うので、ブラウザからの CSRF は成り立たない。

use std::time::Duration;

use axum::http::header;
use serde_json::{Map, Value, json};

use crate::App;
use crate::http::Ctx;

/// auth への問い合わせの上限 (遅い auth に MCP の応答まで巻き込まれないように)
const INTROSPECT_TIMEOUT: Duration = Duration::from_secs(3);

pub struct Session {
    pub claims: Map<String, Value>,
}

impl Session {
    pub fn id(&self) -> &str {
        self.str_claim("id")
    }

    pub fn username(&self) -> &str {
        self.str_claim("username")
    }

    pub fn can_edit(&self) -> bool {
        self.claims
            .get("can_edit")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Wiki を変更してよいか。OAuth のトークンは、許可された scope (write か mcp) の中だけ
    pub fn can_write(&self) -> bool {
        let scope_allows = match self.claims.get("scope").and_then(Value::as_str) {
            Some(scope) => scope_allows_write(scope),
            None => true,
        };
        self.can_edit() && scope_allows
    }

    /// ログインしたときのロールの ID
    pub fn role_ids(&self) -> Vec<&str> {
        self.claims
            .get("roles")
            .and_then(Value::as_array)
            .map(|roles| roles.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    fn str_claim(&self, name: &str) -> &str {
        self.claims.get(name).and_then(Value::as_str).unwrap_or("")
    }
}

/// scope が Wiki の変更を許しているか
pub fn scope_allows_write(scope: &str) -> bool {
    scope.split_whitespace().any(|s| s == "write" || s == "mcp")
}

pub enum Identity {
    /// Bearer が無い
    Anonymous,
    User(Session),
    /// Bearer はあるが、無効か期限切れ (再認可が要る)
    Invalid,
    /// auth に尋ねられなかった。トークンが悪いとは限らないので、再認可させてはいけない
    Unavailable,
}

pub fn bearer_token(ctx: &Ctx) -> Option<&str> {
    ctx.header(header::AUTHORIZATION)
        .strip_prefix("Bearer ")
        .map(str::trim)
}

pub async fn identify(app: &App, ctx: &Ctx) -> Identity {
    let Some(token) = bearer_token(ctx) else {
        return Identity::Anonymous;
    };
    if token.is_empty() {
        return Identity::Invalid;
    }
    introspect(app, token).await
}

async fn introspect(app: &App, token: &str) -> Identity {
    let url = format!("{}/introspect", app.cfg.auth_internal_url);
    let response = app
        .http
        .post(url)
        .timeout(INTROSPECT_TIMEOUT)
        .json(&json!({ "token": token }))
        .send()
        .await;
    let body: Value = match response {
        Ok(res) if res.status().is_success() => match res.json().await {
            Ok(body) => body,
            Err(_) => return Identity::Unavailable,
        },
        Ok(res) => {
            eprintln!("introspect: auth answered {}", res.status());
            return Identity::Unavailable;
        }
        Err(e) => {
            eprintln!("introspect: {e}");
            return Identity::Unavailable;
        }
    };

    match body.get("active").and_then(Value::as_bool) {
        Some(false) => Identity::Invalid,
        Some(true) => match body.get("claims").and_then(Value::as_object) {
            Some(claims) if claims.get("id").is_some_and(Value::is_string) => {
                Identity::User(Session {
                    claims: claims.clone(),
                })
            }
            Some(_) => Identity::Invalid,
            None => Identity::Unavailable,
        },
        None => Identity::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_needs_edit_permission_and_scope() {
        let session = |claims: Value| Session {
            claims: claims.as_object().unwrap().clone(),
        };
        assert!(session(json!({ "can_edit": true })).can_write());
        assert!(session(json!({ "can_edit": true, "scope": "read write" })).can_write());
        assert!(session(json!({ "can_edit": true, "scope": "mcp" })).can_write());
        assert!(!session(json!({ "can_edit": true, "scope": "read" })).can_write());
        assert!(!session(json!({ "can_edit": false, "scope": "write" })).can_write());
    }
}
