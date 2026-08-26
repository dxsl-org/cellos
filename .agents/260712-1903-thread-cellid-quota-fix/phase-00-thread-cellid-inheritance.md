# Phase 00 — Route spawned threads under the parent cell's CellId + CapSet

## Context Links
- Design authority: `.agents/260712-1836-mythos-g123-analysis/dossier-5-thread-cellid.md`
- Plan: `./plan.md`
- Specs: `docs/specs/15-kernel-boundary.md` (kernel-internal, whitelist §Cell lifecycle),
  `docs/specs/02-memory.md` (per-cell quota model)
- Proven-pattern reference: `kernel/src/loader.rs:174-186` (identical CellId(tid)
  correction on the cell-spawn path)

## Overview
- **Priority:** P1 (HIGH severity — defeats G1 graduation criterion #2; LIVE not latent)
- **Status:** pending
- **Description:** Change the `Syscall::Spawn` handler so a spawned thread inherits
  its parent cell's `CellId` and `CapSet` instead of the hardcoded `CellId(0)`. This
  makes the thread's heap/stack allocations chargeable against the parent's quota and
  scopes it to the parent's authority. Kernel-internal; no ABI/Law-1 change.

## Key Insights (verified at HEAD)
- **The bug:** `syscall.rs:1148-1159`, `Syscall::Spawn { entry, arg }` calls
  `super::spawn_with_arg(name, CellId(0), …)` at `:1153`. TODO acknowledging the hole
  at `:1151-1152`.
- **Why CellId(0) escapes quota:** `cell_quota::charge(cell_id_raw, size)`
  (`cell_quota.rs:85-104`) returns `true` immediately when `cell_id_raw == 0`
  (`:86-88`, "kernel itself: unlimited"). `refund` (`:111-119`) short-circuits the
  same way. So every allocation a `CellId(0)` thread makes is uncounted.
- **How the charge is routed (the fix's correctness proof):** quota is keyed by
  `hart_local::current_cell_id()` (`hart_local.rs:149-161`), which the scheduler
  writes from `next_task.cell_id.0` on every context switch
  (`scheduler.rs:751`, `set_current_cell_id(next_task.cell_id.0 as usize)`).
  Therefore **setting the thread task's `cell_id` to the parent's is sufficient** —
  the instant the thread is scheduled, its allocations charge to the parent cell. No
  other plumbing is needed. This is exactly what `loader.rs:181` relies on for the
  cell-spawn path.
- **Thread creation today:** `spawn_with_arg` (`task.rs:379-391`) → `spawn_thread`
  (`scheduler.rs:237-254`) → `Task::new(next_task_id, cell_id, …)` (`:245`).
  `Task::new` (`tcb.rs:327-376`) starts with **all caps `None`** (`:352-358`),
  `syscall_allowlist: u64::MAX` (`:366`), `pku_key/pku_value: 0` (`:361-362`),
  `priority: Normal` (`:363`).
- **CapSet vehicle:** `CapSet::of_task(&parent)` (`cap.rs:147-156`) snapshots the 6
  transferable caps (block_io/network/spawn/hypervisor/mmio_devices/block_regions);
  `CapSet::apply_to(&mut thread)` (`cap.rs:196-203`) writes them in. The three
  singleton/init-only caps (`supervisor_cap`, `pcie_driver_cap`, `platform_cap`,
  `tcb.rs:216-224`) are intentionally NOT in `CapSet` — `platform_cap` is "at most
  one holder ever" (`tcb.rs:223`), so they MUST NOT be copied to a thread.
- **Blast radius is small:** the only userspace entry is `ostd::sys_spawn(entry,arg)`
  (`libs/ostd/src/syscall.rs:219-228`, `ViSyscall::Spawn = 5`). Grep shows no cell in
  `cells/` currently calls it — consistent with the dossier's "under-used /
  latent-in-practice" note. Low regression risk.
- **Caller availability:** `handle_syscall(caller_id, syscall)` (`syscall.rs:785`) —
  `caller_id` is the caller's task id; its `cell_id` and caps are read from
  `SCHEDULER.lock().tasks.get(&caller_id)`.

## Requirements
### Functional
1. A thread spawned via `Syscall::Spawn` runs with `task.cell_id == parent.cell_id`.
2. The thread's TCB caps equal `CapSet::of_task(parent)` (the 6 transferable caps).
3. **(SCOPE EXPANDED — user-confirmed 2026-07-12)** The thread also inherits the
   parent's **`syscall_allowlist`** and **`pku_key`/`pku_value`**. All identity-scoped
   axes are closed together, not just quota+CapSet. Rationale: `Task::new` defaults
   `syscall_allowlist = u64::MAX` (permit-all, `tcb.rs:366`) and `pku_key/value = 0`
   (`tcb.rs:361-362`), so a syscall-restricted or PKU-fenced cell could otherwise
   spawn a permit-all / wrong-domain thread — the SAME root-cause escape via a
   different field.
4. Allocations made by the thread charge against the parent cell's quota (verifiable:
   parent `cell_quota::in_use` rises; parent gets OOM-bounded when threads exceed its
   limit).
5. Nested thread-spawn (a thread spawning a thread) transitively inherits the real
   cell id + all inherited identity fields — falls out automatically because the
   caller's TCB already carries the correct values.

### Non-functional
5. No change to `libs/api` or `libs/types` (no Law-1 surface). No new syscall.
6. Fail-safe: if the caller's TCB cannot be resolved, the spawn returns `Err` — it
   MUST NOT fall back to `CellId(0)` (that would reopen the hole).
7. `#![forbid(unsafe_code)]` unaffected (Law 4) — this is safe kernel code.

## Architecture
### Data flow (after fix)
```
cell C (tid=P, cell_id=CID, caps=K)  ── sys_spawn(entry,arg) ──▶ handle_syscall(P, Spawn)
  1. snapshot under SCHEDULER lock:  CID = tasks[P].cell_id
                                     K   = CapSet::of_task(tasks[P])
                                     (+ allowlist, pku — see Open Q)         [lock drop]
  2. tid_T = spawn_with_arg(name, CID, drivers, entry, arg)   // cell_id fixed at birth
  3. re-lock SCHEDULER; tasks[tid_T]:  K.apply_to(task)       // caps shared
                                       (+ allowlist, pku if in scope)
  4. return tid_T
        │
        ▼ on first context switch: scheduler.rs:751 set_current_cell_id(CID)
        ▼ thread alloc N bytes  → QuotaAlloc → charge(CID, N)  → counted ✔
```
Mirrors `loader.rs:174-186` exactly (spawn with placeholder, then fix-up under lock).
Chosen over threading the values through `spawn_thread` to keep the change local and
the established pattern intact (DRY with the cell-spawn correction).

### Identity-scope decision (locked; scope expanded 2026-07-12)
- Thread inherits the **6-field `CapSet`** of the parent — "same cell, more TIDs."
- **Thread inherits `syscall_allowlist` and `pku_key`/`pku_value`** (user-confirmed).
  These are per-task identity fields whose `Task::new` defaults (permit-all allowlist,
  key 0) would otherwise let a thread escape the parent's restriction. Inheriting them
  makes the thread a faithful continuation of the cell on every axis the kernel gates.
- `supervisor_cap` / `pcie_driver_cap` / `platform_cap` **do NOT propagate**
  (singleton / init-only invariants). Documented limitation: a thread of the
  platform/PCIe/supervisor cell cannot itself call those privileged syscalls; the
  primary TID must. Acceptable — preserves "single holder" and is safe-by-default.
- **The exclusion asymmetry is deliberate:** transferable authority (CapSet,
  allowlist, PKU domain) is *shared* because a thread IS the cell; *singleton* caps are
  *withheld* because duplicating them breaks a one-holder invariant. "Same cell" does
  not mean "same singleton-token holder."

## Related Code Files
### Modify
- `kernel/src/task/syscall.rs` (~:1148-1159) — resolve parent `cell_id` + `CapSet`,
  pass real `cell_id` to `spawn_with_arg`, apply caps to the new thread, fail-safe on
  unresolved caller. Delete the stale TODO.
### Create
- Test-hooks selftest for the quota-charge regression (location TBD in Step 5 —
  extend `kernel/src/layer2_selftest.rs` under `#[cfg(feature = "test-hooks")]`, or a
  focused sibling module).
### Read-only reference (do not edit)
- `kernel/src/loader.rs:174-186`, `kernel/src/task/cap.rs:147-203`,
  `kernel/src/memory/cell_quota.rs:85-125`, `kernel/src/task/scheduler.rs:751`.

## Implementation Steps
1. **VERIFY FIRST (mandated).** Confirm the charge/uncharge keying end-to-end:
   `charge`/`refund`/`in_use` key on the raw cell id (`cell_quota.rs:85-125`); the
   live id comes from `current_cell_id()` (`hart_local.rs:149-161`) set from
   `task.cell_id` at `scheduler.rs:751`. Confirm setting the thread's `cell_id`
   routes the charge — the cell-spawn path (`loader.rs:181`) is the working proof.
   Record the confirmation in the PR/commit body. Do not proceed until confirmed.
2. In `Syscall::Spawn`, before calling `spawn_with_arg`: lock `SCHEDULER`, look up
   `caller_id`; copy out `parent_cell_id = task.cell_id`,
   `parent_caps = CapSet::of_task(task)`, `parent_allowlist = task.syscall_allowlist`,
   `parent_pku = (task.pku_key, task.pku_value)`. If the caller is absent → return
   `Err(SyscallError::Unknown)` (fail-safe, Req 6). Drop the lock.
3. Call `spawn_with_arg(name, parent_cell_id, drivers, entry, arg)` → `tid`.
4. Re-lock `SCHEDULER`; on `tasks[tid]`: `parent_caps.apply_to(task)`; set
   `task.syscall_allowlist = parent_allowlist`; set `task.pku_key = parent_pku.0` and
   `task.pku_value = parent_pku.1`. (Scope-expanded — all identity fields, confirmed.)
5. Add the disambiguating regression selftest (Step-1-verified oracle):
   register a synthetic cell id `C` with a known quota limit via the quota API;
   spawn a thread under `C` whose entry allocates a known `N` bytes and parks;
   schedule until it runs; assert `cell_quota::in_use(C)` rose by `≈ N`
   (pre-fix delta = 0). Emit a decisive serial line, e.g.
   `THREAD-QUOTA-CHARGE: PASS` / `FAIL` (serial-probe oracle — no screendump).
6. `cargo check` + `cargo clippy -- -D warnings` for the kernel on all 3 arches.
7. Boot-green regression on riscv64 / aarch64 / x86_64 (existing console suite).

## Todo List
- [ ] Step 1 — verify charge/uncharge keying; record confirmation
- [ ] Step 2 — snapshot parent cell_id + CapSet, fail-safe on missing caller
- [ ] Step 3 — pass real cell_id to spawn_with_arg
- [ ] Step 4 — apply parent CapSet + syscall_allowlist + pku_key/value to the thread
- [ ] Step 5 — add N-byte quota-charge regression selftest + serial oracle
- [ ] Step 6 — clippy/check clean on 3 arches
- [ ] Step 7 — boot-green on riscv64 + aarch64 + x86_64
- [ ] Remove stale TODO at syscall.rs:1151-1152

## Success Criteria
- **Disambiguating regression test (required):** the N-byte quota selftest asserts
  parent-cell `cell_quota::in_use` rises by `≈ N` after a thread allocates `N`.
  **Fails on HEAD (delta = 0), passes after the fix.** Serial oracle
  `THREAD-QUOTA-CHARGE: PASS`.
- Spawned-thread TCB satisfies `task.cell_id == parent.cell_id`,
  `CapSet::of_task(thread) == CapSet::of_task(parent)`,
  `thread.syscall_allowlist == parent.syscall_allowlist`, and
  `thread.pku_key/pku_value == parent.pku_key/pku_value`.
- **Allowlist-escape negative check:** a cell with a restricted `syscall_allowlist`
  (a bit cleared) spawns a thread; the thread issues the denied syscall → rejected.
  On HEAD the thread would be permit-all and succeed.
- **Boot-green on all 3 arches** (riscv64, aarch64, x86_64) — no regression in the
  console suite.
- `cargo clippy -- -D warnings` clean; zero `libs/api` / `libs/types` diff.

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A privileged cell's thread now inherits block_io/network/spawn caps it didn't have (empty before) | Low | Med | This is the LOCKED intended semantics (thread = same cell). Singleton caps (supervisor/pcie/platform) still excluded. Blast radius tiny (sys_spawn unused in `cells/`). |
| Thread that previously ran uncharged now hits parent quota → parent OOM-killed | Low | Med | Correct behavior (the whole point). Verify boot cells that spawn threads (none found today) still fit their quota; bump limit if a real user appears. |
| Fix-up-after-spawn race: thread scheduled before caps applied | Low | Low | `spawn_thread` inserts as `Ready`; on SMP it could be picked before Step-4 re-lock. Mitigate: apply caps under the same lock acquisition, or spawn in a not-yet-ready state then mark Ready after apply. Decide in Step 4. |
| Deadlock: reading caller under SCHEDULER lock then calling spawn_with_arg (which re-locks) | Med | High | Snapshot-then-drop-lock BEFORE spawn_with_arg (Step 2 drops lock; Step 3 spawns; Step 4 re-locks). Never hold across spawn. Mirrors loader.rs. |
| syscall_allowlist / pku NOT inherited → thread escapes parent's syscall restriction or protection domain | — | — | **RESOLVED (scope expanded 2026-07-12):** both inherited in Step 4; allowlist-escape negative check added to success criteria. No longer a residual hole. |

## Security Considerations
- **Closes a privilege/quota escape**, does not open one. The thread gains exactly the
  parent's transferable authority — no elevation beyond the cell it belongs to.
- **Singleton-cap invariants preserved:** `platform_cap` "at most one holder ever" and
  init-only `supervisor_cap` are deliberately not duplicated to threads.
- **Fail-safe default:** unresolved caller → deny spawn, never `CellId(0)`.
- **All identity-scoped axes closed (scope expanded, confirmed 2026-07-12):**
  `cell_id` (quota), `CapSet` (authority), `syscall_allowlist` (syscall surface), and
  `pku_key/value` (x86 protection domain) are all inherited. There is no longer a
  same-root-cause escape via a sibling identity field — a restricted cell cannot spawn
  a less-restricted thread on any axis the kernel gates.

## Next Steps
- Depends on nothing; kernel-internal.
- Thematically adjacent to the P-TRUST / cap-ceiling quota work
  (`.agents/260712-1100-*`) — can share a kernel-touch cook window.
- After merge: re-audit any future cell that adopts `sys_spawn` for worker threads to
  confirm quota sizing.
