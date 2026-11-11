---
phase: 10
title: "Lock ranks and scheduler hot paths"
status: in-progress
priority: P2
effort: "6–9d"
dependencies: [9]
tier: thinking
checkpoint: false
---

# Phase 10: Lock ranks and scheduler hot paths

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-7, Medium IPC/sched](../reports/architecture-review-261110.md) · `kernel/src/sync.rs` · `kernel/src/task.rs` · `kernel/src/task/scheduler.rs` · `kernel/src/task/async_ipc.rs` · `kernel/src/task/smp.rs` · `scripts/mutable-globals.toml`

Paths below use the pre-Phase-09 layout; apply the Phase 09 move map.

## Overview

Turn lock order from comments into a checked property, and remove the O(tasks) work done under the global scheduler lock. Keeps the big lock; does not attempt fine-grained locking.

## Key Insights

- Reviewer-cited: one `SCHEDULER: Spinlock<Option<Scheduler>>` (`task.rs:548`), 336+ lock sites; 51 "Lock order" comments in 25 files and no checker; `Spinlock::lock` calls up into `task::smp::answer_tlb_flush_in_spin` (`sync.rs:55`); `reply_in_sched` scans all tasks twice (`async_ipc.rs:626-646`); the deadline sweep iterates all tasks on hart 0 only (`scheduler.rs:2185-2189,2262`); nine `force_unlock_locks()` with no caller.
- All RT tasks pin to hart 1 (`smp.rs:47-49`); CPU accounting exists only under `cfg(cellos_g1_cpu)`.

## Requirements

- Functional: every static `Spinlock` has a `const RANK`; under the test posture a per-hart held-rank stack asserts strictly increasing acquisition; the rank table is generated next to the writable-global inventory; the spin hook is injected, not a direct upward call; operation token → owner index replaces the scans; a deadline min-heap replaces the sweep and is serviced on every hart; orphaned force-unlock functions deleted; RT hart set and last-hart affinity are data, not constants.
- Non-functional: zero cost in non-test postures for the rank check; no new allocation in interrupt context.

## Architecture

- `Spinlock<T, const RANK: u16>` or a `rank()` associated const via a marker type — choose the form that needs the fewest call-site edits; the 51 comments become the initial table.
- Spin hook: `static SPIN_HOOK: AtomicPtr<fn()>` set once by SMP init; `sync.rs` no longer names `task::`.
- Index: `BTreeMap<OpToken, TaskId>` owned by the scheduler; maintained at submit/terminal.
- Deadline heap: bounded, preallocated per the existing operation-storage reservations.

## Assumptions

- **Claim:** the documented orders are mutually consistent (no cycle among the 51 comments).
  **Confidence:** low
  **How to verify:** build the graph from the comments first; a cycle is a finding to resolve with the owner before coding.
- **Claim:** the validate-drop-relock sites tolerate a rank assert (they re-acquire in the same order).
  **Confidence:** medium
  **How to verify:** run the x86 2-CPU domain lane and the RV64 `--harts 2` suite with the assert on.
- **Claim:** per-hart timeout servicing does not conflict with the hart-0 assumptions in `pick_next`.
  **Confidence:** low
  **How to verify:** read `scheduler.rs:2185-2330` and the G1 barrier path.

## Related Code Files

- Modify: `kernel/src/sync.rs`, every static `Spinlock` declaration (89 reviewer-counted), `kernel/src/task/{scheduler,async_ipc,smp,cell_session}.rs`, `kernel/src/task/hart_local/ready.rs`, `kernel/src/task.rs`, `scripts/mutable-globals.toml`, `tools/mutable-globals/` (emit rank column)
- Create: `docs/generated/lock-ranks.md` (generated)
- Delete: the nine `force_unlock_locks()` and the stale reference at `memory/pin.rs:1543`

## Implementation Steps

1. Extract the lock-order graph from comments; resolve conflicts; assign ranks.
2. Add ranks and the test-posture assert; fix violations it finds (each logged as a Surprise with the site).
3. Inject the spin hook.
4. Add the token index; delete the scans; keep an agreement assert for one cycle.
5. Add the deadline heap and per-hart servicing; remove the hart-0 sweep.
6. Gate housekeeping drains in `yield_cpu` behind one "work pending" atomic.
7. RT hart set + last-hart affinity field; move the G1 barrier out of `pick_next` into a wake API; delete force-unlock residue.

## Todo List

- [ ] Rank graph, no cycles
- [ ] Rank assert on, violations fixed
- [ ] Spin hook injected
- [ ] Token index
- [ ] Deadline heap
- [ ] RT set / affinity

## Success Criteria

- [ ] Host test: acquiring two ranked locks in descending order panics under the test posture.
- [ ] x86 2-CPU and 4-CPU domain lanes, RV64 `--harts 2`, AArch64 2-CPU lane pass with the assert enabled.
- [ ] A timeout registered on a hart other than 0 fires while hart 0 is held in a busy loop (new kernel fixture).
- [ ] No `for task in tasks` scan remains on the reply path (code review + grep).

## Risk Assessment

- The assert will find real inversions; each fix is a behaviour change in a concurrency path. Keep fixes one per commit with the triggering lane named. Rollback: internal only, fully revertable.
- Heap sizing: derive from existing per-Cell operation reservations; refusal at submit already exists.

## Security Considerations

Lock inversions are the structural source of the TOCTOU/deadlock class in the audit (03 S-3..S-8). No authority semantics change.

## Next Steps

Phase 11 collapses the IPC mechanisms on top of the indexed operation path.

## Deviation Log

None.
