# Phase 03 — Boot Stability & Ring 3 Execution

**Effort:** 40h | **Priority:** P0 (BLOCKING) | **Status:** complete | **Blockers:** none

## Overview

Fix the `init_kernel_paging` hang during early boot on RV64 and establish working user-mode (Ring 3 / U-mode) execution. The kernel currently boots in S-mode but cannot transition tasks to U-mode cleanly. Without this, every cell still runs in supervisor mode — defeating the SAS + LBI security model. Demo target: kernel spawns a minimal user task, the task executes `ecall Log("Hello from Userspace")`, kernel prints it, task exits cleanly.

## Context Links

- `docs/02-memory.md` — SAS, HHDM, registry, paging contract
- `docs/04-hardware.md` — multi-arch HAL traits
- `docs/03-runtime.md` — async safety, owned buffers (Law 2)
- CLAUDE.md — Law 3 (multi-arch), Law 4 (unsafe management)

## Key Insights

- SV39 PTEs: bit 0 V, 1 R, 2 W, 3 X, 4 U, 5 G, 6 A, 7 D. **U bit is mandatory** for any page reachable from U-mode.
- Activation sequence: write `satp` (mode=8 for SV39, ASID, PPN) → `sfence.vma zero,zero`. Forgetting the fence after `satp` causes random TLB-driven faults that look like "boot hang".
- `intrinsics.rs` defines `memset`/`memcpy`; if the compiler emits a call to an undefined symbol, the kernel hangs in a Trap loop on first memmove. QEMU `-d cpu_reset,in_asm` exposes this as repeated illegal-instruction traps.
- Ring 3 entry on RV: set `sstatus.SPP = 0` (return to U-mode), `sepc = user_entry`, then `sret`. Forgetting `sstatus.SPIE = 1` leaves interrupts disabled in user mode and any timer-driven preempt hangs the task.
- `ecall` from U-mode raises `scause=8`; kernel trap dispatcher must dispatch to syscall handler, NOT panic.

## Requirements

**Functional**
- Kernel boots to "Ring 3 ready" log line on `qemu-system-riscv64 -machine virt`
- Kernel spawns a task whose entrypoint is in a U-flagged page
- Task executes `ecall` with syscall number `ViSyscall::Log` and a `&str`
- Kernel handles the trap, writes the string to UART, returns to U-mode
- Task exits via `ViSyscall::Exit(0)`; scheduler reclaims TCB and frames

**Non-functional**
- Boot wall-time < 2s in QEMU (warm)
- No new `unsafe` outside `hal/` and `kernel/src/`
- All new unsafe blocks documented with `// SAFETY:`

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│ Boot (boot.rs, limine.rs)                               │
│   ├─ Parse Limine info / device tree                    │
│   ├─ Initialize UART (serial output up early)           │
│   ├─ Initialize frame allocator (memory/frame.rs)       │
│   └─ Initialize kernel page table (paging.rs)           │
│       ├─ Identity-map kernel image (.text/.rodata/.bss) │
│       ├─ Map HHDM region                                │
│       └─ Write satp + sfence.vma                        │
├─────────────────────────────────────────────────────────┤
│ Task creation (task/task.rs)                            │
│   ├─ Allocate user stack (U-flagged)                    │
│   ├─ Allocate user code page (U|R|X)                    │
│   ├─ Copy entry code in                                 │
│   ├─ Build TCB { sepc=entry, sp=stack_top, satp=… }     │
│   └─ Enqueue to scheduler                               │
├─────────────────────────────────────────────────────────┤
│ Ring 3 entry (hal/rv64/trap.rs trap_return)             │
│   ├─ sstatus.SPP=0, SPIE=1                              │
│   ├─ Restore x1-x31 from TCB                            │
│   └─ sret                                               │
├─────────────────────────────────────────────────────────┤
│ Ecall from U-mode                                       │
│   ├─ Trap into trap.rs handle_trap()                    │
│   ├─ scause == 8 → syscall::dispatch(a7, a0..a6)        │
│   ├─ Return value → a0                                  │
│   └─ sepc += 4 (skip ecall instruction)                 │
└─────────────────────────────────────────────────────────┘
```

## Related Code Files

**Investigate first (read-only audit):**
- `kernel/src/intrinsics.rs` — confirm `memset`, `memcpy`, `memmove`, `memcmp` exist with `#[no_mangle]`
- `kernel/src/boot.rs` — boot info parser
- `kernel/src/boot/limine.rs` — Limine boot protocol glue
- `hal/arch/riscv/src/rv64/boot.rs` — assembly entry (`_start`)

