//! The kernel engine + session manager (spec §5 / §8).
//!
//! [`Engine`] is the single deterministic entry point: it validates each
//! command payload, appends one event to the [`StateStore`], and recomputes the
//! [`InterviewState`] projection by replay. Every read also projects. There is
//! no in-memory mutable standing — replay is the source of truth, so restart-
//! survival is free (spec §8).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{EngineError, Result};
use crate::event::{Event, EvidenceEvent};
use crate::model::{Claim, RemediationEvent, RemediationMove, RequirementMeta};
use crate::projection::{self, InterviewState};
use crate::session::CoverageSchema;
use crate::signal::{next_question, Signal};
use crate::store::{validate_session_id, StateStore};

/// Per-session lock registry: a `session_id -> lock` map behind a `Mutex`.
///
/// The outer `Mutex` guards only the (brief) map lookup/insert; the per-session
/// `Arc<Mutex<()>>` it hands back is what actually serializes a session's
/// read-modify-write critical section. Holding the per-session lock — not the
/// map lock — across the replay/check/append keeps DIFFERENT sessions fully
/// concurrent (spec §4/§8 isolation) while making SAME-session writes mutually
/// exclusive, so the dup/existence checks are race-free.
type LockMap = Mutex<HashMap<String, Arc<Mutex<()>>>>;

/// The kernel engine. Cheap to clone (the store and lock map are `Arc`s; clones
/// share the same per-session locks).
#[derive(Clone)]
pub struct Engine {
    store: Arc<dyn StateStore>,
    /// Shared across clones so concurrent operations on the same `session_id`
    /// — even via different `Engine` clones — contend on one lock.
    session_locks: Arc<LockMap>,
}

