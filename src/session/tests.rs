// RED-phase test: ensures the new session core types and poka-yokes exist.
// This file will fail to compile until the implementation is in place.

#[cfg(test)]
mod poka_yoke_tests {
    use crate::session::{
        machine::{append_answer, append_questions, confirm, request_confirm, SmError},
        schema::{NonEmptyPrompt, Question, QuestionKind},
        state::{Answer, SessionState, SessionStatus, SingleUseToken},
        store::{MemorySessionStore, SessionStore, StoreError},
    };

    fn make_open_text_question(id: &str) -> Question {
        Question {
            id: id.to_string(),
            kind: QuestionKind::OpenText,
            prompt: NonEmptyPrompt::new("What is your goal?").unwrap(),
            suggestions: vec![],
            allow_other: false,
        }
    }

    fn make_single_choice_question(id: &str) -> Question {
        Question {
            id: id.to_string(),
            kind: QuestionKind::SingleChoice,
            prompt: NonEmptyPrompt::new("Pick one").unwrap(),
            suggestions: vec!["A".to_string(), "B".to_string()],
            allow_other: true,
        }
    }

    // ── poka-yoke 1: close-guard ──────────────────────────────────────────────
    // confirm(Yes) from Collecting must be rejected (InvalidTransition).
    #[test]
    fn close_guard_rejects_yes_from_collecting() {
        let mut state = SessionState::new();
        assert_eq!(state.status, SessionStatus::Collecting);
        let err = confirm(&mut state, true).unwrap_err();
        assert!(
            matches!(err, SmError::InvalidTransition(_)),
            "expected InvalidTransition, got {err:?}"
        );
        // Status must not have changed.
        assert_eq!(state.status, SessionStatus::Collecting);
    }

    // ── poka-yoke 2: batch without OpenText rejected ──────────────────────────
    #[test]
    fn batch_missing_open_text_rejected() {
        let mut state = SessionState::new();
        let questions = vec![make_single_choice_question("q1")];
        let err = append_questions(&mut state, questions).unwrap_err();
        assert!(
            matches!(err, SmError::BatchMissingOpenText),
            "expected BatchMissingOpenText, got {err:?}"
        );
    }

    // ── poka-yoke 3: SummaryConfirm in enqueue rejected ───────────────────────
    #[test]
    fn enqueue_summary_confirm_rejected() {
        let mut state = SessionState::new();
        let q = Question {
            id: "sc1".to_string(),
            kind: QuestionKind::SummaryConfirm,
            prompt: NonEmptyPrompt::new("Confirm?").unwrap(),
            suggestions: vec![],
            allow_other: false,
        };
        let err = append_questions(&mut state, vec![q]).unwrap_err();
        assert!(
            matches!(err, SmError::SummaryConfirmReserved),
            "expected SummaryConfirmReserved, got {err:?}"
        );
    }

    // ── poka-yoke 4: single-use token consumed (reuse rejected) ──────────────
    #[test]
    fn single_use_token_consumed_on_first_use() {
        let mut state = SessionState::new();
        let token = SingleUseToken::new();
        let raw = token.value().to_string();
        state.token = Some(token);

        let answer = Answer {
            question_id: "q1".to_string(),
            value: "yes".to_string(),
        };
        // First use: succeeds (ignore question-not-found or other logic for now;
        // the token path must be exercised).
        let _ = append_answer(&mut state, answer.clone(), &raw);

        // Token should now be consumed.
        assert!(
            state.token.is_none(),
            "token must be consumed after first use"
        );

        // Reuse with a new answer must fail with TokenAlreadyUsed (no token present).
        let err = append_answer(&mut state, answer, &raw).unwrap_err();
        assert!(
            matches!(err, SmError::TokenAlreadyUsed | SmError::TokenMismatch),
            "expected TokenAlreadyUsed or TokenMismatch after consumption, got {err:?}"
        );
    }

    // ── poka-yoke 5: revision cap ─────────────────────────────────────────────
    #[test]
    fn revision_cap_enforced() {
        let mut state = SessionState::new();
        // Push enough questions so request_confirm works.
        let qs = vec![make_open_text_question("q1")];
        append_questions(&mut state, qs).unwrap();

        // Drive revision_count to the cap limit by cycling
        // AwaitingConfirm → Collecting (No) until the cap fires.
        let cap = crate::session::machine::REVISION_CAP;
        for _ in 0..cap {
            request_confirm(&mut state).unwrap();
            let result = confirm(&mut state, false);
            // Should be ok until cap.
            // (The last iteration might return the error itself — check after loop.)
            let _ = result;
        }
        // At this point revision_count == cap; one more cycle should be blocked.
        request_confirm(&mut state).unwrap();
        let err = confirm(&mut state, false).unwrap_err();
        assert!(
            matches!(err, SmError::RevisionCapExceeded),
            "expected RevisionCapExceeded, got {err:?}"
        );
    }

    // ── poka-yoke 6: queue depth cap ─────────────────────────────────────────
    #[test]
    fn queue_depth_cap_enforced() {
        let mut state = SessionState::new();
        let cap = crate::session::machine::QUEUE_DEPTH_CAP;

        // Fill queue to cap with valid batches (each containing >=1 OpenText).
        let batch: Vec<Question> = (0..cap)
            .map(|i| make_open_text_question(&format!("q{i}")))
            .collect();
        append_questions(&mut state, batch).unwrap();

        // One more should exceed the cap.
        let extra = vec![make_open_text_question("overflow")];
        let err = append_questions(&mut state, extra).unwrap_err();
        assert!(
            matches!(err, SmError::QueueDepthExceeded),
            "expected QueueDepthExceeded, got {err:?}"
        );
    }

    // ── poka-yoke 7: MemorySessionStore delete refuses non-Closed ────────────
    #[test]
    fn store_delete_refuses_non_closed() {
        let store = MemorySessionStore::new();
        let state = SessionState::new(); // Collecting, not Closed
        store.save("sess1", state.clone()).unwrap();
        let err = store.delete("sess1").unwrap_err();
        assert!(
            matches!(err, StoreError::SessionNotClosed),
            "expected SessionNotClosed, got {err:?}"
        );
    }

    // ── happy-path: full confirm(Yes) flow ───────────────────────────────────
    #[test]
    fn happy_path_confirm_yes_closes_session() {
        let mut state = SessionState::new();
        let qs = vec![make_open_text_question("q1")];
        append_questions(&mut state, qs).unwrap();
        request_confirm(&mut state).unwrap();
        confirm(&mut state, true).unwrap();
        assert_eq!(state.status, SessionStatus::Closed);
    }
}
