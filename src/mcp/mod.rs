//! Remote MCP サーバー
//!
//! - `transport`: 受け口 (つなぎ方の記事への案内、SSE、発見用の JSON、JSON-RPC 2.0)
//! - `tools`: ツールの定義と実行
//! - `metadata`: OAuth の保護リソースのメタデータ

pub mod metadata;
pub mod tools;
pub mod transport;
