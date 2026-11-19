//! 環境変数からの設定。秘密は無く、既定は本番の値にしてある (荒らし対策のコネクタ `CRIMEBUSTER_CONNECTOR_URL` だけは、使うときに指定する)

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    /// 認可サーバー (auth) の公開 URL (末尾の / なし)
    pub issuer: String,
    /// REST API の公開 URL (末尾の / なし)
    pub api_url: String,
    /// この MCP サーバーの公開 URL (末尾の / なし)
    pub mcp_url: String,
    /// Wiki の公開 URL (末尾の / なし)
    pub wiki_url: String,
    /// Cookie 付きで呼べる Origin (他のサブドメインは信じない)
    pub cors_origins: Vec<String>,

    /// トークンの確認を頼む先 (auth の内部の窓口。末尾の / なし)
    pub auth_internal_url: String,
    /// auth の `/introspect` へ付ける共有秘密 (`X-Toro-Token`)。auth が `INTROSPECT_SECRET` を設定したときに要る。無ければ付けない
    pub auth_introspect_secret: Option<String>,
    /// Wiki と稼働状態のデータを頼む先 (api。末尾の / なし)
    pub api_internal_url: String,

    /// 荒らし対策のコネクタ (crime-buster の connector。内部のネットワークだけ。末尾の / なし)。
    /// 無ければ、管理用のツールは出さない。コネクタには、共有の秘密ではなく、利用者のトークンをそのまま渡す
    /// (コネクタが、auth に尋ねて、Admin かを確かめる)
    pub crime_buster_url: Option<String>,
    /// 管理用のツールを使える Discord のロールの ID
    pub admin_roles: Vec<String>,

    /// リクエストのボディを読み切るまで待つ秒数。遅い送信で同時処理の枠を埋めさせない
    pub body_read_timeout_secs: u64,
}

fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn or_default(name: &str, default: &str) -> String {
    optional(name).unwrap_or_else(|| default.to_string())
}

fn trimmed(name: &str, default: &str) -> String {
    or_default(name, default).trim_end_matches('/').to_string()
}

/// 荒らし対策のコネクタの URL を決める。`CRIMEBUSTER_CONNECTOR_URL` を読む。
fn connector_url_from(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    get("CRIMEBUSTER_CONNECTOR_URL").filter(|value| !value.is_empty())
}

/// 管理用のツールを使える Discord のロールの ID を決める。`ADMIN_ROLE_IDS` (カンマ区切り) を読む。
/// 既定の値は持たない。未設定か空なら空の一覧になり、管理用のツールは誰にも出さない (起動は続け、匿名と Wiki のツールだけが使える)
fn admin_roles_from(get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    get("ADMIN_ROLE_IDS")
        .unwrap_or_default()
        .split(',')
        .map(|role| role.trim().to_string())
        .filter(|role| !role.is_empty())
        .collect()
}

impl Config {
    /// Cookie 付きのリクエストを受けてよい送り元か (auth と、指定の Origin)
    pub fn is_trusted_origin(&self, origin: &str) -> bool {
        origin == self.issuer || self.cors_origins.iter().any(|o| o == origin)
    }

    pub fn from_env() -> Result<Self, String> {
        let auth_internal_url = optional("AUTH_INTERNAL_URL")
            .ok_or_else(|| "missing environment variables: AUTH_INTERNAL_URL".to_string())?
            .trim_end_matches('/')
            .to_string();
        Ok(Self {
            port: or_default("PORT", "8080")
                .parse()
                .map_err(|_| "PORT must be a number".to_string())?,
            issuer: trimmed("PUBLIC_AUTH_URL", "https://auth.torosaba.net"),
            api_url: trimmed("PUBLIC_API_URL", "https://api.torosaba.net"),
            mcp_url: trimmed("PUBLIC_MCP_URL", "https://mcp.torosaba.net"),
            wiki_url: trimmed("PUBLIC_WIKI_URL", "https://wiki.torosaba.net"),
            cors_origins: or_default("CORS_ORIGINS", "https://wiki.torosaba.net")
                .split(',')
                .map(|origin| origin.trim().to_string())
                .filter(|origin| !origin.is_empty())
                .collect(),
            auth_internal_url,
            auth_introspect_secret: optional("AUTH_INTROSPECT_SECRET"),
            api_internal_url: trimmed("API_INTERNAL_URL", "http://toro-api:8080"),
            crime_buster_url: connector_url_from(|name| std::env::var(name).ok())
                .map(|url| url.trim_end_matches('/').to_string()),
            admin_roles: admin_roles_from(|name| std::env::var(name).ok()),
            body_read_timeout_secs: 30,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{admin_roles_from, connector_url_from};
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let values: HashMap<&str, &str> = pairs.iter().copied().collect();
        move |name| values.get(name).map(|value| (*value).to_string())
    }

    #[test]
    fn admin_roles_are_empty_when_not_set_or_blank() {
        assert!(admin_roles_from(env_of(&[])).is_empty());
        assert!(admin_roles_from(env_of(&[("ADMIN_ROLE_IDS", "")])).is_empty());
        assert!(admin_roles_from(env_of(&[("ADMIN_ROLE_IDS", " , ,")])).is_empty());
    }

    #[test]
    fn admin_roles_are_split_and_trimmed() {
        let roles = admin_roles_from(env_of(&[("ADMIN_ROLE_IDS", "1, 2 ,,3")]));
        assert_eq!(roles, vec!["1", "2", "3"]);
    }

    #[test]
    fn connector_url_uses_the_connector_name() {
        let url = connector_url_from(env_of(&[(
            "CRIMEBUSTER_CONNECTOR_URL",
            "http://crime-buster:8090",
        )]));
        assert_eq!(url.as_deref(), Some("http://crime-buster:8090"));
    }

    #[test]
    fn connector_url_ignores_the_legacy_name() {
        let url = connector_url_from(env_of(&[(
            "CRIMEBUSTER_CONTROL_URL",
            "http://crime-buster:8090",
        )]));
        assert_eq!(url, None);
    }

    #[test]
    fn connector_url_is_none_when_not_set() {
        let url = connector_url_from(env_of(&[]));
        assert_eq!(url, None);
    }
}
