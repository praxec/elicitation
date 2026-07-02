#![cfg_attr(not(test), warn(clippy::unwrap_used))]

//! `elicitation` — the stdio MCP server wrapping [`elicitation_core`].
//!
//! The server is thin: [`ElicitationServer`] parses each tool call, invokes one
//! [`Engine`](elicitation_core::Engine) method, and serializes the result. All
//! structure and state management lives in the kernel (`elicitation-core`); this
//! crate only translates the MCP wire protocol to/from kernel calls.

pub mod config;
pub mod server;

pub use config::{resolve_state_dir, STATE_DIR_ENV};
pub use server::{
    tool_definitions, ElicitationServer, TOOL_APPEND, TOOL_NAMES, TOOL_QUESTION_NEXT,
    TOOL_READINESS_ASSESS, TOOL_SESSION_OPEN, TOOL_SPEC_EXPORT, TOOL_STATE_GET,
};
