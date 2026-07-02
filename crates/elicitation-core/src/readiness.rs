//! The plan-readiness gate (spec §6): alignment, computed, honest.
//!
//! `ready == true` **iff** all four conditions hold:
//!  1. **Coverage** — every *required* dimension carries a `withstood`+
//!     hypothesis.
//!  2. **No surviving high-severity contradictions** — no `challenged`/`disproven`
//!     claim at high severity.
//!  3. **No open conformance-remediation** — every `accept` stance has had its
//!     contradicting claims brought into conformance.
//!  4. **Requirement quality** — every `must` requirement has acceptance
//!     criteria + an owner.
//!
//! The report **shows its work**: per-dimension standing + every blocker. It
//! asserts "survived our falsification attempts," never "true."

use serde::{Deserialize, Serialize};

use crate::model::{Contradiction, HypothesisStanding, Priority, RequirementCandidate, Severity};
use crate::projection::{AcceptStance, ClaimWithStanding};
use crate::session::CoverageSchema;

/// Per-dimension coverage status (spec §6 "show your work").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionStatus {
    pub dimension: String,
    pub required: bool,
    /// Best standing achieved by any claim tagged with this dimension.
    pub best_standing: Option<HypothesisStanding>,
    /// Claim ids contributing to this dimension.
    pub claims: Vec<String>,
    /// True iff a `withstood`+ hypothesis covers this dimension.
    pub satisfied: bool,
}

/// A coverage gap — a required dimension lacking a `withstood`+ hypothesis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageGap {
    pub dimension: String,
    pub suggested_question: String,
}

/// The computed readiness report (spec §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessReport {
    pub ready: bool,
    pub dimensions: Vec<DimensionStatus>,
    pub gaps: Vec<CoverageGap>,
    pub blockers: Vec<String>,
}

/// Compute the readiness report deterministically (spec §6). Pure over its
/// inputs — the same projection always yields the same report.
pub(crate) fn assess(
    schema: &CoverageSchema,
    claims: &[ClaimWithStanding],
    contradictions: &[Contradiction],
    stances: &[AcceptStance],
    requirements: &[RequirementCandidate],
) -> ReadinessReport {
    let mut dimensions = Vec::new();
    let mut gaps = Vec::new();
    let mut blockers = Vec::new();

    // (1) Coverage — per required dimension, best standing across its claims.
    for dim in &schema.dimensions {
        let mut best: Option<HypothesisStanding> = None;
        let mut dim_claims = Vec::new();
        for cw in claims {
            if cw.claim.dimensions.iter().any(|t| t == &dim.tag) {
                dim_claims.push(cw.claim.id.clone());
                best = Some(match best {
                    Some(b) if b.rank() >= cw.standing.rank() => b,
                    _ => cw.standing,
                });
            }
        }
        let satisfied = best.map(|b| b.is_withstood_or_better()).unwrap_or(false);
        if dim.required && !satisfied {
            let suggested_question = match best {
                None => format!("What is the {} for this work?", dim.tag),
                Some(_) => format!(
                    "The {} hypothesis isn't settled yet — can you confirm or refute it?",
                    dim.tag
                ),
            };
            gaps.push(CoverageGap {
                dimension: dim.tag.clone(),
                suggested_question,
            });
            blockers.push(format!(
                "COVERAGE: required dimension '{}' lacks a withstood hypothesis",
                dim.tag
            ));
        }
        dimensions.push(DimensionStatus {
            dimension: dim.tag.clone(),
            required: dim.required,
            best_standing: best,
            claims: dim_claims,
            satisfied,
        });
    }

    // (2) No surviving high-severity contradictions.
    for c in contradictions {
        if c.severity == Severity::High {
            blockers.push(format!(
                "CONTRADICTION: surviving high-severity contradiction '{}' ({})",
                c.id, c.explanation
            ));
        }
    }

    // (3) No open conformance-remediation: every accept stance must have an
    //     empty nonconforming set.
    for stance in stances {
        if !stance.nonconforming_claims.is_empty() {
            blockers.push(format!(
                "CONFORMANCE: accepted stance '{}' has {} nonconforming claim(s): {}",
                stance.target,
                stance.nonconforming_claims.len(),
                stance.nonconforming_claims.join(", ")
            ));
        }
    }

    // (4) Requirement quality — every `must` requirement needs acceptance
    //     criteria + an owner.
    for req in requirements {
        if req.priority == Some(Priority::Must) {
            if req.acceptance_criteria.is_empty() {
                blockers.push(format!(
                    "REQUIREMENT: must-requirement '{}' has no acceptance criteria",
                    req.id
                ));
            }
            if req.owner.is_none() {
                blockers.push(format!(
                    "REQUIREMENT: must-requirement '{}' has no owner",
                    req.id
                ));
            }
        }
    }

    ReadinessReport {
        ready: blockers.is_empty(),
        dimensions,
        gaps,
        blockers,
    }
}

impl Default for ReadinessReport {
    fn default() -> Self {
        Self {
            ready: false,
            dimensions: Vec::new(),
            gaps: Vec::new(),
            blockers: vec!["EMPTY: no events".to_string()],
        }
    }
}
