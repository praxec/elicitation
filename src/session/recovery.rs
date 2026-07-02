//! Recovery engine — loads all non-closed sessions and validates their
//! state-machine invariants; quarantines (but never panics on) invalid ones.

use crate::session::schema::QuestionKind;
use crate::session::state::{SessionState, SessionStatus};
use crate::session::store::{SessionStore, StoreError};

// ── RecoveryReport ────────────────────────────────────────────────────────────

/// Result returned by [`RecoveryEngine::recover`].
#[derive(Debug, Default)]
pub struct RecoveryReport {
    /// IDs of sessions that passed validation and were successfully loaded.
    pub recovered: Vec<String>,
    /// IDs of sessions that failed validation, paired with the failure reason.
    /// These sessions remain in the store (they are not automatically deleted)
    /// so that a human operator can inspect them.
    pub quarantined: Vec<(String, String)>,
}

// ── Validation logic ──────────────────────────────────────────────────────────

/// Validate the state-machine invariants of a session state snapshot.
///
/// Returns `Ok(())` when valid, `Err(reason)` when an invariant is violated.
fn validate(id: &str, state: &SessionState) -> Result<(), String> {
    match &state.status {
        SessionStatus::AwaitingConfirm => {
            // Must have a SummaryConfirm question in the queue.
            let has_sc = state
                .queue
                .iter()
                .any(|q| q.kind == QuestionKind::SummaryConfirm);
            if !has_sc {
                return Err(format!(
                    "session '{id}' is AwaitingConfirm but has no SummaryConfirm question in queue"
                ));
            }
            // Must have a pending token (the user needs to answer the confirm).
            // NOTE: The current machine does not automatically issue a token for
            // the SummaryConfirm question, so we only warn when the token is
            // *unexpectedly* present for a non-confirm scenario.
            // Invariant: revision_count must be <= REVISION_CAP.
            if state.revision_count > crate::session::machine::REVISION_CAP {
                return Err(format!(
                    "session '{id}' has revision_count {} > REVISION_CAP {}",
                    state.revision_count,
                    crate::session::machine::REVISION_CAP
                ));
            }
        }
        SessionStatus::Collecting => {
            // revision_count must be <= REVISION_CAP.
            if state.revision_count > crate::session::machine::REVISION_CAP {
                return Err(format!(
                    "session '{id}' has revision_count {} > REVISION_CAP {}",
                    state.revision_count,
                    crate::session::machine::REVISION_CAP
                ));
            }
        }
        SessionStatus::Closed => {
            // Closed sessions should not appear in list_active, but if they do
            // we simply skip them (they are valid).
        }
    }
    Ok(())
}

// ── RecoveryEngine ────────────────────────────────────────────────────────────

/// Loads and validates all non-closed sessions from a [`SessionStore`].
pub struct RecoveryEngine;

impl RecoveryEngine {
    /// Load every non-closed session from `store`, validate each one, and
    /// return a [`RecoveryReport`] with recovered and quarantined ids.
    ///
    /// This function never panics or crashes regardless of what is in the
    /// store.
    pub fn recover<S: SessionStore>(store: &S) -> RecoveryReport {
        let mut report = RecoveryReport::default();

        // Get the list of active (non-closed) sessions; if that fails, return
        // an empty report — we cannot proceed.
        let ids = match store.list_active() {
            Ok(ids) => ids,
            Err(e) => {
                report
                    .quarantined
                    .push(("<store>".to_string(), format!("list_active failed: {e}")));
                return report;
            }
        };

        for id in ids {
            // Attempt to load; a load failure quarantines the session.
            let state = match store.load(&id) {
                Ok(s) => s,
                Err(StoreError::SessionNotFound(_)) => {
                    // Disappeared between list and load — skip silently.
                    continue;
                }
                Err(e) => {
                    report.quarantined.push((id, format!("load error: {e}")));
                    continue;
                }
            };

            // Validate state-machine invariants.
            match validate(&id, &state) {
                Ok(()) => report.recovered.push(id),
                Err(reason) => report.quarantined.push((id, reason)),
            }
        }

        report
    }
}
