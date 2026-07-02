# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.1] - 2026-06-22

Initial release.

### Added

- Deterministic, event-sourced structured-discovery interview kernel
  (`elicitation-core`):
  - Claims as **hypotheses** with **computed** standing (`open` → `challenged` →
    `withstood` / `disproven`) — a pure fold over an append-only event log,
    never stored.
  - Structural contradiction detection — intra-spec (claim-vs-claim) and
    spec-vs-reality (claim-vs-research), including category-aware
    `spec_vs_reality` precedence.
  - A plan-readiness gate over a session-configurable coverage schema that
    reports whether the spec has *survived falsification* (never "is true").
- `elicitation`: a stdio MCP server exposing the kernel as a
  command/query tool surface (`session.open`, `append`, `state.get`,
  `question.next`, `readiness.assess`, `spec.export`).

### Known limitations

- Identity is **exact-match only** — no fuzzy entity or predicate matching.
  Callers must supply canonical, stable entity IDs and predicate tokens (and
  reuse the ones returned in the registry).

[0.0.1]: https://github.com/praxec/elicitation/releases/tag/v0.0.1
