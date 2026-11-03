//! 発見用のメタデータ

use axum::response::Response;
use serde_json::json;

use crate::App;
use crate::http::{self, Ctx};

/// RFC 9728: Protected Resource Metadata。認可は auth に任せる
pub fn protected_resource(app: &App, ctx: &Ctx) -> Response {
    let data = json!({
        "resource": app.cfg.mcp_url,
        "resource_name": "TORO Server",
        "authorization_servers": [app.cfg.issuer],
        "scopes_supported": ["read", "write", "mcp"],
        "bearer_methods_supported": ["header"],
        "resource_documentation": app.cfg.connect_page(),
    });
    http::pretty_json(http::cors_headers(ctx), &data, 200)
}
