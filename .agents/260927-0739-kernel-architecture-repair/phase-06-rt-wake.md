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
- [x] Consuming the exact message queues the sender once and requests preemption on its actual target hart iff required.
- [x] 2-hart RT sender runs before the next 10ms timer tick under a lower-priority running target; equal/higher-priority target and stale token do not generate incorrect preemption. *(Decision-level: the request is made and routed to the right hart, which is what removes the timer-tick wait. No latency number is claimed — P99 needs the physical device, see Deviation Log.)*
- [x] Existing non-RT IPC and 1-hart behavior unchanged; hardware latency claim only after physical evidence.

## Progress

### Slice 1 — the consume wake preempts the sender's target hart (2026-09-27) — done

`wake_sender_token` readied a blocked sender and called `Scheduler::push_ready`, but
discarded the priority that call returns, so no preemption was ever requested: an RT sender
whose message was consumed waited for the next timer tick. The other wake paths
(`ipc_send`, `ipc_publish_input`, `ipc_reply`) already pair `push_ready` with
`pend_preempt_if_needed`; the consume path now does the same.

`pend_preempt_if_needed` also decided from the **calling** hart's running priority while
targeting `HART_RT`. On a cross-hart wake that asks the wrong question twice: a busy waking
hart (equal or higher priority) swallowed the request, and an idle one sent a needless IPI.
The predicate now reads the **target** hart's running priority, which is what "does this hart
need an interrupt?" actually means.

Witness `S22-RV64-RT-WAKE` (`task/rt_wake_selftest.rs`, runner case `rt-wake`, requires
`--harts 2`). The fixture holds `SCHEDULER` for its whole body — so the target hart cannot
dispatch the synthetic sender — pins the target hart on a Normal-priority occupant and the
waking hart on a RealTime one, then asserts: (1) consuming the exact message readies the
sender onto the target hart's ready queue and pends exactly one preemption for that hart;
(2) a stale delivery token neither wakes the sender nor pends anything; (3) the decision
follows the target hart. The preemption *decision* is observed through a test-hooks per-hart
counter (`task::smp::preempt_pends_for`) rather than the interrupt, which keeps the property
deterministic instead of timing-dependent.

Evidence — each defect isolated by reverting one hunk at a time:
- consume wake calls nothing (predicate fixed): `.logs/native-domain-qemu/h2-rt-wake-xiIlSU`
  — `FAIL woke=true queued_on_target=true pended=false … target_hart_decides=true`;
- both hunks reverted: `h2-rt-wake-3qm6mA` —
  `FAIL … pended=false … target_hart_decides=false`;
- both fixed: `h2-rt-wake-*` green, with `migration` and `ipc-copy-race` re-run in the same
  image and the 1-hart `ipc-copy`/`admission` cases re-run separately.

Not measured here: wake-to-schedule latency. The plan's P99 numbers and the "runs before the
next 10 ms tick" claim need a physical device; the decision-level property is what this slice
proves, and no latency number is claimed.

## Assumptions / risk / rollback
- [UNVERIFIED] Existing `pend_preempt_if_needed` is sufficient for a remote target; read and measure its source-hart priority logic before using it. Rollback reverts scheduler change with a cold reboot; missed external real-time deadlines during testing cannot be undone. Use workload-safe boards, not robot actuators, for validation.

## Deviation Log

- **Decision-level witness, not latency.** The plan asks for a measured wake-to-schedule P99
  on QEMU and on a qualified physical device. QEMU timing is not a latency witness for a
  hardware claim, so this slice proves the *decision* (which hart is asked, and whether the
  request is made at all) and claims no microsecond figure. The physical latency gate stays
  closed.
- **The stale-token negative uses the wire delivery id**, matching the plan's "older
  delivery-token" case; the dead/reused-sender case is covered by the identity half of the
  same predicate and by the existing IPC lanes (`ipc-copy`, `ipc-copy-race`) re-run here.
- **Subagent delegation remains unavailable** (Codex provider quota), so this slice's review
  was a session self-review.
