/// Contract-test parity: runs the same poka-yoke scenarios against BOTH
/// `MemorySessionStore` AND `SqliteSessionStore` (in-memory SQLite), so the
/// two implementations cannot diverge.
///
/// Also covers:
/// - atomic-persist (write → drop → reload → equal)
/// - recovery quarantines a deliberately-corrupted row
#[cfg(test)]
mod contract_tests {
    use crate::session::{
        machine::{append_questions, confirm, request_confirm},
        recovery::RecoveryEngine,
        registry::SessionRegistry,
        schema::{NonEmptyPrompt, Question, QuestionKind},
        sqlite_store::SqliteSessionStore,
        state::{SessionState, SessionStatus},
        store::{MemorySessionStore, SessionStore, StoreError},
    };

    // ── helpers ───────────────────────────────────────────────────────────────

    fn open_q(id: &str) -> Question {
        Question {
            id: id.to_string(),
            kind: QuestionKind::OpenText,
            prompt: NonEmptyPrompt::new("What is your goal?").unwrap(),
            suggestions: vec![],
            allow_other: false,
        }
    }

    fn closed_state() -> SessionState {
        let mut s = SessionState::new();
        let qs = vec![open_q("q1")];
        append_questions(&mut s, qs).unwrap();
        request_confirm(&mut s).unwrap();
        confirm(&mut s, true).unwrap();
        assert_eq!(s.status, SessionStatus::Closed);
        s
    }

    fn awaiting_confirm_state() -> SessionState {
        let mut s = SessionState::new();
        let qs = vec![open_q("q1")];
        append_questions(&mut s, qs).unwrap();
        request_confirm(&mut s).unwrap();
        assert_eq!(s.status, SessionStatus::AwaitingConfirm);
        s
    }

    // ── parameterized runner ─────────────────────────────────────────────────

    fn run_all_contracts<S: SessionStore>(store: S) {
        contract_close_guard(&store);
        contract_batch_open_text(&store);
        contract_token_single_use(&store);
        contract_revision_cap(&store);
        contract_queue_cap(&store);
        contract_delete_non_closed(&store);
        contract_atomic_persist(&store);
    }

    // ── poka-yoke 1: close-guard ──────────────────────────────────────────────
    fn contract_close_guard<S: SessionStore>(store: &S) {
        let state = SessionState::new(); // Collecting
        store.save("cg_sess", state).unwrap();
        let loaded = store.load("cg_sess").unwrap();
        // confirm(Yes) from Collecting must fail.
        let mut s = loaded;
        let err = confirm(&mut s, true).unwrap_err();
        assert!(
            matches!(err, crate::session::machine::SmError::InvalidTransition(_)),
            "close_guard: expected InvalidTransition"
        );
    }

    // ── poka-yoke 2: batch must contain >=1 OpenText ──────────────────────────
    fn contract_batch_open_text<S: SessionStore>(store: &S) {
        let state = SessionState::new();
        store.save("bot_sess", state).unwrap();
        let mut s = store.load("bot_sess").unwrap();
        let only_single_choice = vec![Question {
            id: "sc".to_string(),
            kind: QuestionKind::SingleChoice,
            prompt: NonEmptyPrompt::new("Pick").unwrap(),
            suggestions: vec!["A".to_string()],
            allow_other: false,
        }];
        let err = append_questions(&mut s, only_single_choice).unwrap_err();
        assert!(
            matches!(err, crate::session::machine::SmError::BatchMissingOpenText),
            "batch_open_text: expected BatchMissingOpenText"
        );
    }

