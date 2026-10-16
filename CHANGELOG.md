# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-16

### Added

- Optional, opt-in Jev assist layer behind the default-on `jev` cargo feature:
  a provider-neutral `Judge` trait and TypeSafe/OpenRouter Jev HTTP client
  (`crates/elicitation/src/assist/`), plus four advisory MCP tools that never
  append events — `assist.classify_reply`, `assist.canonicalize`,
  `assist.contradictions` and `assist.phrase_question`. They return proposals
  with per-option probabilities and the question text sent, require a paid key
  (`TYPESAFE_JEV_API_KEY` or `OPENROUTER_API_KEY`), and fail with a stable
  `ASSIST_UNAVAILABLE` error when unconfigured; the six core tools are
  unaffected and the kernel stays network-free.
- Release engineering brought up to the praxec release standard:
  - `.github/workflows/release.yml` builds six targets (Linux, macOS and
    Windows on x86_64 and ARM64, including `aarch64-pc-windows-msvc`),
    smoke-tests every packaged binary with `scripts/mcp-smoke.mjs`, and
    aggregates one combined `checksums.sha256` and `release-manifest.json` via
    `scripts/release-manifest.sh`, packs the npm launcher with the agent skills
    bundled (attaching `matthew-cochran-elicitation-<version>.tgz` and
    `npm-package.sha256`), and only then publishes the draft release. A separate
    `npm` job publishes that exact tarball over trusted publishing when the
    repository variable `NPM_TRUSTED_PUBLISHING` is `true`.
  - `.github/workflows/image.yml` builds `ghcr.io/praxec/elicitation` from the
    checksum-verified Linux release binaries with `Dockerfile.release`,
    smoke-tests the image over MCP, and publishes `server.json` to the MCP
    registry best-effort.
  - `.github/workflows/installer-tests.yml` runs the installer behavior tests.
  - `scripts/check-version-sync.sh` (with `--tag` for release validation, and
    checks for the `eliciter-core` dependency version),
    `scripts/release-manifest.sh`, `scripts/mcp-smoke.mjs`, `scripts/install.sh`,
    `scripts/install.ps1` and `scripts/release.test.mjs`.
  - `deny.toml` and a `cargo-deny` CI job; clippy is now required to pass with
    `-D warnings` and CI runs the workspace test suite.
  - `docs/releasing.md`, the maintainer release runbook, including the manual
    crates.io publish order (`eliciter-core` before `eliciter`).
- `elicitation` command-line surface: `--version`, `--help`, and
  `skills install|uninstall|list --target <claude|codex|both>` with
  `--dir`/`--project`/`--user`. With no arguments the binary still serves MCP
  over stdio.
- Agent skills under `skills/` (`elicitation`, `elicit-interview`,
  `elicit-to-plan`), embedded into the binary at build time and written with a
  per-root manifest that refuses to overwrite a skill the user edited.
- npm launcher package `@matthew-cochran/elicitation`, which downloads the
  release binary for the host platform, verifies it against the release
  `checksums.sha256`, caches it, and execs it with stdio passed through.

### Changed

- Release assets are no longer uploaded with per-asset checksums (taiki-e);
  the release workflow now produces one combined `checksums.sha256` and a
  `release-manifest.json`.
- `server.json` names the OCI identifier with its version tag
  (`ghcr.io/praxec/elicitation:0.0.1`); the OCI package carries no `version`
  field.
- Renamed the crates for the crates.io release: `elicitation-core` is now
  `eliciter-core` (library name `eliciter_core`) and `elicitation` is now
  `eliciter`. The installed binary is still named `elicitation`.
- Raised the `rmcp` dependency requirement to 1.8.

### Removed

- Removed the unused legacy root `elicitation-session` crate, including its
  bundled SQLite dependency. The repository root is now a virtual workspace with
  members `crates/elicitation-core` and `crates/elicitation`.

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

[0.1.0]: https://github.com/praxec/elicitation/releases/tag/v0.1.0
[0.0.1]: https://github.com/praxec/elicitation/releases/tag/v0.0.1
