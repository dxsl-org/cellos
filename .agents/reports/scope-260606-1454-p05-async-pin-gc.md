# Scope: P05 async-pin / grant GC — is there anything to collect?

**Date:** 2026-06-06  **Track:** Reliability ("không chết") P05 — stop slow death (resource leaks)
**Question:** When a cell dies, does any frame/buffer stay pinned by an in-flight async I/O
or by a zero-copy IPC grant/lease, leaking memory or creating a cross-cell use-after-free?

## TL;DR — MOOT under the current design. No code needed yet.

Async-pin GC and grant/lease GC are **not needed today**. Three independent reasons, each
verified against source. The red-team concern is real *in principle* but does not apply to
the code as it exists. The single future-work trigger is documented below.

## Evidence

### 1. The async FileRead future is Task-owned and never polled after death
- `kernel/src/task.rs:567-592` — `file_read` removes the `FileHandle`, calls
  `file_box.read_async(buf_ptr, buf_len)`, stores the resulting `BoxFuture` in
  `task.pending_future` and sets `task.state = Polling`. The future captures a **raw
  pointer into the cell's OWN buffer** (no separate frame is allocated/pinned).
- `kernel/src/task/scheduler.rs:449-480` — the poll loop iterates `self.tasks.keys()` and
  only polls tasks whose `state == Polling`.
- `kernel/src/task/scheduler.rs:302-304` — `exit_task` does `self.tasks.remove(&tid)` →
  `self.zombies.push(task)` **before** any frame is freed (frames free lazily at reap,
  `take_reapable_zombies` → `Box<Task>` drop). A dead cell is therefore **gone from the
  poll set** the instant it dies → its future is never polled again → the dangling
  `buf_ptr` write can never execute. Single-hart + SCHEDULER-lock means `exit_task` and the
  poll loop cannot interleave, so there is no race.
- At reap, dropping the uncompleted `BoxFuture` just frees the captured `Box<dyn ViFile>` +
  future state — it does **not** write into `buf_ptr`.

### 2. The inner read is synchronous — no lingering DMA outlives the future
- `kernel/src/fs/fat.rs:415-425` — `read_async` is `Box::pin(async move { … let res =
  this.read(buf); … })`: the read runs **only when polled**, synchronously, completing in a
  single poll. No DMA descriptor is registered in any device queue that could outlive the
  future and write into freed memory. (Comment in-source: *"Perform synchronous read (for
  now…)"* / *"In a real async driver, we would need to pin user memory."*)

### 3. Grant/lease IPC cannot be created at runtime — nothing to GC
- `libs/ostd/src/syscall.rs:538-541` — `sys_grant` is a **stub**: `return Err(Unknown)`.
  Cells have no working path to create grants/leases, so `Task.grant_table` and
  `Task.leases` (`tcb.rs:101,107`) are always empty in practice.
- Even when populated, both are `BTreeMap`s **owned by the Task** → freed at reap. They hold
  VA-range metadata, not frames, so they never leak a frame regardless.

## What IS already collected on cell death (for completeness)
Everything a cell owns lives in its `Task` (`tcb.rs:92-159`) and is freed by the existing
reaper (`take_reapable_zombies` + `Box<Task>` drop):
- user/kernel stacks (`Stack::drop`), ELF segment frames (`CellSegments::drop`),
- heap (per-cell quota refunded lock-free as boxes drop, `cell_quota::refund`),
- `pending_future`, `open_files`, `grant_table`, `leases`, caps — all Task fields.

## Future-work trigger (the ONLY condition that makes this real)
When a **real async DMA driver** lands (the fat.rs TODO) — i.e. `read_async` registers a
device descriptor pointing at `buf_ptr` and returns `Pending` while hardware writes in the
background — OR when the kernel goes **SMP** (poll loop on a different hart than the killer),
then a cell killed mid-flight could have hardware write into freed/reused frames. At that
point the cancellation point is **`exit_task`**: cancel/await the device descriptor (or pin
the user frames for the DMA's duration and free them only on completion) **before** the
frames are reclaimed. Until then, adding GC code would be speculative (YAGNI) and would only
risk the "drop heavy resources outside the SCHEDULER lock" invariant
(`scheduler.rs:364-375`).

## Recommendation
- **Mark P05 async-pin/grant GC CLOSED (moot) with the documented trigger.** P05 (stop slow
  death) is then complete: zombie reaper + stack reclaim + segment reclaim + overwrite-guard,
  and async/grant verified leak-free by construction.
- **One zero-risk hardening shipped:** an explanatory comment at the `exit_task` removal site
  recording the invariant ("removed from `self.tasks` before frames free → never polled
  again; a real async-DMA driver MUST add a cancellation point here"), so a future refactor
  that iterates zombies or adds background DMA cannot silently reintroduce a UAF.
