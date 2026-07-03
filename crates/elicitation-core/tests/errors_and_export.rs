//! Stable error-prefix tests (spec §5) and `spec.export` tests (spec §6).

mod common;

use common::{claim, temp_engine};
use elicitation_core::{
    Category, EngineError, EvidenceEvent, EvidenceRef, EvidenceSourceType, Object,
    RemediationEvent, RemediationMove, RequirementMeta,
};

#[test]
fn unknown_session_errors_with_stable_prefix() {
    let (eng, _d) = temp_engine();
    let err = eng.state("ghost").unwrap_err();
    assert!(matches!(err, EngineError::SessionNotFound(_)));
    assert!(err.to_string().starts_with("SESSION_NOT_FOUND:"));
}

#[test]
fn bad_session_id_prefix() {
    let (eng, _d) = temp_engine();
    let err = eng.open_session("../escape", None).unwrap_err();
    assert!(err.to_string().starts_with("BAD_SESSION_ID:"));
}

#[test]
fn invalid_claim_prefix() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let mut c = claim("s", "", "e:app", "p", "o"); // empty id
    c.id = String::new();
    let err = eng.add_claim("s", c, None).unwrap_err();
    assert!(err.to_string().starts_with("INVALID_CLAIM:"));
}

#[test]
fn duplicate_claim_id_prefix() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "p", "o"), None)
        .unwrap();
    let err = eng
        .add_claim("s", claim("s", "c1", "e:app", "p", "o2"), None)
        .unwrap_err();
    assert!(err.to_string().starts_with("DUPLICATE_ID:"));
}

#[test]
fn evidence_for_unknown_claim_prefix() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let err = eng
        .add_evidence(
            "s",
            EvidenceEvent {
                id: "ev1".into(),
                session_id: "s".into(),
                claim_id: "nope".into(),
                source_type: EvidenceSourceType::UserAssent,
                detail: None,
                source: EvidenceRef::new("t"),
            },
        )
        .unwrap_err();
    assert!(err.to_string().starts_with("CLAIM_NOT_FOUND:"));
}

#[test]
fn remediation_unknown_target_prefix() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let err = eng
        .remediate(
            "s",
            RemediationEvent {
                id: "r1".into(),
                session_id: "s".into(),
                r#move: RemediationMove::Retract {
                    claim: "ghost".into(),
                },
                source: EvidenceRef::new("t"),
                detail: None,
            },
        )
        .unwrap_err();
    assert!(err.to_string().starts_with("CLAIM_NOT_FOUND:"));
}

#[test]
fn reconcile_unknown_contradiction_prefix() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let err = eng
        .remediate(
            "s",
            RemediationEvent {
                id: "r1".into(),
                session_id: "s".into(),
                r#move: RemediationMove::ReconcileByEvidence {
                    contradiction: "ctr:does-not-exist".into(),
                },
                source: EvidenceRef::new("t"),
                detail: None,
            },
        )
        .unwrap_err();
    assert!(err.to_string().starts_with("CONTRADICTION_NOT_FOUND:"));
}

#[test]
fn export_groups_withstood_hypotheses_by_dimension() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let mut c = claim("s", "c1", "e:app", "purpose", "billing");
    c.dimensions = vec!["purpose".into()];
    eng.add_claim("s", c, None).unwrap();
    eng.add_evidence(
        "s",
        EvidenceEvent {
            id: "ev1".into(),
            session_id: "s".into(),
            claim_id: "c1".into(),
            source_type: EvidenceSourceType::UserAssent,
            detail: None,
            source: EvidenceRef::new("turn-7"),
        },
    )
    .unwrap();

    let doc = eng.export("s").unwrap();
    let purpose = doc
        .dimensions
        .iter()
        .find(|d| d.dimension == "purpose")
        .unwrap();
    assert_eq!(purpose.hypotheses.len(), 1);
    assert_eq!(purpose.hypotheses[0].claim_id, "c1");
    // Provenance is the *claim's* originating transcript turn (spec §3).
    assert_eq!(purpose.hypotheses[0].source_turn, "turn-1");
}

#[test]
fn export_surfaces_accepted_tensions() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "gov", "e:app", "auth", "oauth"), None)
        .unwrap();
    eng.add_claim("s", claim("s", "other", "e:app", "auth", "ldap"), None)
        .unwrap();
    eng.remediate(
        "s",
        RemediationEvent {
            id: "r1".into(),
            session_id: "s".into(),
            r#move: RemediationMove::Accept {
                target: "gov".into(),
                rationale: "architect decision".into(),
            },
            source: EvidenceRef::new("t"),
            detail: None,
        },
    )
    .unwrap();
    let doc = eng.export("s").unwrap();
    assert_eq!(doc.accepted_tensions.len(), 1);
    assert_eq!(doc.accepted_tensions[0].target, "gov");
    assert_eq!(doc.accepted_tensions[0].nonconforming_claims, vec!["other"]);
}

#[test]
fn self_supersede_is_rejected() {
    // L5: supersede{old: c1, new: c1} would disprove c1 against itself — reject it.
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    let err = eng
        .remediate(
            "s",
            RemediationEvent {
                id: "r1".into(),
                session_id: "s".into(),
                r#move: RemediationMove::Supersede {
                    old: "c1".into(),
                    new: "c1".into(),
                },
                source: EvidenceRef::new("t"),
                detail: None,
            },
        )
        .unwrap_err();
    assert!(err.to_string().starts_with("INVALID_REMEDIATION:"));
}

#[test]
fn unambiguous_check_fails_on_empty_object() {
    // L7: a requirement whose object carries no concrete value and has no
    // explicit statement is ambiguous — the `unambiguous` check must FAIL.
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let mut c = claim("s", "c1", "e:app", "export", "");
    c.category = Category::Requirement;
    c.object = Object::Value {
        value: String::new(),
    };
    let st = eng.add_claim("s", c, None).unwrap();
    let req = st.requirements.iter().find(|r| r.id == "req:c1").unwrap();
    assert!(
        req.quality_checks
            .iter()
            .any(|q| q.name == "unambiguous" && !q.passed),
        "an empty-object requirement with no statement must fail the unambiguous check"
    );
}

#[test]
fn unambiguous_check_passes_on_concrete_object() {
    // L7: a concrete object value makes the requirement unambiguous.
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let mut c = claim("s", "c1", "e:app", "export", "csv");
    c.category = Category::Requirement;
    let st = eng.add_claim("s", c, None).unwrap();
    let req = st.requirements.iter().find(|r| r.id == "req:c1").unwrap();
    assert!(
        req.quality_checks
            .iter()
            .any(|q| q.name == "unambiguous" && q.passed),
        "a concrete-object requirement must pass the unambiguous check"
    );
}

#[test]
fn requirement_derived_with_quality_checks() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let mut c = claim("s", "c1", "e:app", "export", "csv");
    c.category = Category::Requirement;
    let meta = RequirementMeta {
        acceptance_criteria: vec!["downloads csv".into()],
        owner: Some("alice".into()),
        ..Default::default()
    };
    let st = eng.add_claim("s", c, Some(meta)).unwrap();
    let req = st.requirements.iter().find(|r| r.id == "req:c1").unwrap();
    assert!(
        req.quality_checks
            .iter()
            .any(|q| q.name == "has_owner" && q.passed)
    );
    assert!(
        req.quality_checks
            .iter()
            .any(|q| q.name == "has_acceptance" && q.passed)
    );
}
