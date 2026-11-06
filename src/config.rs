//! 環境変数からの設定。秘密は無く、既定は本番の値にしてある (荒らし対策の窓口 `GUARD_CONTROL_URL` だけは、使うときに指定する)

/// 管理用のツールを使える Discord のロール (TORO サーバーの Owner と Admin)
const DEFAULT_ADMIN_ROLES: &str = "338718477332512769,497387545928138762";

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
    /// Wiki と稼働状態のデータを頼む先 (api。末尾の / なし)
    pub api_internal_url: String,

    /// 荒らし対策の窓口 (toro-guard の control。内部のネットワークだけ。末尾の / なし)。
    /// 無ければ、管理用のツールは出さない。窓口には、共有の秘密ではなく、利用者のトークンをそのまま渡す
    /// (窓口が、auth に尋ねて、Admin かを確かめる)
    pub guard_url: Option<String>,
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

impl Config {
    /// Cookie 付きのリクエストを受けてよい送り元か (auth と、指定の Origin)
    pub fn is_trusted_origin(&self, origin: &str) -> bool {
        origin == self.issuer || self.cors_origins.iter().any(|o| o == origin)
    }

    pub fn from_env() -> Result<Self, String> {
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
            auth_internal_url: trimmed("AUTH_INTERNAL_URL", "http://toro-auth:8081"),
            api_internal_url: trimmed("API_INTERNAL_URL", "http://toro-api:8080"),
            guard_url: optional("GUARD_CONTROL_URL")
                .map(|url| url.trim_end_matches('/').to_string()),
            admin_roles: or_default("ADMIN_ROLE_IDS", DEFAULT_ADMIN_ROLES)
                .split(',')
                .map(|role| role.trim().to_string())
                .filter(|role| !role.is_empty())
                .collect(),
            body_read_timeout_secs: 30,
        })
    }
}
