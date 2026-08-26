# Phase 02 — Route the 3 Delivery Sites Through pending_msgs

## Context Links

- Plan: [plan.md](plan.md) · Depends on [Phase 01](phase-01-drain-on-wake-infra.md)
- Research: [research/haily-researcher-02-buffer-pinning-audit.md](research/haily-researcher-02-buffer-pinning-audit.md) (the three unsafe sites this phase closes), [research/red-team-findings.md](research/red-team-findings.md) (heap-quota and mask-preservation concerns still relevant)
- `kernel/src/task.rs:1212-1248` — `ipc_send`'s direct-delivery-into-Recv branch (unsafe write at `1227-1233`)
- `kernel/src/task.rs:1274-1320` — `ipc_post_nonblock` (unsafe write at `1291-1297`), reached from `console_drv.rs:178-222` (shell keyboard input)
- `kernel/src/task.rs:1455-1527` — `ipc_try_send` (unsafe write at `1478-1484`)
- `kernel/src/task/tcb.rs` — `pending_msgs`, `PendingMsg`, depth constants (`HOTSWAP_MSG_QUEUE_DEPTH=64`, `INPUT_EVENT_QUEUE_DEPTH=512`)
- `kernel/src/memory/heap.rs`, `kernel/src/memory/cell_quota.rs` — cell heap-quota accounting the red-team flagged as newly relevant once these sites allocate

## Overview

- **Priority:** P1 · **Status:** completed · **Risk:** medium
- Change the three delivery-side sites so a match against a `Recv`-parked target pushes an owned
  message into `pending_msgs` and wakes the target exactly as today (`push_ready` +
  `pend_preempt_if_needed`, unchanged), instead of writing into the target's `buf_ptr` from the
  sender's context. Phase 01's drain-on-wake logic completes delivery on the receiver's side.

## Key Insights

1. **Wake mechanism is unchanged.** Unlike the original (blocked) design, there is no completion
   queue involved — delivery still does `push_ready(target_id)` +
   `pend_preempt_if_needed(prio)` immediately, exactly as today. RT preemption timing is
   unaffected; this was only a concern in the original design because it deferred wakes through
   `yield_cpu()`.
2. **Match conditions do not change.** `ipc_send`/`ipc_try_send`'s existing mask-honoring guard
   (`mask==0 || mask==sender_id`, deciding *which* Recv-parked target to deliver to) and
   `ipc_post_nonblock`'s existing ignored-mask behavior are both about *finding a match*, not
   about filtering a mailbox afterward — neither needs to change, since this phase only changes
   *what happens once a match is found*, not the match itself.
3. **New allocation on a previously allocation-free path.** Direct-copy delivery never allocated;
   pushing an owned message into `pending_msgs` does. Verification found the existing Frozen path
   uses infallible `to_vec()`: quota exhaustion returns null from `QuotaAlloc`, then the global
   allocation handler loops forever. Add a small fallible owned-copy helper using
   `Vec::try_reserve_exact` before extending the buffer, and use it for both the existing Frozen
   producer and all new Recv-target producers. Allocation failure must return each function's
   existing `Err`/`TryAgain` result before changing task state or waking the receiver.
4. **Depth semantics must match today's producer, not an arbitrary one.** Resolve which depth
   constant governs each of the three call sites: `ipc_send`'s existing Frozen-target push already
   uses one of `HOTSWAP_MSG_QUEUE_DEPTH`/`INPUT_EVENT_QUEUE_DEPTH` — use the *same* constant for
   its Recv-target push, so behavior is consistent within one function. Do the same per-function
   for `ipc_post_nonblock` and `ipc_try_send`, each mirroring their own existing fallback-path
   depth rather than inventing a new shared bound.
5. Preserve `ipc_post_nonblock`'s existing (buggy) ignored-mask behavior verbatim — not fixed here
   (Phase 04 documents it as a known separate issue).

## Requirements

**Functional**
- `ipc_send`'s `Recv`-match branch (`task.rs:1212-1248`): on match, push `PendingMsg{sender_tid,
  data, enqueued_tick}` into target's `pending_msgs` using the same depth constant and
  full-mailbox error behavior as its existing Frozen-target push, then wake via
  `push_ready`/`pend_preempt_if_needed` unchanged. No more `unsafe { copy_nonoverlapping(...) }`
  into `buf_ptr` from this branch.
- `ipc_post_nonblock` (`task.rs:1274-1320`): same change, preserving its existing ignored-mask
  behavior and existing not-in-Recv `pending_msgs` fallback depth/behavior — this phase only
  changes the *is-in-Recv* branch to match what the *not-in-Recv* branch already does.
- `ipc_try_send` (`task.rs:1455-1527`): same change, preserving its mask-honoring guard.
- Full-mailbox behavior on delivery to a `Recv`-parked target must surface the same kind of error
  to the sender that a full mailbox does today for the Frozen/fallback case (do not silently drop;
  do not introduce a new panic/unwrap path on allocation failure).
- Cell-quota/heap allocation failure while creating the owned message must return `Err` without
  entering the global allocation-error loop; no task state or ready queue may change on failure.

**Non-functional**
- Message content and sender identity must reach the receiver identically to today's direct-copy
  path — verified via existing round-trip IPC tests plus Phase 03's expanded matrix.
- No change to observable backpressure behavior beyond what's explicitly required above.

## Architecture

```
// ipc_send / ipc_post_nonblock / ipc_try_send, on match against target in Recv:
// (each site keeps its own existing function signature/error convention —
//  this is the shared shape, not a mandate to unify all three into one helper)

