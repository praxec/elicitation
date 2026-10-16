---
name: elicitation
description: "Use when the elicitation MCP server is available and you need the tool map, the ELICITATION_STATE_DIR and session lifecycle, or the rules of the deterministic structured-discovery kernel. Covers the six tools (session.open, append, state.get, question.next, readiness.assess, spec.export), computed hypothesis standing, structural contradiction detection, and the append-only event log."
---

# elicitation: the structured-discovery kernel

`elicitation` is a deterministic, **offline** MCP server for structured
discovery interviews. It owns structure and state; **you** own the fuzzy work:
turning conversation into candidate claims, voicing questions, and researching
the actual codebase to try to falsify a claim. The server runs no LLM and touches
no network, and it never researches the app itself.

A claim is a **hypothesis**, never an asserted fact. Its `standing`
(`open` -> `challenged` -> `withstood` / `disproven`) is **computed** by folding
the append-only event log; it is never stored. Contradictions and readiness are
the same kind of projection, returned on every read and every `append`, so you
cannot forget to check.

The method that drives this kernel is the
[elicit-interview skill](../elicit-interview/SKILL.md).

## Tools

Everything that mutates is an append; everything that reads is a projection.

| Tool | Kind | Does |
|------|------|------|
| `session.open` | command | Start or resume a session by `session_id`; accepts an optional `coverage_schema`; returns the current `InterviewState` including the entity/predicate registry. |
| `append` | command | Append one event, tagged by `variant`: `add_claim`, `add_evidence`, or `remediate`; returns the recomputed state with fresh signals. |
| `state.get` | query | Read-only `InterviewState`: ledger, computed standing, requirements, contradictions, gaps, registry, readiness. |
| `question.next` | query | The highest-leverage next question, derived deterministically from open `clarify`/`remediate` signals (`{question: null}` when nothing needs asking). |
| `readiness.assess` | query | The `ReadinessReport`: `ready` bool, per-dimension standing, blockers, open signals. |
| `spec.export` | query | The aligned, plan-ready spec document. |

Errors carry stable prefixes: `SESSION_NOT_FOUND`, `BAD_SESSION_ID`,
`INVALID_CLAIM`, `INVALID_EVIDENCE`, `INVALID_REMEDIATION`, `CLAIM_NOT_FOUND`,
`CONTRADICTION_NOT_FOUND`, `DUPLICATE_ID`, `STORE_ERROR`.

## Identity is canonical and caller-supplied

The server does **no** semantics: `subject`/`object` entity ids and `predicate`
tokens are matched by exact string equality. Choose stable ids once
(`entity:auth-service`, `supports`, `max_latency_ms`) and reuse them.
`session.open` and `state.get` return the registry of ids already seen; a
first-seen id comes back marked `first_seen: true`. Reuse the registry ids
instead of inventing synonyms — identity drift is the one weakness of a
semantics-free detector.

## Where state lives

State is an append-only event log per session:
`<state_dir>/<session_id>.jsonl`, one event per line. `InterviewState` is
rebuilt by replaying that log, so restart-survival is just replay.

`state_dir` resolves in this order:

1. `ELICITATION_STATE_DIR` (set this to keep a session with a project);
2. `$XDG_DATA_HOME/elicitation`;
3. `$HOME/.local/share/elicitation`;
4. `./elicitation-state`.

Sessions are isolated by `session_id`; every tool call carries one.

## Session lifecycle

1. **`session.open`** with a `session_id` and, when the interview needs a
   specific checklist, a `coverage_schema` listing the dimensions you require
   (the default is purpose / constraints / success-criteria / scope / non-goals).
2. **`append` `add_claim`** for each hypothesis: a `subject`, `predicate` and
   `object`, with `polarity`, `category`, `modality`, the coverage `dimensions`
   it belongs to, and a `source.turn_id` back to the transcript.
3. **`append` `add_evidence`** as the user assents or refutes
   (`user_assent` / `user_refutation`), and as you research the code
   (`app_research_support` / `app_research_refutation`). Standing recomputes on
   every append.
4. **`append` `remediate`** to resolve a detected contradiction.
5. **`readiness.assess`** when the signals quiet down; when `ready` is true,
   call **`spec.export`** for the plan-ready document.

Readiness is honest by construction: it reports that the spec *survived
falsification*, never that it is true.

## Optional Jev assist tools (advisory, paid)

When the `jev` feature is compiled (default) **and** a paid Jev key is
configured, four additional advisory tools appear. They never append events:
each returns a proposal with per-option probabilities and the question text it
sent, and *you* decide whether to `append` it. With no key they fail with a
stable `ASSIST_UNAVAILABLE` error and the six core tools are unaffected.

| Tool | Does |
|------|------|
| `assist.classify_reply` | `{session_id, claim_id?, reply}` -> `assent` \| `refute` \| `new_claim` \| `off_topic`, plus a proposed `append` payload (`add_evidence` `user_assent`/`user_refutation`, or `add_claim`). |
| `assist.canonicalize` | `{session_id, subject, predicate, object?}` -> the best-matching existing registry entity/predicate ids (or `new`), to fight the exact-match identity weakness. |
| `assist.contradictions` | `{session_id}` -> meaning-level contradiction candidates the structural detectors did not flag, for you to confirm via `remediate`. |
| `assist.phrase_question` | `{session_id}` -> a natural rewording of `question.next`'s `suggested_question`. |

Config: `TYPESAFE_JEV_API_KEY` (TypeSafe) or `OPENROUTER_API_KEY` (OpenRouter);
optional `ELICITATION_JEV_TIMEOUT_SECS`, `ELICITATION_JEV_BASE_URL` and
`ELICITATION_JEV_MODEL`. Set `ELICITATION_ASSIST` to a non-matching list
(e.g. `rig`) to opt out explicitly. Keys are never printed.
