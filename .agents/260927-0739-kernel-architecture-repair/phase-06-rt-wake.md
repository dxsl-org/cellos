---
phase: 6
title: "Preempt promptly when IPC wakes an RT sender"
status: pending
priority: P2
effort: "1 day"
dependencies: [1]
tier: medium
---

# Phase 06: IPC sender wake and RT latency

## Requirements / architecture
`wake_sender_token` validates sender cell generation and message delivery ID before moving a `Sending` task to `Ready`, but discards `Scheduler::push_ready` priority (`kernel/src/task.rs:2300-2322`). Other wake paths call `pend_preempt_if_needed` (`task.rs:2026-2027,2408-2409`); RV64 `push_ready` may target the dedicated RT hart (`task/scheduler.rs:491-563`). Preserve exact token authorization and invoke preemption for the actual **target** hart; inspect `pend_preempt_if_needed` current-hart-priority predicate before reusing it verbatim for cross-hart wake, because low-priority current-hart state does not imply the remote hart needs an IPI. Non-RV64 remains architecture-scoped, no fake latency claim.

## Related files
`kernel/src/task.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/smp.rs`, `kernel/src/task/ipc_pending_selftest.rs` or existing matching runtime test, `tests/integration/tests/boot.rs` only if relevant and not overwritten by concurrent changes.

## Implementation steps
1. Build a deterministic two-hart case: RT sender parks waiting for receiver consume, RT target hart concurrently runs equal/higher-priority or lower-priority work; validate exact sender generation/token, ready-queue target and which hart is interrupted.
2. Change wake to return/routable priority+hart (or use an existing scheduler method with that contract), pend IPI only when target's running priority is lower, including idle target; avoid duplicate wake and self-IPI storm. Keep scheduler lock order and interrupt-disabled span bounded.
3. Measure wake-to-schedule in fresh RV64 2-hart QEMU run and physical qualified device; report observed P99, not a fabricated microsecond threshold. Re-run 1-hart fallback, older-delivery-token and dead/reused-sender negative tests.

## Success criteria
- [ ] Consuming the exact message queues the sender once and requests preemption on its actual target hart iff required.
- [ ] 2-hart RT sender runs before the next 10ms timer tick under a lower-priority running target; equal/higher-priority target and stale token do not generate incorrect preemption.
- [ ] Existing non-RT IPC and 1-hart behavior unchanged; hardware latency claim only after physical evidence.

## Assumptions / risk / rollback
- [UNVERIFIED] Existing `pend_preempt_if_needed` is sufficient for a remote target; read and measure its source-hart priority logic before using it. Rollback reverts scheduler change with a cold reboot; missed external real-time deadlines during testing cannot be undone. Use workload-safe boards, not robot actuators, for validation.

## Deviation Log
None.
