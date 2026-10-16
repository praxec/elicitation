//! Behavioral tests for the four advisory Jev assist MCP tools. Each test
//! makes exactly one behavioral assertion against a public outcome and drives
//! the transport-free async dispatch entry point with an in-process fake judge
//! (no network).

#![cfg(feature = "jev")]

mod assist_support;

use assist_support::{FakeJudge, call, open_with_claim, server_with_judge, server_without_judge};
use elicitation::assist::jev::{JevAnswer, JevQuestion};
use serde_json::json;

const CLASSIFY: &str = "assist.classify_reply";
const CANONICALIZE: &str = "assist.canonicalize";
const CONTRADICTIONS: &str = "assist.contradictions";
const PHRASE: &str = "assist.phrase_question";

/// Answers every choice question with `choice`, giving it most of the mass.
fn choice_judge(choice: &'static str) -> FakeJudge {
    FakeJudge::answering(move |_id, question| match question {
        JevQuestion::Choice { criteria, .. } => {
            let probabilities = criteria
                .keys()
                .map(|key| {
                    let mass = if key == choice { 0.9 } else { 0.9 / 3.0 };
                    (key.clone(), mass)
                })
                .collect();
            JevAnswer::Choice {
                choice: choice.to_string(),
                probabilities,
                confidence: 0.9,
            }
        }
        _ => panic!("assist questions are choice questions"),
    })
}

/// A `Choice` question's option keys, in map order.
fn options(question: &JevQuestion) -> Vec<String> {
    match question {
        JevQuestion::Choice { criteria, .. } => criteria.keys().cloned().collect(),
        _ => panic!("assist questions are choice questions"),
    }
}

fn classify_args(reply: &str) -> serde_json::Value {
    json!({ "session_id": "s1", "claim_id": "c1", "reply": reply })
}

#[tokio::test]
async fn classify_reply_returns_the_refute_classification() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("refute"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(&srv, CLASSIFY, classify_args("No, that is wrong."))
        .await
        .expect("classify_reply");

    assert_eq!(result["classification"], "refute");
}

#[tokio::test]
async fn classify_reply_returns_the_refute_probability() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("refute"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(&srv, CLASSIFY, classify_args("No."))
        .await
        .expect("classify_reply");

    assert_eq!(result["probabilities"]["refute"], 0.9);
}

#[tokio::test]
async fn classify_reply_proposes_an_add_evidence_append() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("refute"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(&srv, CLASSIFY, classify_args("No."))
        .await
        .expect("classify_reply");

    assert_eq!(result["append"]["variant"], "add_evidence");
}

#[tokio::test]
async fn classify_reply_proposes_user_refutation_evidence() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("refute"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(&srv, CLASSIFY, classify_args("No."))
        .await
        .expect("classify_reply");

    assert_eq!(
        result["append"]["evidence"]["source_type"],
        "user_refutation"
    );
}

#[tokio::test]
async fn classify_reply_proposes_assent_evidence_when_the_judge_says_assent() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("assent"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(&srv, CLASSIFY, classify_args("Yes, exactly."))
        .await
        .expect("classify_reply");

    assert_eq!(result["append"]["evidence"]["source_type"], "user_assent");
}

#[tokio::test]
async fn classify_reply_reports_the_question_text_it_sent() {
    let (srv, judge, _d) = server_with_judge(choice_judge("refute"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(&srv, CLASSIFY, classify_args("No."))
        .await
        .expect("classify_reply");
    let sent = judge.instructions(0, "classification");

    assert_eq!(result["question"], sent);
}

#[tokio::test]
async fn classify_reply_proposes_a_new_claim_append_when_the_judge_says_new_claim() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("new_claim"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(&srv, CLASSIFY, classify_args("We also need audit logs."))
        .await
        .expect("classify_reply");

    assert_eq!(result["append"]["variant"], "add_claim");
}

#[tokio::test]
async fn classify_reply_without_a_judge_is_assist_unavailable() {
    let (srv, _d) = server_without_judge();
    open_with_claim(&srv, "s1", "c1").await;

    let err = call(&srv, CLASSIFY, classify_args("No."))
        .await
        .expect_err("no assistant must fail");

    assert!(err.message.contains("ASSIST_UNAVAILABLE"));
}

#[tokio::test]
async fn session_open_still_works_without_an_assist_key() {
    let (srv, _d) = server_without_judge();

    let opened = call(
        &srv,
        elicitation::TOOL_SESSION_OPEN,
        json!({ "session_id": "s1" }),
    )
    .await
    .expect("session.open");

    assert_eq!(opened["session_id"], "s1");
}

#[tokio::test]
async fn canonicalize_returns_the_entity_the_judge_selected() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("entity:app"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(
        &srv,
        CANONICALIZE,
        json!({ "session_id": "s1", "subject": "authentication service", "predicate": "supports" }),
    )
    .await
    .expect("canonicalize");
    let subject = result["matches"]
        .as_array()
        .expect("matches array")
        .iter()
        .find(|entry| entry["field"] == "subject")
        .expect("subject match");

    assert_eq!(subject["match"], "entity:app");
}

#[tokio::test]
async fn canonicalize_returns_per_option_probabilities() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("entity:app"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(
        &srv,
        CANONICALIZE,
        json!({ "session_id": "s1", "subject": "auth", "predicate": "supports" }),
    )
    .await
    .expect("canonicalize");
    let subject = result["matches"]
        .as_array()
        .expect("matches array")
        .iter()
        .find(|entry| entry["field"] == "subject")
        .expect("subject match");

    assert_eq!(subject["probabilities"]["entity:app"], 0.9);
}

#[tokio::test]
async fn canonicalize_lets_the_judge_propose_new_identity() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("new"));
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(
        &srv,
        CANONICALIZE,
        json!({ "session_id": "s1", "subject": "billing", "predicate": "supports" }),
    )
    .await
    .expect("canonicalize");
    let subject = result["matches"]
        .as_array()
        .expect("matches array")
        .iter()
        .find(|entry| entry["field"] == "subject")
        .expect("subject match");

    assert_eq!(subject["match"], "new");
}

