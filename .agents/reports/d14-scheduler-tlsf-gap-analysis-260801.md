# D14 — Scheduler and TLSF gap rows

**Status:** analysed 2026-08-01; docs-only ruling applied 2026-08-01. No runtime or ABI
change has been authorised.

**Ruling:** Recommendation A approved and applied. The scheduler and TLSF rows were
corrected in docs only; Phase 25 stays historical.

**Question from the docket:** delete the two open rows that say the scheduler has no
priorities and TLSF is not implemented; redefine Phase 25 as EDF/CPU-budget only; decide
whether Spec 02's TLSF `O(1)` claim has been measured.

## Answer first

**Do not approve the docket's compound yes/no as written.** The two rows have different
answers:

1. **Scheduler row: delete or replace.** Cellos has a real fixed-priority scheduler with
   the three public tiers `Background < Normal < RealTime`, FIFO within a tier,
   RT-hart routing when the second RV64 hart is online, and an RV64 software-interrupt
   preemption path. The old no-priority scheduler claim is false.
2. **TLSF row: do not delete. Rewrite it.** The `rlsf` allocator is compiled, its 256 KiB
   pool is initialised during boot, and wrappers exist, but no runtime allocation calls
   those wrappers. Stacks still come from the bitmap frame allocator. The documented
   system property “RT cells use TLSF” is not implemented.
3. **Do not retroactively redefine Phase 25.** It is a closed historical phase and commit.
   Put any EDF, admission-control, or execution-budget work in a new follow-up. More
   importantly, EDF is not yet an accepted requirement; fixed-priority scheduling may be
   the intended policy.
4. **Spec 02's system-level `O(1)` claim has not been measured on Cellos.** The dependency
   documents constant-time TLSF operations and publishes an STM32 measurement, but Cellos
   has no TLSF benchmark, no caller, and no worst-case bound including its spinlock.

## 1. What is genuinely implemented in the scheduler

The public API defines exactly three ordered tiers
(`libs/api/src/abi/task.rs:3-21`). Ready queues are keyed by numeric priority and
`pick_local` scans them in descending order, preserving FIFO within a queue
(`kernel/src/task/hart_local/ready.rs:12-43`). `Scheduler::push_ready` sends values at or
above `RealTime` to the dedicated RT hart when it is online
(`kernel/src/task/scheduler.rs:127-149`).

On RV64, waking a higher-priority task pends SSIP locally or sends a cross-hart IPI
(`kernel/src/task/scheduler.rs:152-191`). The non-RV64 implementation is explicitly a
no-op (`:194-197`), so the immediate-preemption guarantee is architecture-scoped even
though priority selection itself is portable.

The code also contains RT-oriented runtime scenarios:

- `preempt_latency` measures RT wake-to-run latency under Normal-priority load;
- `control_loop_jitter` measures periodic jitter and deadline misses;
- both are exercised through the bench cell and referenced by the boot integration lane.

These establish a meaningful verification design, but the consolidated performance report
still says the latency baseline is pending (`docs/performance-report.md:57-76`). Therefore
the implementation claim is supportable; a cross-architecture bounded-latency claim is not.

### Scheduler defects hidden by the stale row

Deleting the old row without qualifying the replacement would hide two concrete contract
bugs:

1. `SpawnPinned` accepts an arbitrary `u8` and writes it directly into the TCB
   (`kernel/src/task/syscall.rs:2676-2689`, `:2757-2761`). Values `3..=255` outrank
   `RealTime` and are routed as RT because the scheduler uses `>= 2`, although the API
   defines only `0..=2`. The cluster-mode prohibition checks `priority == 2`, so an
   authorised spawner can request `3` and bypass that policy while still receiving RT
   treatment (`:2736-2755`).
