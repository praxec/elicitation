# Contributing to elicitation

Thanks for your interest. elicitation is a small, focused workspace — a
deterministic interview kernel plus an MCP server façade.

## Development

```sh
cargo build
cargo test
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

CI runs build + test on Linux/macOS/Windows, plus `rustfmt` and `clippy` (warnings
are denied). Please make sure those pass locally before opening a pull request.

## Guidelines

- Keep the two concerns clean: the pure kernel (`elicitation-core`) has no MCP and
  no network — its only I/O is the event-log store trait; the MCP/stdio layer
  (`elicitation`) sits on top.
- The kernel is a pure function of `(events in) → (computed standing + ...)`. Keep
  it deterministic: no fuzzy matching, no clocks, no I/O in the projection. Add a
  golden/table test for any behavior change.
- Conventional, focused commits.

## Reporting issues

Open an issue with a minimal reproduction — for a detection or readiness bug, the
sequence of events (claims/evidence/remediations) that produces the wrong
standing or readiness result is ideal.
