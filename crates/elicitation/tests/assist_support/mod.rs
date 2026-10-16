//! Shared in-process fakes for the assist behavioral tests. No network.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use elicitation::ElicitationServer;
use elicitation::assist::jev::{JevAnswer, JevQuestion, JevReply, Judge, async_trait};
use elicitation::assist::{AssistUnavailable, UnavailableReason, Usage};
use eliciter_core::{Engine, FilesystemStore};
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};

type Recorded = (Value, BTreeMap<String, JevQuestion>);
type Responder = Box<dyn Fn(&str, &JevQuestion) -> JevAnswer + Send + Sync>;

/// A deterministic [`Judge`] that records every call and answers from an
/// injected closure (or fails every call with a fixed reason).
pub struct FakeJudge {
    responder: Option<Responder>,
    failure: Option<UnavailableReason>,
    calls: Mutex<Vec<Recorded>>,
}

impl FakeJudge {
    pub fn answering(
        responder: impl Fn(&str, &JevQuestion) -> JevAnswer + Send + Sync + 'static,
    ) -> Self {
        Self {
            responder: Some(Box::new(responder)),
            failure: None,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn failing(reason: UnavailableReason) -> Self {
        Self {
            responder: None,
            failure: Some(reason),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Number of calls observed.
    pub fn calls(&self) -> usize {
        self.calls.lock().expect("fake calls mutex").len()
    }

    /// The question map sent on call `index`.
    pub fn questions(&self, index: usize) -> BTreeMap<String, JevQuestion> {
        self.calls.lock().expect("fake calls mutex")[index]
            .1
            .clone()
    }

    /// Instructions sent for question `id` on call `index`.
    pub fn instructions(&self, index: usize, id: &str) -> String {
        match &self.questions(index)[id] {
            JevQuestion::Noul { instructions, .. }
            | JevQuestion::Choice { instructions, .. }
            | JevQuestion::Score { instructions, .. } => instructions.clone(),
        }
    }
}

#[async_trait]
impl Judge for FakeJudge {
    async fn ask(
        &self,
        state: Value,
        questions: BTreeMap<String, JevQuestion>,
    ) -> Result<JevReply, AssistUnavailable> {
        self.calls
            .lock()
            .expect("fake calls mutex")
            .push((state, questions.clone()));
        if let Some(reason) = self.failure {
            return Err(AssistUnavailable::new(reason, "fake judge failure"));
        }
        let responder = self
            .responder
            .as_ref()
            .expect("answering fake must have a responder");
        let answers = questions
            .iter()
            .map(|(id, question)| (id.clone(), responder(id, question)))
            .collect();
        Ok(JevReply {
            model: "fake/jev".to_string(),
            answers,
            usage: Usage::default(),
        })
    }
}

/// A server with an injected fake judge and a throwaway state dir. The judge
/// `Arc` is returned so tests can inspect what was sent.
pub fn server_with_judge(
    judge: FakeJudge,
) -> (ElicitationServer, Arc<FakeJudge>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemStore::new(dir.path()).expect("store");
    let engine = Engine::new(Arc::new(store));
    let judge = Arc::new(judge);
    (
        ElicitationServer::new(engine).with_judge(judge.clone()),
        judge,
        dir,
    )
}

/// A server with no assist backend configured (the no-key path).
pub fn server_without_judge() -> (ElicitationServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemStore::new(dir.path()).expect("store");
    let engine = Engine::new(Arc::new(store));
    (ElicitationServer::new(engine), dir)
}

/// Drive one tool through the async MCP dispatch entry point.
pub async fn call(
    srv: &ElicitationServer,
    name: &str,
    args: Value,
) -> Result<Value, rmcp::ErrorData> {
    let map = match args {
        Value::Object(m) => m,
        _ => panic!("call expects a JSON object"),
    };
    let params = CallToolRequestParams::new(name.to_string()).with_arguments(map);
    srv.dispatch_call_async(params).await
}

/// Open a session and add one non-contradicting claim, returning nothing.
pub async fn open_with_claim(srv: &ElicitationServer, session_id: &str, claim_id: &str) {
    call(
        srv,
        elicitation::TOOL_SESSION_OPEN,
        json!({ "session_id": session_id }),
    )
    .await
    .expect("session.open");
    call(
        srv,
        elicitation::TOOL_APPEND,
        json!({
            "variant": "add_claim",
            "session_id": session_id,
            "claim": {
                "id": claim_id,
                "session_id": session_id,
                "subject": { "id": "entity:app" },
                "predicate": "supports",
                "object": { "kind": "value", "value": "sso" },
                "polarity": "positive",
                "category": "requirement",
                "modality": "goal",
                "source": { "turn_id": "t1" }
            }
        }),
    )
    .await
    .expect("add_claim");
}
