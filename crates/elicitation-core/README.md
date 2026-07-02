# elicitation-core

[![crates.io](https://img.shields.io/crates/v/elicitation-core.svg)](https://crates.io/crates/elicitation-core)
[![docs.rs](https://docs.rs/elicitation-core/badge.svg)](https://docs.rs/elicitation-core)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](../../LICENSE)

The deterministic, offline kernel behind [`elicitation`](../../README.md) — a
structured-discovery interview engine modeled as a falsification-based alignment
mechanism.

Claims are **hypotheses**, not facts. Their standing (`open` → `challenged` →
`withstood` / `disproven`) is *computed* — a pure fold over an append-only event
log — never stored. Exact-match detectors flag intra-spec and spec-vs-reality
contradictions over caller-supplied canonical identity; a deterministic readiness
gate reports whether the spec has *survived falsification*. No LLM, no network, no
fuzzy matching — which is what makes the whole kernel golden-testable.

```rust
use std::sync::Arc;
use elicitation_core::{Engine, FilesystemStore};

let store = FilesystemStore::new("/tmp/elicitation")?;
let engine = Engine::new(Arc::new(store));

let state = engine.open_session("design-review", None)?;
// ... append claims/evidence ...
let report = engine.readiness("design-review")?;
println!("ready to plan? {}", report.ready);
# Ok::<(), elicitation_core::EngineError>(())
```

The MCP server façade lives in the `elicitation` crate; this crate has no MCP
or network dependency. See the [root README](../../README.md) for the tool surface
and [`docs/spec.md`](../../docs/spec.md) for the full design.

## License

[Apache-2.0](../../LICENSE).
