---
phase: 3
title: "Law 1 Reactor Gate"
status: completed
priority: P1
effort: "0.5d"
dependencies: [2]
tier: thinking
---

# Phase 03: Law 1 Reactor Gate

## Overview

Stop before public ABI or semantic changes. This phase creates the explicit decision package for generic completion sources, peer-death completion semantics, and executor-visible wait behavior.

## Requirements

- Functional: request two explicit Law 1 confirmations before any edit under `libs/api/` or `libs/types`, or any semantic widening of existing syscall behavior.
- Functional: document the exact contract to be implemented if confirmed.
- Non-functional: no code changes except plan/status notes in `.agents/` until confirmation is captured.

## Architecture

Data flow before gate: code evidence and guardrail test results enter a decision package; user confirmation exits as either "authorized" or "stop at ABI-free closure." No runtime component changes.

## Assumptions

- **Claim:** The user may choose to stop after ABI-free NET_RX proof.
  **Confidence:** high
  **How to verify:** Ask explicitly during this gate.

## Related Files

- Create: `.agents/260806-1026-midori-reactor-stack-closure/reports/law1-reactor-decision.md`
- Read: `docs/code-standards.md`
- Read: `docs/specs/03b-async-reactor-adr.md`
- Read: `libs/api/src/abi/syscall.rs`
- Read: `libs/api/src/abi/completion.rs`

## Implementation Steps

1. Summarize Phase01/02 evidence and list the exact public surfaces that Phase04/05 would touch.
2. Ask Law 1 confirmation 1/2: "Authorize public ABI/semantic work for generic completion/executor wait semantics?"
3. If confirmation 1 is yes, restate the file list and ask Law 1 confirmation 2/2 immediately before the first public edit.
4. If either confirmation is no, mark phases 04-07 blocked and hand back a NET_RX-only closure report.
5. Record the answer with date, branch, and exact wording in `reports/law1-reactor-decision.md`.

## Success Criteria

- [x] Decision report exists and includes yes/no for confirmation 1 and confirmation 2.
- [x] No `libs/api/` or `libs/types/` diff exists before two yes answers.
- [ ] If denied, Phase07 remains honestly closed as NET_RX-only and Phase08 remains baseline-only.

## Validation Commands

```bash
git diff -- libs/api libs/types
grep -RIn "Law 1" .agents/260806-1026-midori-reactor-stack-closure/reports/law1-reactor-decision.md
```

## Security Considerations

Skipping this gate risks freezing an unreviewed ABI in the cell/kernel boundary and invalidating existing allowlists.

## Risk Notes

- High x High: implementer treats existing ADR acceptance as ABI authorization. Mitigation: require two fresh explicit confirmations in this phase.
- Rollback: delete the decision report if no code followed. Irreversible part: user authorization cannot be inferred later; preserve exact wording.

## Deviation Log

None.
