//! Question schema — type-encoded invariants.
//!
//! - `suggestions` is always empty for [`QuestionKind::OpenText`] and
//!   [`QuestionKind::SummaryConfirm`]; callers cannot smuggle a suggestions
//!   list into those kinds at the type level by construction.
//! - `prompt` is a [`NonEmptyPrompt`] newtype — the compiler prevents an empty
//!   prompt from being constructed.

use serde::{Deserialize, Serialize};

// ── QuestionId newtype ────────────────────────────────────────────────────────

/// A non-empty question identifier.
pub type QuestionId = String;

// ── NonEmptyPrompt ────────────────────────────────────────────────────────────

/// A non-empty prompt string.  Construction fails if the value is blank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NonEmptyPrompt(String);

impl NonEmptyPrompt {
    /// Construct a prompt; returns `None` if `s` is empty or whitespace-only.
    pub fn new(s: impl Into<String>) -> Option<Self> {
        let s = s.into();
        if s.trim().is_empty() {
            None
        } else {
            Some(Self(s))
        }
    }

    /// The underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NonEmptyPrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── QuestionKind ──────────────────────────────────────────────────────────────

/// The kind of a question, determining how the answer is validated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestionKind {
    /// Free-form text answer.
    OpenText,
    /// Exactly one choice from `suggestions` (or free text if `allow_other`).
    SingleChoice,
    /// One or more choices from `suggestions` (or free text if `allow_other`).
    MultiChoice,
    /// A yes/no confirmation of the session transcript — injected by the engine
    /// only via [`crate::session::machine::request_confirm`]; callers may not
    /// enqueue this kind directly.
    SummaryConfirm,
}

// ── Question ──────────────────────────────────────────────────────────────────

/// A question in the interview queue.
///
/// # Invariants
/// - `prompt` is non-empty (enforced by [`NonEmptyPrompt`]).
/// - `suggestions` MUST be non-empty for `SingleChoice` / `MultiChoice`.
/// - `suggestions` MUST be empty for `OpenText` / `SummaryConfirm`.
/// - `SummaryConfirm` questions are reserved for the engine and cannot be
///   enqueued via [`crate::session::machine::append_questions`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// Stable identifier, unique within a session.
    pub id: QuestionId,
    /// The kind of question, governing answer validation.
    pub kind: QuestionKind,
    /// The text shown to the user; must be non-empty.
    pub prompt: NonEmptyPrompt,
    /// The ordered list of choices for `SingleChoice` / `MultiChoice`.
    pub suggestions: Vec<String>,
    /// Whether the user may supply a free-text answer instead of a choice.
    pub allow_other: bool,
}
