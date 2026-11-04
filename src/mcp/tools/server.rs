//! TORO Server 全般のツール

use serde_json::Value;

use super::success;
use crate::{App, api};

pub async fn status(app: &App) -> Value {
    let Some(status) = api::server_status(app).await else {
        return success(
            "[TORO Server (Minecraft) 稼働状態]\n- 状態: [OFFLINE] オフライン\n- 接続アドレス: torosaba.net",
        );
    };

    let version = if status.version.is_empty() {
        "不明"
    } else {
        &status.version
    };
    let mut lines = vec![
        "[TORO Server (Minecraft) 稼働状態]".to_string(),
        "- 状態: [ONLINE] オンライン".to_string(),
        format!("- バージョン: {version}"),
        "- 接続アドレス: torosaba.net".to_string(),
        format!("- プレイヤー数: {} / {}", status.online, status.max),
    ];
    if !status.players.is_empty() {
        lines.push(format!("- ログイン中: {}", status.players.join(", ")));
    }
    if !status.motd.is_empty() {
        lines.push(format!("- MOTD: {}", status.motd.join(" / ")));
    }
    success(&lines.join("\n"))
}

pub fn info(app: &App) -> Value {
    let cfg = &app.cfg;
    success(&format!(
        "【TORO Server 各種サービス一覧】
- 公式Webサイト: https://www.torosaba.net/
- 公式Wiki: {wiki}/
- 2Dワールドマップ: https://map.torosaba.net/
- 3Dワールドマップ (BlueMap): https://3d-map.torosaba.net/
- 認証 (ログイン、トークンの発行): {auth}/
- REST API (Wiki の読み書き): {api}/
- Remote MCP サーバー (AI クライアントの接続先): {mcp}/
- Minecraft 接続先: torosaba.net
- 各サービスの URL と役割: {roles}",
        wiki = cfg.wiki_url,
        auth = cfg.issuer,
        api = cfg.api_url,
        mcp = cfg.mcp_url,
        roles = cfg.roles_page(),
    ))
}