impl Engine {
    /// Build an engine over the given store.
    pub fn new(store: Arc<dyn StateStore>) -> Self {
        Self {
            store,
            session_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Fetch (or create) the per-session lock. The outer map lock is held only
    /// for the lookup/insert, never across the critical section.
    fn session_lock(&self, session_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.session_locks.lock().unwrap_or_else(|p| p.into_inner());
        Arc::clone(
            map.entry(session_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    /// `session.open` (spec §5): start or resume a session. If new, write the
    /// `SessionOpened` event with the supplied (or default) coverage schema. If
    /// it already exists, the schema arg is ignored (the original governs) and
    /// the current state is returned.
    pub fn open_session(
        &self,
        session_id: &str,
        coverage_schema: Option<CoverageSchema>,
    ) -> Result<InterviewState> {
        validate_session_id(session_id)?;
        let lock = self.session_lock(session_id);
        let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        if !self.store.exists(session_id)? {
            let event = Event::SessionOpened {
                session_id: session_id.to_string(),
                coverage_schema: coverage_schema.unwrap_or_default(),
            };
            self.store.append(session_id, &event)?;
        }
        self.state(session_id)
    }

    /// `state.get` (spec §5): read-only projection of the session.
    pub fn state(&self, session_id: &str) -> Result<InterviewState> {
        validate_session_id(session_id)?;
        let events = self.store.replay(session_id)?;
        Ok(projection::project(&events))
    }

    /// `append` variant `add_claim` (spec §5). Validates the claim, rejects
    /// duplicate ids, appends, and returns the recomputed state.
    pub fn add_claim(
        &self,
        session_id: &str,
        claim: Claim,
        requirement: Option<RequirementMeta>,
    ) -> Result<InterviewState> {
        validate_session_id(session_id)?;
        let lock = self.session_lock(session_id);
        let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        let events = self.require_session(session_id)?;
        validate_claim(&claim, session_id)?;
        if events
            .iter()
            .any(|e| matches!(e, Event::ClaimAdded { claim: c, .. } if c.id == claim.id))
        {
            return Err(EngineError::DuplicateId(claim.id));
        }
        self.store.append(
            session_id,
            &Event::ClaimAdded {
                claim: Box::new(claim),
                requirement,
            },
        )?;
        self.state(session_id)
    }

    /// `append` variant `add_evidence` (spec §5).
    pub fn add_evidence(
        &self,
        session_id: &str,
        evidence: EvidenceEvent,
    ) -> Result<InterviewState> {
        validate_session_id(session_id)?;
        let lock = self.session_lock(session_id);
        let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        let events = self.require_session(session_id)?;
        if evidence.id.is_empty() {
            return Err(EngineError::InvalidEvidence("empty evidence id".into()));
        }
        if evidence.session_id != session_id {
            return Err(EngineError::InvalidEvidence(format!(
                "session_id mismatch: payload '{}' vs '{session_id}'",
                evidence.session_id
            )));
        }
        if !claim_exists(&events, &evidence.claim_id) {
            return Err(EngineError::ClaimNotFound(evidence.claim_id));
        }
        if duplicate_event_id(&events, &evidence.id) {
            return Err(EngineError::DuplicateId(evidence.id));
        }
        self.store
            .append(session_id, &Event::EvidenceAdded { evidence })?;
        self.state(session_id)
    }

    /// `append` variant `remediate{...}` (spec §5). Validates that every target
    /// referenced by the move exists, then appends.
    pub fn remediate(
        &self,
        session_id: &str,
        remediation: RemediationEvent,
    ) -> Result<InterviewState> {
        validate_session_id(session_id)?;
        let lock = self.session_lock(session_id);
        let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        let events = self.require_session(session_id)?;
        if remediation.id.is_empty() {
            return Err(EngineError::InvalidRemediation(
                "empty remediation id".into(),
            ));
        }
        if remediation.session_id != session_id {
            return Err(EngineError::InvalidRemediation(format!(
                "session_id mismatch: payload '{}' vs '{session_id}'",
                remediation.session_id
            )));
        }
        if duplicate_event_id(&events, &remediation.id) {
            return Err(EngineError::DuplicateId(remediation.id));
        }
        validate_remediation(&events, &remediation)?;
        self.store
            .append(session_id, &Event::Remediated { remediation })?;
        self.state(session_id)
    }

    /// `question.next` (spec §5): the highest-leverage next question, or `None`.
    pub fn next_question(&self, session_id: &str) -> Result<Option<Signal>> {
        let state = self.state(session_id)?;
        Ok(next_question(&state.signals).cloned())
    }

    /// `readiness.assess` (spec §5 / §6): the readiness report.
    pub fn readiness(&self, session_id: &str) -> Result<crate::readiness::ReadinessReport> {
        Ok(self.state(session_id)?.readiness)
    }

    /// `spec.export` (spec §6): the aligned, plan-ready spec document.
    pub fn export(&self, session_id: &str) -> Result<crate::export::SpecDocument> {
        let state = self.state(session_id)?;
        Ok(crate::export::build(&state))
    }

    fn require_session(&self, session_id: &str) -> Result<Vec<Event>> {
        if !self.store.exists(session_id)? {
            return Err(EngineError::SessionNotFound(session_id.to_string()));
        }
        self.store.replay(session_id)
    }
}

// --- validation helpers ----------------------------------------------------

fn validate_claim(claim: &Claim, session_id: &str) -> Result<()> {
    if claim.id.is_empty() {
        return Err(EngineError::InvalidClaim("empty claim id".into()));
    }
    if claim.session_id != session_id {
        return Err(EngineError::InvalidClaim(format!(
            "session_id mismatch: payload '{}' vs '{session_id}'",
            claim.session_id
        )));
    }
    if claim.subject.id.is_empty() {
        return Err(EngineError::InvalidClaim("empty subject id".into()));
    }
    if claim.predicate.is_empty() {
        return Err(EngineError::InvalidClaim("empty predicate token".into()));
    }
    Ok(())
}

fn claim_exists(events: &[Event], claim_id: &str) -> bool {
    events
        .iter()
        .any(|e| matches!(e, Event::ClaimAdded { claim, .. } if claim.id == claim_id))
}

fn duplicate_event_id(events: &[Event], id: &str) -> bool {
    events.iter().any(|e| match e {
        Event::ClaimAdded { claim, .. } => claim.id == id,
        Event::EvidenceAdded { evidence } => evidence.id == id,
        Event::Remediated { remediation } => remediation.id == id,
        Event::SessionOpened { .. } => false,
    })
}

fn validate_remediation(events: &[Event], remediation: &RemediationEvent) -> Result<()> {
    let claim_ok = |id: &str| claim_exists(events, id);
    match &remediation.r#move {
        RemediationMove::Supersede { old, new } => {
            if old == new {
                return Err(EngineError::InvalidRemediation(format!(
                    "supersede old and new must differ (both '{old}')"
                )));
            }
            if !claim_ok(old) {
                return Err(EngineError::ClaimNotFound(old.clone()));
            }
            if !claim_ok(new) {
                return Err(EngineError::ClaimNotFound(new.clone()));
            }
        }
        RemediationMove::Retract { claim }
        | RemediationMove::Qualify { claim, .. }
        | RemediationMove::Accept { target: claim, .. } => {
            if !claim_ok(claim) {
                return Err(EngineError::ClaimNotFound(claim.clone()));
            }
        }
        RemediationMove::ReconcileByEvidence { contradiction } => {
            // Validate the contradiction currently exists in the projection.
            let state = projection::project(events);
            if !state.contradictions.iter().any(|c| &c.id == contradiction) {
                return Err(EngineError::ContradictionNotFound(contradiction.clone()));
            }
        }
    }
    Ok(())
}
