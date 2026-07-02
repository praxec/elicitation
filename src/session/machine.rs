//! State-machine transitions (poka-yoke guards).
//!
//! All functions are **pure** — they take `&mut SessionState` and return
//! `Result<_, SmError>`.  No I/O, no async.

use crate::session::schema::{NonEmptyPrompt, Question, QuestionKind};
use crate::session::state::{Answer, SessionState, SessionStatus};

/// Maximum number of No-rejections before the session is locked.
pub const REVISION_CAP: u32 = 10;

/// Maximum total questions that may sit in the queue at once.
pub const QUEUE_DEPTH_CAP: usize = 50;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors the session state machine can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmError {
    /// A batch was submitted without at least one [`QuestionKind::OpenText`]
    /// question.
    BatchMissingOpenText,

    /// A caller tried to enqueue a [`QuestionKind::SummaryConfirm`] question,
    /// which is reserved for the engine.
    SummaryConfirmReserved,

    /// Adding the batch would push the queue past [`QUEUE_DEPTH_CAP`].
    QueueDepthExceeded,

    /// The supplied token string does not match the session's current token.
    TokenMismatch,

    /// The session has no pending token (it was already consumed, or none was
    /// issued).
    TokenAlreadyUsed,

    /// The requested transition is not valid from the current status.
    InvalidTransition(String),

    /// The session has reached the maximum number of revision cycles.
    RevisionCapExceeded,
}

impl std::fmt::Display for SmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BatchMissingOpenText => {
                write!(
                    f,
                    "BATCH_MISSING_OPEN_TEXT: batch must contain >=1 OpenText question"
                )
            }
            Self::SummaryConfirmReserved => {
                write!(f, "SUMMARY_CONFIRM_RESERVED: SummaryConfirm questions are reserved for the engine")
            }
            Self::QueueDepthExceeded => {
                write!(
                    f,
                    "QUEUE_DEPTH_EXCEEDED: queue would exceed cap of {QUEUE_DEPTH_CAP}"
                )
            }
            Self::TokenMismatch => {
                write!(f, "TOKEN_MISMATCH: the supplied token does not match")
            }
            Self::TokenAlreadyUsed => {
                write!(
                    f,
                    "TOKEN_ALREADY_USED: no token is pending (already consumed or not issued)"
                )
            }
            Self::InvalidTransition(msg) => {
                write!(f, "INVALID_TRANSITION: {msg}")
            }
            Self::RevisionCapExceeded => {
                write!(
                    f,
                    "REVISION_CAP_EXCEEDED: max {REVISION_CAP} revision cycles reached"
                )
            }
        }
    }
}

impl std::error::Error for SmError {}

// ── Transitions ───────────────────────────────────────────────────────────────

/// Enqueue a batch of questions.
///
/// # Poka-yokes
/// 1. Batch must contain `>=1` [`QuestionKind::OpenText`] question →
///    [`SmError::BatchMissingOpenText`].
/// 2. Batch must not contain any [`QuestionKind::SummaryConfirm`] question →
///    [`SmError::SummaryConfirmReserved`].
/// 3. Resulting queue length must not exceed [`QUEUE_DEPTH_CAP`] →
///    [`SmError::QueueDepthExceeded`].
pub fn append_questions(state: &mut SessionState, questions: Vec<Question>) -> Result<(), SmError> {
    // Guard 2: no SummaryConfirm
    if questions
        .iter()
        .any(|q| q.kind == QuestionKind::SummaryConfirm)
    {
        return Err(SmError::SummaryConfirmReserved);
    }

    // Guard 1: at least one OpenText
    if !questions.iter().any(|q| q.kind == QuestionKind::OpenText) {
        return Err(SmError::BatchMissingOpenText);
    }

    // Guard 3: queue depth cap
    if state.queue.len() + questions.len() > QUEUE_DEPTH_CAP {
        return Err(SmError::QueueDepthExceeded);
    }

    for q in questions {
        state.queue.push_back(q);
    }
    state.touch();
    Ok(())
}

/// Record an answer against the pending token.
///
/// # Poka-yoke
/// The caller must present the exact single-use token stored in `state.token`.
/// On success the token is consumed (set to `None`).  On failure the token is
/// left intact so the caller can retry with the correct value.
pub fn append_answer(state: &mut SessionState, answer: Answer, token: &str) -> Result<(), SmError> {
    match &state.token {
        None => return Err(SmError::TokenAlreadyUsed),
        Some(t) if !t.matches(token) => return Err(SmError::TokenMismatch),
        Some(_) => {}
    }
    // Consume token.
    state.token = None;
    state.ledger.push(answer);
    state.touch();
    Ok(())
}

/// Transition `Collecting → AwaitingConfirm` by injecting a `SummaryConfirm`
/// question that carries a verbatim replay of the ledger as its prompt.
pub fn request_confirm(state: &mut SessionState) -> Result<(), SmError> {
    if state.status != SessionStatus::Collecting {
        return Err(SmError::InvalidTransition(format!(
            "request_confirm requires Collecting status, got {:?}",
            state.status
        )));
    }
    // Build the transcript replay for the SummaryConfirm prompt.
    let replay = build_transcript(state);
    let prompt_text = format!("Please confirm the following transcript:\n\n{replay}");
    let prompt = NonEmptyPrompt::new(prompt_text)
        .unwrap_or_else(|| NonEmptyPrompt::new("Please confirm the transcript.").unwrap());

    let confirm_q = Question {
        id: format!("_summary_confirm_{}", state.revision_count),
        kind: QuestionKind::SummaryConfirm,
        prompt,
        suggestions: vec![],
        allow_other: false,
    };
    state.queue.push_back(confirm_q);
    state.status = SessionStatus::AwaitingConfirm;
    state.touch();
    Ok(())
}

/// Resolve the confirmation decision.
///
/// - `decision = true` (Yes): `AwaitingConfirm → Closed`.
/// - `decision = false` (No): `AwaitingConfirm → Collecting` with
///   `revision_count++`; returns [`SmError::RevisionCapExceeded`] when the
///   counter would exceed [`REVISION_CAP`].
/// - Any other origin status: [`SmError::InvalidTransition`] (the **close
///   guard**).
pub fn confirm(state: &mut SessionState, decision: bool) -> Result<(), SmError> {
    if state.status != SessionStatus::AwaitingConfirm {
        return Err(SmError::InvalidTransition(format!(
            "confirm requires AwaitingConfirm status, got {:?}",
            state.status
        )));
    }

    if decision {
        state.status = SessionStatus::Closed;
    } else {
        // Check cap *before* incrementing.
        if state.revision_count >= REVISION_CAP {
            return Err(SmError::RevisionCapExceeded);
        }
        state.revision_count += 1;
        state.status = SessionStatus::Collecting;
        // Remove the SummaryConfirm question that was injected by request_confirm.
        state
            .queue
            .retain(|q| q.kind != QuestionKind::SummaryConfirm);
    }
    state.touch();
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_transcript(state: &SessionState) -> String {
    if state.ledger.is_empty() {
        return "(no answers yet)".to_string();
    }
    state
        .ledger
        .iter()
        .map(|a| format!("Q:{} A:{}", a.question_id, a.value))
        .collect::<Vec<_>>()
        .join("\n")
}
