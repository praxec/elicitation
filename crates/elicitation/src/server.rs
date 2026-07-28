//! MCP tool surface for the elicitation kernel (spec §5).
//!
//! [`ElicitationServer`] wraps an [`Engine`] and exposes the six command/query
//! tools over MCP. The server is **thin**: every handler parses args, calls one
//! `Engine` method, and serializes the result. All structure/state lives in the
//! kernel.
//!
//! # Tool surface (spec §5)
//!
//! | Tool               | Kind    | Engine method                  |
//! |--------------------|---------|--------------------------------|
//! | `session.open`     | command | [`Engine::open_session`]       |
//! | `append`           | command | `add_claim`/`add_evidence`/`remediate` |
//! | `state.get`        | query   | [`Engine::state`]              |
//! | `question.next`    | query   | [`Engine::next_question`]      |
//! | `readiness.assess` | query   | [`Engine::readiness`]          |
//! | `spec.export`      | query   | [`Engine::export`]             |
//!
//! Errors propagate the kernel's stable prefixes (`SESSION_NOT_FOUND`,
//! `INVALID_CLAIM`, `BAD_SESSION_ID`, …) verbatim in the MCP error message.

use std::borrow::Cow;
use std::sync::Arc;

use elicitation_core::{
    Claim, Engine, EngineError, EvidenceEvent, RemediationEvent, RemediationMove, RequirementMeta,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Implementation, InitializeRequestParams,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities,
    ServerInfo, Tool,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use rmcp::transport::stdio;
use rmcp::{ServerHandler, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use elicitation_core::CoverageSchema;

/// Tool names (spec §5), `noun.verb` like the rest of the workspace.
pub const TOOL_SESSION_OPEN: &str = "session.open";
pub const TOOL_APPEND: &str = "append";
pub const TOOL_STATE_GET: &str = "state.get";
pub const TOOL_QUESTION_NEXT: &str = "question.next";
pub const TOOL_READINESS_ASSESS: &str = "readiness.assess";
pub const TOOL_SPEC_EXPORT: &str = "spec.export";

/// All tool names in declaration order.
pub const TOOL_NAMES: &[&str] = &[
    TOOL_SESSION_OPEN,
    TOOL_APPEND,
    TOOL_STATE_GET,
    TOOL_QUESTION_NEXT,
    TOOL_READINESS_ASSESS,
    TOOL_SPEC_EXPORT,
];

// --- wire arg structs ------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionOpenArgs {
    session_id: String,
    #[serde(default)]
    coverage_schema: Option<CoverageSchema>,
}

/// The `append` command is a tagged union over its three variants (spec §5).
#[derive(Debug, Deserialize)]
#[serde(tag = "variant", rename_all = "snake_case", deny_unknown_fields)]
enum AppendArgs {
    AddClaim {
        session_id: String,
        // Boxed to balance the enum's variant sizes (clippy `large_enum_variant`).
        claim: Box<Claim>,
        #[serde(default)]
        requirement: Option<RequirementMeta>,
    },
    AddEvidence {
        session_id: String,
        evidence: EvidenceEvent,
    },
    Remediate {
        session_id: String,
        remediation: RemediationInput,
    },
}

/// `remediate` payload: the move plus its envelope fields.
///
/// No `deny_unknown_fields` here: serde's `flatten` is fundamentally
/// incompatible with it (flatten consumes the remaining keys, which the deny
/// pass then rejects). The flattened [`RemediationMove`] is internally tagged on
/// `move`, so unknown keys would land there and fail enum parsing anyway.
#[derive(Debug, Deserialize)]
struct RemediationInput {
    id: String,
    session_id: String,
    #[serde(flatten)]
    r#move: RemediationMove,
    source: elicitation_core::EvidenceRef,
    #[serde(default)]
    detail: Option<String>,
}

impl From<RemediationInput> for RemediationEvent {
    fn from(i: RemediationInput) -> Self {
        RemediationEvent {
            id: i.id,
            session_id: i.session_id,
            r#move: i.r#move,
            source: i.source,
            detail: i.detail,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionIdArgs {
    session_id: String,
}

#[derive(Debug, Serialize)]
struct NextQuestionResponse {
    question: Option<elicitation_core::Signal>,
}

// --- server ----------------------------------------------------------------

/// MCP server façade over an [`Engine`]. Cheap to clone.
#[derive(Clone)]
pub struct ElicitationServer {
    engine: Engine,
    server_name: String,
    server_version: String,
}

impl ElicitationServer {
    /// Build a server backed by the supplied engine.
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            server_name: "elicitation".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Borrow the inner engine (tests drive state directly).
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Serve over stdio. Blocks until the peer disconnects.
    pub async fn serve_stdio(self) -> anyhow::Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }

    /// Transport-free dispatch entry point (tests call this directly).
    pub fn dispatch_call(&self, request: CallToolRequestParams) -> Result<Value, McpError> {
        let args: Value = request
            .arguments
            .as_ref()
            .map(|m| Value::Object(m.clone()))
            .unwrap_or_else(|| json!({}));

        match request.name.as_ref() {
            TOOL_SESSION_OPEN => self.handle_session_open(args),
            TOOL_APPEND => self.handle_append(args),
            TOOL_STATE_GET => self.handle_state_get(args),
            TOOL_QUESTION_NEXT => self.handle_question_next(args),
            TOOL_READINESS_ASSESS => self.handle_readiness(args),
            TOOL_SPEC_EXPORT => self.handle_export(args),
            other => Err(McpError::invalid_params(
                format!(
                    "Unknown tool '{other}'. Available: {}.",
                    TOOL_NAMES.join(", ")
                ),
                None,
            )),
        }
    }

    fn handle_session_open(&self, args: Value) -> Result<Value, McpError> {
        let parsed: SessionOpenArgs = parse_args(args)?;
        let state = self
            .engine
            .open_session(&parsed.session_id, parsed.coverage_schema)
            .map_err(engine_error_to_mcp)?;
        to_value(&state)
    }

    fn handle_append(&self, args: Value) -> Result<Value, McpError> {
        let parsed: AppendArgs = parse_args(args)?;
        let state = match parsed {
            AppendArgs::AddClaim {
                session_id,
                claim,
                requirement,
            } => self.engine.add_claim(&session_id, *claim, requirement),
            AppendArgs::AddEvidence {
                session_id,
                evidence,
            } => self.engine.add_evidence(&session_id, evidence),
            AppendArgs::Remediate {
                session_id,
                remediation,
            } => self.engine.remediate(&session_id, remediation.into()),
        }
        .map_err(engine_error_to_mcp)?;
        to_value(&state)
    }

    fn handle_state_get(&self, args: Value) -> Result<Value, McpError> {
        let parsed: SessionIdArgs = parse_args(args)?;
        let state = self
            .engine
            .state(&parsed.session_id)
            .map_err(engine_error_to_mcp)?;
        to_value(&state)
    }

    fn handle_question_next(&self, args: Value) -> Result<Value, McpError> {
        let parsed: SessionIdArgs = parse_args(args)?;
        let question = self
            .engine
            .next_question(&parsed.session_id)
            .map_err(engine_error_to_mcp)?;
        to_value(&NextQuestionResponse { question })
    }

    fn handle_readiness(&self, args: Value) -> Result<Value, McpError> {
        let parsed: SessionIdArgs = parse_args(args)?;
        let report = self
            .engine
            .readiness(&parsed.session_id)
            .map_err(engine_error_to_mcp)?;
        to_value(&report)
    }

    fn handle_export(&self, args: Value) -> Result<Value, McpError> {
        let parsed: SessionIdArgs = parse_args(args)?;
        let doc = self
            .engine
            .export(&parsed.session_id)
            .map_err(engine_error_to_mcp)?;
        to_value(&doc)
    }
}

// --- ServerHandler ---------------------------------------------------------

impl ServerHandler for ElicitationServer {
    fn get_info(&self) -> ServerInfo {
        let mut server_info =
            Implementation::new(self.server_name.clone(), self.server_version.clone());
        server_info.title = Some("elicitation".to_string());
        server_info.description = Some(
            "Deterministic, offline structured-discovery interview engine over MCP.".to_string(),
        );

        let mut info = InitializeResult::default();
        info.protocol_version = ProtocolVersion::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = server_info;
        info.instructions = Some(instructions().to_string());
        info
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }
        Ok(self.get_info())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tool_definitions()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.dispatch_call(request).map(CallToolResult::structured)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tool_definitions().into_iter().find(|t| t.name == name)
    }

    async fn on_initialized(&self, _context: NotificationContext<RoleServer>) {
        tracing::info!("elicitation client initialized");
    }
}

// --- helpers ---------------------------------------------------------------

fn parse_args<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, McpError> {
    serde_json::from_value(args)
        .map_err(|e| McpError::invalid_params(format!("invalid arguments: {e}"), None))
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, McpError> {
    serde_json::to_value(value)
        .map_err(|e| McpError::internal_error(format!("response serialisation failed: {e}"), None))
}

/// Map a kernel error to an MCP error, preserving the stable prefix in the
/// message. `BAD_SESSION_ID` / `INVALID_*` / `DUPLICATE_ID` are caller bugs
/// (`invalid_params`); the rest are surfaced as `internal_error`.
fn engine_error_to_mcp(err: EngineError) -> McpError {
    let msg = err.to_string();
    match err {
        EngineError::BadSessionId(_)
        | EngineError::InvalidClaim(_)
        | EngineError::InvalidEvidence(_)
        | EngineError::InvalidRemediation(_)
        | EngineError::DuplicateId(_) => McpError::invalid_params(msg, None),
        EngineError::SessionNotFound(_)
        | EngineError::ClaimNotFound(_)
        | EngineError::ContradictionNotFound(_)
        | EngineError::StoreError(_) => McpError::internal_error(msg, None),
    }
}

fn schema_object(value: Value) -> Arc<rmcp::model::JsonObject> {
    debug_assert!(value.is_object(), "schema_object expects an object literal");
    let obj = match value.as_object() {
        Some(o) => o.clone(),
        None => serde_json::Map::new(),
    };
    Arc::new(obj)
}

/// Build the six advertised tool definitions. Schemas are intentionally lenient
/// on the deep claim/evidence/remediation shapes (kernel validation is
/// authoritative and returns stable error prefixes); they document the required
/// envelope keys so an agent knows the call shape.
pub fn tool_definitions() -> Vec<Tool> {
    vec![
        Tool::new(
            Cow::Borrowed(TOOL_SESSION_OPEN),
            Cow::Borrowed(
                "Start or resume a session by session_id. Optional coverage_schema \
                 (defaults to purpose/constraints/success-criteria/scope/non-goals). \
                 Returns the current InterviewState including the entity/predicate registry.",
            ),
            schema_object(json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "coverage_schema": {
                        "type": ["object", "null"],
                        "properties": {
                            "dimensions": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "tag": { "type": "string" },
                                        "required": { "type": "boolean" }
                                    },
                                    "required": ["tag"]
                                }
                            }
                        }
                    }
                },
                "required": ["session_id"],
                "additionalProperties": false
            })),
        ),
        Tool::new(
            Cow::Borrowed(TOOL_APPEND),
            Cow::Borrowed(
                "Append one event. Tagged by `variant`: \
                 add_claim{session_id, claim, requirement?} | \
                 add_evidence{session_id, evidence} | \
                 remediate{session_id, remediation}. \
                 Returns the recomputed InterviewState with freshly-computed signals.",
            ),
            schema_object(json!({
                "type": "object",
                "properties": {
                    "variant": {
                        "type": "string",
                        "enum": ["add_claim", "add_evidence", "remediate"]
                    },
                    "session_id": { "type": "string" },
                    "claim": { "type": "object" },
                    "requirement": { "type": ["object", "null"] },
                    "evidence": { "type": "object" },
                    "remediation": { "type": "object" }
                },
                "required": ["variant", "session_id"]
            })),
        ),
        Tool::new(
            Cow::Borrowed(TOOL_STATE_GET),
            Cow::Borrowed(
                "Read-only InterviewState projection: ledger + computed standing + \
                 requirements + contradictions + signals + registry + readiness.",
            ),
            session_id_schema(),
        ),
        Tool::new(
            Cow::Borrowed(TOOL_QUESTION_NEXT),
            Cow::Borrowed(
                "The highest-leverage next question, derived deterministically from open \
                 clarify/remediate signals. Returns {question: null} when nothing needs asking.",
            ),
            session_id_schema(),
        ),
        Tool::new(
            Cow::Borrowed(TOOL_READINESS_ASSESS),
            Cow::Borrowed(
                "Compute the ReadinessReport: ready bool + per-dimension standing + \
                 blockers + gaps. Asserts 'survived falsification', never 'true'.",
            ),
            session_id_schema(),
        ),
        Tool::new(
            Cow::Borrowed(TOOL_SPEC_EXPORT),
            Cow::Borrowed(
                "Emit the aligned, plan-ready spec document: withstood+ hypotheses by \
                 dimension, requirement candidates, and a visible accepted-tensions section.",
            ),
            session_id_schema(),
        ),
    ]
}

