---
phase: 00
title: Fault-Path Crash-Safety (Foundation)
priority: P0
status: planned
depends_on: []
risk: high
origin: red-team C1/C2
---

# Phase 00 — Fault-Path Crash-Safety

## Context Links
- Red-team: `.agents/reports/red-team-260605-2126-full-reliability-track.md` (C1, C2)
- Code: [kernel/src/main.rs](../../kernel/src/main.rs) (`#[panic_handler]` @310-343)
- Code: [kernel/src/task.rs](../../kernel/src/task.rs) (`terminate_current_cell_on_fault` @158-197)
- Code: [kernel/src/task/scheduler.rs](../../kernel/src/task/scheduler.rs) (`exit_task` @239-269)
- Code: [kernel/src/task/syscall.rs](../../kernel/src/task/syscall.rs) (`Exit` waiter-wake @620-639, `ForceExit` @700-710)

## Overview
- **Priority:** P0 — **hard prerequisite for the entire track.**
- **Status:** planned
- **Description:** Two pre-existing fault-path defects make "kill the cell, kernel survives"
  false in important cases. They must be fixed before auto-restart (P04) and reclamation (P05)
  raise the frequency of kernel alloc/free (and thus the odds of hitting them).

## The Two Defects (verified)
1. **Lock-leak via panic misclassification (C1).** The panic handler decides cell-vs-kernel
   solely by `CURRENT_CELL_ID != 0` ([main.rs:310-323](../../kernel/src/main.rs)). But kernel
   syscall code runs with the *calling cell's* id still set. So a kernel `panic!`/`unwrap()`
   while servicing a syscall (e.g. in `fat.rs`/`frame.rs`/virtio while `FRAME_ALLOCATOR` or
   `BLOCK_DEVICE` is locked) is mis-killed as a *cell* fault. The fault path force-unlocks
   **only `SCHEDULER`** ([task.rs:172](../../kernel/src/task.rs)) → the held global lock stays
   locked forever → next alloc/block-I/O deadlocks the whole system.
2. **Death notification skips the fault path (C2).** Waiter-wake lives inside the `Exit` and
   `ForceExit` *syscall handlers* ([syscall.rs:620-639,700-710](../../kernel/src/task/syscall.rs)),
   not in `exit_task`. The fault path calls `exit_task` directly ([task.rs:179](../../kernel/src/task.rs))
   and never wakes waiters → `Wait{pid}` already hangs forever when the target dies by fault,
   and the planned `NotifyOnExit` (P03) would miss the exact fault-restart case it exists for.

## Requirements
**Functional**
- The kernel distinguishes "fault trapped in cell code" from "panic raised in kernel code on
  behalf of a cell" — using an explicit reentrancy signal, NOT `CURRENT_CELL_ID`.
- A kernel-context panic (even mid-syscall) takes the kernel path: reboot (Phase 01), never a
  silent cell-kill that leaks locks.
- The fault path releases ALL global kernel locks it might hold, not just `SCHEDULER`.
- `exit_task` becomes the single chokepoint that wakes `waiters` (and later delivers
  `NotifyOnExit`), so Exit, ForceExit, AND fault all notify uniformly.

**Non-functional**
- No behavior change for the normal (non-fault) cell exit path beyond centralizing waiter-wake.
- Force-unlock list must be explicit and reviewed — over-unlocking a lock not held is itself a
  hazard.

