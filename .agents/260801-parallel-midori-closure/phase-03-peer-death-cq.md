---
phase: 3
title: "Peer-Death Completion Readiness Audit"
status: complete
priority: P1
effort: "4h"
dependencies: []
tier: thinking
---

# Phase 03: Peer-Death Completion Readiness Audit

## Overview

Decide what is still missing before the Midori Phase 07 peer-death/waiter slice can be implemented. This phase does not modify scheduler or completion code.

## Requirements

- Functional: Produce a readiness report and ADR stub naming the future submission owner, target-dependency lifecycle, result semantics, and stop gates.
- Non-functional: No code implementation, no new ABI, no scheduler edits.

## Architecture

Observed foundation:
- `WaitCompletion = 242` already exists: `libs/api/src/abi/syscall.rs:429`.
- Completion queue is kernel-owned and not a grant: `kernel/src/task/completion.rs:1`, `kernel/src/task/completion.rs:3`, `kernel/src/task/completion.rs:102`.
- Queue has waiter registration and deferred wake: `kernel/src/task/completion.rs:285`, `kernel/src/task/completion.rs:352`.
- `completion_wait` currently accepts only `NET_RX`: `kernel/src/task/completion_wait.rs:55`.
- `exit_task` only wakes `TaskState::Sending` peers and `Wait(tid)` waiters today: `kernel/src/task/scheduler.rs:512`, `kernel/src/task/scheduler.rs:536`.

Conclusion from source: CQ exists, but no async IPC submission currently registers "this CQ slot depends on target tid X." Implementing peer-death completion now would invent that owner ahead of the real async IPC migration. The safe output is an ADR/readiness package, not code.

## Assumptions

- **Claim:** The first real owner should be the future async IPC submit path rather than NET_RX.
  **Confidence:** high
  **How to verify:** grep for current CQ submissions; only `NET_RX` is accepted in `completion_wait`.

## File Ownership

- Owns: `.agents/260801-parallel-midori-closure/reports/peer-death-cq-readiness.md` and optional ADR stub under `.agents/260801-parallel-midori-closure/reports/`.
- Read-only evidence: `kernel/src/task/completion.rs`, `kernel/src/task/completion_wait.rs`, `kernel/src/task/scheduler.rs`, `libs/api/src/abi/completion.rs`.

## Implementation Steps

1. Create worktree: `git worktree add .worktrees/midori-peer-death-cq-audit -b codex/midori-peer-death-cq-audit`.
2. Grep every CQ reservation/registration and prove which sources exist today.
3. Trace `exit_task` death paths and current wake behavior for `Sending`, `Wait(tid)`, and notify-on-exit watchers.
4. Write readiness report: missing owner, required internal registry shape, timeout/unregister cleanup points, result-code decision, and Law-1 triggers.
5. Write ADR stub for the future implementation; do not modify product code.

## Success Criteria

- [x] Report proves current CQ sources and explains why implementation is premature.
- [x] ADR stub defines the future target-dependency registration contract.
- [x] Stop gates name every ABI trigger.
- [x] No product source file changes.

## Security Considerations

The future implementation must prevent stale target tids from waking a new cell after tid reuse. The audit must call out whether generation tagging is required.

## Risk Notes

- Risk high: implementing before the async IPC owner exists creates dead code or wrong ownership. Mitigation: audit-only.
- Risk medium: ADR may reveal Law-1 is required. Mitigation: stop and package confirmation request.
- Rollback: delete the report/ADR stub; no product state changes.

## Deviation Log

Evidence reports: [peer-death-cq-readiness.md](./reports/peer-death-cq-readiness.md) and [adr-peer-death-cq-owner-stub.md](./reports/adr-peer-death-cq-owner-stub.md).