2. The ordinary spawn path inserts the new task into a ready queue while its priority is
   still `Normal` (`kernel/src/task/scheduler.rs:225-293`). `SpawnPinned` changes the TCB
   only after `spawn_from_path` returns and does not remove/reinsert the ready entry. The
   task's first dispatch is consequently queued as Normal; its requested priority takes
   effect only after a later requeue.

`core_id` also accepts only zero (`kernel/src/task/syscall.rs:2685-2689`); RT placement on
hart 1 is an internal policy, not caller-selected pinning. The syscall name and comments
should not imply general CPU affinity.

The old manual Phase-25 tests are not reliable evidence for current code. They mutate a
task's TCB priority after it was enqueued and still reference removed scheduler fields such
as `current_task_id` and `ready_queues` (`kernel/src/task/tests.rs:34-98`, `:260-294`). A
normal kernel check passes, but a test-target check cannot run on the bare-metal target
because the Rust `test` crate is unavailable; these functions are not a maintained host
test lane.

## 2. TLSF exists as code but not as an allocation path

The repository links `rlsf 0.2.3` (`kernel/Cargo.toml`, `Cargo.lock`) and defines a
`Tlsf<'static, u32, u16, 20, 16>` over a 256 KiB static pool
(`kernel/src/memory/rt_heap.rs:14-31`). Boot calls `memory::rt_heap::init()` after the main
heap is ready (`kernel/src/main.rs:458-475`). The module exposes `alloc` and `dealloc`
wrappers (`kernel/src/memory/rt_heap.rs:64-87`).

Repository-wide call-site inspection gives the decisive result:

- `rt_heap::init` is called once;
- `rt_heap::force_unlock_locks` is called from fault teardown;
- **`rt_heap::alloc` and `rt_heap::dealloc` have no callers.**

All cell/task stacks instead use `Stack::new_kernel` and `Stack::new_user`, which allocate
contiguous physical frames from `FRAME_ALLOCATOR`, install page mappings, and create an
unmapped guard page (`kernel/src/task/stack.rs:51-200`). This is structurally different
from returning bytes inside a static TLSF array.

The roadmap's statement that “RT cells use `rt_alloc()` for stacks” is therefore false
(`docs/project-roadmap.md:919-930`). There is no function named `rt_alloc` in the current
tree.

### The current pool cannot hold one current task's stack pair

`STACK_PAGES` is 64, or 256 KiB usable per stack (`kernel/src/task.rs:38-40`). Each kernel
and user stack also reserves one 4 KiB guard frame. One task therefore reserves:

```text
2 * (64 + 1) * 4096 = 532,480 bytes
```

The RT TLSF pool is 262,144 bytes. Even if TLSF were wired directly to stacks and guard
pages were ignored, the pool is only half the size of one kernel+user stack pair. The
module comment saying 256 KiB is enough for “~4 RealTime cells with 64 KiB stacks” reflects
an older stack size, not the current system (`kernel/src/memory/rt_heap.rs:21-27`).

TLSF also cannot simply replace the frame-stack path: stacks require page alignment,
page-table permissions, physical-frame ownership, and a deliberately unmapped guard page.
The correct future use must be specified first—cell dynamic heaps, a kernel RT object pool,
or stack backing through a redesigned virtual-memory allocator are different mechanisms.

## 3. What `O(1)` currently means—and does not mean

The upstream `rlsf` contract says allocation and deallocation complete in constant time;
its implementation documents the relevant operations likewise. That supports an
**algorithmic-complexity statement about `Tlsf::allocate/deallocate`**.

It does not establish the current Spec 02 system claim:

- Cellos never invokes those operations in a runtime allocation path.
- There is no Cellos scenario reporting TLSF allocation latency, p99/max, fragmentation
  state, OOM behaviour, or concurrent RT-hart contention.
- The local wrapper serialises the entire pool with a spinlock. Upstream explicitly notes
  that lock contention changes worst-case execution time to depend on the number of
  contenders. Normal/AI heap traffic cannot take this dedicated lock, but multiple RT
  users could—once callers exist.
