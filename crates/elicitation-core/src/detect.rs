//! Structural contradiction & gap detection (spec §3 / §4).
//!
//! Detectors are **exact-match pure functions** over caller-supplied canonical
//! identity (spec §2): no fuzzy matching, no case-folding, no synonym tables.
//! That is what makes the kernel 100% golden-testable offline.
//!
//! Two contradiction kinds (spec §3):
//!  - `intra_spec`      — two live claims about the same `(subject, predicate)`
//!    disagree (opposite polarity, or different `Value` objects) *and* share an
//!    overlapping scope (neither has been `qualify`-d apart).
//!  - `spec_vs_reality` — a live claim carries an `app_research_refutation`
//!    evidence event (the caller researched the app and it contradicts the spec).
//!
//! Plus accept-stance conformance pressure (spec §4 `accept`) and coverage gaps
//! (spec §6.1) feeding the `clarify` signal.

use std::collections::BTreeSet;

use crate::model::{
    Claim, Contradiction, ContradictionKind, EvidenceSourceType, Object, Polarity, RemediationMove,
    Severity,
};
use crate::projection::{AcceptStance, InternalRawFold};

/// The full output of the detection pass.
#[derive(Debug, Default)]
pub(crate) struct DetectionOutput {
    pub contradictions: Vec<Contradiction>,
    pub accepted_stances: Vec<AcceptStance>,
    /// Claims under accept-conformance pressure (challenged but no symmetric
    /// contradiction record).
    pub nonconforming_claims: Vec<String>,
}

/// Run all detectors over the raw fold. `disproven` are claims already killed by
/// a remediation move (retract/supersede) — they are excluded from detection so
/// dead hypotheses don't generate phantom tensions.
pub(crate) fn detect(fold: &InternalRawFold, disproven: &BTreeSet<String>) -> DetectionOutput {
    let live: Vec<&Claim> = fold
        .claims_iter()
        .filter(|c| !disproven.contains(&c.id))
        .collect();

    let mut contradictions = Vec::new();
    detect_intra_spec(&live, &mut contradictions);
    detect_spec_vs_reality(fold, &live, &mut contradictions);

    let (accepted_stances, nonconforming_claims) =
        detect_accept_conformance(fold, &live, disproven);

    DetectionOutput {
        contradictions,
        accepted_stances,
        nonconforming_claims,
    }
}

/// Two claims with the same `(subject.id, predicate)` conflict if they have
/// opposite polarity, or positive polarity with different `Value` objects.
/// They are NOT a contradiction if they have been `qualify`-d into disjoint
/// scopes (different `condition` or `time_scope`).
fn detect_intra_spec(live: &[&Claim], out: &mut Vec<Contradiction>) {
    for i in 0..live.len() {
        for j in (i + 1)..live.len() {
            let a = live[i];
            let b = live[j];
            if a.subject.id != b.subject.id || a.predicate != b.predicate {
                continue;
            }
            if scopes_disjoint(a, b) {
                continue;
            }
            if let Some((sev, explanation)) = conflict(a, b) {
                out.push(Contradiction {
                    id: contradiction_id(ContradictionKind::IntraSpec, &[&a.id, &b.id]),
                    kind: ContradictionKind::IntraSpec,
                    claims: vec![a.id.clone(), b.id.clone()],
                    severity: sev,
                    explanation,
                    suggested_question: format!(
                        "Claims '{}' and '{}' both describe {} {} but disagree — which holds?",
                        a.id, b.id, a.subject.id, a.predicate
                    ),
                });
            }
        }
    }
}

/// Two claims sit in disjoint scopes (so both can hold) iff their `condition`s
/// or `time_scope`s are both present and differ. If either is unscoped, their
/// scopes overlap.
fn scopes_disjoint(a: &Claim, b: &Claim) -> bool {
    let cond_disjoint = match (&a.condition, &b.condition) {
        (Some(x), Some(y)) => x != y,
        _ => false,
    };
    let time_disjoint = match (&a.time_scope, &b.time_scope) {
        (Some(x), Some(y)) => x != y,
        _ => false,
    };
    cond_disjoint || time_disjoint
}

/// Determine whether two same-`(subject,predicate)` claims actually conflict,
/// and at what severity. `constraint`/`decision` conflicts are high severity;
/// others medium.
fn conflict(a: &Claim, b: &Claim) -> Option<(Severity, String)> {
    let polarity_conflict = a.polarity != b.polarity && objects_equal(&a.object, &b.object);
    let value_conflict = a.polarity == Polarity::Positive
        && b.polarity == Polarity::Positive
        && !objects_equal(&a.object, &b.object)
        && both_values(&a.object, &b.object);

    if !polarity_conflict && !value_conflict {
        return None;
    }

    let severity = severity_for(a, b);
    let explanation = if polarity_conflict {
        format!(
            "Claim '{}' asserts and '{}' denies the same fact ({} {}).",
            a.id, b.id, a.subject.id, a.predicate
        )
    } else {
        format!(
            "Claims '{}' and '{}' assign different values to {} {}.",
            a.id, b.id, a.subject.id, a.predicate
        )
    };
    Some((severity, explanation))
}

fn severity_for(a: &Claim, b: &Claim) -> Severity {
    use crate::model::Category::{Constraint, Decision, Requirement};
    let high = |c: &Claim| matches!(c.category, Constraint | Decision | Requirement);
    if high(a) || high(b) {
        Severity::High
    } else {
        Severity::Medium
    }
}

fn objects_equal(a: &Object, b: &Object) -> bool {
    a == b
}

fn both_values(a: &Object, b: &Object) -> bool {
    matches!(a, Object::Value { .. }) && matches!(b, Object::Value { .. })
}

