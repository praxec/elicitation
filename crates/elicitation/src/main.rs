//! `elicitation` — standalone stdio MCP server for the deterministic
//! structured-discovery interview kernel.
//!
//! Run from source:
//!
//! ```bash
//! cargo run -p eliciter
//! ```
//!
//! After install the binary is on your PATH as `elicitation`. With no arguments
//! it speaks MCP over stdio (the standard transport for Claude Code, Cursor, and
//! most MCP clients). It also takes:
//!
//! ```bash
//! elicitation --version
//! elicitation --help
//! elicitation skills install --target claude --dir .
//! ```
//!
//! The on-disk event log lives under the resolved state dir (see
//! [`elicitation::resolve_state_dir`]); override with `ELICITATION_STATE_DIR`.

use std::process::ExitCode;
use std::sync::Arc;

use elicitation::{ElicitationServer, resolve_state_dir};
use eliciter_core::{Engine, FilesystemStore};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
elicitation: a structured-discovery interview MCP server.

Usage:
  elicitation                 Run the MCP server on stdio (how MCP clients start it)
  elicitation skills <cmd>    Install, uninstall or list the agent skills
  elicitation --version       Print the version
  elicitation --help          Print this help
";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // No arguments: the MCP stdio server, exactly as every MCP client runs it.
    // Nothing else may reach stdout in this mode.
    if args.is_empty() {
        return run_server().await;
    }
    match args[0].as_str() {
        "--version" | "-V" if args.len() == 1 => {
            println!("elicitation {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "--help" | "-h" | "help" if args.len() == 1 => {
            print!("{USAGE}\n{}", elicitation::skills::USAGE);
            ExitCode::SUCCESS
        }
        "skills" => ExitCode::from(elicitation::skills::run(&args[1..])),
        _ => {
            eprintln!(
                "elicitation: unexpected arguments `{}`\n\n{USAGE}",
                args.join(" ")
            );
            ExitCode::from(elicitation::skills::EXIT_USAGE)
        }
    }
}

async fn run_server() -> ExitCode {
    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err:?}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> anyhow::Result<()> {
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
