//! Async dispatch for the four advisory Jev assist MCP tools. Every handler is
//! read-only over the kernel: it reads a projection, asks the injected
//! [`Judge`], and returns a proposal the caller may append itself. No handler
//! ever calls an `Engine` mutation.
//!
//! Question/option limits are enforced while building the request, before
//! [`Judge::ask`] is ever called.

use std::collections::{BTreeMap, BTreeSet};

use eliciter_core::{ClaimWithStanding, Engine, HypothesisStanding, InterviewState, Object};
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::jev::{
    JevAnswer, JevQuestion, JevReply, Judge, MAX_CHOICE_OPTIONS, MAX_QUESTIONS_PER_CALL,
};
use super::{AssistUnavailable, UnavailableReason};
use crate::server::{
    TOOL_ASSIST_CANONICALIZE, TOOL_ASSIST_CLASSIFY_REPLY, TOOL_ASSIST_CONTRADICTIONS,
    TOOL_ASSIST_PHRASE_QUESTION, engine_error_to_mcp,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyReplyArgs {
    session_id: String,
    #[serde(default)]
    claim_id: Option<String>,
    reply: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalizeArgs {
    session_id: String,
    subject: String,
    predicate: String,
    #[serde(default)]
    object: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionOnlyArgs {
    session_id: String,
}

/// The decoded outcome of one `Choice` question.
struct Answered {
    choice: String,
    probabilities: BTreeMap<String, f64>,
    confidence: f64,
}

/// Dispatch one assist tool call. Returns `ASSIST_UNAVAILABLE` when no judge is
/// configured and `BAD_*`-style MCP errors for malformed arguments.
pub async fn call(
    engine: &Engine,
    judge: Option<&dyn Judge>,
    unavailable: Option<&AssistUnavailable>,
    tool: &str,
    args: Value,
) -> Result<Value, McpError> {
    let judge = match judge {
        Some(judge) => judge,
        None => {
            let error = unavailable.cloned().unwrap_or_else(|| {
                AssistUnavailable::new(
                    UnavailableReason::NoKey,
                    "assist judge is not configured; set ELICITATION_ASSIST and \
                         TYPESAFE_JEV_API_KEY or OPENROUTER_API_KEY",
                )
            });
            return Err(error.to_mcp());
        }
    };
    match tool {
        TOOL_ASSIST_CLASSIFY_REPLY => classify_reply(engine, judge, args).await,
        TOOL_ASSIST_CANONICALIZE => canonicalize(engine, judge, args).await,
        TOOL_ASSIST_CONTRADICTIONS => contradictions(engine, judge, args).await,
        TOOL_ASSIST_PHRASE_QUESTION => phrase_question(engine, judge, args).await,
        other => Err(McpError::invalid_params(
            format!("unknown assist tool '{other}'"),
            None,
        )),
    }
}

// ── assist.classify_reply ──

async fn classify_reply(
    engine: &Engine,
    judge: &dyn Judge,
    args: Value,
) -> Result<Value, McpError> {
    let parsed: ClassifyReplyArgs = parse(args)?;
    let state = state_for(engine, &parsed.session_id)?;
    if let Some(claim_id) = &parsed.claim_id
        && !state.claims.iter().any(|cw| &cw.claim.id == claim_id)
    {
        return Err(McpError::invalid_params(
            format!("CLAIM_NOT_FOUND: no claim '{claim_id}'"),
            None,
        ));
    }
    let target = parsed
        .claim_id
        .as_ref()
        .and_then(|id| state.claims.iter().find(|cw| &cw.claim.id == id));
    let instructions = classify_instructions(&parsed.reply, target);
    let answered = ask_one(
        judge,
        session_state(&state),
        "classification",
        instructions.clone(),
        classification_criteria(),
    )
    .await
    .map_err(|error| error.to_mcp())?;

    Ok(json!({
        "session_id": parsed.session_id,
        "classification": answered.choice,
        "probabilities": answered.probabilities,
        "confidence": answered.confidence,
        "question": instructions,
        "append": classify_append(&parsed, &answered.choice),
        "advisory": true,
    }))
}

fn classify_instructions(reply: &str, target: Option<&ClaimWithStanding>) -> String {
    let about = match target {
        Some(cw) => format!(
            "the target claim '{}' ({} {}).",
            cw.claim.id, cw.claim.subject.id, cw.claim.predicate
        ),
        None => "the ongoing interview.".to_string(),
    };
    format!(
        "A user replied: \"{reply}\". Classify the reply's intent toward {about} \
         Choose exactly one token: assent, refute, new_claim or off_topic."
    )
}

fn classification_criteria() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "assent".to_string(),
            "The reply clearly agrees with or confirms the claim.".to_string(),
        ),
        (
            "refute".to_string(),
            "The reply clearly denies or contradicts the claim.".to_string(),
        ),
        (
            "new_claim".to_string(),
            "The reply introduces a distinct new claim, requirement, risk or decision.".to_string(),
        ),
        (
            "off_topic".to_string(),
            "The reply is unrelated to the target claim.".to_string(),
        ),
    ])
}

