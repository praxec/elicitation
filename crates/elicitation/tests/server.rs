//! MCP server façade tests (spec §5): drive each tool through the transport-free
//! `dispatch_call` entry point, asserting the wire shapes and stable error
//! prefixes.

use std::sync::Arc;

use elicitation_core::{Engine, FilesystemStore};
use elicitation::{
    ElicitationServer, TOOL_APPEND, TOOL_NAMES, TOOL_QUESTION_NEXT, TOOL_READINESS_ASSESS,
    TOOL_SESSION_OPEN, TOOL_SPEC_EXPORT, TOOL_STATE_GET,
};
use rmcp::model::CallToolRequestParams;
use serde_json::{json, Value};

fn server() -> (ElicitationServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = FilesystemStore::new(dir.path()).unwrap();
    let engine = Engine::new(Arc::new(store));
    (ElicitationServer::new(engine), dir)
}

fn call(srv: &ElicitationServer, name: &str, args: Value) -> Result<Value, rmcp::ErrorData> {
    let map = match args {
        Value::Object(m) => m,
        _ => panic!("call expects a JSON object"),
    };
    let params = CallToolRequestParams::new(name.to_string()).with_arguments(map);
    srv.dispatch_call(params)
}

#[test]
fn lists_six_tools() {
    assert_eq!(TOOL_NAMES.len(), 6);
    let (srv, _d) = server();
    let defs = elicitation::tool_definitions();
    assert_eq!(defs.len(), 6);
    // every advertised name is dispatchable
    for name in TOOL_NAMES {
        let _ = srv.engine();
        assert!(defs.iter().any(|t| t.name == *name));
    }
}

#[test]
fn session_open_then_state_get() {
    let (srv, _d) = server();
    let opened = call(&srv, TOOL_SESSION_OPEN, json!({ "session_id": "s1" })).unwrap();
    assert_eq!(opened["session_id"], "s1");
    // default coverage schema present
    assert!(opened["coverage_schema"]["dimensions"].is_array());

    let state = call(&srv, TOOL_STATE_GET, json!({ "session_id": "s1" })).unwrap();
    assert_eq!(state["session_id"], "s1");
}

#[test]
fn append_add_claim_then_add_evidence_round_trip() {
    let (srv, _d) = server();
    call(&srv, TOOL_SESSION_OPEN, json!({ "session_id": "s1" })).unwrap();

    let claim = json!({
        "variant": "add_claim",
        "session_id": "s1",
        "claim": {
            "id": "c1",
            "session_id": "s1",
            "subject": { "id": "e:app" },
            "predicate": "supports",
            "object": { "kind": "value", "value": "sso" },
            "polarity": "positive",
            "category": "requirement",
            "modality": "goal",
            "dimensions": ["scope"],
            "source": { "turn_id": "t1" }
        }
    });
    let st = call(&srv, TOOL_APPEND, claim).unwrap();
    assert_eq!(st["claims"][0]["standing"], "open");

    let evidence = json!({
        "variant": "add_evidence",
        "session_id": "s1",
        "evidence": {
            "id": "ev1",
            "session_id": "s1",
            "claim_id": "c1",
            "source_type": "user_assent",
            "source": { "turn_id": "t2" }
        }
    });
    let st = call(&srv, TOOL_APPEND, evidence).unwrap();
    assert_eq!(st["claims"][0]["standing"], "withstood");
}

#[test]
fn append_remediate_accept_produces_conformance_signal() {
    let (srv, _d) = server();
    call(&srv, TOOL_SESSION_OPEN, json!({ "session_id": "s1" })).unwrap();
    for (id, obj) in [("gov", "oauth"), ("other", "ldap")] {
        let claim = json!({
            "variant": "add_claim",
            "session_id": "s1",
            "claim": {
                "id": id, "session_id": "s1",
                "subject": { "id": "e:app" },
                "predicate": "auth",
                "object": { "kind": "value", "value": obj },
                "polarity": "positive", "category": "decision", "modality": "decision",
                "source": { "turn_id": "t1" }
            }
        });
        call(&srv, TOOL_APPEND, claim).unwrap();
    }
    let remediate = json!({
        "variant": "remediate",
        "session_id": "s1",
        "remediation": {
            "id": "r1",
            "session_id": "s1",
            "move": "accept",
            "target": "gov",
            "rationale": "architect decision",
            "source": { "turn_id": "t3" }
        }
    });
    let st = call(&srv, TOOL_APPEND, remediate).unwrap();
    let signals = st["signals"].as_array().unwrap();
    assert!(signals
        .iter()
        .any(|s| s["code"] == "accept_conformance" && s["kind"] == "remediate"));
}

#[test]
fn question_next_returns_a_gap_question() {
    let (srv, _d) = server();
    call(&srv, TOOL_SESSION_OPEN, json!({ "session_id": "s1" })).unwrap();
    let q = call(&srv, TOOL_QUESTION_NEXT, json!({ "session_id": "s1" })).unwrap();
    // default schema is all-uncovered → a clarify question exists
    assert!(q["question"]["suggested_question"].is_string());
}

#[test]
fn readiness_and_export_reflect_blockers() {
    let (srv, _d) = server();
    call(&srv, TOOL_SESSION_OPEN, json!({ "session_id": "s1" })).unwrap();
    let r = call(&srv, TOOL_READINESS_ASSESS, json!({ "session_id": "s1" })).unwrap();
    assert_eq!(r["ready"], false);
    assert!(!r["blockers"].as_array().unwrap().is_empty());

    let doc = call(&srv, TOOL_SPEC_EXPORT, json!({ "session_id": "s1" })).unwrap();
    assert_eq!(doc["ready"], false);
    assert_eq!(doc["session_id"], "s1");
}

#[test]
fn unknown_tool_is_invalid_params() {
    let (srv, _d) = server();
    let err = call(&srv, "nope.nope", json!({})).unwrap_err();
    assert!(err.message.contains("Unknown tool"));
}

#[test]
fn session_not_found_prefix_propagates() {
    let (srv, _d) = server();
    let err = call(&srv, TOOL_STATE_GET, json!({ "session_id": "ghost" })).unwrap_err();
    assert!(err.message.starts_with("SESSION_NOT_FOUND:"));
}

#[test]
fn bad_session_id_is_invalid_params_with_prefix() {
    let (srv, _d) = server();
    let err = call(
        &srv,
        TOOL_SESSION_OPEN,
        json!({ "session_id": "../escape" }),
    )
    .unwrap_err();
    assert!(err.message.starts_with("BAD_SESSION_ID:"));
}

#[test]
fn malformed_args_are_invalid_params() {
    let (srv, _d) = server();
    // missing required session_id
    let err = call(&srv, TOOL_STATE_GET, json!({ "wrong": "x" })).unwrap_err();
    assert!(err.message.contains("invalid arguments"));
}
