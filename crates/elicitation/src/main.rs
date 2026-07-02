//! `elicitation` — standalone stdio MCP server for the deterministic
//! structured-discovery interview kernel.
//!
//! Run from source:
//!
//! ```bash
//! cargo run -p elicitation
//! ```
//!
//! After install the binary is on your PATH as `elicitation`. It speaks MCP
//! over stdio (the standard transport for Claude Code, Cursor, and most MCP
//! clients). The on-disk event log lives under the resolved state dir (see
//! [`elicitation::resolve_state_dir`]); override with `ELICITATION_STATE_DIR`.

use std::sync::Arc;

use elicitation::{resolve_state_dir, ElicitationServer};
use elicitation_core::{Engine, FilesystemStore};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let state_dir = resolve_state_dir();
    tracing::info!(?state_dir, "starting elicitation stdio server");
    let store = FilesystemStore::new(&state_dir)?;
    let engine = Engine::new(Arc::new(store));
    let server = ElicitationServer::new(engine);
    server.serve_stdio().await?;
    Ok(())
}

fn init_tracing() {
    // Log to stderr so stdout stays the MCP transport channel.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}
