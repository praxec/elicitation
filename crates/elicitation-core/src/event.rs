//! The append-only event log (spec §8).
//!
//! One [`Event`] per line of `<state_dir>/<session_id>.jsonl`. `InterviewState`
//! is rebuilt purely by replaying these events in order — provenance is free,
//! appends are crash-resilient, restart-survival is just replay.

use serde::{Deserialize, Serialize};

use crate::model::{Claim, EvidenceRef, EvidenceSourceType, RemediationEvent};
use crate::session::CoverageSchema;

/// An evidence event in the log (spec §3 `EvidenceEvent`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceEvent {
    pub id: String,
    pub session_id: String,
    pub claim_id: String,
    pub source_type: EvidenceSourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub source: EvidenceRef,
}

/// A single line in a session's append-only log. The `type` tag keeps the
/// JSONL self-describing and forward-compatible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Session opened (or re-opened with a coverage schema). The first line of
    /// every log; replaying it sets the coverage schema.
    SessionOpened {
        session_id: String,
        coverage_schema: CoverageSchema,
    },
    /// A new hypothesis was added. `claim` is boxed to keep this variant from
    /// dominating the enum's size (clippy `large_enum_variant`).
    ClaimAdded {
        claim: Box<Claim>,
        /// Optional requirement enrichment carried alongside the claim.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requirement: Option<crate::model::RequirementMeta>,
    },
    /// Evidence accrued against a claim.
    EvidenceAdded { evidence: EvidenceEvent },
    /// A remediation move was applied.
    Remediated { remediation: RemediationEvent },
}

impl Event {
    /// The session this event belongs to.
    pub fn session_id(&self) -> &str {
        match self {
            Event::SessionOpened { session_id, .. } => session_id,
            Event::ClaimAdded { claim, .. } => &claim.session_id,
            Event::EvidenceAdded { evidence } => &evidence.session_id,
            Event::Remediated { remediation } => &remediation.session_id,
        }
    }
}