let target = sched.tasks.get_mut(&target_id).expect("matched above");
match push_pending_msg(target, sender_id, msg) {           // same depth/error semantics as
    Ok(()) => {                                             // this function's existing
        sched.push_ready(target_id);                        // Frozen/fallback push
        let prio = /* existing priority lookup */;
        sched.pend_preempt_if_needed(prio);
    }
    Err(_) => { /* same error this function already returns when its existing
                   fallback push_pending_msg call hits a full mailbox */ }
}
```

## Related Code Files

**Modify**
- `kernel/src/task.rs` — add/reuse a private fallible owned-message copy helper and migrate the
  existing Frozen/fallback allocation sites needed to guarantee graceful failure
- `kernel/src/task.rs:1212-1248` — `ipc_send` Recv-match branch
- `kernel/src/task.rs:1274-1320` — `ipc_post_nonblock`
- `kernel/src/task.rs:1455-1527` — `ipc_try_send`
- `kernel/src/task.rs:1529` — `ipc_reply` is read-only verification context; no change required

**Create**
- None expected — reuse each function's existing `pending_msgs` push helper/pattern

## Implementation Steps

1. Carry forward Phase 01's verified findings: `ipc_reply` needs no producer edit, while removing
   all three foreign-buffer writes also prevents delivery through `RecvScatter`'s stale temp pointer.
2. Add a private fallible message-copy helper based on `Vec::try_reserve_exact`; migrate the
   existing Frozen/fallback push allocations that the new Recv branches mirror.
3. For each of the three sites, identify its existing `pending_msgs` push call for the
   non-Recv/fallback case and reuse the exact same depth constant and error path for the new
   Recv-match case.
4. Replace each site's `unsafe { copy_nonoverlapping(...) }` block with the push-then-wake pattern
   above.
5. Verify allocation-failure behavior explicitly (force a full mailbox / simulate quota
   exhaustion in a test) and confirm it surfaces as the existing error type, not a hang.
6. Run full existing IPC test suite — round-trip send/recv, non-blocking send into an idle
   receiver, TryRecv, RecvTimeout.
7. Manually trace the shell keyboard input path end to end (`console_drv.rs` → `ipc_post_nonblock`
   → `pending_msgs` → Phase 01's drain-on-wake → shell resumes) to confirm no silent drop.

## Todo List

- [x] Rewrite `ipc_send` Recv-match delivery
- [x] Rewrite `ipc_post_nonblock` Recv-match delivery
- [x] Rewrite `ipc_try_send` Recv-match delivery
- [x] Make owned-message allocation fallible for the affected existing and new producers
- [x] Confirm `ipc_reply` needs no producer change
- [x] Confirm zero remaining `unsafe { copy_nonoverlapping }` writes into a foreign `Recv.buf_ptr`
- [x] Verify allocation-failure/full-mailbox behavior is graceful, not a hang
- [x] Run full IPC test suite
- [x] Manually trace shell keyboard input path

## Evidence

- `PendingMsgData` keeps IRQ-sized messages inline and tracks the receiver CellId for fallible
  heap-backed payload allocation and refund.
- `ipc_pending_delivery_selftest_passes`, `console_near_depth_burst_is_lossless`, and
  `input_keyboard_e2e` passed against the final release kernel.

## Deviation Log

- Review required a focused `PendingMailbox` module so mailbox-container allocations remain
  kernel-owned while payload quota remains receiver-owned; this replaces the plan's initial
  expectation that no new module would be needed.

## Success Criteria

- The three unsafe sites named in
  [research/haily-researcher-02-buffer-pinning-audit.md](research/haily-researcher-02-buffer-pinning-audit.md)
  no longer write into a foreign task's `buf_ptr`.
- All existing IPC round-trip, non-blocking-send, TryRecv, and RecvTimeout tests pass with
  identical observable results (message content, sender identity, backpressure errors).
- Shell keyboard input integration test passes.
- A deliberately-exhausted mailbox/quota scenario produces the existing graceful error, not a
  hang or panic.

## Risk Assessment

- **Depth-semantics mismatch** — mitigated by mirroring each function's *own* existing fallback
  depth rather than picking one constant for all three; verify per-function, not once globally.
- **Heap-quota exhaustion under load** — new allocation on a previously allocation-free path; test
  explicitly rather than assuming today's Frozen-path handling transfers automatically.
- **`ipc_reply` untouched if it doesn't route through `ipc_send`** — would leave a fourth
  direct-write site unaddressed; must be resolved by Phase 01's investigation before this phase is
  considered complete.

## Security Considerations

- This phase is exactly what closes the buffer-pinning hazard the ADR requires be audited and
  fixed ahead of any future executor change — closing it now, independent of whether the
  completion-queue migration ever happens, removes a real memory-safety hazard on its own merits.

## Next Steps

- Phase 03 exercises this phase's change under the full test matrix, including the adversarial
  cases the red-team identified (RecvScatter, ipc_reply regression, allocation failure, depth
  mismatch).
- Phase 04 documents the items this phase deliberately does not fix, plus a note on why the
  completion-queue migration itself is deferred.

## Assumptions

- **Claim:** Each of the three functions already has its own `pending_msgs` push helper for the
  non-Recv/fallback case that can be reused verbatim for the new Recv-match case.
  **Confidence:** high — confirmed by research (`task.rs:1195-1199` for `ipc_send`'s Frozen push,
  analogous existing pushes for the other two).
- **Observed:** The existing Frozen-path `to_vec()` allocation is not graceful on quota exhaustion;
  `QuotaAlloc` returns null and `alloc_error_handler` loops forever. Phase 02 must replace it with
  a fallible reserve-and-copy path before reusing the producer pattern.
