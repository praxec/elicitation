//! Session registry — per-session `tokio::Mutex` locks backed by a
//! [`SessionStore`].
//!
//! # Design
//! - `get_locked(id)` returns a `RegistryGuard` that holds both the
//!   `Arc<tokio::Mutex<SessionState>>` lock and the state value.
//! - There is **no** get-or-create; sessions must already exist in the store.
//! - `commit()` on the guard persists the state back to the store and returns
//!   a `Result` — Drop does NOT silently save (Drop cannot return an error).

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{Mutex as TokioMutex, OwnedMutexGuard};

use crate::session::state::SessionState;
use crate::session::store::{SessionStore, StoreError};

// ── RegistryError ─────────────────────────────────────────────────────────────

/// Errors returned by [`SessionRegistry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The requested session was not found in the store.
    NotFound(String),
    /// A storage error occurred.
    Store(StoreError),
}

impl From<StoreError> for RegistryError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::SessionNotFound(id) => RegistryError::NotFound(id),
            other => RegistryError::Store(other),
        }
    }
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "REGISTRY_NOT_FOUND: '{id}'"),
            Self::Store(e) => write!(f, "REGISTRY_STORE: {e}"),
        }
    }
}

impl std::error::Error for RegistryError {}

// ── SessionRegistry ───────────────────────────────────────────────────────────

/// A per-session locking registry backed by any [`SessionStore`].
///
/// Each session gets exactly one `Arc<TokioMutex<SessionState>>` entry; all
/// concurrent waiters share it, ensuring serialised access per session while
/// allowing parallel access across different sessions.
pub struct SessionRegistry<S: SessionStore> {
    store: Arc<S>,
    /// session_id → Arc<TokioMutex<SessionState>>
    locks: StdMutex<HashMap<String, Arc<TokioMutex<SessionState>>>>,
}

impl<S: SessionStore> SessionRegistry<S> {
    /// Create a new registry wrapping `store`.
    pub fn new(store: S) -> Self {
        Self {
            store: Arc::new(store),
            locks: StdMutex::new(HashMap::new()),
        }
    }

    /// Acquire the per-session lock for `session_id`.
    ///
    /// On first call for a given `id` the session is loaded from the store and
    /// its mutex is created.  Subsequent calls reuse the same mutex.
    ///
    /// Returns [`RegistryError::NotFound`] if the session does not exist in
    /// the store.
    pub async fn get_locked(&self, session_id: &str) -> Result<SessionGuard<S>, RegistryError> {
        // Get or create the Arc<TokioMutex<SessionState>>.
        let arc = {
            let mut map = self.locks.lock().map_err(|e| {
                RegistryError::Store(StoreError::Internal(format!(
                    "registry mutex poisoned: {e}"
                )))
            })?;

            if let Some(existing) = map.get(session_id) {
                Arc::clone(existing)
            } else {
                // Load from store — do NOT create; return NotFound if missing.
                let state = self.store.load(session_id)?;
                let arc = Arc::new(TokioMutex::new(state));
                map.insert(session_id.to_string(), Arc::clone(&arc));
                arc
            }
        };

        let guard = arc.lock_owned().await;

        Ok(SessionGuard {
            session_id: session_id.to_string(),
            store: Arc::clone(&self.store),
            guard,
        })
    }
}

// ── SessionGuard ─────────────────────────────────────────────────────────────

/// A held lock on a session's state, plus a reference to the store for
/// explicit commits.
#[derive(Debug)]
pub struct SessionGuard<S: SessionStore> {
    session_id: String,
    store: Arc<S>,
    guard: OwnedMutexGuard<SessionState>,
}

impl<S: SessionStore> SessionGuard<S> {
    /// Read the currently-held session state.
    pub fn state(&self) -> &SessionState {
        &*self.guard
    }

    /// Mutably access the session state.
    pub fn state_mut(&mut self) -> &mut SessionState {
        &mut *self.guard
    }

    /// Persist the current state back to the store.
    ///
    /// This is the **only** way to durably write changes; dropping the guard
    /// does NOT automatically save (Drop cannot return a Result).
    pub fn commit(&self) -> Result<(), StoreError> {
        self.store.save(&self.session_id, self.guard.clone())
    }
}
