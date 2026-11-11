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

- [x] Rank graph, no cycles
- [x] Rank assert on, violations fixed (none found: the order was learned from the lanes)
- [x] Spin hook injected
- [x] Token index
- [x] Deadline heap (sorted queue; see log)
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

- **Step 1: rank graph (`3f1dd7078` tree).** The 50 "lock order" comments in 27 files yield these nesting edges between static locks:
  - `*_GRANT_TABLE → SCHEDULER`
  - `SCHEDULER → {SWAP_CEILINGS, RETIREMENT_VERDICTS, DEATH_SUBSCRIBERS, ROLE_PUBLICATION, SERVICE_REGISTRY, QUOTA_LIMITS, ACCOUNTING, PIPES, per-hart ready}`
  - `DEATH_SUBSCRIBERS → RETIREMENT_VERDICTS`
  - `ROLE_PUBLICATION → {BLOCK_ROUTES, NIC_DRIVER_STATE, service/role leaves}`
  - `OWNERS → SERVICE_REGISTRY`
  - `CELL_AUTHORITIES → CSPACE_FILES`
  - `RISCV_DOMAINS → CQ_TRANSACTION`
  - `VM_REGISTRY → FRAME_ALLOCATOR → KERNEL_ROOT`
  - `FRAME_ALLOCATOR → stack registry`
  - grant tables `→ gate → receiver slot → KERNEL_ROOT`
  - grant table `→ PIN_REGISTRY`

  Leaves taken alone: completion ring, `NET_RX_WAIT`, pin `REGISTRY` (never across FRAME_ALLOCATOR, KERNEL_ROOT or SCHEDULER). **No cycle**, so no stop is needed.

  The comments cover only part of the 87 static locks. The remaining order is learned. Host tests, with the check active, report each violation, and the table is re-sorted topologically until clean (26 edges, all consistent). Then the test-hooks QEMU lanes run in a temporary record-only mode that logs every observed `held → acquired` pair. The host-only `TEST_STATE_LOCK` (test serialization) ranks first.
- **Steps 2 and 3, one commit (deviation).** The spin hook and the rank check are both new state in `Spinlock::lock`, so they land together.
  - **Form:** `Spinlock::ranked(data, rank::NAME)`, with a `rank` field compiled only under `any(test-hooks, test)`. Dev and release kernels store and do nothing.
  - **Edits:** the 88 static initializers. Instance locks keep `Spinlock::new`, which means unranked.
  - **Held set:** a 128-bit per-hart mask, or a per-thread mask in host tests. Release order is free. `try_lock` records without checking, and `force_unlock` clears.
  - **Hooks:** `sync::install_hart_hooks` (from `task::init`, before any secondary) sets the hart-id reader and the TLB-flush spin answer. `sync.rs` no longer names `sched::`.
  - **Order:** 87 ranks satisfy 125 edges.
    - 26 come from host tests and 33 from the first assert run.
    - 61 more come from a temporary record-all run of the x86 domain (1/2/4 CPU), RV64 native-domain and AArch64 test-hooks lanes.
    - The documented edges from step 1 are included.
    - No cycle. Locks with no observed or documented successor go innermost.
  - **Generated output:** `tools/mutable-globals` now records each static's rank and fails on an unranked static spinlock or an unknown rank. It also writes `docs/generated/lock-ranks.md`, checked by its `--check`.
- **Assert-mode lanes.** These pass with the check on and no `[lockrank]` report: x86 domain 1/2/4 CPU, `pcie-bar-window`, `local-service-lifecycle-x86` and the host tests (both feature sets).
  - **Pre-existing failures** (identical on `c2776928b` source, rebuilt and run as the baseline):
    - `vfs-quota`: missing the `[stack-baseline] name=shell` marker.
    - `qemu-native-domain-test --harts 2`: RV64 "kernel stack overflow: trap entry could not save a frame" after `S22-RV64-SWITCH: PASS`.
    - `qemu-aarch64-test-hooks`: `S22-AARCH64-DOMAIN-LIVE: FAIL asid=0`.
  - All three are manual-tier, except `vfs-quota`, which is `pr`.
