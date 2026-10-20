---
name: elicit-to-plan
description: "Use when an elicitation spec.export has produced plan-ready requirements and you need to turn them into a cpm plan: each requirement becomes a deliverable with an inspectable artifact, acceptance criteria, an owner and a milestone, joined by consumption edges. Bridges the elicitation interview to the cpm-plan / deliverable-cpm skills."
---

# From an elicited spec to a cpm plan

Calling the tools: see [Calling the tools](../elicitation/SKILL.md#calling-the-tools)
(registered MCP server or a host wrapper such as `junior tools call`).

Run the [elicit-interview method](../elicit-interview/SKILL.md) until
`readiness.assess` reports `ready`, then call `spec.export`. Its output is the
bridge: a list of requirement candidates (each with acceptance criteria,
priority, owner and `source_claims` provenance), the `withstood` hypotheses
grouped by coverage dimension, and a visible **accepted-tensions** section.

This skill turns that document into a plan whose deliverables are artifacts, not
activities. The authoring rules, edge rules and lint fixes live in the
`deliverable-cpm` skill of the cpm-planner package; this skill is only the
mapping from elicited requirements to a plan.

## 1. One requirement, one deliverable

For every `must` requirement in the exported spec, write one deliverable whose
`metadata.artifact` names the concrete thing a reviewer can inspect — a file, a
schema, an endpoint — not a verb like "implement" or "review".

- Title the deliverable after the artifact it produces.
- Copy the requirement's `acceptance_criteria` into the deliverable; they are its
  definition of done.
- Carry `priority: must` to `metadata.kind: "manual"` only when no agent may
  lease it; otherwise let it be leased.
- Set `metadata.owner` to the requirement's owner, or the executor class that
  owns that artifact.
- Trace the source: record the requirement id and its `source_claims` in the
  deliverable so the plan points back at the evidence that justified it.

`should` and `could` requirements become deliverables too, but attach them with
a lower-priority flag or leave them out of the critical path deliberately; never
silently drop a requirement — put it in the plan or say why it is deferred.

## 2. Acceptance points become milestones

Each requirement that the exported spec treats as a completion gate becomes a
deliverable with `"milestone": true`. A milestone is an artifact that *is* the
acceptance evidence (a passing conformance test, a signed-off schema), not a
meeting. The accepted-tensions section maps to explicit conformance
deliverables so no stance is waved through.

## 3. Edges are consumption

Add an edge only when one deliverable consumes something another produces. Name
the consumed thing in `consumes` and mark whether it is an `artifact` or an
`interface`; interfaces usually target a contract deliverable. An edge that
merely expresses "do this first" is not a consumption edge — encode it as
`lag_hours` on the real edge instead.

## 4. Lint, sync, schedule

Write the plan as code (in cpm-planner this is a file under
`.cpm-planner/plans/<name>/<variant>.json`), then lint until clean, sync it, and
check the critical path and levelled schedule. If lint or scheduling reveals an
unstated dependency, that is new information: go back to
`append`/`remediate` in the interview rather than guessing in the plan.

## Checklist

- Every `must` requirement has exactly one deliverable naming an artifact.
- Every deliverable carries the requirement's acceptance criteria and an owner.
- Every acceptance gate is a `milestone` deliverable.
- Every accepted tension has a conformance deliverable.
- Every edge is a real consumption, not a sequencing hint.
- The plan lints clean before any work is leased.
