//! TORO Server の AI コネクタ (mcp)。役割と URL は `site` を見てください。
//!
//! - `mcp`: Remote MCP サーバー (受け口とツール)
//! - `identity`: 呼び出し元の確認 (トークンは auth に尋ねる)
//! - `api`: Wiki と稼働状態のデータの取得 (TORO-API に尋ねる)
//! - `static_files`: アイコン
//!
//! 共通の部品は `config` / `site` / `http` / `util`、URL から各機能への振り分けは `routes` にある。

pub mod api;
pub mod config;
pub mod http;
pub mod identity;
pub mod mcp;
pub mod routes;
pub mod site;
pub mod static_files;
pub mod util;

use std::sync::Arc;
use std::time::Duration;

use config::Config;
use tokio::sync::Semaphore;

/// 同時に処理するリクエストの上限 (ボディは最大 2 MiB なので、メモリは約 512 MiB まで)
const MAX_CONCURRENT_REQUESTS: usize = 256;
/// 開けたままにできる SSE の上限
pub const MAX_SSE_STREAMS: usize = 128;

/// リクエストをまたいで共有するもの
pub struct App {
    pub cfg: Config,
    pub http: reqwest::Client,
    pub requests: Semaphore,
    pub sse_streams: Arc<Semaphore>,
}

impl App {
    pub fn new(cfg: Config) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .user_agent("TORO-MCP")
            .build()?;
        Ok(Self {
            cfg,
            http,
            requests: Semaphore::new(MAX_CONCURRENT_REQUESTS),
            sse_streams: Arc::new(Semaphore::new(MAX_SSE_STREAMS)),
        })
    }
}
