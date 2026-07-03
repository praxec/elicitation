// Restriction-category lint on production code only. `cargo test` compiles with
// `cfg(test)`, which silences this everywhere — tests may `unwrap`; production
// code propagates errors. Mirrors the cpm-planner convention.
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

//! `elicitation-core` — the deterministic, offline kernel for structured
//! discovery interviews, modeled as an **alignment mechanism run by the
//! scientific method** (see `docs/spec.md`).
//!
//! Claims are **hypotheses**; standing is **computed** (a pure fold over an
//! append-only event log), never stored. Detectors are exact-match pure
//! functions over caller-supplied canonical identity — no fuzzy matching, no
//! normalization, no LLM, no network. The kernel is a pure function of
//! `(events in) → (computed standing + contradictions + readiness +
//! next-question out)`.
//!
//! # Layout
//!
//! - [`model`]      — the typed data model (claim, evidence, contradiction,
//!   remediation, requirement, entity).
//! - [`session`]    — the session-configurable coverage schema.
//! - [`event`]      — the append-only event log line type.
//! - [`store`]      — the [`StateStore`](store::StateStore) trait + filesystem
//!   (JSONL) impl.
//! - [`projection`] — the `InterviewState` fold + computed standing.
//! - [`detect`]     — intra-spec / spec-vs-reality contradiction + conformance
//!   detection.
//! - [`signal`]     — the three first-class signals + `question.next`.
//! - [`readiness`]  — the plan-readiness gate.
//! - [`export`]     — `spec.export`.
//! - [`engine`]     — the [`Engine`](engine::Engine) tying it together.
//! - [`research`]   — the future `ResearchProvider` seam (`None` in v1).

pub mod detect;
pub mod engine;
pub mod error;
pub mod event;
pub mod export;
pub mod model;
pub mod projection;
pub mod readiness;
pub mod research;
pub mod session;
pub mod signal;
pub mod store;

pub use engine::Engine;
pub use error::{EngineError, Result};
pub use event::{Event, EvidenceEvent};
pub use export::SpecDocument;
pub use model::{
    Category, Claim, Contradiction, ContradictionKind, EntityRef, EvidenceRef, EvidenceSourceType,
    HypothesisStanding, Modality, Object, Polarity, Priority, RemediationEvent, RemediationMove,
    RequirementCandidate, RequirementMeta, Severity, TimeScope,
};
pub use projection::{AcceptStance, ClaimWithStanding, InterviewState, Registry};
pub use readiness::ReadinessReport;
pub use research::{NoResearch, ResearchProvider};
pub use session::{CoverageDimension, CoverageSchema};
pub use signal::{Signal, SignalKind, next_question};
pub use store::{FilesystemStore, StateStore};
