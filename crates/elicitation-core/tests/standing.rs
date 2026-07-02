//! Computed-standing golden tests (spec §3): the same events always fold to the
//! same discrete `HypothesisStanding`.

mod common;

use common::{claim, temp_engine};
use elicitation_core::{EvidenceEvent, EvidenceRef, EvidenceSourceType, HypothesisStanding};

fn evidence(session: &str, id: &str, claim_id: &str, st: EvidenceSourceType) -> EvidenceEvent {
    EvidenceEvent {
        id: id.to_string(),
        session_id: session.to_string(),
        claim_id: claim_id.to_string(),
        source_type: st,
        detail: None,
        source: EvidenceRef::new("turn-x"),
    }
}

fn standing_of(state: &elicitation_core::InterviewState, id: &str) -> HypothesisStanding {
    state
        .claims
        .iter()
        .find(|c| c.claim.id == id)
        .unwrap_or_else(|| panic!("claim {id} not found"))
        .standing
}

#[test]
fn fresh_claim_is_open() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    let st = eng
        .add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Open);
}

#[test]
fn user_assent_makes_it_withstood() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    let st = eng
        .add_evidence(
            "s",
            evidence("s", "ev1", "c1", EvidenceSourceType::UserAssent),
        )
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Withstood);
}

#[test]
fn user_refutation_disproves_it() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    let st = eng
        .add_evidence(
            "s",
            evidence("s", "ev1", "c1", EvidenceSourceType::UserRefutation),
        )
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Disproven);
}

#[test]
fn user_reassent_after_refutation_revives_to_withstood() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    eng.add_evidence(
        "s",
        evidence("s", "ev1", "c1", EvidenceSourceType::UserRefutation),
    )
    .unwrap();
    let st = eng
        .add_evidence(
            "s",
            evidence("s", "ev2", "c1", EvidenceSourceType::UserAssent),
        )
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Withstood);
}

#[test]
fn app_research_refutation_challenges_but_user_assent_outranks() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    // App says no → challenged.
    let st = eng
        .add_evidence(
            "s",
            evidence("s", "ev1", "c1", EvidenceSourceType::AppResearchRefutation),
        )
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Challenged);

    // User overrides → user facts outrank external findings (spec §1 non-goal 6).
    let st = eng
        .add_evidence(
            "s",
            evidence("s", "ev2", "c1", EvidenceSourceType::UserAssent),
        )
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Withstood);
}

#[test]
fn corroboration_alone_withstands() {
    let (eng, _d) = temp_engine();
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    let st = eng
        .add_evidence(
            "s",
            evidence("s", "ev1", "c1", EvidenceSourceType::Corroboration),
        )
        .unwrap();
    assert_eq!(standing_of(&st, "c1"), HypothesisStanding::Withstood);
}
