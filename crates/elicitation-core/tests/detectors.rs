//! Detector table tests (spec §9): one per contradiction kind, plus the scope
//! and severity rules.

mod common;

use common::{claim, temp_engine};
use elicitation_core::{
    Category, ContradictionKind, EvidenceEvent, EvidenceRef, EvidenceSourceType, Object, Polarity,
    Severity,
};

fn ev(session: &str, id: &str, claim_id: &str, st: EvidenceSourceType) -> EvidenceEvent {
    EvidenceEvent {
        id: id.to_string(),
        session_id: session.to_string(),
        claim_id: claim_id.to_string(),
        source_type: st,
        detail: None,
        source: EvidenceRef::new("turn-x"),
    }
}

#[test]
fn intra_spec_opposite_polarity_same_object() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    let mut neg = claim("s", "c2", "e:app", "supports", "sso");
    neg.polarity = Polarity::Negative;
    let st = eng.add_claim("s", neg, None).unwrap();

    assert_eq!(st.contradictions.len(), 1);
    let c = &st.contradictions[0];
    assert_eq!(c.kind, ContradictionKind::IntraSpec);
    assert!(c.claims.contains(&"c1".to_string()) && c.claims.contains(&"c2".to_string()));
}

#[test]
fn intra_spec_different_values_positive() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim(
        "s",
        claim("s", "c1", "e:app", "max_latency_ms", "100"),
        None,
    )
    .unwrap();
    let st = eng
        .add_claim(
            "s",
            claim("s", "c2", "e:app", "max_latency_ms", "250"),
            None,
        )
        .unwrap();
    assert_eq!(st.contradictions.len(), 1);
    assert_eq!(st.contradictions[0].kind, ContradictionKind::IntraSpec);
}

#[test]
fn no_contradiction_when_different_subject_or_predicate() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim(
        "s",
        claim("s", "c1", "e:app", "max_latency_ms", "100"),
        None,
    )
    .unwrap();
    // different subject
    eng.add_claim("s", claim("s", "c2", "e:db", "max_latency_ms", "250"), None)
        .unwrap();
    // different predicate
    let st = eng
        .add_claim(
            "s",
            claim("s", "c3", "e:app", "min_latency_ms", "250"),
            None,
        )
        .unwrap();
    assert!(st.contradictions.is_empty(), "{:?}", st.contradictions);
}

#[test]
fn qualified_scopes_dissolve_contradiction() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim(
        "s",
        claim("s", "c1", "e:app", "max_latency_ms", "100"),
        None,
    )
    .unwrap();
    let st = eng
        .add_claim(
            "s",
            claim("s", "c2", "e:app", "max_latency_ms", "250"),
            None,
        )
        .unwrap();
    assert_eq!(st.contradictions.len(), 1);

    // Qualify c1 into a distinct condition; the conflict dissolves.
    let rem = elicitation_core::RemediationEvent {
        id: "r1".into(),
        session_id: "s".into(),
        r#move: elicitation_core::RemediationMove::Qualify {
            claim: "c1".into(),
            condition: Some("peak".into()),
            time_scope: None,
        },
        source: EvidenceRef::new("turn-r"),
        detail: None,
    };
    let st = eng.remediate("s", rem).unwrap();
    // c2 still unscoped → overlapping scope → still contradicts. Qualify c2 too.
    assert_eq!(st.contradictions.len(), 1);
    let rem2 = elicitation_core::RemediationEvent {
        id: "r2".into(),
        session_id: "s".into(),
        r#move: elicitation_core::RemediationMove::Qualify {
            claim: "c2".into(),
            condition: Some("offpeak".into()),
            time_scope: None,
        },
        source: EvidenceRef::new("turn-r"),
        detail: None,
    };
    let st = eng.remediate("s", rem2).unwrap();
    assert!(st.contradictions.is_empty(), "{:?}", st.contradictions);
}

#[test]
fn spec_vs_reality_fires_on_app_research_refutation() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    let st = eng
        .add_evidence(
            "s",
            ev("s", "ev1", "c1", EvidenceSourceType::AppResearchRefutation),
        )
        .unwrap();
    assert_eq!(st.contradictions.len(), 1);
    assert_eq!(st.contradictions[0].kind, ContradictionKind::SpecVsReality);
    assert_eq!(st.contradictions[0].severity, Severity::High);
}