**Modify:**
- `kernel/src/memory/paging.rs` — fix SV39 mapping (U bit handling, fence sequence)
- `hal/arch/riscv/src/rv64/paging.rs` — `PageTableTrait` impl
- `hal/arch/riscv/src/rv64/trap.rs` — Ring 3 entry, syscall dispatch path
- `hal/arch/riscv/src/rv64/context.rs` — context save/restore including sstatus
- `kernel/src/task.rs` — `spawn_user_task(entry: VAddr, stack: VAddr)` <!-- Updated: Validation Session 1 - correct path (task.rs at kernel/src/ level, not task/task.rs) -->
- `kernel/src/task/syscall.rs` — wire `ViSyscall::Log` and `ViSyscall::Exit` to U-mode callers
- `kernel/src/task/tcb.rs` — TCB must hold `satp`, `sepc`, `x1..x31`, `sstatus`

**Create:**
- `kernel/src/task/user_hello.rs` — minimal embedded user task used as smoke test
- `scripts/debug-boot-trace.sh` — wrapper: `qemu -d cpu_reset,int,in_asm -D qemu-trace.log …`

## Implementation Steps

1. **Verify intrinsics exist** — `grep -n "memcpy\|memset\|memmove\|memcmp" kernel/src/intrinsics.rs`. If any missing, implement minimal `#[no_mangle] pub unsafe extern "C" fn …`. Test that release build doesn't emit external references: `riscv64-elf-nm target/.../kernel | grep ' U '` should show none of these as undefined.
2. **Capture failing trace** — `bash scripts/debug-boot-trace.sh > before.log 2>&1`. Identify the last successful instruction before hang. Likely candidates: `csrw satp`, store to unmapped page, illegal instruction.
3. **Audit `paging.rs` map_range**:
   - Verify each PTE has correct flags (V|R|X|A|D for kernel text, V|R|W|A|D for data, V|R|W|U|A|D for user stack, V|R|X|U|A|D for user code).
   - Check the `sfence.vma zero, zero` is issued AFTER every PTE write that affects a currently-active mapping.
   - Confirm HHDM region uses 1GB superpages (PTE level 2) for boot speed.
4. **Audit `satp` activation in `rv64/boot.rs`**:
   - Order must be: build initial PT → flush data cache (`fence rw,rw`) → write satp → `sfence.vma zero,zero` → continue.
   - The instruction after `satp` write should be in identity-mapped region (or also reachable via new PT) — common boot bug.
5. **Implement Ring 3 entry in `rv64/trap.rs`**:
   ```rust
   pub unsafe fn enter_user(tcb: &TaskControlBlock) -> ! {
       // SAFETY: tcb fields validated; sret returns to U-mode.
       asm!(
           "csrw satp, {satp}",
           "sfence.vma zero, zero",
           "csrw sepc, {sepc}",
           "csrw sstatus, {sstatus}",  // SPP=0, SPIE=1
           "mv sp, {sp}",
           // restore x1..x31 from tcb (load each)
           "sret",
           satp = in(reg) tcb.satp,
           sepc = in(reg) tcb.sepc,
           sstatus = in(reg) tcb.sstatus,
           sp = in(reg) tcb.sp,
           options(noreturn)
       );
   }
   ```
