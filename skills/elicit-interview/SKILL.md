---
name: elicit-interview
description: "Use when requirements need a structured discovery interview with falsifiable claims and readiness before planning: open a session, capture requirements as hypotheses with canonical subject/predicate/object identity, gather user and code evidence (app_research_*), act on question.next, remediate contradictions, stop at readiness.assess ready, then spec.export."
---

# Running the elicitation interview

Calling the tools: see [Calling the tools](../elicitation/SKILL.md#calling-the-tools)
(registered MCP server or a host wrapper such as `junior tools call`).

Use the [elicitation kernel](../elicitation/SKILL.md) to run a discovery
interview as a **falsification loop**. You do not record facts; you record
candidate requirements as hypotheses and then try to break them, with the human
and with the code. The loop ends when the spec has *survived falsification*,
which `readiness.assess` decides.

## The loop

1. **Open.** `session.open` with a `session_id`. Pass a `coverage_schema` whose
   required dimensions are the areas this spec must settle; otherwise the default
   purpose / constraints / success-criteria / scope / non-goals applies. Keep the
   returned registry.
2. **Hypothesize.** `append` `add_claim` for each statement the user makes. One
   claim per testable proposition.
3. **Try to falsify.** Gather evidence for and against each claim:
   - **From the user** — `user_assent` when they confirm, `user_refutation` when
     they reject. User-sourced evidence outranks code research.
   - **From the code** — actually read the repository and append
     `app_research_support` when the code does what the claim says, and
     `app_research_refutation` when it does not. Do not trust a claim about the
     system because the user asserted it; go look.
4. **Ask the next question.** Call `question.next` after every append. It derives
   the highest-leverage question from the open `clarify`/`remediate` signals. Put
   that question to the user or resolve it by research; do not invent your own
   agenda while a signal is open.
5. **Remediate contradictions.** When a `notify` signal or a contradiction
   appears, choose a move and `append` `remediate`:
   - `supersede` — a better claim replaces the old one;
   - `retract` — withdraw a hypothesis the user retracts;
   - `qualify` — both claims hold under different conditions (`condition` /
     `time_scope`);
   - `reconcile_by_evidence` — the "fix the system" path: change the app, then
     submit `app_research_support`; the contradiction clears on rerun;
   - `accept` — take a stance; contradicting claims become `challenged` with a
     conformance signal and readiness stays blocked until they conform.
6. **Stop at readiness.** Poll `readiness.assess`. It is ready only when every
   required dimension carries a `withstood` hypothesis, no high-severity
   contradiction survives, every `accept` stance has conformed, and every `must`
   requirement has acceptance criteria and an owner. Read `blockers` and keep
   interviewing until `ready` is true.
7. **Export.** Call `spec.export` for the aligned, plan-ready document, then hand
   it to the [elicit-to-plan skill](../elicit-to-plan/SKILL.md).

## Claim shape

A claim carries a caller-assigned canonical identity — the detectors match ids
and predicates by exact equality, so reuse the ids in the registry:

```json
{
  "variant": "add_claim",
  "session_id": "checkout",
  "claim": {
    "id": "c-sso",
    "session_id": "checkout",
    "subject": { "id": "entity:checkout" },
    "predicate": "supports",
    "object": { "kind": "value", "value": "sso-login" },
    "polarity": "positive",
    "category": "requirement",
    "modality": "goal",
    "dimensions": ["scope"],
    "source": { "turn_id": "t12" }
  },
  "requirement": {
    "statement": "Checkout supports SSO login.",
    "capability": "sso-login",
    "acceptance_criteria": ["A Google account can sign in and reach the cart."],
    "priority": "must",
    "owner": "team:checkout"
  }
}
```

Evidence names its claim and where it came from:

```json
{
  "variant": "add_evidence",
  "session_id": "checkout",
  "evidence": {
    "id": "e-1",
    "session_id": "checkout",
    "claim_id": "c-sso",
    "source_type": "app_research_refutation",
    "detail": "No OAuth provider is configured in the auth module.",
    "source": { "turn_id": "t14" }
  }
}
```

## Rules

- Never mark a requirement satisfied because it was requested; only evidence
  moves standing.
- Never route around a blocker by lowering the coverage schema mid-interview.
- Keep one session per interview so the log stays a faithful transcript.
- `spec.export` is not a gate; only `readiness.assess`'s `ready` value is.
