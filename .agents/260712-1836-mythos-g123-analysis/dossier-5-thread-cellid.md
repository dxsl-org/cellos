---
title: "Dossier 5 — Thread spawn under CellId(0) is a memory-quota escape (LIVE, hits G1 graduation criterion #2)"
description: "Syscall::Spawn hardcodes CellId(0), the system cell whose allocations bypass the quota charge — so any cell that spawns threads has unbounded, uncharged memory. That directly violates 'bounded memory enforced on EVERY write path.' Verdict + minimal fix. Analysis-only."
status: verdict-final (small correctness fix, not a plan)
window: mythos-analysis-only (expires 2026-07-14)
severity: HIGH — defeats a G1 graduation criterion; LIVE, not latent
created: 2026-07-12
---

# Dossier 5 — Thread CellId quota escape

## Reclassification: this is not a "design gap," it is a live quota escape

The roadmap/audit filed `syscall.rs:1151` as a low-priority "design gap (rare
case)." Reading the code path, it is a **live violation of a G1 graduation
criterion**, verified at HEAD:

```
Syscall::Spawn { entry, arg } =>                       // syscall.rs:1148
    let tid = super::spawn_with_arg(name, CellId(0), …) // syscall.rs:1153 — hardcoded CellId(0)
```
```
// TODO: Spawned threads should inherit parent's CellId or be assigned properly
// For now, use CellId(0) as default (system/kernel cell)         // syscall.rs:1151-1152
```

`CellId(0)` is the **system/kernel cell**, and the memory `charge()` path
**short-circuits for `cell_id == 0`** (loader.rs:174-184 corrects the *cell*-spawn
path to `CellId(tid)` precisely *because* "charge() short-circuits for cell_id ==
0"). The thread-spawn path has **no such correction** — `spawn_thread` creates a
`Task::new(id, CellId(0), …)` (scheduler.rs:245) and leaves it there.

**Consequence:** every heap/stack allocation made by a thread spawned via
`Syscall::Spawn` is charged to `CellId(0)` and therefore **not counted against the
parent cell's memory quota**. A cell can spawn N threads and consume unbounded
memory that the quota enforcer never sees.

## Why this matters more than its size

G1 graduation criterion #2 (roadmap line ~142) is: *"Bounded memory enforced on
EVERY write path (Write/Append/IPC)."* Thread-spawned allocation is a write path
that is **not** bounded — the criterion is not actually met while this stands. It is
small in LOC but load-bearing for the never-die / bounded-memory story that is the
whole point of G1. That is the gap between "rare edge case" and "defeats a
graduation gate."

## Second-order problem: cap scope and cell identity

Beyond quota, `CellId(0)` mis-identifies the thread as a **system cell** for any
`cell_id`-keyed policy, and `Task::new` starts the thread with an **empty CapSet**
(no inheritance from the parent). So a "thread" today is neither quota-bound to its
parent nor capability-scoped to it — it is an orphan task mislabeled as kernel. Two
things a real thread should share with its parent cell are both wrong:
- **quota** — should charge the parent cell (currently bypassed via CellId 0);
- **capability scope** — a thread of cell C should act within C's authority
  (currently empty caps, which is *safe* but means these "threads" can't do the
  cell's work — suggesting the primitive is under-used today, which is why the hole
  is latent-in-practice despite being live-in-principle).

## Verdict: threads inherit the parent's CellId

The fix the TODO gestures at is correct and unambiguous: **a spawned thread runs
under its parent's `CellId`, not `CellId(0)`.** Concretely, `Syscall::Spawn` must
resolve the caller's `cell_id` from its TCB and pass **that** to `spawn_with_arg`
instead of `CellId(0)`. Then:
- allocations charge to the parent cell's quota (criterion #2 satisfied);
- the thread shares the parent cell's identity for any `cell_id`-keyed check;
- capability scope: decide explicitly — either the thread **shares** the parent
  cell's CapSet (true "thread of the cell" semantics) or starts empty. Given SAS +
  same-cell semantics, **share the parent's caps** is the coherent choice (a thread
  is not a lesser-privileged child; it is the same cell running more TIDs). This
  must be a deliberate line in the fix, not left to `Task::new`'s default.

## One verification before the fix

Confirm how `charge()`/`uncharge()` key their accounting and that changing the
thread's `cell_id` to the parent's routes the charge correctly (the cell-spawn path
proves the mechanism works — loader.rs sets `CellId(tid)` for that reason). The
disambiguating test: spawn a thread that allocates a known N bytes; assert the
parent cell's quota usage rises by ~N (today it rises by 0). That test is also the
regression guard — it fails today, passes after the fix.

## Scope: small correctness fix, not a plan

This is ~a handful of lines (resolve caller `cell_id`, pass it through, decide cap
inheritance) + one quota-accounting test. It is **coding**, so it waits for the
Mythos window to end — but it should be treated as a **priority correctness fix**,
not backlog, because it silently falsifies a graduation criterion. It has no Law 1
surface (kernel-internal: `syscall.rs` + `scheduler.rs`). Recommend folding it into
the next kernel-touch cook (it is thematically adjacent to the P-TRUST/quota work)
rather than a standalone plan.

## If the user wants it treated as urgent

Because it defeats a stated graduation gate, this is the one item in the whole
analysis set that has a defensible claim to jump the window's analysis-only rule.
Flagging it explicitly so the user can decide; default (per window) is document-now,
fix-after.
