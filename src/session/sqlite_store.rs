//! SQLite-backed [`SessionStore`] implementation.
//!
//! One row per session:
//!   session_id TEXT PRIMARY KEY,
//!   state_json TEXT NOT NULL,    -- serde-JSON of SessionState
//!   status     TEXT NOT NULL,    -- "Collecting" | "AwaitingConfirm" | "Closed"
//!   updated_at INTEGER NOT NULL  -- Unix-epoch seconds
//!
//! `save` uses a `BEGIN IMMEDIATE` transaction for atomic writes (single-writer).
//! `open()` runs `PRAGMA integrity_check`; on failure the database file is
//! copied to `<path>.corrupted` and a fresh database is started.
//! `delete` returns [`StoreError::SessionNotClosed`] unless the session is
//! `Closed`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, TransactionBehavior};
use serde_json;

use crate::session::state::{SessionState, SessionStatus};
use crate::session::store::{SessionStore, StoreError};

// ── internal helpers ──────────────────────────────────────────────────────────

fn status_str(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Collecting => "Collecting",
        SessionStatus::AwaitingConfirm => "AwaitingConfirm",
        SessionStatus::Closed => "Closed",
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn map_rusqlite(e: rusqlite::Error) -> StoreError {
    StoreError::Internal(e.to_string())
}

fn map_json_err(e: serde_json::Error) -> StoreError {
    StoreError::Internal(format!("json error: {e}"))
}

// ── SqliteSessionStore ────────────────────────────────────────────────────────

/// A file-backed (or in-memory, for tests) [`SessionStore`] using SQLite.
#[derive(Debug, Clone)]
pub struct SqliteSessionStore {
    /// Shared connection protected by a mutex (rusqlite connections are not
    /// `Sync`).
    conn: Arc<Mutex<Connection>>,
    /// The path on disk (used for the corruption-recovery copy).
    db_path: Option<PathBuf>,
}

impl SqliteSessionStore {
    /// Open (or create) a SQLite database at `path`.
    ///
    /// On failure of `PRAGMA integrity_check` the database is copied to
    /// `<path>.corrupted` and a fresh database is created.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path_buf = path.as_ref().to_path_buf();
        let conn = Connection::open(&path_buf).map_err(map_rusqlite)?;
        let store = Self::init(conn, Some(path_buf.clone()))?;

        // Integrity check.
        let ok = store.integrity_check()?;
        if !ok {
            // Copy current (corrupt) file.
            let corrupted = path_buf.with_extension("corrupted");
            let _ = std::fs::copy(&path_buf, &corrupted);
            // Remove corrupt file and start fresh.
            drop(store);
            let _ = std::fs::remove_file(&path_buf);
            let fresh_conn = Connection::open(&path_buf).map_err(map_rusqlite)?;
            return Self::init(fresh_conn, Some(path_buf));
        }
        Ok(store)
    }

    /// Open an in-memory database (useful for contract tests).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(map_rusqlite)?;
        Self::init(conn, None)
    }

    fn init(conn: Connection, db_path: Option<PathBuf>) -> Result<Self, StoreError> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS sessions (
                 session_id TEXT PRIMARY KEY,
                 state_json TEXT NOT NULL,
                 status     TEXT NOT NULL,
                 updated_at INTEGER NOT NULL
             );",
        )
        .map_err(map_rusqlite)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    /// Run `PRAGMA integrity_check` and return `true` iff "ok".
    fn integrity_check(&self) -> Result<bool, StoreError> {
        let guard = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(format!("mutex poisoned: {e}")))?;
        let result: String = guard
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(map_rusqlite)?;
        Ok(result.trim() == "ok")
    }

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.conn
            .lock()
            .map_err(|e| StoreError::Internal(format!("mutex poisoned: {e}")))
    }

    /// The path to the database file, if any (used for testing).
    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }
}

impl SessionStore for SqliteSessionStore {
    fn load(&self, session_id: &str) -> Result<SessionState, StoreError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn
            .prepare("SELECT state_json FROM sessions WHERE session_id = ?1")
            .map_err(map_rusqlite)?;
        let json: String = stmt
            .query_row(params![session_id], |row| row.get(0))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StoreError::SessionNotFound(session_id.to_string())
                }
                other => map_rusqlite(other),
            })?;
        serde_json::from_str(&json).map_err(map_json_err)
    }

    fn save(&self, session_id: &str, state: SessionState) -> Result<(), StoreError> {
        let json = serde_json::to_string(&state).map_err(map_json_err)?;
        let status = status_str(&state.status);
        let now = now_secs();
        let mut conn = self.lock_conn()?;
        // Use an IMMEDIATE transaction so concurrent readers see a consistent
        // snapshot and no interleaving is possible.
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_rusqlite)?;
        tx.execute(
            "INSERT INTO sessions (session_id, state_json, status, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE SET
                 state_json = excluded.state_json,
                 status     = excluded.status,
                 updated_at = excluded.updated_at",
            params![session_id, json, status, now],
        )
        .map_err(map_rusqlite)?;
        tx.commit().map_err(map_rusqlite)
    }

    fn delete(&self, session_id: &str) -> Result<(), StoreError> {
        let conn = self.lock_conn()?;
        // Check existence and status in its own scope so the Statement borrow
        // is dropped before we call execute below.
        let status: String = {
            let mut stmt = conn
                .prepare("SELECT status FROM sessions WHERE session_id = ?1")
                .map_err(map_rusqlite)?;
            stmt.query_row(params![session_id], |row| row.get(0))
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        StoreError::SessionNotFound(session_id.to_string())
                    }
                    other => map_rusqlite(other),
                })?
        };
        if status != "Closed" {
            return Err(StoreError::SessionNotClosed);
        }
        conn.execute(
            "DELETE FROM sessions WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(map_rusqlite)?;
        Ok(())
    }

    fn list_active(&self) -> Result<Vec<String>, StoreError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn
            .prepare("SELECT session_id FROM sessions WHERE status != 'Closed'")
            .map_err(map_rusqlite)?;
        let ids: Result<Vec<String>, _> = stmt
            .query_map([], |row| row.get(0))
            .map_err(map_rusqlite)?
            .collect::<Result<Vec<_>, _>>();
        ids.map_err(map_rusqlite)
    }
}
