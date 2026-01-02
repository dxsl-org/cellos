# Debug Report: Illegal Instruction Fix

## Problem
The Kernel was crashing with `Exception 0x2 (Illegal Instruction)` at random addresses (e.g., `0x80061E7A`) often located in the gap between stacks. This occurred during concurrent execution of tasks (`vios-hello` and `vios-driver-motor`).

## Analysis
The address `0x80061E7A` is outside the valid executable code range and outside the valid stack range for Task 1 (`0x80061DF0`). Execution at this address implies a corrupted Return Address (`ra`) or Program Counter (`mepc`).

The root cause was identified as a **Race Condition in `yield_cpu`**:
1. `sys_yield` is called from M-mode software (Interrupts Enabled).
2. `yield_cpu` is called. It releases `SCHEDULER` lock.
3. `Context::switch` is called.
4. If a Timer Interrupt occurs *during* `Context::switch`, the `TrapHandler` is invoked.
5. The `TrapHandler` saves the interrupted state (which is halfway through switching) to the stack.
6. `TrapHandler` calls `yield_cpu` again (nested).
7. `yield_cpu` switches context again.
8. When the stack unwinds, the saving/restoring logic of `Context::switch` conflicts with the `TrapFrame` saving mechanism, leading to register corruption (specifically `ra` or `sp`).

## Fix
Implemented a **Critical Section** in `kernel/src/process/mod.rs`:
- Added `crate::arch::trap::read_mstatus()` helper.
- In `yield_cpu`, we now:
    1. Read current `mstatus` (check if interrupts enabled).
    2. **Disable Interrupts** (`cli`) before calling `Context::switch`.
    3. Perform `Context::switch`.
    4. **Restore Interrupts** (`sti`) only if they were enabled previously.

This ensures `Context::switch` is atomic with respect to interrupts, preventing the race condition.

## Verification
- Kernel builds successfully with `--no-default-features`.
- Validated that `prelude` is correctly used across kernel modules.
