//! Concurrency safety for the per-session read-modify-write critical section
//! (spec §4 / §8: concurrent sessions are fully isolated; same-session writes
//! must be serialized so the dup/existence checks are race-free).
//!
//! Each engine write command (`open_session`, `add_claim`, `add_evidence`,
//! `remediate`) is a TOCTOU read-modify-write: it replays the log, checks
//! duplicates / existence, then appends. Without a per-session lock two
//! concurrent operations on the SAME session can both pass the check then both
//! append, so a claim is lost (two distinct ids collapse via interleaving) or a
//! dup slips past the check. These tests fan out `std::thread` workers against a
//! shared `Arc<Engine>` and assert the invariants hold deterministically.

mod common;

use std::sync::{Arc, Barrier};
use std::thread;

use common::{claim, temp_engine};
use elicitation_core::EngineError;

/// N threads each add a claim with a DISTINCT id to the SAME session. All N
/// must persist — none lost, no panic, replay shows exactly N.
#[test]
fn concurrent_distinct_claims_all_persist() {
    const N: usize = 16;
    let (engine, _dir) = temp_engine();
    let engine = Arc::new(engine);
    let session = "concurrent-distinct";
    engine.open_session(session, None).expect("open");

    let barrier = Arc::new(Barrier::new(N));
    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let engine = Arc::clone(&engine);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            // Maximize the race window: every thread releases at once.
            barrier.wait();
            let c = claim(session, &format!("c{i}"), "svc", "uses", "db");
            engine
                .add_claim(session, c, None)
                .expect("add_claim must succeed");
        }));
    }
    for h in handles {
        h.join().expect("worker thread panicked");
    }

    let state = engine.state(session).expect("state");
    assert_eq!(
        state.claims.len(),
        N,
        "all {N} distinct claims must be present in the projection"
    );
}

/// N threads race to add a claim with the SAME id. Exactly one wins; every other
/// thread gets `DuplicateId` (the dup check must be race-free).
#[test]
fn concurrent_same_id_yields_one_success_rest_duplicate() {
    const N: usize = 16;
    let (engine, _dir) = temp_engine();
    let engine = Arc::new(engine);
    let session = "concurrent-same-id";
    engine.open_session(session, None).expect("open");

    let barrier = Arc::new(Barrier::new(N));
    let mut handles = Vec::with_capacity(N);
    for _ in 0..N {
        let engine = Arc::clone(&engine);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let c = claim(session, "dup", "svc", "uses", "db");
            engine.add_claim(session, c, None)
        }));
    }

    let mut successes = 0usize;
    let mut duplicates = 0usize;
    for h in handles {
        match h.join().expect("worker thread panicked") {
            Ok(_) => successes += 1,
            Err(EngineError::DuplicateId(_)) => duplicates += 1,
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    assert_eq!(successes, 1, "exactly one writer must win the id");
    assert_eq!(duplicates, N - 1, "every other writer must see DuplicateId");

    let state = engine.state(session).expect("state");
    assert_eq!(state.claims.len(), 1, "exactly one claim must be persisted");
}

/// Different sessions stay concurrent + isolated: interleaving writes across two
/// sessions must not cross-contaminate (spec §4/§8 isolation).
#[test]
fn distinct_sessions_isolated() {
    const N: usize = 8;
    let (engine, _dir) = temp_engine();
    let engine = Arc::new(engine);
    let sessions = ["iso-a", "iso-b"];
    for s in sessions {
        engine.open_session(s, None).expect("open");
    }

    let barrier = Arc::new(Barrier::new(N * sessions.len()));
    let mut handles = Vec::new();
    for s in sessions {
        for i in 0..N {
            let engine = Arc::clone(&engine);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                let c = claim(s, &format!("{s}-c{i}"), "svc", "uses", "db");
                engine.add_claim(s, c, None).expect("add_claim");
            }));
        }
    }
    for h in handles {
        h.join().expect("worker thread panicked");
    }

    for s in sessions {
        let state = engine.state(s).expect("state");
        assert_eq!(
            state.claims.len(),
            N,
            "session {s} must have exactly its own {N} claims"
        );
    }
}
