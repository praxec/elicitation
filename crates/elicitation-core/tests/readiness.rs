//! Readiness gate table tests (spec §6 / §9): each blocker condition provable
//! independently.

mod common;

use common::{claim, temp_engine};
use elicitation_core::{
    Category, CoverageDimension, CoverageSchema, EvidenceEvent, EvidenceRef, EvidenceSourceType,
    Polarity, Priority, RemediationEvent, RemediationMove, RequirementMeta,
};

fn assent(session: &str, id: &str, claim_id: &str) -> EvidenceEvent {
    EvidenceEvent {
        id: id.to_string(),
        session_id: session.to_string(),
        claim_id: claim_id.to_string(),
        source_type: EvidenceSourceType::UserAssent,
        detail: None,
        source: EvidenceRef::new("t"),
    }
}

/// A single-dimension schema makes coverage tests crisp.
fn one_dim_schema(tag: &str) -> CoverageSchema {
    CoverageSchema {
        dimensions: vec![CoverageDimension::required(tag)],
    }
}

#[test]
fn empty_session_blocked_on_coverage() {
    let (eng, _d) = temp_engine();
    let st = eng
        .open_session("s", Some(one_dim_schema("purpose")))
        .unwrap();
    assert!(!st.readiness.ready);
    assert!(st
        .readiness
        .blockers
        .iter()
        .any(|b| b.starts_with("COVERAGE")));
}

#[test]
fn covered_and_withstood_is_ready() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", Some(one_dim_schema("purpose")))
        .unwrap();
    let mut c = claim("s", "c1", "e:app", "purpose", "billing");
    c.dimensions = vec!["purpose".into()];
    eng.add_claim("s", c, None).unwrap();
    let st = eng.add_evidence("s", assent("s", "ev1", "c1")).unwrap();
    assert!(st.readiness.ready, "{:?}", st.readiness.blockers);
}

#[test]
fn covered_but_only_open_is_blocked() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", Some(one_dim_schema("purpose")))
        .unwrap();
    let mut c = claim("s", "c1", "e:app", "purpose", "billing");
    c.dimensions = vec!["purpose".into()];
    let st = eng.add_claim("s", c, None).unwrap();
    assert!(!st.readiness.ready);
    assert!(st
        .readiness
        .blockers
        .iter()
        .any(|b| b.starts_with("COVERAGE")));
}

#[test]
fn high_severity_contradiction_blocks() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", Some(one_dim_schema("constraints")))
        .unwrap();
    let mut c1 = claim("s", "c1", "e:app", "max_latency_ms", "100");
    c1.category = Category::Constraint;
    c1.dimensions = vec!["constraints".into()];
    eng.add_claim("s", c1, None).unwrap();
    eng.add_evidence("s", assent("s", "ev1", "c1")).unwrap();
    let mut c2 = claim("s", "c2", "e:app", "max_latency_ms", "250");
    c2.category = Category::Constraint;
    c2.dimensions = vec!["constraints".into()];
    let st = eng.add_claim("s", c2, None).unwrap();
    assert!(st
        .readiness
        .blockers
        .iter()
        .any(|b| b.starts_with("CONTRADICTION")));
    assert!(!st.readiness.ready);
}

#[test]
fn must_requirement_without_acceptance_or_owner_blocks() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", Some(one_dim_schema("scope")))
        .unwrap();
    let mut c = claim("s", "c1", "e:app", "export", "csv");
    c.category = Category::Requirement;
    c.dimensions = vec!["scope".into()];
    let meta = RequirementMeta {
        priority: Some(Priority::Must),
        ..Default::default()
    };
    eng.add_claim("s", c, Some(meta)).unwrap();
    let st = eng.add_evidence("s", assent("s", "ev1", "c1")).unwrap();
    assert!(st
        .readiness
        .blockers
        .iter()
        .any(|b| b.contains("acceptance criteria")));
    assert!(st.readiness.blockers.iter().any(|b| b.contains("no owner")));
}

#[test]
fn must_requirement_complete_is_ready() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", Some(one_dim_schema("scope")))
        .unwrap();
    let mut c = claim("s", "c1", "e:app", "export", "csv");
    c.category = Category::Requirement;
    c.dimensions = vec!["scope".into()];
    let meta = RequirementMeta {
        priority: Some(Priority::Must),
        acceptance_criteria: vec!["downloads a .csv".into()],
        owner: Some("alice".into()),
        ..Default::default()
    };
    eng.add_claim("s", c, Some(meta)).unwrap();
    let st = eng.add_evidence("s", assent("s", "ev1", "c1")).unwrap();
    assert!(st.readiness.ready, "{:?}", st.readiness.blockers);
}

#[test]
fn accept_conformance_blocks_until_resolved() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", Some(one_dim_schema("scope")))
        .unwrap();
    let mut gov = claim("s", "gov", "e:app", "auth", "oauth");
    gov.dimensions = vec!["scope".into()];
    eng.add_claim("s", gov, None).unwrap();
    eng.add_evidence("s", assent("s", "ev1", "gov")).unwrap();
    let mut other = claim("s", "other", "e:app", "auth", "ldap");
    other.polarity = Polarity::Positive;
    eng.add_claim("s", other, None).unwrap();
    let st = eng
        .remediate(
            "s",
            RemediationEvent {
                id: "r1".into(),
                session_id: "s".into(),
                r#move: RemediationMove::Accept {
                    target: "gov".into(),
                    rationale: "chosen".into(),
                },
                source: EvidenceRef::new("t"),
                detail: None,
            },
        )
        .unwrap();
    assert!(!st.readiness.ready);
    assert!(st
        .readiness
        .blockers
        .iter()
        .any(|b| b.starts_with("CONFORMANCE")));
}

#[test]
fn non_required_dimension_does_not_block() {
    let (eng, _d) = temp_engine();
    let schema = CoverageSchema {
        dimensions: vec![CoverageDimension {
            tag: "nice-to-have".into(),
            required: false,
        }],
    };
    let st = eng.open_session("s", Some(schema)).unwrap();
    assert!(st.readiness.ready, "{:?}", st.readiness.blockers);
}