    // ── poka-yoke 3: token single-use ─────────────────────────────────────────
    fn contract_token_single_use<S: SessionStore>(store: &S) {
        use crate::session::machine::append_answer;
        use crate::session::state::{Answer, SingleUseToken};

        let mut state = SessionState::new();
        let tok = SingleUseToken::new();
        let raw = tok.value().to_string();
        state.token = Some(tok);
        store.save("tu_sess", state).unwrap();

        let mut s = store.load("tu_sess").unwrap();
        let a = Answer {
            question_id: "q1".to_string(),
            value: "v".to_string(),
        };
        let _ = append_answer(&mut s, a.clone(), &raw);
        assert!(s.token.is_none(), "token must be consumed");

        let err = append_answer(&mut s, a, &raw).unwrap_err();
        assert!(
            matches!(
                err,
                crate::session::machine::SmError::TokenAlreadyUsed
                    | crate::session::machine::SmError::TokenMismatch
            ),
            "token_single_use: expected TokenAlreadyUsed/TokenMismatch"
        );
    }

    // ── poka-yoke 4: revision cap ─────────────────────────────────────────────
    fn contract_revision_cap<S: SessionStore>(store: &S) {
        let mut s = SessionState::new();
        let qs = vec![open_q("q1")];
        append_questions(&mut s, qs).unwrap();
        store.save("rc_sess", s).unwrap();

        let mut s = store.load("rc_sess").unwrap();
        let cap = crate::session::machine::REVISION_CAP;
        for _ in 0..cap {
            request_confirm(&mut s).unwrap();
            let _ = confirm(&mut s, false);
        }
        request_confirm(&mut s).unwrap();
        let err = confirm(&mut s, false).unwrap_err();
        assert!(
            matches!(err, crate::session::machine::SmError::RevisionCapExceeded),
            "revision_cap: expected RevisionCapExceeded"
        );
    }

    // ── poka-yoke 5: queue depth cap ─────────────────────────────────────────
    fn contract_queue_cap<S: SessionStore>(store: &S) {
        let mut s = SessionState::new();
        store.save("qc_sess", s.clone()).unwrap();
        let mut s2 = store.load("qc_sess").unwrap();
        let cap = crate::session::machine::QUEUE_DEPTH_CAP;
        let batch: Vec<Question> = (0..cap).map(|i| open_q(&format!("q{i}"))).collect();
        append_questions(&mut s2, batch).unwrap();
        let extra = vec![open_q("overflow")];
        let err = append_questions(&mut s2, extra).unwrap_err();
        assert!(
            matches!(err, crate::session::machine::SmError::QueueDepthExceeded),
            "queue_cap: expected QueueDepthExceeded"
        );
        let _ = s; // silence unused warning
    }

    // ── poka-yoke 6: delete refuses non-Closed ────────────────────────────────
    fn contract_delete_non_closed<S: SessionStore>(store: &S) {
        let state = SessionState::new(); // Collecting
        store.save("dnc_sess", state).unwrap();
        let err = store.delete("dnc_sess").unwrap_err();
        assert!(
            matches!(err, StoreError::SessionNotClosed),
            "delete_non_closed: expected SessionNotClosed, got {err:?}"
        );

        // But deleting a Closed session must succeed.
        let closed = closed_state();
        store.save("dnc_closed", closed).unwrap();
        store.delete("dnc_closed").unwrap();
    }

    // ── atomic persist: write → reload → equal ────────────────────────────────
    fn contract_atomic_persist<S: SessionStore>(store: &S) {
        let mut original = SessionState::new();
        let qs = vec![open_q("ap_q1")];
        append_questions(&mut original, qs).unwrap();
        store.save("ap_sess", original.clone()).unwrap();

        let reloaded = store.load("ap_sess").unwrap();
        // Compare the fields we care about.
        assert_eq!(
            reloaded.status, original.status,
            "atomic_persist: status mismatch"
        );
        assert_eq!(
            reloaded.queue.len(),
            original.queue.len(),
            "atomic_persist: queue length mismatch"
        );
        assert_eq!(
            reloaded.ledger, original.ledger,
            "atomic_persist: ledger mismatch"
        );
        assert_eq!(
            reloaded.revision_count, original.revision_count,
            "atomic_persist: revision_count mismatch"
        );
    }

    // ── test entry points ─────────────────────────────────────────────────────

    #[test]
    fn memory_store_contracts() {
        let store = MemorySessionStore::new();
        run_all_contracts(store);
    }

