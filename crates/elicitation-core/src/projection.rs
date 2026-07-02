//! The `InterviewState` fold (spec §3 / §8): replay the append-only log into a
//! live snapshot with **computed** hypothesis standing.
//!
//! Standing is never stored — it is a pure function of the events. The fold
//! happens in two passes:
//!  1. Collect raw claims, evidence, remediations, and the coverage schema.
//!  2. Detect contradictions ([`crate::detect`]) and compute each claim's
//!     discrete [`HypothesisStanding`].
//!
//! Standing rules (spec §3 thesis — falsification, never "true"):
//!  - `disproven`  — retracted, superseded (as the *old* claim), or carrying a
//!    surviving refutation that outranks its support.
//!  - `challenged` — in a live (unresolved) contradiction, or contradicting a
//!    governing `accept` stance (conformance pressure), and not disproven.
//!  - `withstood`  — has supporting evidence and is neither challenged nor
//!    disproven (survived falsification attempts).
//!  - `open`       — none of the above (no evidence yet, no live tension).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::detect::{self, DetectionOutput};
use crate::event::{Event, EvidenceEvent};
use crate::model::{
    Claim, Contradiction, HypothesisStanding, RemediationEvent, RemediationMove,
    RequirementCandidate, RequirementMeta,
};
use crate::readiness::ReadinessReport;
use crate::session::CoverageSchema;
use crate::signal::Signal;

/// A governing stance taken via the `accept` move (spec §4). Its target claim
/// is elevated; contradicting claims are placed under conformance pressure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptStance {
    pub remediation_id: String,
    pub target: String,
    pub rationale: String,
    /// Claim ids currently in tension with the governing target (computed).
    pub nonconforming_claims: Vec<String>,
}

/// The session registry (spec §2): the known entity & predicate ids so callers
/// reuse stable identity instead of re-declaring it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    pub entity_ids: Vec<String>,
    pub predicate_tokens: Vec<String>,
}

/// The full computed projection of a session (spec §3 `InterviewState`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterviewState {
    pub session_id: String,
    pub coverage_schema: CoverageSchema,
    /// Live hypotheses with their computed standing, in insertion order.
    pub claims: Vec<ClaimWithStanding>,
    pub contradictions: Vec<Contradiction>,
    pub requirements: Vec<RequirementCandidate>,
    pub accepted_stances: Vec<AcceptStance>,
    pub registry: Registry,
    pub signals: Vec<Signal>,
    pub readiness: ReadinessReport,
}

/// A claim paired with its computed standing (spec §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimWithStanding {
    #[serde(flatten)]
    pub claim: Claim,
    pub standing: HypothesisStanding,
    /// Per-claim evidence event ids, for "show your work".
    pub evidence: Vec<String>,
}

/// Intermediate raw fold of the log, before detection/standing are computed.
#[derive(Debug, Default)]
pub(crate) struct RawFold {
    pub session_id: String,
    pub coverage_schema: CoverageSchema,
    /// Claims in insertion order.
    pub claims: Vec<Claim>,
    /// Requirement metadata keyed by claim id.
    pub requirement_meta: BTreeMap<String, RequirementMeta>,
    /// Evidence grouped by claim id, in append order.
    pub evidence: BTreeMap<String, Vec<EvidenceEvent>>,
    /// All remediation events in append order.
    pub remediations: Vec<RemediationEvent>,
}

impl RawFold {
    fn claim(&self, id: &str) -> Option<&Claim> {
        self.claims.iter().find(|c| c.id == id)
    }
}

/// Fold a replayed event stream into a [`RawFold`] (pass 1). Later events that
/// mutate a claim (e.g. `qualify` adds a condition) are applied here.
pub(crate) fn raw_fold(events: &[Event]) -> RawFold {
    let mut fold = RawFold::default();
    for event in events {
        match event {
            Event::SessionOpened {
                session_id,
                coverage_schema,
            } => {
                fold.session_id = session_id.clone();
                fold.coverage_schema = coverage_schema.clone();
            }
            Event::ClaimAdded { claim, requirement } => {
                fold.claims.push((**claim).clone());
                if let Some(meta) = requirement {
                    fold.requirement_meta.insert(claim.id.clone(), meta.clone());
                }
            }
            Event::EvidenceAdded { evidence } => {
                fold.evidence
                    .entry(evidence.claim_id.clone())
                    .or_default()
                    .push(evidence.clone());
            }
            Event::Remediated { remediation } => {
                apply_qualify(&mut fold, remediation);
                fold.remediations.push(remediation.clone());
            }
        }
    }
    fold
}

/// `qualify` mutates the targeted claim's scope so a contradiction dissolves.
fn apply_qualify(fold: &mut RawFold, remediation: &RemediationEvent) {
    if let RemediationMove::Qualify {
        claim,
        condition,
        time_scope,
    } = &remediation.r#move
    {
        if let Some(c) = fold.claims.iter_mut().find(|c| &c.id == claim) {
            if let Some(cond) = condition {
                c.condition = Some(cond.clone());
            }
            if let Some(ts) = time_scope {
                c.time_scope = Some(ts.clone());
            }
        }
    }
}