/// True iff the *last* app-research verdict on this claim was a refutation
/// (spec §4 last-research-wins). A later `AppResearchSupport` supersedes an
/// earlier `AppResearchRefutation`, mirroring last-user-verdict-wins. Returns
/// false when there is no app-research verdict at all.
pub(crate) fn app_research_refutes(evidence: &[crate::event::EvidenceEvent]) -> bool {
    let last_app = evidence.iter().rev().find(|e| {
        matches!(
            e.source_type,
            EvidenceSourceType::AppResearchRefutation | EvidenceSourceType::AppResearchSupport
        )
    });
    matches!(
        last_app.map(|e| e.source_type),
        Some(EvidenceSourceType::AppResearchRefutation)
    )
}

/// Whether an `app_research_refutation` on `claim` should be suppressed by a
/// `user_assent`. Shared by detection and standing so they agree (E1 fix).
///
/// - For **intent** claims (`requirement | constraint | decision`) the
///   refutation always surfaces: the operator asserting the requirement does
///   NOT make the code conform, so the intent↔code divergence is the finding.
/// - For **fact-like** claims (`context | risk`) the existing user-outranks-app
///   rule holds: a `user_assent` suppresses the app refutation (spec §1
///   non-goal 6).
pub(crate) fn app_refutation_suppressed_by_user(
    claim: &Claim,
    evidence: &[crate::event::EvidenceEvent],
) -> bool {
    if claim.category.is_intent() {
        return false;
    }
    evidence
        .iter()
        .any(|e| e.source_type == EvidenceSourceType::UserAssent)
}

/// A live claim with an `app_research_refutation` evidence event contradicts
/// reality (spec §3 `spec_vs_reality`) — unless the user has assented and the
/// claim is fact-like (user facts outrank app research, spec §1 non-goal 6).
/// For intent claims (requirement/constraint/decision) user assent does NOT
/// suppress the refutation: the divergence between intent and code is the
/// finding (E1).
fn detect_spec_vs_reality(fold: &InternalRawFold, live: &[&Claim], out: &mut Vec<Contradiction>) {
    for claim in live {
        let evs = fold.evidence_for(&claim.id);
        let suppressed = app_refutation_suppressed_by_user(claim, evs);
        // Last-research-wins (spec §4 `reconcile_by_evidence`): a later
        // `AppResearchSupport` supersedes an earlier `AppResearchRefutation` on
        // the same claim, mirroring last-user-verdict-wins.
        if app_research_refutes(evs) && !suppressed {
            out.push(Contradiction {
                id: contradiction_id(ContradictionKind::SpecVsReality, &[&claim.id]),
                kind: ContradictionKind::SpecVsReality,
                claims: vec![claim.id.clone()],
                severity: Severity::High,
                explanation: format!(
                    "App research refutes claim '{}' ({} {}): the system does not match the spec.",
                    claim.id, claim.subject.id, claim.predicate
                ),
                suggested_question: format!(
                    "Reality contradicts '{}'. Refine the spec, or change the app and re-evidence?",
                    claim.id
                ),
            });
        }
    }
}

/// Resolve accept stances (spec §4 `accept`). For each `accept(target)`, the
/// target governs; any live claim that *would* contradict the target (same
/// subject+predicate, conflicting) is placed under conformance pressure.
fn detect_accept_conformance(
    fold: &InternalRawFold,
    live: &[&Claim],
    disproven: &BTreeSet<String>,
) -> (Vec<AcceptStance>, Vec<String>) {
    let mut stances = Vec::new();
    let mut all_nonconforming: BTreeSet<String> = BTreeSet::new();

    for r in fold.remediations_iter() {
        let RemediationMove::Accept { target, rationale } = &r.r#move else {
            continue;
        };
        if disproven.contains(target) {
            continue;
        }
        let Some(target_claim) = fold.claim_by_id(target) else {
            continue;
        };

        let mut nonconforming = Vec::new();
        for candidate in live {
            if candidate.id == *target {
                continue;
            }
            if candidate.subject.id != target_claim.subject.id
                || candidate.predicate != target_claim.predicate
            {
                continue;
            }
            if scopes_disjoint(candidate, target_claim) {
                continue;
            }
            if conflict(candidate, target_claim).is_some() {
                nonconforming.push(candidate.id.clone());
                all_nonconforming.insert(candidate.id.clone());
            }
        }

        stances.push(AcceptStance {
            remediation_id: r.id.clone(),
            target: target.clone(),
            rationale: rationale.clone(),
            nonconforming_claims: nonconforming,
        });
    }

    (stances, all_nonconforming.into_iter().collect())
}

/// Deterministic, content-addressed contradiction id so the same tension always
/// gets the same id across replays (claim ids sorted for stability).
fn contradiction_id(kind: ContradictionKind, claim_ids: &[&str]) -> String {
    let mut ids: Vec<&str> = claim_ids.to_vec();
    ids.sort_unstable();
    let prefix = match kind {
        ContradictionKind::IntraSpec => "ctr:intra",
        ContradictionKind::SpecVsReality => "ctr:reality",
    };
    format!("{prefix}:{}", ids.join("+"))
}

// --- raw-fold accessors used only by detection -----------------------------

impl InternalRawFold {
    fn claims_iter(&self) -> impl Iterator<Item = &Claim> {
        self.claims.iter()
    }

    fn remediations_iter(&self) -> impl Iterator<Item = &crate::model::RemediationEvent> {
        self.remediations.iter()
    }

    fn evidence_for(&self, claim_id: &str) -> &[crate::event::EvidenceEvent] {
        self.evidence
            .get(claim_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}