- **Step 4: token index.** `Scheduler::async_index` (`task::async_ipc::AsyncIndex`) has two tables:
  - `owners`: `(token, owner)` pairs sorted by token. Rows are added by `push_operation` (submit and the self-test seam), removed by `remove_operation` (take and submit rollback), and pruned by `forget_task` at both task-table removals.
  - `holders`: the task whose request context last received a token, at most one row per task.
  - **Design decision:** a token can be held by more than its operation's `peer_tid`, for example a worker in the host model or a hot-swap replacement that dequeues the wire. Every holder is recorded instead.
    - `Task::set_received_request_context` now returns the token it set (`#[must_use]`).
    - All 9 delivery sites record the holder: 3 in `task.rs`, 6 in `syscall/ipc.rs`.
  - `reply_in_sched`, `take` and `transfer_consumption` use the index, so the reply path has no task scan.
  - Capacity for both tables is reserved at submit (`Busy` on OOM, like the operation record), so delivery and removal do not allocate.
  - **Agreement asserts:** test kernels and host tests compare the index with the old scan, for owners and for holders of the replied token. They stay for one release cycle, then go.
  - Lanes pass with the asserts on: x86 domain 1/2/4 CPU, x86 boot, RV64 and AArch64 boots, `pcie-bar-window`, and lifecycle 3/3 on two reruns (the first run hit the known `multi_source_wait` timeout).
- **Step 5: deadline queue and per-hart sweep.**
  - **Data structure (deviation from "min-heap"):** a sorted `TaskTable::deadlines` vector with at most one row per task. It pops due rows in order and dedupes on re-arm, which a heap does not do cheaply. Next to it, `event_waiters` lists tasks in `WaitEvent`, `WaitCompletion` or `WaitIrq`.
  - **Where rows come from:** the switch-out branch of `pick_next_local` (`TaskTable::note_parked`), since every deadline or event wait is a self-park. There is no hook at each site that sets a state.
  - **Capacity** follows the task rows: `insert` / `reserve_publication`, so parking never allocates. `TaskTable::remove` drops a task's rows.
  - **The sweep** examines only due deadlines and event waiters, ascending by tid like the old scan, with the same per-task body. Before, it scanned every task. Test kernels and host tests assert that every parked task (one not current on any hart) is tracked.
  - **Any hart sweeps.** The hart-0-only gate is removed, so the sweep runs on any hart, still serialized by `SCHEDULER` and gated by `last_global_sweep_tick`. The G1 barrier block stays on hart 0.
  - **Behaviour changes:**
    - Woken tasks are queued on the sweeping hart, not hart 0 (`push_ready` targets the current hart).
    - A task that parks with an already-expired deadline wakes on the next sweep instead of the current one.
    - x86 still advances `system_ticks` only on hart 0, so on x86 time itself stops while hart 0 is held.
  - **Still scanning (out of scope, logged):** the heartbeat sweep and the `Polling` future scan.
  - **New fixture** `sched/off_hart0_timeout_selftest.rs` (test-hooks): a real kernel task parks in `Sleeping`, and hart 0 is held with interrupts masked.
    - AArch64 `-smp 2`: `OFF-HART0-TIMEOUT: PASS harts=2 deadline=17 woke_at=17`.
    - Negative control, with the hart-0 gate restored: `FAIL … still sleeping`.
    - x86 reports SKIP (ticks advance on hart 0 only), as do single-hart boots.
  - **Lanes, with the asserts on:**
    - pass: x86 boot, domain 1/2/4 CPU, lifecycle 3/3, two-node C2C, RV64 boot and `pcie-bar-window`, AArch64 boot;
    - AArch64 test-hooks: fixture PASS, lane still FAIL on the pre-existing `DOMAIN-LIVE` marker.
- **Step 6: housekeeping gate.** `yield_cpu` took `SCHEDULER` six or seven times per pass for its drains, even with nothing to drain: zombies, completion slots, grant reap, quiesced root retirements, VFS context and holder releases.
  - **Change:** those drains now run only when `task::HOUSEKEEPING` is set. Each producer sets it under `SCHEDULER` right after queuing (6 sites in `scheduler.rs`).
  - **Re-arm:** after draining, the consumer re-checks `Scheduler::housekeeping_pending` under `SCHEDULER` and re-arms the flag. Work that is not ready yet (a zombie still current on a remote hart, a root waiting on switch completions) is therefore retried on every pass, as before.
  - **Left outside the gate:** the IOMMU cleanup retry and `dma_export::retry_pending`. They do not take `SCHEDULER`, and the latter already has its own atomic gate.
  - **Lanes pass:** x86 boot, domain 1/2/4 CPU, lifecycle 3/3, two-node C2C, RV64 boot and `pcie-bar-window`, AArch64 boot.
- **Owner decisions for step 7 (2026-11-11):**
  - RT placement uses a runtime RT-hart mask that defaults to every secondary hart. A task wakes on its last hart if that hart is in the set and online, otherwise on the least-loaded RT hart.
  - The G1 barrier move is **deferred** until the G1 lane can run.
