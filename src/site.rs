//! 各サービスの URL と、人向けのページ。URL は `Config` だけが持ち、応答はここから作る
//!
//! - auth: だれなのかを確かめる (ログイン、トークンの発行、OAuth の認可)
//! - api: データを読み書きする (REST API)
//! - mcp: AI クライアントがつなぐ (この Remote MCP サーバー)
//!
//! 人向けの正本は Wiki の「サービスのURLと役割」のページ。

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::config::Config;
use crate::util::encode_uri_component;

const ROLES_PAGE: &str = "サービスのURLと役割";
const CONNECT_PAGE: &str = "AIクライアントの接続方法";

/// 本体を埋め込んだ TORO のアイコン。Gemini は CSP で外部の画像を読めないので、AI クライアントにはこれを伝える
pub fn icon_data_uri() -> String {
    let png = STANDARD.encode(include_bytes!("../assets/icons/icon-64.png"));
    format!("data:image/png;base64,{png}")
}

impl Config {
    /// Wiki のページの URL。末尾の / まで付ける (無いと Wiki が 301 で付け直す)
    fn wiki_page(&self, name: &str) -> String {
        format!("{}/wiki/{}/", self.wiki_url, encode_uri_component(name))
    }

    /// 人向けの、URL と役割のページ
    pub fn roles_page(&self) -> String {
        self.wiki_page(ROLES_PAGE)
    }

    /// 人向けの、AI クライアントのつなぎ方のページ
    pub fn connect_page(&self) -> String {
        self.wiki_page(CONNECT_PAGE)
    }
}
