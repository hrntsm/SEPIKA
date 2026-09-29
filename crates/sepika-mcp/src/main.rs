//! SEPIKA MCP サーバの起動バイナリ（stdio トランスポート）。
//!
//! 使い方:
//!   sepika-mcp [MODEL.ovika]
//!
//! stdout は MCP の JSON-RPC トランスポートそのものなので、ログや診断は
//! 一切 stdout に書かないこと。

use sepika_core::model::Model;
use sepika_mcp::server::run_stdio_server;
use sepika_mcp::{default_result_dir, ServerState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = match std::env::args().nth(1) {
        Some(path) => sepika_io::ovika::load_ovika(std::path::Path::new(&path))?.model,
        None => Model::default(),
    };
    let state = ServerState::with_fs_store(model, default_result_dir())?;
    run_stdio_server(state).await
}
