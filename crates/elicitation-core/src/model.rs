//! Typed data model for the elicitation kernel (spec §3).
//!
//! Everything here is plain data with `serde` derives. The model is the
//! vocabulary of the append-only event log; standing and contradictions are
//! *computed* (see [`crate::projection`] / [`crate::detect`]), never stored.

use serde::{Deserialize, Serialize};

/// A caller-assigned, stable identity for an entity in the interview graph
/// (spec §2). The kernel does **no** semantics over these — detectors match
/// `id` by exact equality only. `label` is human-facing and never compared.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityRef {
    /// Caller-assigned stable id, e.g. `entity:auth-service`.
    pub id: String,
    /// Optional human-readable label; informational, never matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl EntityRef {
    /// Construct a bare entity reference with no label.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: None,
        }
    }
}

/// The object side of a claim: either another entity or a literal value
/// (spec §3, `object: EntityRef | Value`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Object {
    /// Reference to another entity by stable id.
    Entity(EntityRef),
    /// A literal scalar value (string-canonicalized by the caller).
    Value { value: String },
}

/// Provenance pointer — every claim/evidence/remediation points back to a
/// transcript turn (spec §3, `EvidenceRef { turn_id }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRef {
    /// Caller-supplied transcript turn id.
    pub turn_id: String,
}

impl EvidenceRef {
    /// Construct from a turn id.
    pub fn new(turn_id: impl Into<String>) -> Self {
        Self {
            turn_id: turn_id.into(),
        }
    }
}

/// Sign of a claim — whether it asserts or denies the predicate (spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    Positive,
    Negative,
}

/// Coarse classification of a claim (spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Requirement,
    Constraint,
    Risk,
    Context,
    Decision,
}

impl Category {
    /// True for **intent** categories (`requirement | constraint | decision`):
    /// claims that express what the operator *wants the system to be*, not facts
    /// about the world. For these, an `app_research_refutation` (the code does
    /// not conform) IS the finding — the operator asserting the requirement does
    /// not make the code meet it, so user assent must NOT suppress the
    /// spec-vs-reality signal.
    ///
    /// `context | risk` are fact-like: there the user-outranks-app rule holds
    /// (spec §1 non-goal 6) and a user verdict overrides app research.
    pub fn is_intent(self) -> bool {
        matches!(
            self,
            Category::Requirement | Category::Constraint | Category::Decision
        )
    }
}

/// Epistemic modality of a claim (spec §3) — orthogonal to [`Category`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Fact,
    Goal,
    Constraint,
    Assumption,
    Decision,
    Risk,
    Preference,
}

/// A caller-supplied coverage tag (spec §3, `dimensions: [DimensionTag]`).
/// Compared by exact string equality against the session coverage schema.
pub type DimensionTag = String;

/// An optional scope window for a claim (spec §3, `time_scope?`). Used by the
/// `qualify` remediation move to let two otherwise-contradicting claims coexist
/// under different conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeScope {
    /// Free-form caller-supplied scope token (e.g. `v2`, `peak-load`).
    pub condition: String,
}

/// A **hypothesis** (spec §3). Never an asserted fact: standing is computed by
/// folding the evidence/remediation log, never stored on the claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub session_id: String,
    pub subject: EntityRef,
    /// Predicate token from a caller-controlled vocabulary (e.g. `supports`).
    pub predicate: String,
    pub object: Object,
    pub polarity: Polarity,
    pub category: Category,
    pub modality: Modality,
    #[serde(default)]
    pub dimensions: Vec<DimensionTag>,
    pub source: EvidenceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_scope: Option<TimeScope>,
    /// Optional qualifying condition added by the `qualify` move. Distinct from
    /// `time_scope`: a condition is a generic guard ("under SSO"), a time_scope
    /// is temporal. Either dissolves an intra-spec contradiction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// The computed discrete standing of a hypothesis (spec §3). **Never stored.**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStanding {
    /// No corroborating or refuting evidence yet.
    Open,
    /// Live refutation/contradiction against it that has not been remediated.
    Challenged,
    /// Corroborated and not currently challenged — survived falsification.
    Withstood,
    /// Refuted/superseded/retracted — falsified.
    Disproven,
}

impl HypothesisStanding {
    /// Rank for the readiness "withstood or better" comparison (spec §6.1).
    /// Higher is better; `withstood` is the bar.
    pub fn rank(self) -> u8 {
        match self {
            HypothesisStanding::Disproven => 0,
            HypothesisStanding::Challenged => 1,
            HypothesisStanding::Open => 2,
            HypothesisStanding::Withstood => 3,
        }
    }