## Architecture
```
Add: static IN_SYSCALL_DEPTH (per-hart) — incremented on ecall entry, decremented on return.
panic_handler:
   if IN_SYSCALL_DEPTH > 0 || CURRENT_CELL_ID == 0:  → KERNEL panic → reboot (Phase 01)
   else (genuine cell-code fault):                   → terminate_current_cell_on_fault

terminate_current_cell_on_fault:
   force-unlock the registered global locks (SCHEDULER + FRAME_ALLOCATOR + BLOCK_DEVICE +
       CELL_REGISTRY + QUOTA_LIMITS + RT_HEAP + FROZEN + audit) — only those that exist;
       document each with a // SAFETY: note (cell code holds none of these legitimately).
   exit_task(tid)   ← now also wakes waiters / fires notifications

exit_task(tid):
   move to zombies; purge ready queues; unblock stuck senders (existing)
   + wake task.waiters with reason   ← MOVED here from Exit/ForceExit handlers
   + (P03) deliver NotifyOnExit to subscribers
```
Note: the cell-code path holds no kernel Spinlock legitimately (cells can't lock kernel
state), so force-unlocking on a genuine cell fault is safe. The danger was the *misclassified*
kernel panic — the `IN_SYSCALL_DEPTH` flag closes that.

## Related Code Files
**Modify**
- `hal/arch/riscv/src/rv64/trap.rs` — bump/clear `IN_SYSCALL_DEPTH` around ecall dispatch.
- `kernel/src/main.rs` — panic handler classification uses depth flag, not just CURRENT_CELL_ID.
- `kernel/src/task.rs` — `terminate_current_cell_on_fault` force-unlocks the full lock list.
- `kernel/src/task/scheduler.rs` — `exit_task` owns waiter-wake (with reason).
- `kernel/src/task/syscall.rs` — `Exit`/`ForceExit` delegate waiter-wake to `exit_task` (DRY).

## Implementation Steps
1. Add a per-hart `IN_SYSCALL_DEPTH` (atomic / per-hart cell); inc on ecall enter, dec on
   sret in the trap dispatch.
2. Rewrite panic classification: kernel path if `depth > 0 || CURRENT_CELL_ID == 0`.
3. Build an explicit registered force-unlock routine covering all global kernel Spinlocks;
   call it at the top of `terminate_current_cell_on_fault`. `// SAFETY:` each.
4. Move waiter-wake (with a reason enum: Exit{code}/Fault{scause}) into `exit_task`; make
   `Exit`/`ForceExit` call through it (remove the duplicated wake loops).
5. Build per-arch; boot QEMU.

## Todo List
- [ ] `IN_SYSCALL_DEPTH` per-hart, inc/dec around ecall
- [ ] Panic classification uses depth flag
- [ ] Full force-unlock list on fault (SAFETY-commented)
- [ ] `exit_task` owns waiter-wake (reason-tagged); Exit/ForceExit delegate
- [ ] Test: inject a kernel `unwrap` while FRAME_ALLOCATOR held → system reboots (not deadlock)
- [ ] Test: `Wait{pid}` watcher is woken when target dies by FAULT (not just clean exit)

## Success Criteria
- A deliberately-injected kernel panic while a global lock is held → **reboot**, not a hung
  kernel (verified in QEMU; remove the injection after).
- A `Wait{pid}` watcher unblocks when its target is killed by fault/watchdog, not only on
  clean `Exit`.
- Normal cell exit/spawn/hotswap regression: unchanged.

## Risk Assessment
- **Over-unlocking a lock not actually held (High).** Force-unlocking a free Spinlock can mask
  a real invariant or double-unlock. *Mitigation:* a Spinlock `force_unlock` that is
  idempotent/safe on an unheld lock; unit-test the routine; keep the list explicit and minimal.
- **`IN_SYSCALL_DEPTH` not cleared on the fault path (Med).** A fault mid-syscall must reset
  depth so the post-fault kernel isn't stuck "in syscall". *Mitigation:* reset depth in the
  fault path alongside CURRENT_CELL_ID.
- **Per-hart correctness for future SMP (Med).** Use per-hart storage now to avoid a rewrite.

## Security Considerations
- A cell must not be able to set `IN_SYSCALL_DEPTH` or trigger the force-unlock path to escape
  a fault — both are kernel-internal, reached only via the trap/panic path. No new surface.

## Next Steps
- With a crash-safe fault path, Phase 01 can safely make panics reboot and faults trap.