6. **Implement trap handler dispatch in `rv64/trap.rs`**:
   - Save all GPRs to per-task TCB on entry
   - Read `scause`, `stval`, `sepc`
   - If `scause == 8` (U-mode ecall): increment `sepc` by 4, call `syscall::dispatch(a7, a0..a6)`, store return in `a0`
   - If async interrupt (`scause` bit 63 set): dispatch to PLIC handler
   - Otherwise: kernel panic with full register dump
7. **Create `kernel/src/task/user_hello.rs`**:
   ```rust
   // SAFETY: assembled to a small, position-known blob; mapped to user code page.
   #[link_section = ".user_hello"]
   #[no_mangle]
   pub unsafe extern "C" fn user_hello_entry() -> ! {
       // ecall Log("Hi from U-mode")
       // ecall Exit(0)
       core::arch::asm!(/* … */, options(noreturn));
   }
   ```
8. **Spawn user task at boot end**: in `kernel/src/main.rs` after init, call `task::spawn_user_task(user_hello_entry as VAddr, user_stack_top)`. Spawn function lives in `kernel/src/task.rs`. Run scheduler.
9. **Run `bash scripts/debug-boot-trace.sh > after.log 2>&1`**. Expect:
   ```
   [ViOS] kernel boot v0.2.1
   [paging] kernel PT active
   [task] spawning user_hello at VAddr(…)
   Hi from U-mode
   [task] user_hello exited(0)
   ```
10. Add an integration test `tests/integration/ring3_smoke.rs` that runs the above via `qemu` and asserts the string sequence.

## Todo List

- [x] Verify intrinsics symbols present, no undefined externals
- [x] Capture failing boot trace `before.log`
- [x] Audit `paging.rs` PTE flags (U bit handling)
- [x] Audit `satp` write + `sfence.vma` order in `rv64/boot.rs`
- [x] Implement `enter_user` in `rv64/trap.rs`
- [x] Implement trap handler dispatch (ecall, interrupt, fault paths)
- [x] Create `user_hello.rs` blob
- [x] Wire `spawn_user_task` in `task/task.rs`
- [x] Wire `Log` + `Exit` syscalls (or stubs) in `task/syscall.rs`
- [x] Boot → see "Hi from U-mode" on serial
- [x] Add `tests/integration/ring3_smoke.rs`
- [x] CI green

## Success Criteria

- Booting QEMU shows kernel banner → user task log line → clean task exit
- No trap-loop or hang for 60s of soak
- `tests/integration/ring3_smoke.rs` passes in CI
- All new `unsafe` blocks carry `// SAFETY:` comments referencing the spec rule justifying them

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `satp` activation continues into unmapped instruction | High | High | Identity-map the byte range immediately after the `satp` write before activating |
| SBI variant differences (OpenSBI vs Rust-SBI) cause divergent timer behavior | Med | Med | Pin SBI in `run.ps1` / CI; document QEMU `-bios default` is OpenSBI |
| User stack overflow silently corrupts kernel memory (no guard page) | Med | High | Allocate +1 unmapped frame above user stack as guard; fault on overflow gives clean signal |
| Compiler emits libcall to missing intrinsic in release mode only | Med | Med | Audit `nm` after release build; add intrinsic if missing |
| Async executor races with scheduler preemption from S-mode timer | Low | Med | Defer multitasking until smoke test is single-task; expand in Phase 11 |

## Security Considerations

- The U bit MUST NOT appear on any kernel page; verify via PT walk in a self-test.
- SUM bit in `sstatus` controls supervisor access to user memory — keep cleared except inside explicit `copy_from_user`/`copy_to_user` helpers (introduce in Phase 06).
- TCB layout must not leak kernel pointers via reusable U-mapped registers — zero scratch regs on user re-entry.

## Rollback

This phase is foundational; reverting drops the project back to S-mode-only execution. If a regression appears post-merge, gate user-task spawning behind a kernel boot flag (`-Dkernel.ring3=off`) and disable in `kernel/src/main.rs` until fix re-merges.

## Next Steps

Unblocks: Phase 06 (external ELF loading), Phase 07 (FileHandle IPC), Phase 11 (integration tests), Phase 20 (hot migration). Phase 04/05 can run in parallel.
