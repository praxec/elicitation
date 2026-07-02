# elicitation — v1 design spec

> A deterministic, offline, pure-compute MCP server for **structured discovery interviews**, modeled as an
> **alignment mechanism run by the scientific method**. The caller LLM does all fuzzy work (turning
> conversation into candidate claims, voicing questions, *and* researching the actual system); this server
> owns **structure and state management** — a typed, event-sourced claim ledger where claims are
> *hypotheses*, evidence accrues with provenance, contradictions are detected structurally, and a
> deterministic plan-readiness gate reports whether the spec has *survived falsification* (never "is true").
>
> A standalone open-source tool. It composes with any MCP client over the protocol and **vendors
> nothing** — there is no code dependency on any client; the entire surface is the MCP tool set.

## 0. Thesis — alignment, not verification

An elicitation engine is **not a verifier**. Verificationism ("a human stamps a claim `confirmed` → it is
true") is a hard gate pretending an absolute exists. There is none. Instead we use **falsification**:

- A claim is a **hypothesis**. It is never "confirmed true" — it can only *survive falsification attempts*
  or be *disproven*.
- The server maintains an **evidence ledger per hypothesis** and emits **signals** when structure says a
  hypothesis is threatened. The fuzzy work of *gathering* evidence stays with the caller — including the
  caller going to **research the actual app/codebase** to try to disprove a hypothesis.
- This makes the tool an **alignment mechanism**: classic falsification tests a hypothesis against fixed
  reality, but here *reality (the app) is itself mutable*. A contradiction between a requirement-hypothesis
  and the system has **two legal cures** — refine the hypothesis (edit the spec) **or** refine the system
  (change the app so reality matches) — then re-evidence and rerun. The tool keeps recomputing alignment
  until intent and reality agree. (The recursion is literal: it can align a spec with the very thing being
  built — including building a tool like this one with itself.)

**Division of responsibility (load-bearing):**

| Concern | Owner |
|---|---|
| Conversation → candidate claims; voicing questions; **researching the app**; relaying user assent/refutation; **driving the detect→remediate→rerun loop** | **Caller LLM** (the calling agent / MCP host) |
| **Structure** (typed claim/evidence/remediation/stance event model + registry) and **state management** (append-only log, computed discrete standing, deterministic projections + signals) | **elicitation** (deterministic) |

The server is a pure function of `(events in) → (computed standing + issues + readiness + next-question out)`.
Offline, no LLM, no network — a deterministic compute-on-demand kernel.

## 1. Scope

**In v1 (the deterministic kernel + stdio server):** session lifecycle with a session-configurable coverage
schema; an append-only event log (claims, evidence, remediations, stances); computed discrete hypothesis
standing; two structural contradiction detectors (intra-spec + spec-vs-reality); gap detection; the three
signals; next-question suggestion; plan-readiness scoring; spec export. Multi-session isolation + on-disk
durability via replay.

**Deferred (designed-for, not built):** the online research backend (Perplexity/etc.) — a future
`ResearchProvider` seam, `None` in v1; cross-MCP routing to Intent/Structure/Security; a rich entity graph
beyond what requirement/claim linking needs.

**Hard boundaries / non-goals:**
1. **No LLM, no network inside the server.**
2. **The server never researches the app itself** — app-research is the caller's job; the server only
   ingests `app_research_*` evidence events.
3. **The server does not orchestrate the alignment loop** — it is a pure compute-on-demand kernel; each
   `append`/read recomputes. The detect→remediate→rerun loop is driven by the caller. No background jobs,
   no scheduling, no autonomous re-evaluation.
4. `ResearchProvider` stays a `None` seam.
5. No cross-MCP routing; no rich entity graph.
6. **User-provided facts always outrank any future external finding.**

## 2. Determinism boundary (the crux of "pure compute")

Detection needs **identity**: to fire a contradiction the server must know two claims are "about the same
thing." Under v1 that identity is **caller-supplied and canonical** — the server does **no semantics**:

- `Claim.subject`/`Claim.object` are `EntityRef`s with **caller-assigned stable IDs** (e.g.
  `entity:auth-service`); `predicate` is a caller-supplied token from a controlled vocabulary
  (e.g. `supports`, `max_latency_ms`).
- Detectors are **exact-match pure functions** over these. The server **never** fuzzy-matches, case-folds,
  or consults a synonym/alias table. This is what makes the kernel 100% golden-testable offline.

**Identity drift** (the one weakness this introduces) is handled by a **passive registry**, not by matching:
`session.open`/`state.get` return the session's known entity & predicate IDs so the caller reuses them;
`append` marks any first-seen ID (`first_seen: true`) as a plain fact. No `declare` ceremony, no
string-distance hints. The caller is assumed to be a disciplined agent honoring a skill — not an adversary.

## 3. Data model (typed, event-sourced)

Standing is **never stored** — it is a pure projection (fold) of the append-only event log. The same events
always recompute the same standing.

```
Claim {                                       // a HYPOTHESIS, not an asserted fact
  id, session_id,
  subject: EntityRef, predicate: token, object: EntityRef | Value,
  polarity: positive | negative,
  category: requirement | constraint | risk | context | decision,
  modality: fact | goal | constraint | assumption | decision | risk | preference,
  dimensions: [DimensionTag],                 // caller-supplied coverage tags (orthogonal to category)
  source: EvidenceRef { turn_id },            // provenance — every claim points back to a transcript turn
  time_scope?: TimeScope,
}

// Standing is COMPUTED, never stored:
HypothesisStanding = open | challenged | withstood | disproven

EvidenceEvent {                               // append-only; carries provenance + source-type
  id, session_id, claim_id,
  source_type: user_assent | user_refutation | corroboration
             | app_research_support | app_research_refutation,
  detail?, source: EvidenceRef { turn_id },
}

// The server's own structurally-detected contradiction is itself a first-class issue:
Contradiction {
  id, kind: intra_spec | spec_vs_reality,     // claim-vs-claim | claim-vs-app_research_refutation
  claims: [claim_id], severity: low | medium | high,
  explanation, suggested_question,
}

RemediationEvent {                            // a TRANSITION DRIVER, not terminal
  id, session_id, move: supersede | retract | qualify | reconcile_by_evidence | accept,
  targets: [claim_id | contradiction_id], detail?, source: EvidenceRef,
}

RequirementCandidate {                        // derived from claims, not stored raw
  id, statement, subject, capability, condition?,
  acceptance_criteria: [string], priority?: must | should | could,
  owner?, rationale?, source_claims: [claim_id],
  quality_checks: [RequirementQualityCheck],  // testable? has-acceptance? has-owner? unambiguous?
}

// Future-only (seam, not built in v1):
ExternalFinding { id, query, summary, source_urls, confidence, applies_to_claims,
                  status: unverified_context | confirmed_by_user | rejected_by_user }
```

`InterviewState` is the fold of a session's event log: live hypotheses + computed standing + derived
requirements + open contradictions/gaps + registry + readiness.

## 4. Signals (first-class outputs)

Detection is **not a tool** — it is a projection returned on **every** `append` and **every** read, so the
caller can never forget to check. Three signal kinds:

- **`notify`** — a contradiction/disproof was detected (an event happened).
- **`clarify`** — a gap/ambiguity needs a question to the human (this *is* `question.next`).
- **`remediate`** — a standing hypothesis has open disproof; resolve it via a remediation move.

### Remediation moves (v1 set — LOCKED but TO BE VETTED during the build)

| Move | Effect |
|---|---|
| `supersede(old → new)` | new claim/evidence wins; old → `disproven`. |
| `retract(claim)` | withdraw a hypothesis. |
| `qualify(claim, condition \| time_scope)` | add scope so both claims hold under different conditions → contradiction dissolves. |
| `reconcile_by_evidence` | the "refine the system" path: caller changes the app, submits `app_research_support`; contradiction clears on rerun. |
| `accept(target, rationale)` | **take a stance.** Elevates a position to *governing*; every claim contradicting it becomes `challenged` with a conformance `remediate` signal. Does **not** clear a blocker — readiness stays blocked until the graph conforms. Constraint propagation, not suppression. |

## 5. MCP tool surface (a command/query split — read-only queries separated from mutations)

Everything that *mutates* is an append to the log; everything that *reads* is a projection. Errors carry
stable prefixes (`SESSION_NOT_FOUND`, `INVALID_CLAIM`, `BAD_SESSION_ID`, … — the cpm-planner convention).

| Tool | Kind | Does |
|---|---|---|
| `session.open` | command | Start/resume a session by `session_id`; accepts an optional `coverage_schema`; returns current `InterviewState` (incl. registry). |
| `append` | command | Append an event — variant `add_claim` \| `add_evidence` \| `remediate{supersede\|retract\|qualify\|reconcile\|accept}`. Returns updated state + freshly-computed signals. |
| `state.get` | query | Read-only `InterviewState` projection (ledger + computed standing + requirements + contradictions + gaps + registry + readiness). |
| `question.next` | query | The highest-leverage next question, derived deterministically from open `clarify`/`remediate` signals (gap-driven, never re-asks a satisfied dimension). |
| `readiness.assess` | query | Compute the `ReadinessReport` (see §6). |
| `spec.export` | query | Emit the structured, plan-ready spec document (see §6). |

## 6. The readiness gate (alignment, computed, honest)

`readiness.assess → ReadinessReport { ready: bool, dimensions: {...}, blockers: [string], open_signals: [...] }`.
It **shows its work** — per-dimension standing + every open signal — and asserts *"survived our falsification
attempts,"* never *"true."* Computed deterministically:

`ready == true` **iff**

1. **Coverage** — every *required* dimension in the session's `coverage_schema` carries a hypothesis whose
   computed standing is `withstood` or better.
2. **No surviving high-severity contradictions** — no hypothesis stands `challenged` or `disproven` at high
   severity.
3. **No open conformance-remediation** — every `accept` stance has had its contradicting claims brought into
   conformance.
4. **Requirement quality** — every `must` requirement has acceptance criteria + an owner.

`spec.export` emits the aligned spec: `withstood`+ hypotheses grouped by dimension, requirement candidates
with acceptance criteria + provenance, and a visible **accepted-tensions** section (so nothing is waved
through silently). A calling workflow can guard its own "ready to plan?" gate on `ready == true` — replacing
any guidance-only, prose-based readiness check with a deterministic call to `readiness.assess`.

## 7. Crate shape

- `elicitation-core` — the deterministic kernel (data model, event-fold projections, detectors, readiness).
  No MCP, no I/O beyond the event-log store trait. Golden-tested.
- `elicitation` — the MCP server (stdio) wrapping the kernel; binary `elicitation`.
- `StateStore` trait (event-log append/replay) with a filesystem impl; `ResearchProvider` trait (future
  seam) with a `None` impl in v1.

## 8. Sessions & durability

- Every call carries a **`session_id`**; all state keyed by it; concurrent sessions fully isolated.
- Persistence is an **append-only event log per session** — `<state_dir>/<session_id>.jsonl`, one event per
  line. `InterviewState` (incl. all computed standing) is rebuilt by replaying the log. Provenance is free,
  appends are crash-resilient, sessions isolate by file, restart-survival is just replay. (Snapshotting is a
  later optimization.)
- `state_dir` is configurable (flag/env), default an XDG data dir.

## 9. Testing

- **Golden tests on the kernel:** fixture event logs → expected computed standing / contradictions /
  readiness. Deterministic, offline, fast. Same events → same projection.
- **Round-trip:** append events → replay → identical `InterviewState`.
- **Detector table tests:** one per contradiction kind (intra-spec, spec-vs-reality) and per remediation move
  (incl. the `accept` conformance-propagation behavior).
- **Readiness table tests:** each blocker condition independently provable.

## 10. Build path (self-hosting)

This spec was itself produced by a structured discovery interview and then driven through a greenfield
build pipeline: the elicit-phase output (this document) is turned into a task graph, scheduled (e.g. with
`cpm-planner`), signed off by a human, scaffolded, built TDD-style, then verified and reviewed. The pattern
is self-hosting: once elicitation ships, it can serve as the deterministic backend of the very
elicit phase that produced it — a spec aligned by the tool it specifies.
