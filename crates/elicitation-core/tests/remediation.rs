//! Remediation-move table tests (spec §4 / §9): one per move, including the
//! `accept` conformance-propagation behavior.

mod common;

use common::{claim, temp_engine};
use elicitation_core::{
    EvidenceEvent, EvidenceRef, EvidenceSourceType, HypothesisStanding, Polarity, RemediationEvent,
    RemediationMove, SignalKind,
};

fn standing_of(state: &elicitation_core::InterviewState, id: &str) -> HypothesisStanding {
    state
        .claims
        .iter()
        .find(|c| c.claim.id == id)
        .unwrap_or_else(|| panic!("claim {id} not found"))
        .standing
}

fn rem(id: &str, mv: RemediationMove) -> RemediationEvent {
    RemediationEvent {
        id: id.to_string(),
        session_id: "s".to_string(),
        r#move: mv,
        source: EvidenceRef::new("turn-r"),
        detail: None,
    }
}

#[test]
fn supersede_disproves_old_claim() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "old", "e:app", "supports", "sso"), None)
        .unwrap();
    eng.add_claim("s", claim("s", "new", "e:app", "supports", "saml"), None)
        .unwrap();
    let st = eng
        .remediate(
            "s",
            rem(
                "r1",
                RemediationMove::Supersede {
                    old: "old".into(),
                    new: "new".into(),
                },
            ),
        )
        .unwrap();
    assert_eq!(standing_of(&st, "old"), HypothesisStanding::Disproven);
}

#[test]
fn retract_disproves_claim() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    let st = eng
        .remediate(
            "s",
            rem("r1", RemediationMove::Retract { claim: "c1".into() }),
        )
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Disproven);
}

#[test]
fn qualify_clears_contradiction_and_revives_both() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim(
        "s",
        claim("s", "c1", "e:app", "max_latency_ms", "100"),
        None,
    )
    .unwrap();
    eng.add_claim(
        "s",
        claim("s", "c2", "e:app", "max_latency_ms", "250"),
        None,
    )
    .unwrap();
    eng.remediate(
        "s",
        rem(
            "r1",
            RemediationMove::Qualify {
                claim: "c1".into(),
                condition: Some("peak".into()),
                time_scope: None,
            },
        ),
    )
    .unwrap();
    let st = eng
        .remediate(
            "s",
            rem(
                "r2",
                RemediationMove::Qualify {
                    claim: "c2".into(),
                    condition: Some("offpeak".into()),
                    time_scope: None,
                },
            ),
        )
        .unwrap();
    assert!(st.contradictions.is_empty());
    // neither is challenged any more
    assert_ne!(standing_of(&st, "c1"), HypothesisStanding::Challenged);
    assert_ne!(standing_of(&st, "c2"), HypothesisStanding::Challenged);
}

#[test]
fn reconcile_by_evidence_then_app_support_clears_reality_contradiction() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    let st = eng
        .add_evidence(
            "s",
            EvidenceEvent {
                id: "ev1".into(),
                session_id: "s".into(),
                claim_id: "c1".into(),
                source_type: EvidenceSourceType::AppResearchRefutation,
                detail: None,
                source: EvidenceRef::new("t"),
            },
        )
        .unwrap();
    let ctr_id = st.contradictions[0].id.clone();

    // reconcile_by_evidence records the intent (validates the contradiction exists)
    eng.remediate(
        "s",
        rem(
            "r1",
            RemediationMove::ReconcileByEvidence {
                contradiction: ctr_id,
            },
        ),
    )
    .unwrap();

    // caller changed the app and submits app_research_support (M2). A later
    // app-research verdict supersedes the earlier refutation (last-research-wins,
    // mirroring last-user-verdict-wins), so the reality contradiction CLEARS.
    let st = eng
        .add_evidence(
            "s",
            EvidenceEvent {
                id: "ev2".into(),
                session_id: "s".into(),
                claim_id: "c1".into(),
                source_type: EvidenceSourceType::AppResearchSupport,
                detail: None,
                source: EvidenceRef::new("t"),
            },
        )
        .unwrap();
    assert!(
        st.contradictions.is_empty(),
        "a later app_research_support must clear the spec_vs_reality contradiction"
    );
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Withstood);
}

#[test]
fn user_refutation_outranks_later_app_support() {
    // M2 guard: a later app-research verdict must NOT override a user verdict.
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    eng.add_evidence(
        "s",
        EvidenceEvent {
            id: "ev1".into(),
            session_id: "s".into(),
            claim_id: "c1".into(),
            source_type: EvidenceSourceType::UserRefutation,
            detail: None,
            source: EvidenceRef::new("t"),
        },
    )
    .unwrap();
    // even a later app support cannot revive a user-refuted claim
    let st = eng
        .add_evidence(
            "s",
            EvidenceEvent {
                id: "ev2".into(),
                session_id: "s".into(),
                claim_id: "c1".into(),
                source_type: EvidenceSourceType::AppResearchSupport,
                detail: None,
                source: EvidenceRef::new("t"),
            },
        )
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Disproven);
}

