# Phase 03 — Cell Fault Isolation

**Status**: 📋 PLANNED  
**Priority**: P1  
**Effort**: 3 days  
**Depends on**: Phase 02 (CURRENT_CELL_ID atomic must exist)

---

## Context Links

- Trap handler exceptions: `hal/arch/riscv/src/rv64/trap.rs:120-150`
- Kernel panic handler: `kernel/src/main.rs:283-310`
- Task exit path: `kernel/src/task/syscall.rs:599-650`
- `exit_task()`: `kernel/src/task/scheduler.rs:197-211`
- Spec panic isolation: `docs/specs/02-memory.md` (OOM → Result::Err, not panic)

---

## Overview

**The problem**: When any unhandled exception fires (illegal instruction, page fault, misaligned access), the current trap handler calls `panic!()`. With `panic = "abort"`, this halts the entire machine — one faulty Cell kills everything.

**Why `catch_unwind` doesn't work**: `panic = "abort"` is set in both profiles. The compiler emits no unwind tables, no landing pads. `catch_unwind` requires unwinding — it is categorically impossible without switching the panic runtime (~25 KB binary overhead, unacceptable).

**The correct approach**: Use the RISC-V hardware trap as the isolation boundary. When an exception fires in a Cell's context:
1. The kernel identifies the fault as originating from a Cell (not kernel code)
2. The kernel calls `exit_task(current_cell_id)` + `yield_cpu()` instead of `panic!()`
3. The faulty Cell becomes a zombie; the scheduler picks the next ready task

This is analogous to seL4's fault endpoint model and Tock's process fault handling — no unwinding, no `catch_unwind`, pure trap-based isolation.

---

## Key Insight: Context Discrimination

The trap handler must distinguish two scenarios:

| Scenario | `scause` type | `sepc` location | `CURRENT_CELL_ID` | Action |
|----------|--------------|-----------------|-------------------|--------|
| Kernel bug | Exception | Kernel `.text` | 0 | `panic!()` — true kernel crash |
| Cell fault (in U-mode) | Exception | Cell `.text` | > 0 | `exit_task(id)` + `yield_cpu()` |
| Cell fault (mid-syscall) | Exception | Kernel `.text` | > 0 | `exit_task(id)` + `yield_cpu()` |

