# S5 handoff request — lock ranks and scheduler hot paths (phase 10)

**Date:** 2026-11-11 · **Plan:** [plan.md](plan.md) · **Prior slice:** S4b Phase 09 closed 2026-11-11 (owner). S1 is still open, only for the post-CI untrack and the signing-blocked release item.

## 1. Scope (measured on `3f1dd7078`, Phase 09 layout)

Phase 10 as planned. It keeps the single `SCHEDULER` lock; there is no fine-grained locking.

| Target | Today |
|---|---|
| Lock order | 50 "lock order" comments in 27 files, no checker; 87 `static … Spinlock`; 357 `SCHEDULER.lock()` sites |
| Spin hook | `sync.rs:55` calls up into `crate::sched::smp::answer_tlb_flush_in_spin` from inside `Spinlock::lock` |
| Reply path | `async_ipc::reply_in_sched` scans every task twice: first to find the token owner, then to clear the caller context (`task/async_ipc.rs:618-641`) |
| Timeouts | The global sweep (timer wakes, heartbeat, async-poll, watchdog) runs on hart 0 only (`sched/scheduler.rs:2207`) |
| Dead code | 9 `force_unlock_locks()` with no caller |
| RT placement | `HART_RT = 1` is a constant (`sched/smp.rs:49`) |

Steps, one commit each. A rank violation found in step 2 gets its own fix commit, naming the lane that found it.

1. **Rank graph.** Build the graph from the 50 comments and check that it has no cycle. A cycle stops the work and comes back to you before any code changes.
2. **Ranks and the assert.** Every static `Spinlock` gets a rank. Under the test posture, a per-hart stack of held ranks asserts that acquisitions strictly increase. In dev and release kernels the check costs nothing. The rank table is generated next to the writable-global inventory (`docs/generated/lock-ranks.md`).
3. **Spin hook injection.** SMP init sets the hook once. `sync.rs` no longer names `sched::`.
4. **Token index.** A scheduler-owned map from operation token to owner replaces both scans. An assert checks for one cycle that the index and the old scan agree.
5. **Deadline heap.** A bounded, preallocated heap, sized from the existing per-Cell operation reservations, is serviced on every hart. The hart-0-only sweep is removed.
6. **Housekeeping gate.** The drains in `yield_cpu` run only when one "work pending" atomic is set.
7. **Data, not constants.** The RT hart set and last-hart affinity become data. The G1 barrier moves out of `pick_next` into a wake API. The `force_unlock_locks` residue is deleted.

## 2. File set (file-bounded exception)

- **Modify:**
  - kernel: `kernel/src/sync.rs`; every file that declares a static `Spinlock` (rank annotation only); `kernel/src/sched/**`, `kernel/src/task.rs`, `kernel/src/task/{async_ipc,tcb,completion,completion_wait}.rs`, `kernel/src/ipc/**`; the files holding the 9 `force_unlock_locks`; `kernel/src/memory/pin.rs` (stale reference)
  - inventories: `scripts/mutable-globals.toml`, `tools/mutable-globals/**` (rank column), `docs/generated/mutable-globals.md`, `kernel/residents.toml` (for any new file)
  - lanes: the RV64 `--harts 2` and AArch64 2-CPU lane scripts, if they need the test posture
- **Create:** `docs/generated/lock-ranks.md`, and one new kernel fixture: a timeout registered on a hart other than 0 fires while hart 0 busy-loops.
- **All steps:** this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`.
- **No `libs/api` / `libs/types` change.** If one turns out to be needed, it stops for CP1.
- **Not touched:** `cells/**`, `libs/viui`, `libs/ostd`. Your UI session keeps those files.

## 3. Behaviour changes the owner should expect

- **Test-posture kernels panic on an inverted lock acquisition.** Each inversion it finds is a real bug fixed in its own commit.
- **Timeouts and heartbeats fire on any hart,** not only hart 0. When hart 0 is busy, timeout latency drops.
- **The reply path costs O(log n) instead of O(tasks).** Results do not change.
- **No ABI change and no authority change.**

## 4. Irreversible or externally visible effects

None. The changes are kernel-internal and can be reverted commit by commit.

## 5. Conflicts

- **The iGPU DMA-export slice** keeps waiting.
- **Your live UI session** works in `cells/` and `libs/`, outside the §2 file set.

## 6. Checkpoint and verification

Phase 10 is not a checkpoint phase. The success criteria are:
- a host test: two ranked locks taken in descending order panic under the test posture;
- with the assert on, the x86 2-CPU and 4-CPU domain lanes, RV64 `--harts 2` and the AArch64 2-CPU lane pass;
- the new off-hart-0 timeout fixture passes;
- no `for task in tasks` scan is left on the reply path.

I also run the usual 3-arch boot and the release lane. One limit: G1 accounting (`g1-cpu-accounting`) is build-checked only; its private lane cannot run here.

## 7. Questions to the owner

1. Grant S5 with the §2 file set.
2. Where is the rank assert on: in the test posture only (`test-hooks`, as planned), or in dev images too, so every lane exercises it? Recommendation: test posture only. The domain, AArch64 test-hooks and RV64 SMP lanes already build test-hooks.
3. If step 1 finds a cycle among the documented orders: stop and ask (planned), or let me pick the order the code actually follows and log it?