#[tokio::test]
async fn contradictions_returns_the_meaning_level_pair_the_judge_flagged() {
    let (srv, _judge, _d) = server_with_judge(choice_judge("contradicts"));
    open_with_claim(&srv, "s1", "c1").await;
    call(
        &srv,
        elicitation::TOOL_APPEND,
        json!({
            "variant": "add_claim",
            "session_id": "s1",
            "claim": {
                "id": "c2", "session_id": "s1",
                "subject": { "id": "entity:app" },
                "predicate": "stores_pii",
                "object": { "kind": "value", "value": "yes" },
                "polarity": "positive", "category": "risk", "modality": "fact",
                "source": { "turn_id": "t2" }
            }
        }),
    )
    .await
    .expect("add_claim");

    let result = call(&srv, CONTRADICTIONS, json!({ "session_id": "s1" }))
        .await
        .expect("contradictions");
    let candidates = result["candidates"].as_array().expect("candidates array");

    assert_eq!(
        candidates
            .first()
            .and_then(|entry| entry["claims"].as_array())
            .map(Vec::len),
        Some(2)
    );
}

#[tokio::test]
async fn phrase_question_returns_the_rewording_the_judge_selected() {
    let judge = FakeJudge::answering(|_id, question| {
        let keys = options(question);
        let pick = keys.get(1).cloned().unwrap_or_else(|| keys[0].clone());
        let probabilities = keys.iter().map(|key| (key.clone(), 0.5)).collect();
        JevAnswer::Choice {
            choice: pick,
            probabilities,
            confidence: 0.5,
        }
    });
    let (srv, _judge, _d) = server_with_judge(judge);
    open_with_claim(&srv, "s1", "c1").await;

    let result = call(&srv, PHRASE, json!({ "session_id": "s1" }))
        .await
        .expect("phrase_question");

    assert!(
        result["rephrased"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "phrase_question returns a non-empty rewording"
    );
}

#[tokio::test]
async fn assist_tools_leave_the_session_log_unchanged() {
    let judge = FakeJudge::answering(|_id, question| {
        let keys = options(question);
        let probabilities = keys.iter().map(|key| (key.clone(), 0.5)).collect();
        JevAnswer::Choice {
            choice: keys[0].clone(),
            probabilities,
            confidence: 0.5,
        }
    });
    let (srv, _judge, _d) = server_with_judge(judge);
    open_with_claim(&srv, "s1", "c1").await;
    let before = call(
        &srv,
        elicitation::TOOL_STATE_GET,
        json!({ "session_id": "s1" }),
    )
    .await
    .expect("state.get before");

    let _ = call(
        &srv,
        CLASSIFY,
        json!({ "session_id": "s1", "claim_id": "c1", "reply": "yes" }),
    )
    .await;
    let _ = call(
        &srv,
        CANONICALIZE,
        json!({ "session_id": "s1", "subject": "auth", "predicate": "supports" }),
    )
    .await;
    let _ = call(&srv, CONTRADICTIONS, json!({ "session_id": "s1" })).await;
    let _ = call(&srv, PHRASE, json!({ "session_id": "s1" })).await;

    let after = call(
        &srv,
        elicitation::TOOL_STATE_GET,
        json!({ "session_id": "s1" }),
    )
    .await
    .expect("state.get after");

    assert_eq!(before, after);
}