fn classify_append(args: &ClassifyReplyArgs, classification: &str) -> Value {
    match classification {
        "assent" => args
            .claim_id
            .as_ref()
            .map(|claim_id| evidence_append(&args.session_id, claim_id, "user_assent", &args.reply))
            .unwrap_or(Value::Null),
        "refute" => args
            .claim_id
            .as_ref()
            .map(|claim_id| {
                evidence_append(&args.session_id, claim_id, "user_refutation", &args.reply)
            })
            .unwrap_or(Value::Null),
        "new_claim" => new_claim_append(&args.session_id, &args.reply),
        _ => Value::Null,
    }
}

fn evidence_append(session_id: &str, claim_id: &str, source_type: &str, reply: &str) -> Value {
    let id = proposal_id(
        "evidence",
        &format!("{session_id}:{claim_id}:{source_type}:{reply}"),
    );
    json!({
        "variant": "add_evidence",
        "session_id": session_id,
        "evidence": {
            "id": id,
            "session_id": session_id,
            "claim_id": claim_id,
            "source_type": source_type,
            "detail": reply,
            "source": { "turn_id": "assist:classify_reply" }
        },
        "advisory": true
    })
}

fn new_claim_append(session_id: &str, reply: &str) -> Value {
    let id = proposal_id("claim", &format!("{session_id}:{reply}"));
    json!({
        "variant": "add_claim",
        "session_id": session_id,
        "claim": {
            "id": id,
            "session_id": session_id,
            "subject": { "id": "entity:unspecified" },
            "predicate": "unspecified",
            "object": { "kind": "value", "value": reply },
            "polarity": "positive",
            "category": "context",
            "modality": "assumption",
            "dimensions": [],
            "source": { "turn_id": "assist:classify_reply" }
        },
        "advisory": true
    })
}

// ── assist.canonicalize ──

async fn canonicalize(engine: &Engine, judge: &dyn Judge, args: Value) -> Result<Value, McpError> {
    let parsed: CanonicalizeArgs = parse(args)?;
    let state = state_for(engine, &parsed.session_id)?;

    let entity_criteria = identity_criteria(&state.registry.entity_ids, "entity id");
    let predicate_criteria = identity_criteria(&state.registry.predicate_tokens, "predicate token");

    let subject_instructions = format!(
        "Match the subject '{}' to an existing registry entity id, or 'new' if none fits.",
        parsed.subject
    );
    let predicate_instructions = format!(
        "Match the predicate '{}' to an existing registry predicate token, or 'new' if none fits.",
        parsed.predicate
    );

    let mut questions = BTreeMap::new();
    questions.insert(
        "subject".to_string(),
        JevQuestion::choice(subject_instructions, entity_criteria).map_err(|e| e.to_mcp())?,
    );
    questions.insert(
        "predicate".to_string(),
        JevQuestion::choice(predicate_instructions, predicate_criteria).map_err(|e| e.to_mcp())?,
    );
    if let Some(object) = &parsed.object {
        let instructions = format!(
            "Match the object '{}' to an existing registry entity id, or 'new' if none fits.",
            object
        );
        questions.insert(
            "object".to_string(),
            JevQuestion::choice(
                instructions,
                identity_criteria(&state.registry.entity_ids, "entity id"),
            )
            .map_err(|e| e.to_mcp())?,
        );
    }
    ensure_question_budget(&questions)?;

    let sent = questions.clone();
    let reply = judge
        .ask(session_state(&state), questions)
        .await
        .map_err(|e| e.to_mcp())?;

    let mut matches = Vec::new();
    for field in ["subject", "predicate", "object"] {
        if !sent.contains_key(field) {
            continue;
        }
        let answered = choice_from(&reply, field).map_err(|e| e.to_mcp())?;
        matches.push(json!({
            "field": field,
            "match": answered.choice,
            "probabilities": answered.probabilities,
            "confidence": answered.confidence,
            "question": question_text(&sent, field),
        }));
    }

    Ok(json!({
        "session_id": parsed.session_id,
        "matches": matches,
        "advisory": true,
    }))
}

fn identity_criteria(existing: &[String], kind: &str) -> BTreeMap<String, String> {
    let mut criteria = BTreeMap::new();
    let limit = MAX_CHOICE_OPTIONS.saturating_sub(1);
    for id in existing.iter().take(limit) {
        criteria.insert(id.clone(), format!("Existing {kind} '{id}'."));
    }
    criteria.insert(
        "new".to_string(),
        format!("No existing {kind} matches; mint a new one."),
    );
    criteria
}

