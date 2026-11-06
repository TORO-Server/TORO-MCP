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
        let scope_allows = match self.claims.get("scope") {
            Some(scope) => scope.as_str().is_some_and(scope_allows_write),
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

    /// Admin 以上のロールを持つ人か。ロールが `admin_roles` のどれかであること。
    /// ロールは、auth が確認のたびに、いまの Discord で確かめた値 (`roles_source` が `live`) だけを信じる。
    /// OAuth のアクセストークンも 90 日トークンも同じに扱える (トークンに入っている古いロールは見ない)
    pub fn is_admin(&self, admin_roles: &[String]) -> bool {
        self.claims.get("roles_source").and_then(Value::as_str) == Some("live")
            && self
                .role_ids()
                .iter()
                .any(|role| admin_roles.iter().any(|admin| admin == role))
    }

    /// 管理用のツールを使ってよいか。Admin 以上のロールで、トークンがそこまで許されていること。
    /// scope が無いのは本人が発行したトークンで、OAuth のトークンは同意した scope に mcp があるときだけ
    pub fn can_use_admin_tools(&self, admin_roles: &[String]) -> bool {
        let scope_allows = match self.claims.get("scope") {
            Some(scope) => scope.as_str().is_some_and(scope_allows_admin_tools),
            None => true,
        };
        scope_allows && self.is_admin(admin_roles)
    }

    fn str_claim(&self, name: &str) -> &str {
        self.claims.get(name).and_then(Value::as_str).unwrap_or("")
    }
}

/// scope が Wiki の変更を許しているか
pub fn scope_allows_write(scope: &str) -> bool {
    scope.split_whitespace().any(|s| s == "write" || s == "mcp")
}

/// scope が管理用のツールまで許しているか
pub fn scope_allows_admin_tools(scope: &str) -> bool {
    scope.split_whitespace().any(|s| s == "mcp")
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
        // 文字列でない scope は、無いものと同じに扱わない
        assert!(!session(json!({ "can_edit": true, "scope": null })).can_write());
        assert!(!session(json!({ "can_edit": true, "scope": ["write"] })).can_write());
    }

    #[test]
    fn admin_tools_need_the_role_and_the_scope() {
        let admins = vec!["10".to_string()];
        let session = |scope: Option<Value>, roles: Value| {
            let mut claims = json!({ "roles": roles, "roles_source": "live" });
            if let Some(scope) = scope {
                claims["scope"] = scope;
            }
            Session {
                claims: claims.as_object().unwrap().clone(),
            }
        };
        // 本人が発行したトークンには scope が無い。OAuth のトークンは mcp に同意したものだけ
        for scope in [
            None,
            Some(json!("mcp")),
            Some(json!("read write mcp")),
            Some(json!("mcp read")),
        ] {
            assert!(
                session(scope.clone(), json!(["10"])).can_use_admin_tools(&admins),
                "{scope:?}"
            );
            // ロールが無ければ、scope があっても使えない
            assert!(
                !session(scope.clone(), json!(["5"])).can_use_admin_tools(&admins),
                "{scope:?}"
            );
        }
        for scope in [
            json!("read"),
            json!("read write"),
            json!("write"),
            json!(""),
            json!("mcpx"),
            json!("xmcp"),
            json!("MCP"),
            Value::Null,
            json!(["mcp"]),
            json!(1),
        ] {
            assert!(
                !session(Some(scope.clone()), json!(["10"])).can_use_admin_tools(&admins),
                "{scope:?}"
            );
        }
    }

    #[test]
    fn admin_needs_one_of_the_admin_roles() {
        let session = |claims: Value| Session {
            claims: claims.as_object().unwrap().clone(),
        };
        let admins = vec!["10".to_string(), "20".to_string()];
        let live = |roles: Value| session(json!({ "roles": roles, "roles_source": "live" }));
        assert!(live(json!(["5", "20"])).is_admin(&admins));
        assert!(!live(json!(["5", "6"])).is_admin(&admins));
        assert!(!live(json!([])).is_admin(&admins));
        assert!(!live(json!(["10"])).is_admin(&[]));
        // 数字でないロールや、ロールの名前は通さない
        assert!(!live(json!([10])).is_admin(&admins));
        // いまの Discord で確かめていないロール (トークンに入っていた古い値) は、信じない
        assert!(!session(json!({ "roles": ["20"] })).is_admin(&admins));
        assert!(!session(json!({ "roles": ["20"], "roles_source": "token" })).is_admin(&admins));
        assert!(
            !session(json!({ "roles": ["20"], "roles_source": "unavailable" })).is_admin(&admins)
        );
    }
}