/// Build the registry from the raw fold (spec §2): every entity id and
/// predicate token ever seen, sorted & deduped for stable output.
fn build_registry(fold: &RawFold) -> Registry {
    let mut entities: BTreeSet<String> = BTreeSet::new();
    let mut predicates: BTreeSet<String> = BTreeSet::new();
    for c in &fold.claims {
        entities.insert(c.subject.id.clone());
        if let crate::model::Object::Entity(e) = &c.object {
            entities.insert(e.id.clone());
        }
        predicates.insert(c.predicate.clone());
    }
    Registry {
        entity_ids: entities.into_iter().collect(),
        predicate_tokens: predicates.into_iter().collect(),
    }
}

/// Set of claim ids that have been retracted or superseded (terminally
/// disproven by an explicit remediation move).
fn disproven_by_remediation(fold: &RawFold) -> BTreeSet<String> {
    let mut disproven = BTreeSet::new();
    for r in &fold.remediations {
        match &r.r#move {
            RemediationMove::Retract { claim } => {
                disproven.insert(claim.clone());
            }
            RemediationMove::Supersede { old, .. } => {
                disproven.insert(old.clone());
            }
            _ => {}
        }
    }
    disproven
}

/// Compute the discrete standing of every claim (pass 2).
///
/// `contradictions` is the detector output over the *current* (post-qualify)
/// claim set and reality evidence; `accept_targets` are claims governing under
/// an accept stance; `nonconforming` maps a governing target to the set of
/// claims contradicting it.
fn compute_standings(
    fold: &RawFold,
    detection: &DetectionOutput,
    disproven: &BTreeSet<String>,
) -> BTreeMap<String, HypothesisStanding> {
    // An `accept(target)` ELEVATES its target to governing (spec §4): the target
    // must NOT be marked contradicted by the very intra-spec tension(s) the
    // stance governs — the conformance burden falls only on the nonconforming
    // claims. Collect governing targets so we can exclude them below.
    let accept_targets: BTreeSet<&str> = detection
        .accepted_stances
        .iter()
        .map(|s| s.target.as_str())
        .collect();

    // Claims implicated in a live contradiction are challenged (unless disproven
    // or governing under an accept stance).
    let mut contradicted: BTreeSet<String> = BTreeSet::new();
    for c in &detection.contradictions {
        for id in &c.claims {
            if accept_targets.contains(id.as_str()) {
                continue;
            }
            contradicted.insert(id.clone());
        }
    }
    // Claims under accept-conformance pressure are challenged too.
    for id in &detection.nonconforming_claims {
        contradicted.insert(id.clone());
    }

    let mut standings = BTreeMap::new();
    for claim in &fold.claims {
        let standing = compute_one(fold, claim, disproven, &contradicted);
        standings.insert(claim.id.clone(), standing);
    }
    standings
}

