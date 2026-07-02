//! Session store trait + in-memory implementation for tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::session::state::{SessionState, SessionStatus};

// ── StoreError ────────────────────────────────────────────────────────────────

/// Errors the session store can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The requested session was not found.
    SessionNotFound(String),

    /// `delete` was called on a session that has not yet been closed.
    ///
    /// Poka-yoke: only `Closed` sessions may be deleted.
    SessionNotClosed,

    /// An internal storage error.
    Internal(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionNotFound(id) => write!(f, "SESSION_NOT_FOUND: '{id}'"),
            Self::SessionNotClosed => {
                write!(f, "SESSION_NOT_CLOSED: only Closed sessions may be deleted")
            }
            Self::Internal(msg) => write!(f, "STORE_INTERNAL: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

// ── SessionStore trait ────────────────────────────────────────────────────────

/// Persistence contract for session state snapshots.
pub trait SessionStore: Send + Sync {
    /// Load the session state for `session_id`.
    fn load(&self, session_id: &str) -> Result<SessionState, StoreError>;

    /// Persist (create or overwrite) the session state.
    fn save(&self, session_id: &str, state: SessionState) -> Result<(), StoreError>;

    /// Delete the session.  Returns [`StoreError::SessionNotClosed`] unless
    /// the session's status is [`SessionStatus::Closed`] — the poka-yoke that
    /// prevents accidental deletion of live sessions.
    fn delete(&self, session_id: &str) -> Result<(), StoreError>;

    /// Return the ids of all sessions whose status is not `Closed`.
    fn list_active(&self) -> Result<Vec<String>, StoreError>;
}

// ── MemorySessionStore ────────────────────────────────────────────────────────

/// An in-memory [`SessionStore`] backed by a `HashMap` behind an `Arc<Mutex>`.
///
/// Suitable for tests and single-process use; state is lost on drop.
#[derive(Debug, Clone, Default)]
pub struct MemorySessionStore {
    inner: Arc<Mutex<HashMap<String, SessionState>>>,
}

impl MemorySessionStore {
    /// Create a new, empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, SessionState>>, StoreError> {
        self.inner
            .lock()
            .map_err(|e| StoreError::Internal(format!("mutex poisoned: {e}")))
    }
}

impl SessionStore for MemorySessionStore {
    fn load(&self, session_id: &str) -> Result<SessionState, StoreError> {
        let map = self.lock()?;
        map.get(session_id)
            .cloned()
            .ok_or_else(|| StoreError::SessionNotFound(session_id.to_string()))
    }

    fn save(&self, session_id: &str, state: SessionState) -> Result<(), StoreError> {
        let mut map = self.lock()?;
        map.insert(session_id.to_string(), state);
        Ok(())
    }

    fn delete(&self, session_id: &str) -> Result<(), StoreError> {
        let mut map = self.lock()?;
        match map.get(session_id) {
            None => return Err(StoreError::SessionNotFound(session_id.to_string())),
            Some(s) if s.status != SessionStatus::Closed => {
                return Err(StoreError::SessionNotClosed);
            }
            Some(_) => {}
        }
        map.remove(session_id);
        Ok(())
    }

    fn list_active(&self) -> Result<Vec<String>, StoreError> {
        let map = self.lock()?;
        Ok(map
            .iter()
            .filter(|(_, s)| s.status != SessionStatus::Closed)
            .map(|(id, _)| id.clone())
            .collect())
    }
}
