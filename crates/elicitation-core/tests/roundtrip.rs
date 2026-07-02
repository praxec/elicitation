//! Determinism round-trip (spec §9): append events → replay → identical
//! `InterviewState`, and a fresh engine over the same on-disk log reconstructs
//! the same projection (restart survival).

mod common;

use std::sync::Arc;

use common::{claim, temp_engine};
use elicitation_core::{
    Engine, EvidenceEvent, EvidenceRef, EvidenceSourceType, FilesystemStore, RemediationEvent,
    RemediationMove,
};

fn drive(eng: &Engine) {
    eng.open_session("s", None).unwrap();
    eng.add_claim("s", claim("s", "c1", "e:app", "supports", "sso"), None)
        .unwrap();
    eng.add_claim(
        "s",
        claim("s", "c2", "e:app", "max_latency_ms", "100"),
        None,
    )
    .unwrap();
    eng.add_evidence(
        "s",
        EvidenceEvent {
            id: "ev1".into(),
            session_id: "s".into(),
            claim_id: "c1".into(),
            source_type: EvidenceSourceType::UserAssent,
            detail: None,
            source: EvidenceRef::new("t"),
        },
    )
    .unwrap();
    eng.remediate(
        "s",
        RemediationEvent {
            id: "r1".into(),
            session_id: "s".into(),
            r#move: RemediationMove::Retract { claim: "c2".into() },
            source: EvidenceRef::new("t"),
            detail: None,
        },
    )
    .unwrap();
}

#[test]
fn replay_is_idempotent() {
    let (eng, _d) = temp_engine();
    drive(&eng);
    let a = eng.state("s").unwrap();
    let b = eng.state("s").unwrap();
    assert_eq!(a, b);
}

#[test]
fn fresh_engine_reconstructs_identical_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = FilesystemStore::new(dir.path()).unwrap();
    let eng = Engine::new(Arc::new(store));
    drive(&eng);
    let before = eng.state("s").unwrap();

    // Simulate restart: brand-new engine over the same on-disk log.
    let store2 = FilesystemStore::new(dir.path()).unwrap();
    let eng2 = Engine::new(Arc::new(store2));
    let after = eng2.state("s").unwrap();

    assert_eq!(before, after);
}

#[test]
fn json_serialization_round_trips_state() {
    let (eng, _d) = temp_engine();
    drive(&eng);
    let st = eng.state("s").unwrap();
    let json = serde_json::to_string(&st).unwrap();
    let back: elicitation_core::InterviewState = serde_json::from_str(&json).unwrap();
    assert_eq!(st, back);
}