#[test]
fn constraint_conflict_is_high_severity() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let mut c1 = claim("s", "c1", "e:app", "max_latency_ms", "100");
    c1.category = Category::Constraint;
    eng.add_claim("s", c1, None).unwrap();
    let mut c2 = claim("s", "c2", "e:app", "max_latency_ms", "250");
    c2.category = Category::Constraint;
    let st = eng.add_claim("s", c2, None).unwrap();
    assert_eq!(st.contradictions[0].severity, Severity::High);
}

#[test]
fn context_value_conflict_is_medium_severity() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    // both Context (the default in the builder)
    eng.add_claim("s", claim("s", "c1", "e:app", "color", "blue"), None)
        .unwrap();
    let st = eng
        .add_claim("s", claim("s", "c2", "e:app", "color", "green"), None)
        .unwrap();
    assert_eq!(st.contradictions[0].severity, Severity::Medium);
}

#[test]
fn requirement_claim_app_refutation_surfaces_spec_vs_reality() {
    // E1 regression: when the operator ASSERTS a requirement/constraint and
    // app-research shows the code does NOT meet it, the divergence IS the
    // finding. A `user_assent` must NOT silence the spec-vs-reality signal for
    // INTENT-category claims (requirement | constraint | decision).
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let mut c1 = claim("s", "c1", "e:matrix", "grounded_in", "nasa-8004");
    c1.category = Category::Constraint;
    eng.add_claim("s", c1, None).unwrap();

    // User asserts the requirement holds...
    eng.add_evidence("s", ev("s", "ev1", "c1", EvidenceSourceType::UserAssent))
        .unwrap();
    // ...but app research refutes it (the code doesn't conform).
    let st = eng
        .add_evidence(
            "s",
            ev("s", "ev2", "c1", EvidenceSourceType::AppResearchRefutation),
        )
        .unwrap();

    // The intent↔code conflict must surface as a spec_vs_reality contradiction.
    assert_eq!(
        st.contradictions.len(),
        1,
        "expected one spec_vs_reality contradiction, got {:?}",
        st.contradictions
    );
    assert_eq!(st.contradictions[0].kind, ContradictionKind::SpecVsReality);

    // The claim is challenged, not withstood.
    let standing = st
        .claims
        .iter()
        .find(|c| c.claim.id == "c1")
        .unwrap()
        .standing;
    assert_eq!(
        standing,
        elicitation_core::HypothesisStanding::Challenged,
        "intent claim with app refutation must be challenged regardless of user assent"
    );

    // Readiness must NOT be ready — a real intent↔code conflict stands.
    assert!(
        !st.readiness.ready,
        "readiness must be blocked: {:?}",
        st.readiness.blockers
    );
    assert!(
        !st.readiness.blockers.is_empty(),
        "expected a readiness blocker"
    );
}

#[test]
fn context_claim_keeps_user_outranks_app() {
    // Preserve existing behavior for FACT-like categories: a `context` claim
    // with user_assent over an app refutation stays withstood (user facts
    // outrank external findings, spec §1 non-goal 6).
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    // `claim` builder defaults to Category::Context.
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    eng.add_evidence(
        "s",
        ev("s", "ev1", "c1", EvidenceSourceType::AppResearchRefutation),
    )
    .unwrap();
    let st = eng
        .add_evidence("s", ev("s", "ev2", "c1", EvidenceSourceType::UserAssent))
        .unwrap();

    assert!(
        st.contradictions.is_empty(),
        "user assent should suppress app refutation for context claims: {:?}",
        st.contradictions
    );
    let standing = st
        .claims
        .iter()
        .find(|c| c.claim.id == "c1")
        .unwrap()
        .standing;
    assert_eq!(standing, elicitation_core::HypothesisStanding::Withstood);
}

#[test]
fn entity_objects_equal_then_polarity_conflict() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let mut c1 = claim("s", "c1", "e:app", "depends_on", "x");
    c1.object = Object::Entity(elicitation_core::EntityRef::new("e:db"));
    eng.add_claim("s", c1, None).unwrap();
    let mut c2 = claim("s", "c2", "e:app", "depends_on", "x");
    c2.object = Object::Entity(elicitation_core::EntityRef::new("e:db"));
    c2.polarity = Polarity::Negative;
    let st = eng.add_claim("s", c2, None).unwrap();
    assert_eq!(st.contradictions.len(), 1);
}
