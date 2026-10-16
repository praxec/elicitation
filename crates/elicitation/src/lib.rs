#![cfg_attr(not(test), warn(clippy::unwrap_used))]

//! `elicitation` — the stdio MCP server wrapping [`eliciter_core`].
//!
//! The server is thin: [`ElicitationServer`] parses each tool call, invokes one
//! [`Engine`](eliciter_core::Engine) method, and serializes the result. All
//! structure and state management lives in the kernel (`elicitation-core`); this
//! crate only translates the MCP wire protocol to/from kernel calls.

pub mod assist;
pub mod config;
pub mod server;
pub mod skills;

pub use config::{STATE_DIR_ENV, resolve_state_dir};
pub use server::{
    ASSIST_TOOL_NAMES, ElicitationServer, TOOL_APPEND, TOOL_ASSIST_CANONICALIZE,
    TOOL_ASSIST_CLASSIFY_REPLY, TOOL_ASSIST_CONTRADICTIONS, TOOL_ASSIST_PHRASE_QUESTION,
    TOOL_NAMES, TOOL_QUESTION_NEXT, TOOL_READINESS_ASSESS, TOOL_SESSION_OPEN, TOOL_SPEC_EXPORT,
    TOOL_STATE_GET, all_tool_definitions, tool_definitions,
};