#[test]
fn accept_target_disproven_drops_conformance_pressure() {
    // M3: when the accept-stance target is retracted, it is no longer governing,
    // so the conformance pressure (and CONFORMANCE blocker) on the other claim
    // must drop. The other claim is then judged on its own merits.
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "gov", "e:app", "auth", "oauth"), None)
        .unwrap();
    eng.add_claim("s", claim("s", "other", "e:app", "auth", "ldap"), None)
        .unwrap();
    eng.remediate(
        "s",
        rem(
            "r1",
            RemediationMove::Accept {
                target: "gov".into(),
                rationale: "chosen".into(),
            },
        ),
    )
    .unwrap();
    // now disprove the governing target
    let st = eng
        .remediate(
            "s",
            rem(
                "r2",
                RemediationMove::Retract {
                    claim: "gov".into(),
                },
            ),
        )
        .unwrap();

    assert_eq!(standing_of(&st, "gov"), HypothesisStanding::Disproven);
    // no governing stance survives, so no conformance pressure and no blocker
    assert!(
        st.accepted_stances.is_empty(),
        "a disproven accept target is no longer a governing stance"
    );
    assert!(
        !st.readiness
            .blockers
            .iter()
            .any(|b| b.starts_with("CONFORMANCE")),
        "conformance pressure must drop when the governing target is disproven"
    );
    // but the live intra-spec contradiction between gov(dead) and other is gone
    // too (dead hypotheses generate no tension); other stands on its own.
    assert_ne!(standing_of(&st, "other"), HypothesisStanding::Challenged);
}

#[test]
fn accept_propagates_conformance_challenge() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    // governing decision, with user assent so it has supporting evidence.
    // Tagged on a coverage dimension so it appears in spec.export sections.
    let mut gov = claim("s", "gov", "e:app", "auth", "oauth");
    gov.dimensions = vec!["purpose".into()];
    eng.add_claim("s", gov, None).unwrap();
    eng.add_evidence(
        "s",
        EvidenceEvent {
            id: "ev-gov".into(),
            session_id: "s".into(),
            claim_id: "gov".into(),
            source_type: EvidenceSourceType::UserAssent,
            detail: None,
            source: EvidenceRef::new("t"),
        },
    )
    .unwrap();
    // a contradicting position
    eng.add_claim("s", claim("s", "other", "e:app", "auth", "ldap"), None)
        .unwrap();

    let st = eng
        .remediate(
            "s",
            rem(
                "r1",
                RemediationMove::Accept {
                    target: "gov".into(),
                    rationale: "chosen by architect".into(),
                },
            ),
        )
        .unwrap();

    // H1: the governing target must be ELEVATED, not challenged. It survived.
    assert_eq!(
        standing_of(&st, "gov"),
        HypothesisStanding::Withstood,
        "accept(gov) must elevate gov, not challenge it"
    );
    // the contradicting claim is now challenged (conformance pressure)
    assert_eq!(standing_of(&st, "other"), HypothesisStanding::Challenged);
    // a remediate signal of code accept_conformance is present
    assert!(
        st.signals
            .iter()
            .any(|s| s.kind == SignalKind::Remediate && s.code == "accept_conformance")
    );
    // the stance is recorded with the nonconforming claim
    assert_eq!(st.accepted_stances.len(), 1);
    assert_eq!(st.accepted_stances[0].nonconforming_claims, vec!["other"]);
    // readiness is blocked by conformance
    assert!(
        st.readiness
            .blockers
            .iter()
            .any(|b| b.starts_with("CONFORMANCE"))
    );

    // H1: spec.export must INCLUDE gov as a governing (withstood) hypothesis.
    // gov is tagged on a dimension so it lands in a dimension section.
    let doc = eng.export("s").unwrap();
    let gov_exported = doc
        .dimensions
        .iter()
        .flat_map(|d| &d.hypotheses)
        .any(|h| h.claim_id == "gov");
    assert!(
        gov_exported,
        "the governing accepted target must appear in spec.export, not be dropped as challenged"
    );
}

#[test]
fn accept_conformance_clears_when_nonconformer_retracted() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "gov", "e:app", "auth", "oauth"), None)
        .unwrap();
    let mut other = claim("s", "other", "e:app", "auth", "ldap");
    other.polarity = Polarity::Positive;
    eng.add_claim("s", other, None).unwrap();
    eng.remediate(
        "s",
        rem(
            "r1",
            RemediationMove::Accept {
                target: "gov".into(),
                rationale: "chosen".into(),
            },
        ),
    )
    .unwrap();
    // bring into conformance by retracting the nonconformer
    let st = eng
        .remediate(
            "s",
            rem(
                "r2",
                RemediationMove::Retract {
                    claim: "other".into(),
                },
            ),
        )
        .unwrap();
    assert!(st.accepted_stances[0].nonconforming_claims.is_empty());
    assert!(
        !st.readiness
            .blockers
            .iter()
            .any(|b| b.starts_with("CONFORMANCE"))
    );
}