fn session_id_schema() -> Arc<rmcp::model::JsonObject> {
    schema_object(json!({
        "type": "object",
        "properties": { "session_id": { "type": "string" } },
        "required": ["session_id"],
        "additionalProperties": false
    }))
}

fn instructions() -> &'static str {
    r#"elicitation — a deterministic, offline structured-discovery interview engine.

Claims are HYPOTHESES; standing is COMPUTED (a fold over an append-only log), never stored.
You (the caller) do all fuzzy work: turn conversation into candidate claims, voice questions,
research the app, and relay user assent/refutation. This server owns structure + state.

Tools (six; command/query split):
  session.open      — start/resume a session; optional coverage_schema; returns InterviewState
  append            — variant add_claim | add_evidence | remediate; returns state + signals
  state.get         — read-only InterviewState projection
  question.next     — highest-leverage next question (or null)
  readiness.assess  — ReadinessReport (ready iff coverage withstood + no high contradiction +
                      no open conformance + must-requirements have acceptance+owner)
  spec.export       — the aligned, plan-ready spec document

Supply CANONICAL identity: subject/object entity ids and predicate tokens are exact-matched.
Reuse the ids in the returned registry. Errors carry stable prefixes: SESSION_NOT_FOUND,
BAD_SESSION_ID, INVALID_CLAIM, INVALID_EVIDENCE, INVALID_REMEDIATION, CLAIM_NOT_FOUND,
CONTRADICTION_NOT_FOUND, DUPLICATE_ID, STORE_ERROR.

Remediation moves: supersede{old,new} | retract{claim} | qualify{claim,condition?|time_scope?}
| reconcile_by_evidence{contradiction} | accept{target,rationale}.
"#
}
