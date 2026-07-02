#![cfg_attr(not(test), warn(clippy::unwrap_used))]

//! `elicitation-session` — the deterministic session core for structured
//! discovery interviews.
//!
//! This crate is **pure logic**: no I/O, no async, no networking. It provides:
//!
//! - [`session::state`]   — [`SessionState`](session::state::SessionState) +
//!   [`SessionStatus`](session::state::SessionStatus) + single-use token.
//! - [`session::schema`]  — [`Question`](session::schema::Question) with
//!   type-encoded invariants.
//! - [`session::machine`] — deterministic state-machine transitions with
//!   poka-yoke guards (batch-open-text, SummaryConfirm-reserved, token
//!   single-use, revision-cap, queue-depth-cap, close-guard).
//! - [`session::store`]   — [`SessionStore`](session::store::SessionStore)
//!   trait + [`MemorySessionStore`](session::store::MemorySessionStore).

pub mod session;
