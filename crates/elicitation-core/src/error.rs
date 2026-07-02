//! Kernel error type with stable, machine-parseable prefixes (spec §5).
//!
//! The `Display` output of each variant begins with a stable uppercase code
//! (`SESSION_NOT_FOUND`, `INVALID_CLAIM`, `BAD_SESSION_ID`, …) — the
//! cpm-planner convention — so the MCP server can surface them verbatim and
//! clients can pattern-match on the prefix.

use thiserror::Error;

/// All errors the kernel can return. Each `Display` begins with a stable code.
#[derive(Debug, Error)]
pub enum EngineError {
    /// The referenced session has no event log.
    #[error("SESSION_NOT_FOUND: no session '{0}'")]
    SessionNotFound(String),

    /// The session id is empty or contains path-unsafe characters.
    #[error("BAD_SESSION_ID: {0}")]
    BadSessionId(String),

    /// A claim payload is structurally invalid (empty id, predicate, etc.).
    #[error("INVALID_CLAIM: {0}")]
    InvalidClaim(String),

    /// An evidence payload is invalid (e.g. references an unknown claim).
    #[error("INVALID_EVIDENCE: {0}")]
    InvalidEvidence(String),

    /// A remediation payload is invalid (e.g. unknown target).
    #[error("INVALID_REMEDIATION: {0}")]
    InvalidRemediation(String),

    /// A referenced claim does not exist in the session.
    #[error("CLAIM_NOT_FOUND: no claim '{0}'")]
    ClaimNotFound(String),

    /// A referenced contradiction does not currently exist.
    #[error("CONTRADICTION_NOT_FOUND: no contradiction '{0}'")]
    ContradictionNotFound(String),

    /// A duplicate claim/event id was submitted.
    #[error("DUPLICATE_ID: {0}")]
    DuplicateId(String),

    /// Underlying store I/O failure.
    #[error("STORE_ERROR: {0}")]
    StoreError(String),
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, EngineError>;
