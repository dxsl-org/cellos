---
title: "Fix thread-spawn memory-quota escape (Syscall::Spawn CellId(0))"
description: "Spawned threads run under CellId(0) → allocations bypass per-cell quota, defeating G1 graduation criterion #2. Route threads under the parent cell's CellId + CapSet."
status: done (kernel-side) — VFS-side tracked elsewhere, see Closure Note
priority: P1
effort: 1 phase (~half day)
branch: main
tags: [kernel, quota, security, g1-graduation, thread-spawn]
created: 2026-07-12
closed: 2026-07-27
---

# Thread-spawn CellId quota-escape fix

> ## Closure Note (2026-07-27)
>
> **Kernel-side: DONE.** `Syscall::Spawn` now snapshots the parent's identity and spawns the thread
> under the parent cell's `CellId` + transferable `CapSet` + syscall allowlist + PKU domain
> (`kernel/src/task/syscall.rs:1415-1450`). The code carries an explicit comment: *"Fail-safe: an
> unresolved caller DENIES the spawn — it must never fall back to CellId(0), which is exactly the
> quota-escape this closes."* Singleton caps (supervisor/pcie_driver/platform) are deliberately not
> propagated. The line reference in "Problem" below (`syscall.rs:1153`) is stale.
>
> **Still open — VFS-side, tracked in `.agents/260727-2101-midori-lessons-cellos/` phase 02:**
> the kernel now gives a thread its parent's `cell_id`, but VFS never reads it — it builds
> `types::CellId(sender as u64)` from the **tid** (`cells/services/vfs/src/dispatch.rs:49`, `:113`,
> `:124`). For a loader-spawned cell `CellId(tid) == cell_id` by coincidence; for a **thread** VFS
> fabricates a CellId matching no cell, so its quota charges a phantom bucket instead of the parent's.
> **Latent, not live**: `sys_spawn` exists in ostd (`libs/ostd/src/syscall.rs:233`) but no cell calls
> it yet. Fix = kernel attests `cell_id` on the IPC (Law 1 change already scoped in that plan's
> phase 02), so one ABI change serves both the ACL and quota accounting.

## Problem (one line)
`Syscall::Spawn` (kernel/src/task/syscall.rs:1153 — **stale ref, see Closure Note**) hardcodes
`CellId(0)` for every spawned thread. `CellId(0)` is the kernel cell, whose allocations short-circuit the
quota `charge()` (kernel/src/memory/cell_quota.rs:86-88). A cell that spawns threads
therefore has UNCHARGED, unbounded memory — a live violation of G1 graduation
criterion #2 ("bounded memory enforced on EVERY write path").

## Locked design (from dossier-5 + scope expansion confirmed 2026-07-12)
- A spawned thread runs under its **parent cell's `CellId`** (resolved from the
  caller's TCB), never `CellId(0)`.
- A thread **shares the parent cell's `CapSet`** — it is the same cell running more
  TIDs, not a lesser-privileged child. Deliberate, not `Task::new`'s empty default.
- **Scope expanded (user-confirmed):** the thread also inherits the parent's
  **`syscall_allowlist`** and **`pku_key`/`pku_value`**. `Task::new` defaults these to
  permit-all / key-0, so without inheritance a restricted cell could spawn a
  less-restricted thread — the same root-cause escape via sibling identity fields. All
  identity-scoped axes are now closed together. Singleton caps
  (`supervisor`/`pcie_driver`/`platform`) still do NOT propagate (one-holder invariant).
- Kernel-internal only. **No Law 1 surface** (syscall.rs + scheduler-path only).

## Window note
Analysis-only window expires 2026-07-14. This item has the one defensible claim to
jump the window (it silently falsifies a stated graduation gate) — **user's call**.
Default per window: plan now, code after 2026-07-14.

## Phases
| # | Phase | Status | Effort | Blockers |
|---|-------|--------|--------|----------|
| P00 | [Route threads under parent CellId + CapSet](phase-00-thread-cellid-inheritance.md) | pending | ~half day | none |

Single phase: the fix is a handful of lines + one disambiguating regression test.
Step 1 of P00 is the mandated verification (confirm charge/uncharge keying) — must
pass before the edit lands.

## Key dependencies
- Mechanism proof: loader.rs:174-186 already applies the identical `CellId(tid)`
  correction to the cell-spawn path for exactly this reason — P00 mirrors it.
- Routing: quota keys off `hart_local::current_cell_id()`, set on context switch
  from `task.cell_id.0` (scheduler.rs:751). Setting the thread's `cell_id` is
  sufficient and provably routes the charge.

## Open questions (see phase file)
1. ~~Scope of same-root-cause hardening~~ — **RESOLVED 2026-07-12: expanded scope
   confirmed.** Inherit `cell_id` + `CapSet` + `syscall_allowlist` + `pku_key/value`.
2. Whether to run the fix now (jump window) or after 2026-07-14 — **still user's call.**
