# elicitation

[![CI](https://github.com/praxec/elicitation/actions/workflows/ci.yml/badge.svg)](https://github.com/praxec/elicitation/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/elicitation.svg)](https://crates.io/crates/elicitation)
[![docs.rs](https://docs.rs/elicitation-core/badge.svg)](https://docs.rs/elicitation-core)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A deterministic, offline, **pure-compute** MCP server for structured discovery
interviews — modeled as a **falsification-based alignment mechanism**. The
calling LLM does the fuzzy work (turning conversation into candidate claims and
voicing questions); this server owns the *structure and state*.

## What it does

Discovery interviews go wrong in a predictable way: the model nods along, forgets
what was said three turns ago, and never notices that two requirements
contradict each other. elicitation supplies the missing half.

Every claim is treated as a **hypothesis**, not a fact. Evidence accrues against
each hypothesis with provenance; its **standing** (`open` → `challenged` →
`withstood` / `disproven`) is *computed*, never stored. Structural detectors
flag contradictions — both claim-vs-claim (intra-spec) and claim-vs-reality
(spec-vs-reality, with category-aware precedence). A deterministic
**readiness gate** then reports whether the spec has *survived falsification* —
never "is true" — turning "did we interview well enough to plan?" into a
checkable boolean that shows its work.

It runs no LLM and touches no network. It is a pure function of
`(events in) → (computed standing + contradictions + readiness + next question out)`,
which makes the whole kernel golden-testable.

## Install

From crates.io:

```sh
cargo install elicitation
```

Or download a pre-built binary for your platform from the
[latest release](https://github.com/praxec/elicitation/releases/latest)
(verify against the release's `checksums.sha256`).

## MCP client config

It speaks MCP over stdio (the standard transport). Wire it into your editor or
agent — Claude Code, Cursor, a custom orchestrator, or any MCP client — like any
other MCP server:

```jsonc
{ "command": "elicitation" }
```

State persists to an append-only event log under an XDG data dir by default;
override the location with the `ELICITATION_STATE_DIR` env var.

## MCP tools

A command/query split: everything that *mutates* is an append to the log;
everything that *reads* is a projection.

| Tool | Kind | Does |
|------|------|------|
| `session.open` | command | Start or resume a session by `session_id`; accepts an optional `coverage_schema`; returns the current `InterviewState` (including the entity/predicate registry). |
| `append` | command | Append one event — `add_claim`, `add_evidence`, or `remediate`; returns the recomputed state with freshly-computed signals. |
| `state.get` | query | Read-only `InterviewState` projection: ledger + computed standing + requirements + contradictions + gaps + registry + readiness. |
| `question.next` | query | The highest-leverage next question, derived deterministically from open `clarify`/`remediate` signals. |
| `readiness.assess` | query | Compute the `ReadinessReport` — `ready` bool, per-dimension standing, blockers, open signals. |
| `spec.export` | query | Emit the aligned, plan-ready spec document. |

## Use `elicitation-core` as a library

The kernel is also a plain Rust library, independent of MCP:

```rust
use std::sync::Arc;
use elicitation_core::{Engine, FilesystemStore};

let store = FilesystemStore::new("/tmp/elicitation")?;
let engine = Engine::new(Arc::new(store));

// open → returns the initial InterviewState (with its registry)
let state = engine.open_session("design-review", None)?;

// ... append claims/evidence (see the docs for Claim / EvidenceEvent shapes) ...

// readiness is computed, never stored — the same events always recompute it
let report = engine.readiness("design-review")?;
println!("ready to plan? {}", report.ready);
# Ok::<(), elicitation_core::EngineError>(())
```

See the [API docs](https://docs.rs/elicitation-core) and
[`docs/spec.md`](docs/spec.md) for the full design.

## Session lifecycle (quickstart)

A session is a thin loop: open it, append claims and the evidence for/against
them, and ask whether it is ready.

1. **`session.open`** with a `session_id` (and optionally a `coverage_schema`
   listing the dimensions you require). Returns the empty `InterviewState`.
2. **`append` `add_claim`** for each hypothesis — a `subject`/`predicate`/`object`
   with caller-assigned canonical identity, tagged with its coverage dimensions.
3. **`append` `add_evidence`** as the user assents or refutes, or as you research
   the actual system. Standing recomputes on every append; contradictions and
   the next question come back in the response.
4. **`readiness.assess`** when the signals quiet down. If `ready`, call
   **`spec.export`** for the aligned, plan-ready document; otherwise read the
   `blockers` and keep going.

## Development

```sh
cargo build
cargo test
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

CI runs build + test on Linux/macOS/Windows, plus `rustfmt` and `clippy`
(warnings denied). Please make sure those pass locally before opening a pull
request. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Using it with Praxec

This is an MCP tool used by [Praxec](https://github.com/praxec/praxec) packs. The easiest way to
get it — and a workflow pack that uses it — up and running is the one-command setup:

```bash
curl -fsSL https://raw.githubusercontent.com/praxec/packs/main/setup.sh | bash
```

See the [pack registry](https://github.com/praxec/packs) for this tool's provider coordinates
(container image / release binary) and which packs depend on it.

## License

[Apache-2.0](LICENSE).