// ── assist.contradictions ──

async fn contradictions(
    engine: &Engine,
    judge: &dyn Judge,
    args: Value,
) -> Result<Value, McpError> {
    let parsed: SessionOnlyArgs = parse(args)?;
    let state = state_for(engine, &parsed.session_id)?;
    let pairs = candidate_pairs(&state);

    let mut questions = BTreeMap::new();
    for (index, (left, right)) in pairs.iter().enumerate() {
        let instructions = format!(
            "Do claims '{left}' and '{right}' contradict each other in meaning, even though the \
             structural detectors did not flag them?"
        );
        let criteria = BTreeMap::from([
            (
                "contradicts".to_string(),
                "They assert incompatible meanings.".to_string(),
            ),
            ("compatible".to_string(), "They can both hold.".to_string()),
            (
                "unclear".to_string(),
                "Not enough information to tell.".to_string(),
            ),
        ]);
        questions.insert(
            format!("pair:{index}"),
            JevQuestion::choice(instructions, criteria).map_err(|e| e.to_mcp())?,
        );
    }
    ensure_question_budget(&questions)?;

    let mut candidates = Vec::new();
    if !questions.is_empty() {
        let sent = questions.clone();
        let reply = judge
            .ask(session_state(&state), questions)
            .await
            .map_err(|e| e.to_mcp())?;
        for (index, (left, right)) in pairs.iter().enumerate() {
            let id = format!("pair:{index}");
            let answered = choice_from(&reply, &id).map_err(|e| e.to_mcp())?;
            if answered.choice == "contradicts" {
                candidates.push(json!({
                    "claims": [left, right],
                    "classification": answered.choice,
                    "probabilities": answered.probabilities,
                    "confidence": answered.confidence,
                    "question": question_text(&sent, &id),
                    "confirm_with": "remediate",
                }));
            }
        }
    }

    Ok(json!({
        "session_id": parsed.session_id,
        "candidates": candidates,
        "advisory": true,
    }))
}

/// Pairs of live claims the structural detectors did not already flag, capped
/// at Jev's per-call question limit.
fn candidate_pairs(state: &InterviewState) -> Vec<(String, String)> {
    let structural: BTreeSet<Vec<String>> = state
        .contradictions
        .iter()
        .map(|contradiction| {
            let mut ids = contradiction.claims.clone();
            ids.sort();
            ids
        })
        .collect();
    let live: Vec<&str> = state
        .claims
        .iter()
        .filter(|cw| cw.standing != HypothesisStanding::Disproven)
        .map(|cw| cw.claim.id.as_str())
        .collect();

    let mut pairs = Vec::new();
    'outer: for (index, left) in live.iter().enumerate() {
        for right in live.iter().skip(index + 1) {
            let mut key = vec![(*left).to_string(), (*right).to_string()];
            key.sort();
            if structural.contains(&key) {
                continue;
            }
            pairs.push((key[0].clone(), key[1].clone()));
            if pairs.len() >= MAX_QUESTIONS_PER_CALL {
                break 'outer;
            }
        }
    }
    pairs
}

// ── assist.phrase_question ──

async fn phrase_question(
    engine: &Engine,
    judge: &dyn Judge,
    args: Value,
) -> Result<Value, McpError> {
    let parsed: SessionOnlyArgs = parse(args)?;
    let state = state_for(engine, &parsed.session_id)?;
    let suggested = eliciter_core::next_question(&state.signals)
        .and_then(|signal| signal.suggested_question.clone());

    let Some(suggested) = suggested else {
        return Ok(json!({
            "session_id": parsed.session_id,
            "suggested_question": Value::Null,
            "rephrased": Value::Null,
            "probabilities": {},
            "confidence": Value::Null,
            "question": Value::Null,
            "advisory": true,
        }));
    };

    let candidates = phrasing_candidates(&suggested);
    let criteria: BTreeMap<String, String> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            (
                candidate.clone(),
                format!("Candidate wording {}.", index + 1),
            )
        })
        .collect();
    let instructions = format!(
        "Choose the most natural, polite way to ask this discovery question: \"{suggested}\""
    );
    let mut questions = BTreeMap::new();
    questions.insert(
        "phrasing".to_string(),
        JevQuestion::choice(instructions, criteria).map_err(|e| e.to_mcp())?,
    );
    ensure_question_budget(&questions)?;

    let sent = questions.clone();
    let reply = judge
        .ask(session_state(&state), questions)
        .await
        .map_err(|e| e.to_mcp())?;
    let answered = choice_from(&reply, "phrasing").map_err(|e| e.to_mcp())?;

    Ok(json!({
        "session_id": parsed.session_id,
        "suggested_question": suggested,
        "rephrased": answered.choice,
        "probabilities": answered.probabilities,
        "confidence": answered.confidence,
        "question": question_text(&sent, "phrasing"),
        "advisory": true,
    }))
}

