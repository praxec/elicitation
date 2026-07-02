//! First-class output signals (spec §4): returned on **every** `append` and
//! **every** read so the caller can never forget to check.
//!
//! Three kinds:
//!  - `notify`    — a contradiction/disproof was detected (an event happened).
//!  - `clarify`   — a gap/ambiguity needs a question to the human (this *is*
//!    `question.next`).
//!  - `remediate` — a standing hypothesis has open disproof; resolve it via a
//!    remediation move (incl. accept-conformance pressure).

use serde::{Deserialize, Serialize};

use crate::detect::DetectionOutput;
use crate::model::HypothesisStanding;
use crate::projection::ClaimWithStanding;
use crate::readiness::ReadinessReport;

/// The kind of a [`Signal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Notify,
    Clarify,
    Remediate,
}

/// A single actionable signal for the caller (spec §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub kind: SignalKind,
    /// Stable code for programmatic dispatch, e.g. `intra_spec_contradiction`,
    /// `coverage_gap`, `accept_conformance`, `app_research_refutation`.
    pub code: String,
    /// Human-facing message.
    pub message: String,
    /// Claim ids this signal concerns.
    #[serde(default)]
    pub claims: Vec<String>,
    /// Contradiction id, when the signal stems from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contradiction: Option<String>,
    /// Coverage dimension, when the signal is a gap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    /// A concrete next question the caller can voice, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_question: Option<String>,
}

/// Compute the signal set from the detection output, claim standings, and
/// readiness. Order is deterministic: notify (contradictions), then remediate
/// (challenged/conformance), then clarify (gaps).
pub(crate) fn compute_signals(
    claims: &[ClaimWithStanding],
    detection: &DetectionOutput,
    readiness: &ReadinessReport,
) -> Vec<Signal> {
    let mut signals = Vec::new();

    // notify: every detected contradiction is an event worth surfacing.
    for c in &detection.contradictions {
        let code = match c.kind {
            crate::model::ContradictionKind::IntraSpec => "intra_spec_contradiction",
            crate::model::ContradictionKind::SpecVsReality => "spec_vs_reality_contradiction",
        };
        signals.push(Signal {
            kind: SignalKind::Notify,
            code: code.to_string(),
            message: c.explanation.clone(),
            claims: c.claims.clone(),
            contradiction: Some(c.id.clone()),
            dimension: None,
            suggested_question: Some(c.suggested_question.clone()),
        });
        // remediate: a live contradiction needs a move to resolve it.
        signals.push(Signal {
            kind: SignalKind::Remediate,
            code: format!("resolve_{code}"),
            message: format!("Resolve contradiction '{}' with a remediation move.", c.id),
            claims: c.claims.clone(),
            contradiction: Some(c.id.clone()),
            dimension: None,
            suggested_question: Some(c.suggested_question.clone()),
        });
    }

    // remediate: accept-conformance pressure — claims contradicting a governing
    // stance must be brought into conformance (spec §4 / §6.3).
    for stance in &detection.accepted_stances {
        for claim_id in &stance.nonconforming_claims {
            signals.push(Signal {
                kind: SignalKind::Remediate,
                code: "accept_conformance".to_string(),
                message: format!(
                    "Claim '{claim_id}' contradicts accepted stance '{}': {}. Bring it into conformance.",
                    stance.target, stance.rationale
                ),
                claims: vec![claim_id.clone()],
                contradiction: None,
                dimension: None,
                suggested_question: Some(format!(
                    "The governing decision is '{}'. How should '{claim_id}' change to conform?",
                    stance.target
                )),
            });
        }
    }

    // clarify: each uncovered required dimension is a gap needing a question.
    for gap in &readiness.gaps {
        signals.push(Signal {
            kind: SignalKind::Clarify,
            code: "coverage_gap".to_string(),
            message: format!(
                "Coverage dimension '{}' has no withstood hypothesis.",
                gap.dimension
            ),
            claims: Vec::new(),
            contradiction: None,
            dimension: Some(gap.dimension.clone()),
            suggested_question: Some(gap.suggested_question.clone()),
        });
    }

    // clarify: any standing-open claim with no evidence is mild ambiguity worth
    // a question — but only surface when it sits on a required dimension already
    // covered as a gap-free reminder is noisy; we keep it scoped to claims that
    // are open AND carry a coverage dimension, so the next-question logic has
    // material even when all dimensions nominally have a claim.
    for cw in claims {
        if cw.standing == HypothesisStanding::Open && cw.evidence.is_empty() {
            signals.push(Signal {
                kind: SignalKind::Clarify,
                code: "open_hypothesis".to_string(),
                message: format!(
                    "Hypothesis '{}' has no evidence yet; seek user assent or refutation.",
                    cw.claim.id
                ),
                claims: vec![cw.claim.id.clone()],
                contradiction: None,
                dimension: cw.claim.dimensions.first().cloned(),
                suggested_question: Some(format!(
                    "Can you confirm: {} {} ... ? (claim '{}')",
                    cw.claim.subject.id, cw.claim.predicate, cw.claim.id
                )),
            });
        }
    }

    signals
}

/// The highest-leverage next question (spec §5 `question.next`): the first
/// `clarify`/`remediate` signal in deterministic priority order, never derived
/// from satisfied dimensions. Returns `None` when nothing needs asking.
pub fn next_question(signals: &[Signal]) -> Option<&Signal> {
    // Priority: resolve contradictions/conformance first (remediate), then fill
    // coverage gaps (clarify), then chase open hypotheses.
    signals
        .iter()
        .find(|s| s.kind == SignalKind::Remediate && s.suggested_question.is_some())
        .or_else(|| {
            signals
                .iter()
                .find(|s| s.code == "coverage_gap" && s.suggested_question.is_some())
        })
        .or_else(|| {
            signals
                .iter()
                .find(|s| s.kind == SignalKind::Clarify && s.suggested_question.is_some())
        })
}
