# 2026-08-21 — Tier 1 provenance contract

## What happened
Drafted and committed the proposed detached Publisher Provenance Envelope V1 for future Tier 1 Claim A. No runtime admission behavior changed.

## Decisions
- Bind final ELF, current payload, checked source, F1/F5, toolchain, dependency, recipe, and CI build handoff in a strict detached record.
- Require the CI/KMS gateway to consume an authenticated receipt and content-addressed unsigned output; arbitrary caller ELF paths are rejected.
- Keep owner authorization and the external anti-replay floor separate from publisher provenance.

## Lessons
- A source check followed by arbitrary-target signing does not prove source-to-artifact provenance.
- Production admission cannot proceed without a qualified non-replayable floor and independent approvals.

## Next steps
- Obtain security-owner and independent-reviewer design approval.
- Qualify a real external floor before implementing the owner A/B store and hostile failure suite.
