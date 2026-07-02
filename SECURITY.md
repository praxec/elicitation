# Security Policy

## Reporting a vulnerability

Please report security issues privately via GitHub's
[security advisories](https://github.com/praxec/elicitation/security/advisories/new)
rather than a public issue. You'll get an acknowledgement, and a fix or mitigation
will be coordinated before public disclosure.

## Scope

elicitation is a deterministic, offline MCP server that speaks over stdio and
holds interview state in an append-only event log on the local filesystem. It runs
**no LLM, executes no user-supplied code, and makes no network calls** — its inputs
are claim/evidence/remediation events and read projections. Of particular interest:
any input (a crafted claim, evidence sequence, remediation, or `session_id`) that
causes a panic, a hang, incorrect computed standing or readiness, or a path that
escapes the configured state directory.
