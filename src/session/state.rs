//! Session state — the mutable snapshot of a single interview session.
//!
//! All fields are plain data; the state machine transitions live in
//! [`crate::session::machine`].

use std::collections::VecDeque;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::session::schema::Question;

// ── Status ────────────────────────────────────────────────────────────────────

/// The lifecycle phase of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Normal interview phase — questions are being collected and answered.
    Collecting,
    /// The engine has injected a `SummaryConfirm` question and is waiting for
    /// the user to accept or reject the transcript.
    AwaitingConfirm,
    /// The session has been confirmed and is permanently closed.
    Closed,
}

// ── Token ─────────────────────────────────────────────────────────────────────

/// A single-use token that must accompany each answer submission.
///
/// Once consumed (matched and removed from the state) it cannot be reused;
/// the engine must issue a fresh token for the next answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingleUseToken {
    value: String,
}

impl SingleUseToken {
    /// Generate a fresh token backed by a random UUID-style string.
    pub fn new() -> Self {
        // In a no-std-friendly, no-UUID-dep way we derive entropy from the
        // system clock combined with a simple counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let t = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            value: format!("{t:x}-{n:x}"),
        }
    }

    /// Create a token with a specific value (useful in tests).
    pub fn from_value(v: impl Into<String>) -> Self {
        Self { value: v.into() }
    }

    /// The raw token string the caller must echo back.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Check whether the supplied string matches this token.
    pub fn matches(&self, candidate: &str) -> bool {
        self.value == candidate
    }
}

impl Default for SingleUseToken {
    fn default() -> Self {
        Self::new()
    }
}

// ── Answer ────────────────────────────────────────────────────────────────────

/// An answer recorded in the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    /// The id of the question this answers.
    pub question_id: String,
    /// The free-text or choice value supplied by the user.
    pub value: String,
}

// ── SessionState ──────────────────────────────────────────────────────────────

/// The full mutable state of one interview session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// Current lifecycle phase.
    pub status: SessionStatus,
    /// Pending questions to be answered, in FIFO order.
    pub queue: VecDeque<Question>,
    /// Immutable answer transcript (append-only from the session's perspective).
    pub ledger: Vec<Answer>,
    /// The single-use token the next `append_answer` call must present.
    /// `None` means no answer is expected right now.
    pub token: Option<SingleUseToken>,
    /// How many times the user has rejected the summary and resumed collecting.
    pub revision_count: u32,
    /// Wall-clock timestamp of the last mutation (seconds since UNIX epoch).
    pub last_active_at: u64,
}

impl SessionState {
    /// Create a fresh session in the [`SessionStatus::Collecting`] phase.
    pub fn new() -> Self {
        Self {
            status: SessionStatus::Collecting,
            queue: VecDeque::new(),
            ledger: Vec::new(),
            token: None,
            revision_count: 0,
            last_active_at: now_secs(),
        }
    }

    /// Touch `last_active_at`.
    pub(crate) fn touch(&mut self) {
        self.last_active_at = now_secs();
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