fn compute_one(
    fold: &RawFold,
    claim: &Claim,
    disproven: &BTreeSet<String>,
    contradicted: &BTreeSet<String>,
) -> HypothesisStanding {
    if disproven.contains(&claim.id) {
        return HypothesisStanding::Disproven;
    }

    let evidence = fold
        .evidence
        .get(&claim.id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let has_user_refutation = evidence
        .iter()
        .any(|e| e.source_type == crate::model::EvidenceSourceType::UserRefutation);

    // A user refutation falsifies the hypothesis — unless the user later
    // assented after refuting (last user verdict wins). User facts outrank
    // app-research either way (spec §1 non-goal 6).
    if has_user_refutation && !user_reasserted_after_refutation(evidence) {
        return HypothesisStanding::Disproven;
    }

    let has_support = evidence.iter().any(|e| e.source_type.is_support());
    // Last-research-wins (spec §4): a later app support supersedes an earlier
    // app refutation, so the claim is only app-refuted if the *last* app verdict
    // refutes it.
    let app_refutes = detect::app_research_refutes(evidence);

    // Live structural tension (intra-spec contradiction, spec-vs-reality, or
    // accept-conformance pressure) challenges the claim.
    if contradicted.contains(&claim.id) {
        return HypothesisStanding::Challenged;
    }

    // An app-research refutation challenges the claim. User assent only
    // overrides it for fact-like claims (context | risk); for intent claims
    // (requirement | constraint | decision) the intent↔code divergence stands
    // regardless of assent (E1). Shared with detection so they agree.
    if app_refutes && !detect::app_refutation_suppressed_by_user(claim, evidence) {
        return HypothesisStanding::Challenged;
    }

    if has_support {
        return HypothesisStanding::Withstood;
    }

    HypothesisStanding::Open
}

/// True if the *last* user verdict in the evidence stream was assent (the user
/// reasserted the claim after a prior refutation).
fn user_reasserted_after_refutation(evidence: &[EvidenceEvent]) -> bool {
    let last_user = evidence
        .iter()
        .rev()
        .find(|e| e.source_type.is_user_sourced());
    matches!(
        last_user.map(|e| e.source_type),
        Some(crate::model::EvidenceSourceType::UserAssent)
    )
}

/// Derive requirement candidates from `category: requirement` claims and their
/// attached metadata (spec §3). Quality checks are computed deterministically.
fn derive_requirements(
    fold: &RawFold,
    standings: &BTreeMap<String, HypothesisStanding>,
) -> Vec<RequirementCandidate> {
    use crate::model::{Category, Object, Priority, RequirementQualityCheck};

    let mut out = Vec::new();
    for claim in &fold.claims {
        if claim.category != Category::Requirement {
            continue;
        }
        // Disproven requirements drop out of the derived set.
        if standings.get(&claim.id) == Some(&HypothesisStanding::Disproven) {
            continue;
        }
        let meta = fold
            .requirement_meta
            .get(&claim.id)
            .cloned()
            .unwrap_or_default();
        let capability = meta.capability.unwrap_or_else(|| claim.predicate.clone());
        let has_explicit_statement = meta.statement.is_some();
        let statement = meta.statement.unwrap_or_else(|| {
            let obj = match &claim.object {
                Object::Entity(e) => e.id.clone(),
                Object::Value { value } => value.clone(),
            };
            format!("{} {} {}", claim.subject.id, claim.predicate, obj)
        });
        let priority = meta.priority;
        let owner = meta.owner;

        let mut quality_checks = Vec::new();
        let has_acceptance = !meta.acceptance_criteria.is_empty();
        quality_checks.push(RequirementQualityCheck {
            name: "has_acceptance".into(),
            passed: has_acceptance,
            detail: None,
        });
        quality_checks.push(RequirementQualityCheck {
            name: "has_owner".into(),
            passed: owner.is_some(),
            detail: None,
        });
        // `testable`: a requirement is testable iff it has acceptance criteria.
        quality_checks.push(RequirementQualityCheck {
            name: "testable".into(),
            passed: has_acceptance,
            detail: None,
        });
        // `unambiguous`: the requirement names a concrete thing — either the
        // caller supplied an explicit statement, or the claim's object carries a
        // non-empty value / entity id. An empty object value with no statement is
        // ambiguous (we cannot say *what* must hold).
        let object_is_concrete = match &claim.object {
            Object::Entity(e) => !e.id.trim().is_empty(),
            Object::Value { value } => !value.trim().is_empty(),
        };
        quality_checks.push(RequirementQualityCheck {
            name: "unambiguous".into(),
            passed: has_explicit_statement || object_is_concrete,
            detail: None,
        });

        out.push(RequirementCandidate {
            id: format!("req:{}", claim.id),
            statement,
            subject: claim.subject.clone(),
            capability,
            condition: claim.condition.clone(),
            acceptance_criteria: meta.acceptance_criteria,
            priority: priority.or({
                // Default a `must` for requirement claims if none given? No —
                // priority is caller intent; leave None.
                None::<Priority>
            }),
            owner,
            rationale: meta.rationale,
            source_claims: vec![claim.id.clone()],
            quality_checks,
        });
    }
    out
}

/// Build the complete [`InterviewState`] from a replayed event stream. This is
/// the single deterministic entry point used by both `state.get` and `append`.
pub fn project(events: &[Event]) -> InterviewState {
    let fold = raw_fold(events);
    let disproven = disproven_by_remediation(&fold);

    // Detection needs to know which claims are already disproven so it doesn't
    // flag tensions involving dead hypotheses.
    let detection = detect::detect(&fold, &disproven);
    let standings = compute_standings(&fold, &detection, &disproven);
    let requirements = derive_requirements(&fold, &standings);
    let registry = build_registry(&fold);

    let claims: Vec<ClaimWithStanding> = fold
        .claims
        .iter()
        .map(|c| ClaimWithStanding {
            claim: c.clone(),
            standing: standings
                .get(&c.id)
                .copied()
                .unwrap_or(HypothesisStanding::Open),
            evidence: fold
                .evidence
                .get(&c.id)
                .map(|evs| evs.iter().map(|e| e.id.clone()).collect())
                .unwrap_or_default(),
        })
        .collect();

    let readiness = crate::readiness::assess(
        &fold.coverage_schema,
        &claims,
        &detection.contradictions,
        &detection.accepted_stances,
        &requirements,
    );

    let signals = crate::signal::compute_signals(&claims, &detection, &readiness);

    InterviewState {
        session_id: fold.session_id.clone(),
        coverage_schema: fold.coverage_schema.clone(),
        claims,
        contradictions: detection.contradictions,
        requirements,
        accepted_stances: detection.accepted_stances,
        registry,
        signals,
        readiness,
    }
}

// Re-export so `detect` can reach the raw fold helpers it needs.
pub(crate) use RawFold as InternalRawFold;

impl RawFold {
    /// Public-within-crate accessor used by detection.
    pub(crate) fn claim_by_id(&self, id: &str) -> Option<&Claim> {
        self.claim(id)
    }
}