- Big-O alone is not a real-time bound. A release claim needs a measured or analytically
  bounded WCET in cycles/time for the actual architecture, build mode, lock protocol,
  request-size/alignment range, and fragmentation envelope.

The only current allocator measurement in `docs/performance-report.md` is total
allocator-committed memory (129.49 MiB). It says nothing about TLSF latency.

An accurate present-tense Spec 02 statement would be:

> The kernel contains an isolated TLSF pool whose allocator operations are algorithmically
> constant-time, but no production allocation path consumes it and Cellos has not qualified
> its end-to-end worst-case latency.

## 4. EDF and CPU budgets are separate policy decisions

No EDF scheduler exists. Existing `deadline` fields belong to blocking/wakeup operations,
heartbeat detection, and benchmark accounting; ready-task ordering does not use deadlines.

The scheduler has a coarse CPU-monopoly watchdog: 500 consecutive 10 ms ticks (5 seconds),
with an 80% warning. That is a runaway-cell safety fuse, not per-period execution-budget
enforcement, replenishment, admission control, or deadline scheduling
(`kernel/src/task/scheduler.rs:49-66`). Calling it a CPU-budget scheduler would overstate
the mechanism.

Phase 25 is already recorded complete in the roadmap and corresponds to commit `1012e21f`
(`feat(kernel,hal): Phase 25 — priority scheduler with timer preemption + RT heap`). Editing
that historical phase to mean EDF/budgets would erase what was actually attempted and make
old references ambiguous. Use a new phase/deliverable if those policies are approved.

## 5. Recommended ruling [FINAL]

**Approve recommendation A — split the false compound close into one stale row and one
real implementation gap.**

1. Delete the old no-priority scheduler wording. Replace the scheduler description
   and stale performance-report paragraph with the shipped fixed-priority model and its
   architecture limits.
2. Replace the stale TLSF row with:
   **“TLSF engine initialised but unused; RT allocation path and WCET qualification absent.”**
3. Keep Phase 25 closed as historical. Open a separately named RT-scheduling/allocator
   qualification follow-up only after deciding whether fixed-priority scheduling or EDF is
   the intended architecture.
4. Correct the roadmap/README/pattern claims that RT stacks or cell allocations already use
   TLSF.
5. Before claiming the priority contract complete:
   - reject priorities outside `TaskPriority` before performing the spawn;
   - make priority assignment and ready-queue insertion atomic;
   - enforce the cluster RT prohibition using the validated enum, not `== 2`;
   - add maintained tests for invalid priorities, first dispatch, FIFO ties, RT wake
     preemption, and non-RV64 behaviour.
6. Before claiming Spec 02's RT allocation guarantee:
   - choose the resource TLSF owns;
   - size/account the pool against the current resource model;
   - wire real callers and OOM/deallocation paths;
   - benchmark worst-case latency under fragmentation and maximum supported contention;
   - state the supported architectures and release bound.

### Rejected alternatives

- **Delete both rows:** treats file presence and boot initialisation as delivered behaviour;
  contradicted by the absence of any allocation caller.
- **Rename Phase 25 to EDF/CPU-budget:** rewrites history and assumes EDF is already the
  chosen scheduling policy.
- **Keep both rows unchanged:** preserves a plainly false statement that priority levels do
  not exist and hides useful shipped scheduler work.
- **Treat upstream TLSF measurements as Cellos qualification:** measures a different board,
  integration, lock context, workload, and version boundary; it is evidence for the
  dependency, not the Cellos system guarantee.

## Verification performed

- Repository-wide static call-site and documentation search.
- Normal WSL build check: `cargo check -p vicell-kernel` — **PASS**.
- `cargo check -p vicell-kernel --tests` — **not a valid bare-metal test lane**; it fails
  because the target has no Rust `test` crate, before scheduler test compatibility can be
  compiled. This result is recorded as a verification gap, not a kernel failure.