    #[test]
    fn sqlite_store_contracts() {
        let store = SqliteSessionStore::open_in_memory().unwrap();
        run_all_contracts(store);
    }

    // ── sqlite-specific: atomic persist across separate open calls ────────────
    #[test]
    fn sqlite_atomic_persist_file() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "elicitation_test_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));

        // Write.
        {
            let store = SqliteSessionStore::open(&path).unwrap();
            let mut s = SessionState::new();
            append_questions(&mut s, vec![open_q("q_file")]).unwrap();
            store.save("file_sess", s).unwrap();
        }
        // Drop + reload.
        {
            let store = SqliteSessionStore::open(&path).unwrap();
            let s = store.load("file_sess").unwrap();
            assert_eq!(s.queue.len(), 1);
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── recovery: quarantines a corrupted row ─────────────────────────────────
    // We simulate a "corrupted" session by directly saving a state whose
    // invariants are broken (AwaitingConfirm but no SummaryConfirm question).
    #[test]
    fn recovery_quarantines_invalid_awaiting_confirm() {
        let store = MemorySessionStore::new();

        // Valid session.
        let valid = awaiting_confirm_state();
        store.save("valid_sess", valid).unwrap();

        // Invalid session: AwaitingConfirm status but queue has been emptied,
        // simulating corruption.
        let mut invalid = SessionState::new();
        invalid.status = SessionStatus::AwaitingConfirm;
        // queue is empty — no SummaryConfirm question present.
        store.save("invalid_sess", invalid).unwrap();

        let report = RecoveryEngine::recover(&store);

        assert!(
            report.recovered.contains(&"valid_sess".to_string()),
            "valid session must be recovered"
        );
        let quarantined_ids: Vec<&str> = report
            .quarantined
            .iter()
            .map(|(id, _)| id.as_str())
            .collect();
        assert!(
            quarantined_ids.contains(&"invalid_sess"),
            "invalid session must be quarantined; report = {report:?}"
        );
    }

    // ── recovery: sqlite variant ──────────────────────────────────────────────
    #[test]
    fn recovery_sqlite_quarantines_invalid() {
        let store = SqliteSessionStore::open_in_memory().unwrap();

        let valid = awaiting_confirm_state();
        store.save("valid_s", valid).unwrap();

        let mut invalid = SessionState::new();
        invalid.status = SessionStatus::AwaitingConfirm;
        store.save("invalid_s", invalid).unwrap();

        let report = RecoveryEngine::recover(&store);
        assert!(report.recovered.contains(&"valid_s".to_string()));
        let quarantined_ids: Vec<&str> = report
            .quarantined
            .iter()
            .map(|(id, _)| id.as_str())
            .collect();
        assert!(quarantined_ids.contains(&"invalid_s"));
    }

    // ── registry: get_locked + commit ─────────────────────────────────────────
    #[test]
    fn registry_get_locked_and_commit() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemorySessionStore::new();
            let state = SessionState::new();
            store.save("reg_sess", state).unwrap();

            let registry = SessionRegistry::new(store);

            // Acquire, mutate, commit, then DROP the guard before re-locking.
            {
                let mut guard = registry.get_locked("reg_sess").await.unwrap();
                let qs = vec![open_q("rq1")];
                append_questions(guard.state_mut(), qs).unwrap();
                // Commit persists the state to the backing store.
                guard.commit().unwrap();
                // Guard is dropped here, releasing the mutex.
            }

            // Now we can re-lock without deadlock.
            let guard2 = registry.get_locked("reg_sess").await.unwrap();
            assert_eq!(
                guard2.state().queue.len(),
                1,
                "committed state must be readable via second lock"
            );
        });
    }

    // ── registry: not-found for missing session ───────────────────────────────
    #[test]
    fn registry_not_found_for_missing_session() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemorySessionStore::new();
            let registry = SessionRegistry::new(store);
            let err = registry.get_locked("nonexistent").await.unwrap_err();
            assert!(
                matches!(err, crate::session::registry::RegistryError::NotFound(_)),
                "expected NotFound, got {err:?}"
            );
        });
    }
}