The discriminator is `CURRENT_CELL_ID != 0`. This is sufficient because:
- During early boot (kernel init), `CURRENT_CELL_ID == 0` → true kernel panic
- Once Cells are running, `CURRENT_CELL_ID` is updated on every context switch
- Mid-syscall faults (kernel code processing a Cell's request) are also attributed to the Cell

---

## Related Code Files

### Modify
- `hal/arch/riscv/src/rv64/trap.rs:120-150` — replace `panic!()` in exception handlers with fault isolation call
- `kernel/src/main.rs:283-310` — update `#[panic_handler]` to check `CURRENT_CELL_ID`

### Create
- No new files needed

---

## Implementation Steps

### Step 1 — Fault isolation function in `task.rs`

```rust
/// Terminate the currently-executing Cell due to a hardware fault.
///
/// Called from the trap handler when an unrecoverable exception fires in
/// a Cell context.  The Cell is moved to the zombie list; the scheduler
/// picks the next ready task.  The kernel itself is NOT affected.
///
/// # Safety
/// Must be called from trap context with interrupts disabled.
/// Calls `SCHEDULER.force_unlock()` first to guard against being called from
/// a mid-pick_next panic (where the lock may already be held).
pub fn terminate_current_cell_on_fault(scause: usize, sepc: usize) {
    // If a panic fires inside pick_next (e.g., OOM during scheduler's own
    // BTreeMap ops), SCHEDULER is already locked.  Force-release it so the
    // exit path below can re-acquire cleanly.
    // SAFETY: single-hart kernel; no other core can hold the lock.
    unsafe { SCHEDULER.force_unlock(); }

    let cell_id_raw = crate::task::scheduler::CURRENT_CELL_ID.load(
        core::sync::atomic::Ordering::Relaxed
    );
    log::error!(
        "[fault] Cell {} killed: scause={:#x} sepc={:#x}",
        cell_id_raw, scause, sepc
    );

    // Look up the task ID for this Cell.  There may be multiple tasks per Cell;
    // we kill the currently-running one (current_task_id in scheduler).
    let task_id = if let Some(sched) = SCHEDULER.lock().as_ref() {
        sched.current_task_id
    } else {
        None
    };

    if let Some(tid) = task_id {
        // SAFETY: called from trap handler, interrupts disabled, no lock held.
        if let Some(sched) = SCHEDULER.lock().as_mut() {
            sched.exit_task(tid);
        }
    }

    // Switch to next ready task — does not return to the faulting Cell.
    yield_cpu();
}
```

### Step 2 — Update exception dispatch in `trap.rs`

Replace the exception handling block (currently falls through to `panic!()`) with:

```rust
} else {
    // Exception — check if this is a Cell fault or a true kernel bug.
    let cell_id = crate::task::scheduler::CURRENT_CELL_ID.load(
        core::sync::atomic::Ordering::Relaxed
    );

    if cell_id != 0 {
        // Cell fault: illegal instruction, page fault, etc.
        // Kill the Cell; kernel continues.
        // SAFETY: called from trap handler; SCHEDULER not held; interrupts disabled.
        unsafe { crate::task::terminate_current_cell_on_fault(code, frame.sepc); }
        // terminate_current_cell_on_fault calls yield_cpu() which does not return
        // to this point — the scheduler switches to the next ready task.
        unreachable!("yield_cpu in terminate_current_cell_on_fault must not return here");
    } else {
        // True kernel fault: panic as before.
        panic!("Kernel exception: scause={:#x} sepc={:#x} stval={:#x}", code, frame.sepc, frame.stval);
    }
}
```

### Step 3 — Update `#[panic_handler]` in `main.rs`

The panic handler also needs the Cell-vs-kernel check for the OOM case (Cell's allocation fails → Cell code panics → aborts):

```rust
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let cell_id = crate::task::scheduler::CURRENT_CELL_ID.load(
        core::sync::atomic::Ordering::Relaxed
    );

    if cell_id != 0 {
        // A Cell's code panicked (e.g., OOM from QuotaAlloc returning null_mut).
        // Log and terminate the Cell; kernel continues.
        // Interrupts are disabled (panic = abort), so direct scheduler manipulation is safe.
        log::error!("[panic] Cell {} panicked: {}", cell_id, info);
        // SAFETY: panic context, interrupts disabled, no concurrent access.
        unsafe { crate::task::terminate_current_cell_on_fault(0, 0); }
        // unreachable — yield_cpu switches away
    }

    // Kernel-level panic: halt as before.
    puts("\n[KERNEL PANIC] ");
    // ... existing panic output ...
    loop { unsafe { core::arch::asm!("wfi") }; }
}
```

---

## Best-Effort IPC Cleanup on Cell Exit (user confirmed)

When a Cell is killed mid-syscall, other tasks may be `Sending { target: dead_cell_id }` or have `current_caller = Some(dead_cell_id)`. Add a cleanup pass in `exit_task()`:

```rust
// In scheduler::exit_task(tid), after moving task to zombies:
let dead_id = tid;
for task in self.tasks.values_mut() {
    // Unblock tasks blocked waiting to send TO the dead cell
    if let TaskState::Sending { target, .. } = task.state {
        if target == dead_id {
            task.state = TaskState::Ready;
            task.trap_frame.regs[10] = usize::MAX; // error return value (USIZE_MAX = ViError)
            // push_ready called by caller after drop
        }
    }
    // Clear stale current_caller references
    if task.current_caller == Some(dead_id) {
        task.current_caller = None;
    }
}
```

This catches the most common staleness case. Does NOT handle multi-hop IPC chains or stashed state (those require a more complete state machine — deferred to Phase 27).

---

## Todo List

- [ ] Add `pub fn terminate_current_cell_on_fault(scause, sepc)` to `kernel/src/task.rs`
- [ ] Update exception dispatch in `trap.rs` — check `CURRENT_CELL_ID` before `panic!()`
- [ ] Update `#[panic_handler]` in `main.rs` — check `CURRENT_CELL_ID`, kill cell if non-zero
- [ ] `cargo check -p vicell-kernel` — zero errors
- [ ] Test: spawn a Cell that executes `undef` (illegal instruction) → Cell killed, kernel continues
- [ ] Verify: shell prompt returns after faulty Cell is killed

---

## Success Criteria

- [ ] A Cell that calls `core::arch::asm!("unimp")` (illegal instruction) is terminated
- [ ] Kernel log shows `[fault] Cell N killed: scause=0x2 sepc=...`
- [ ] Shell prompt returns within 1 timer tick after fault
- [ ] A true kernel exception (in init with no Cell running) still triggers full kernel panic
- [ ] All 65 existing integration tests pass

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| SCHEDULER double-lock in terminate_current_cell_on_fault | Medium | `terminate_current_cell_on_fault` acquires lock only briefly; must not be called while lock is held. Add `debug_assert` |
| yield_cpu() returns to faulting Cell (if no other tasks ready) | Low | If no tasks ready, yield_cpu switches to BOOT_CONTEXT (idle), not back to faulting Cell (it's in zombies) |
| Mid-syscall fault: kernel stack may be in inconsistent state | Medium | Kernel stacks are per-task; after exit_task, the zombie's stack is abandoned, not used again. Acceptable. |
| CURRENT_CELL_ID == 0 for legitimate Cell fault during early spawn | Low | CURRENT_CELL_ID is set to cell_id in pick_next before first context switch — only 0 during kernel init |
