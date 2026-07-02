//! `spec.export` (spec §6): emit the aligned, plan-ready spec document.
//!
//! Groups `withstood`+ hypotheses by coverage dimension, lists requirement
//! candidates with acceptance criteria + provenance, and includes a visible
//! **accepted-tensions** section so nothing is waved through silently.

use serde::{Deserialize, Serialize};

use crate::model::{HypothesisStanding, RequirementCandidate};
use crate::projection::InterviewState;

/// A hypothesis as it appears in the exported spec (settled facts only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedHypothesis {
    pub claim_id: String,
    pub statement: String,
    pub standing: HypothesisStanding,
    pub source_turn: String,
}

/// `withstood`+ hypotheses grouped under one coverage dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionSection {
    pub dimension: String,
    pub hypotheses: Vec<ExportedHypothesis>,
}

/// An accepted tension (spec §6): a governing `accept` stance, surfaced so it
/// is never silent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedTension {
    pub target: String,
    pub rationale: String,
    pub nonconforming_claims: Vec<String>,
}

/// The exported plan-ready spec document (spec §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecDocument {
    pub session_id: String,
    pub ready: bool,
    pub dimensions: Vec<DimensionSection>,
    pub requirements: Vec<RequirementCandidate>,
    pub accepted_tensions: Vec<AcceptedTension>,
    /// Carried verbatim so the caller sees exactly why it is/isn't plan-ready.
    pub blockers: Vec<String>,
}

/// Build the exported document from a projection (deterministic).
pub(crate) fn build(state: &InterviewState) -> SpecDocument {
    let mut dimensions = Vec::new();
    for dim in &state.coverage_schema.dimensions {
        let mut hypotheses = Vec::new();
        for cw in &state.claims {
            if !cw.standing.is_withstood_or_better() {
                continue;
            }
            if cw.claim.dimensions.iter().any(|t| t == &dim.tag) {
                hypotheses.push(ExportedHypothesis {
                    claim_id: cw.claim.id.clone(),
                    statement: statement_for(&cw.claim),
                    standing: cw.standing,
                    source_turn: cw.claim.source.turn_id.clone(),
                });
            }
        }
        dimensions.push(DimensionSection {
            dimension: dim.tag.clone(),
            hypotheses,
        });
    }

    let accepted_tensions = state
        .accepted_stances
        .iter()
        .map(|s| AcceptedTension {
            target: s.target.clone(),
            rationale: s.rationale.clone(),
            nonconforming_claims: s.nonconforming_claims.clone(),
        })
        .collect();

    SpecDocument {
        session_id: state.session_id.clone(),
        ready: state.readiness.ready,
        dimensions,
        requirements: state.requirements.clone(),
        accepted_tensions,
        blockers: state.readiness.blockers.clone(),
    }
}

fn statement_for(claim: &crate::model::Claim) -> String {
    let obj = match &claim.object {
        crate::model::Object::Entity(e) => e.id.clone(),
        crate::model::Object::Value { value } => value.clone(),
    };
    let polarity = match claim.polarity {
        crate::model::Polarity::Positive => "",
        crate::model::Polarity::Negative => "NOT ",
    };
    let scope = claim
        .condition
        .as_ref()
        .map(|c| format!(" [when {c}]"))
        .unwrap_or_default();
    format!(
        "{}{} {} {}{scope}",
        polarity, claim.subject.id, claim.predicate, obj
    )
}