    /// True iff this standing satisfies the coverage bar (`withstood`+).
    pub fn is_withstood_or_better(self) -> bool {
        self.rank() >= HypothesisStanding::Withstood.rank()
    }
}

/// Where a piece of evidence came from (spec §3). User-sourced evidence always
/// outranks app-research evidence (spec §1, non-goal 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSourceType {
    /// The human confirmed the hypothesis.
    UserAssent,
    /// The human refuted the hypothesis.
    UserRefutation,
    /// Independent corroboration (another claim/turn agrees).
    Corroboration,
    /// Caller researched the app and found it supports the hypothesis.
    AppResearchSupport,
    /// Caller researched the app and found it refutes the hypothesis.
    AppResearchRefutation,
}

impl EvidenceSourceType {
    /// True if this evidence supports (corroborates) the hypothesis.
    pub fn is_support(self) -> bool {
        matches!(
            self,
            EvidenceSourceType::UserAssent
                | EvidenceSourceType::Corroboration
                | EvidenceSourceType::AppResearchSupport
        )
    }

    /// True if this evidence refutes the hypothesis.
    pub fn is_refutation(self) -> bool {
        matches!(
            self,
            EvidenceSourceType::UserRefutation | EvidenceSourceType::AppResearchRefutation
        )
    }

    /// True if this evidence came directly from the human operator. User facts
    /// always outrank external findings (spec §1).
    pub fn is_user_sourced(self) -> bool {
        matches!(
            self,
            EvidenceSourceType::UserAssent | EvidenceSourceType::UserRefutation
        )
    }
}

/// Severity of a detected contradiction (spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
}

/// The kind of structurally-detected contradiction (spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContradictionKind {
    /// Claim-vs-claim within the spec.
    IntraSpec,
    /// Claim vs an `app_research_refutation` evidence event (reality).
    SpecVsReality,
}

/// A computed contradiction — itself a first-class, non-stored projection
/// (spec §3 / §4 `notify`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contradiction {
    pub id: String,
    pub kind: ContradictionKind,
    pub claims: Vec<String>,
    pub severity: Severity,
    pub explanation: String,
    pub suggested_question: String,
}

/// A remediation move (spec §4). A transition *driver*, not terminal state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "move", rename_all = "snake_case")]
pub enum RemediationMove {
    /// `new` wins; `old` → `disproven`.
    Supersede { old: String, new: String },
    /// Withdraw a hypothesis.
    Retract { claim: String },
    /// Add scope so two claims hold under different conditions.
    Qualify {
        claim: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time_scope: Option<TimeScope>,
    },
    /// The "refine the system" path — caller will submit `app_research_support`;
    /// the contradiction clears on rerun. Records the intent + which
    /// contradiction is being reconciled.
    ReconcileByEvidence { contradiction: String },
    /// Take a stance: elevate `target` to *governing*. Every claim contradicting
    /// it becomes `challenged` with a conformance remediate signal. Readiness
    /// stays blocked until the graph conforms (spec §4 / §6.3).
    Accept { target: String, rationale: String },
}

/// A remediation event in the log (spec §3, `RemediationEvent`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationEvent {
    pub id: String,
    pub session_id: String,
    #[serde(flatten)]
    pub r#move: RemediationMove,
    pub source: EvidenceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A quality check on a derived requirement (spec §3 `RequirementQualityCheck`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementQualityCheck {
    /// Check name, e.g. `has_acceptance`, `has_owner`, `testable`.
    pub name: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Priority of a requirement (spec §3, MoSCoW subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Must,
    Should,
    Could,
}

/// A derived requirement candidate (spec §3). Derived from claims, not stored
/// raw — but the caller may enrich a claim's requirement metadata via the
/// `add_claim` payload (acceptance criteria, owner, priority).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementCandidate {
    pub id: String,
    pub statement: String,
    pub subject: EntityRef,
    pub capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default)]
    pub source_claims: Vec<String>,
    #[serde(default)]
    pub quality_checks: Vec<RequirementQualityCheck>,
}

/// Optional requirement enrichment a caller attaches when adding a claim of
/// `category: requirement`. Kept on the claim's add event so requirement
/// candidates can be derived deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RequirementMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}
