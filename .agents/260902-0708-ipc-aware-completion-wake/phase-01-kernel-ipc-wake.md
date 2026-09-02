---
phase: 1
title: "Kernel IPC Wake and Race Proof"
status: completed
priority: P1
effort: 1d
dependencies: []
tier: thinking
---

# Phase 01: Kernel IPC Wake and Race Proof

> **Required — deviation-log:** Record each Decision / Deviation / Surprise immediately. Choose the smallest reversible response; escalate any ABI- or ownership-breaking change.

## Overview

Make a successfully queued IPC record wake a task parked in `WaitCompletion(NET_RX)` and close the enqueue-before-park race without changing any public completion surface.

## Requirements

- The enqueue and receiver-state decision are serialized by `SCHEDULER`; state changes happen only after `queue_wire_msg` succeeds.
- A compatible `Recv` keeps each producer's current mask/admission behavior. A `WaitCompletion` wake is allowed only when `source == NET_RX`; TIMER remains deadline-only.
- `ipc_send`, `ipc_post_nonblock` (including SendGather and direct kernel posts), and `ipc_try_send` use one wake classifier, then push ready and pend preemption at most once.
- `ipc_send` interrupted-wait delivery retains the busy-target path (`Sending`, `Ok(1)`); post remains nonblocking; TrySend remains rejected unless its existing Recv/trusted-input admission permits publication.
- The wait returns raw `0` and writes no completion record when IPC alone interrupts it. It never consumes the mailbox record.
- NET_RX reservation cleanup is exactly the existing `Owned`/`Completing`/completed state machine, including a real NET_RX record winning a concurrent race.

## Architecture

After publication, a crate-private helper classifies `Recv` using a caller-supplied existing eligibility result or detects `WaitCompletion { source: NET_RX }`, changes only those tasks to `Ready`, and reports the wake cause. Callers retain their distinct return/blocking decisions.

Factor NET_RX end-of-wait cleanup into a non-allocating step (`NoRecord`, `Completion`, or `RetryCompleting`) used by both the normal resumed-without-record path and the new pre-park abort. Under one scheduler lock, the waiter checks `pending_msgs`: nonempty selects cleanup/return-0; empty publishes `WaitCompletion` before unlock/yield. The cleanup loop retries `Completing` until the producer publishes, then drains rather than releasing producer-owned state.

## Assumptions

None — producer admission, scheduler locking, wait registration, raw return semantics, and NET_RX split-publication hooks were read directly from the listed kernel files.

## Related Files

- Modify: `kernel/src/task.rs` — shared post-publication wake classification and all three producers.
- Modify: `kernel/src/task/completion_wait.rs` — atomic mailbox/park decision and shared NET_RX cleanup step.
- Modify: `kernel/src/task/ipc_pending_selftest.rs` — producer, full-mailbox, and pre-park interleaving cases.
- Modify: `kernel/src/task/net_rx_selftest.rs` — IPC-abort versus `Completing` ownership case.
- Modify: `tests/integration/tests/boot.rs` — require both decisive boot self-test markers and reject FAIL markers.

## Implementation Steps

1. Add the scheduler-held wake classifier beside IPC publication. Preserve each call site's current Recv eligibility; add only NET_RX completion-wait classification.
2. Invoke it after successful `queue_wire_msg` in Send, post, and TrySend. Push/preempt once for either wake cause, but branch sender behavior on the original Recv cause so WaitCompletion does not turn blocking Send into eager success.
3. Factor lines currently handling `disarm_net_rx` outcomes into the shared cleanup step/loop; do not call `release(slot)` on `Completing` or manufacture a queue record.
4. In the park critical section, test `pending_msgs.is_empty()` immediately before assigning `TaskState::WaitCompletion`. If false, skip yield, run shared cleanup, and map `NoRecord` to `Ok(0)`.
5. Extend boot self-tests with serialized pre-park and post-publication interleavings. Use the real producer functions; snapshot/restore `INPUT_CELL_TID` when exercising TrySend's already-trusted route.
6. For the pre-park case, queue while the receiver is runnable, invoke the actual atomic park decision, and assert: no park/yield, no completion bytes, no waiter/slot leak, raw no-record outcome, unchanged mailbox record, and no deferred phantom wake.
7. For `Completing`, call `begin_signal_net_rx_for_test`, assert one cleanup step returns retry without release/return-0, call `finish_signal_net_rx_for_test`, then assert the next step drains exactly one real NET_RX completion while the IPC record remains queued.
8. Add full-mailbox cases for all producers: the producer's existing error/block result, WaitCompletion state, mailbox depth, ready-queue membership, and preemption state remain unchanged.

## Success Criteria

- [x] IPC-before-park refuses the park; IPC-after-state-publication readies and enqueues the receiver exactly once without a scheduler tick.
- [x] Send, post/SendGather, and permitted TrySend each queue one unchanged wire record and preserve their prior caller result/state; disallowed TrySend remains disallowed.
- [x] IPC-only interruption selects raw `0`/no record, clears waiter metadata, and releases exactly the waiter's NET_RX slot.
- [x] A forced NET_RX `Completing` interleaving cannot return `0` or release early; it drains the genuine NET_RX completion and leaves IPC queued.
- [x] `cargo test -p api --target x86_64-unknown-linux-gnu` passed 91 tests with no completion-source/layout/authority addition.
- [x] A freshly rebuilt RV64 release kernel passed the exact `ipc_pending_delivery_selftest_passes` integration gate 1/1; the gate required both decisive PASS markers and rejected their FAIL markers.

## Evidence and Results

- The kernel producer path classifies wake eligibility only after successful queue publication, so queue-full/error paths cannot create a wake candidate.
- Deterministic self-tests cover before-park, after-publication, all producer classes, full-mailbox side-effect freedom, and NET_RX `Completing` ownership.
- API: 91 passed. Fresh RV64 release kernel: built successfully. Exact boot gate: 1 passed, 0 failed.
- Final review found no remaining Critical, High, or Medium issue in the kernel publication, cleanup, or admission behavior.

## Security Considerations

Do not broaden TrySend trust, Recv mask matching, frozen/retiring admission, mailbox depth, or task-exit cleanup. Queue-full and copy/admission failures must be side-effect free. The scheduler lock is the linearization boundary; no second queue or unbounded wake list is allowed.

## Risk Notes

The highest risk is confusing “receiver became runnable” with “blocking Send completed”; keep wake cause separate from sender return semantics. A second risk is releasing a slot owned by an in-flight NIC producer; the deterministic split-publication test is a mandatory phase gate.

## Deviation Log

- **No phase-local protocol deviation.** The scheduler linearization point, raw-zero/no-record behavior, producer admission, and NET_RX ownership rules were implemented as planned.