/// Candidate wordings: the original plus a few polite framings. Distinct and
/// never empty, so the `Choice` option rule holds.
fn phrasing_candidates(original: &str) -> Vec<String> {
    let trimmed = original.trim();
    let base = trimmed.trim_end_matches('?').trim();
    let mut candidates = vec![trimmed.to_string()];
    for prefix in [
        "Could you help me understand",
        "To make sure I have this right",
        "I would like to confirm",
    ] {
        let candidate = format!("{prefix}: {base}?");
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates.retain(|candidate| !candidate.is_empty());
    if candidates.is_empty() {
        candidates.push("Can you tell me more?".to_string());
    }
    candidates
}

// ── shared helpers ──

fn parse<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, McpError> {
    serde_json::from_value(args)
        .map_err(|error| McpError::invalid_params(format!("invalid arguments: {error}"), None))
}

fn state_for(engine: &Engine, session_id: &str) -> Result<InterviewState, McpError> {
    engine.state(session_id).map_err(engine_error_to_mcp)
}

/// A bounded, credential-free view of the session sent to the judge.
fn session_state(state: &InterviewState) -> Value {
    let claims: Vec<Value> = state
        .claims
        .iter()
        .map(|cw| {
            json!({
                "id": cw.claim.id,
                "subject": cw.claim.subject.id,
                "predicate": cw.claim.predicate,
                "object": object_repr(&cw.claim.object),
                "polarity": cw.claim.polarity,
                "category": cw.claim.category,
                "standing": cw.standing,
            })
        })
        .collect();
    json!({
        "sessionId": state.session_id,
        "claims": claims,
        "contradictions": state
            .contradictions
            .iter()
            .map(|contradiction| json!({
                "id": contradiction.id,
                "claims": contradiction.claims,
            }))
            .collect::<Vec<_>>(),
        "registry": {
            "entityIds": state.registry.entity_ids,
            "predicateTokens": state.registry.predicate_tokens,
        },
    })
}

fn object_repr(object: &Object) -> Value {
    match object {
        Object::Entity(entity) => json!({ "kind": "entity", "id": entity.id }),
        Object::Value { value } => json!({ "kind": "value", "value": value }),
    }
}

async fn ask_one(
    judge: &dyn Judge,
    state: Value,
    id: &str,
    instructions: String,
    criteria: BTreeMap<String, String>,
) -> Result<Answered, AssistUnavailable> {
    let question = JevQuestion::choice(instructions, criteria)?;
    let mut questions = BTreeMap::new();
    questions.insert(id.to_string(), question);
    let reply = judge.ask(state, questions).await?;
    choice_from(&reply, id)
}

fn choice_from(reply: &JevReply, id: &str) -> Result<Answered, AssistUnavailable> {
    match reply.answers.get(id) {
        Some(JevAnswer::Choice {
            choice,
            probabilities,
            confidence,
        }) => Ok(Answered {
            choice: choice.clone(),
            probabilities: probabilities.clone(),
            confidence: *confidence,
        }),
        _ => Err(AssistUnavailable::new(
            UnavailableReason::BadResponse,
            format!("judge did not return a choice answer for '{id}'"),
        )),
    }
}

fn question_text(questions: &BTreeMap<String, JevQuestion>, id: &str) -> String {
    match questions.get(id) {
        Some(JevQuestion::Noul { instructions, .. })
        | Some(JevQuestion::Choice { instructions, .. })
        | Some(JevQuestion::Score { instructions, .. }) => instructions.clone(),
        None => String::new(),
    }
}

/// Reject an over-budget request before the judge is ever called.
fn ensure_question_budget(questions: &BTreeMap<String, JevQuestion>) -> Result<(), McpError> {
    if questions.len() > MAX_QUESTIONS_PER_CALL {
        return Err(AssistUnavailable::new(
            UnavailableReason::BadConfig,
            format!(
                "at most {MAX_QUESTIONS_PER_CALL} questions per call, got {}",
                questions.len()
            ),
        )
        .to_mcp());
    }
    Ok(())
}

fn proposal_id(kind: &str, seed: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(seed.as_bytes());
    let mut suffix = String::new();
    for byte in digest.iter().take(8) {
        suffix.push_str(&format!("{byte:02x}"));
    }
    format!("assist:{kind}:{suffix}")
}
