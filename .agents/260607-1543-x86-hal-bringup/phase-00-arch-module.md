# Phase 00 — x86_64 HAL `arch` Module + ViTrapFrame

**Status:** TODO  
**Priority:** CRITICAL — blocks ALL other phases  
**Estimated effort:** Medium (~100 lines across 3 files)

---

## Context Links

- `kernel/src/task.rs:22` — `crate::hal::arch::ViTrapFrame` — compile error on x86_64
- `kernel/src/task.rs:39-49` — `BOOT_CONTEXT` riscv64+aarch64 only — needs x86_64 arm
- `kernel/src/task.rs:59` — `crate::hal::arch::get_gp_tp()` — RISC-V only, needs x86 stub
- `kernel/src/task/scheduler.rs:257,434-435,672-680,748-750` — `hal::arch::...` multiple usages
- `kernel/src/task/tcb.rs:1-2` — `use crate::hal::arch::{Context, ViTrapFrame}`
- `hal/arch/riscv/src/rv64.rs:12-21` — reference: RISC-V `pub mod arch { ... }` pattern
- `hal/arch/x86/src/x86_64.rs` — facade, needs `pub mod arch { ... }`

---

## Overview

The kernel addresses `crate::hal::arch::...` for:
- `ViTrapFrame` — task trap/syscall register save area
- `Context` — callee-saved registers for cooperative context switch  
- `get_gp_tp() -> (usize, usize)` — RISC-V GP/TP fetch (x86: return (0,0))
- `thread_trampoline` — function pointer to the assembly trampoline that bootstraps new threads
- `set_kernel_stack(sp: usize)` — update TSS.rsp0 + GS-based kernel stack ptr
- `init()` — arch HAL init (called by `scheduler::init()`)
- `enable_interrupts()` — called by `scheduler::init()`

The x86_64 HAL currently has all these as top-level exports **except** they are not
wrapped in a `pub mod arch { ... }` and `ViTrapFrame` / `get_gp_tp` / `thread_trampoline`
don't exist. This phase creates the missing pieces.

---

## Requirements

- `crate::hal::arch::ViTrapFrame` resolves on x86_64
- `crate::hal::arch::Context` resolves on x86_64
- `crate::hal::arch::get_gp_tp()` resolves on x86_64 and returns `(0, 0)`
- `crate::hal::arch::thread_trampoline` extern fn resolves on x86_64
- `crate::hal::arch::set_kernel_stack(sp)` resolves on x86_64
- `crate::hal::arch::init()` and `enable_interrupts()` resolve on x86_64
- `BOOT_CONTEXT` in `task.rs` has an x86_64 arm with correct `CpuContext` literal

---

## Architecture

### New file: `hal/arch/x86/src/x86_64/trap.rs`

```rust
//! x86_64 trap / exception frame.
//!
//! Layout matches the RISC-V ViTrapFrame shape (32 regs + sstatus + sepc + stval + scause)
//! so the kernel's task.rs and tcb.rs compile unchanged across architectures.
//! Semantic mapping:
//!   regs[2]  = RSP  (user stack pointer)
//!   regs[7]  = RDI  (syscall arg1)
//!   regs[8]  = RSI  (syscall arg2)
//!   regs[10] = RDX  (syscall arg3 / R10 for syscall convention)
//!   sstatus  = RFLAGS
//!   sepc     = RIP  (return address / entry point)

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ViTrapFrame {
    /// General-purpose register slots (same count as RISC-V for kernel-side compat).
    /// Populated by the x86_64 syscall/interrupt entry asm.
    pub regs: [usize; 32],
    /// x86_64: RFLAGS (init to 0x202 = IF|Reserved for user tasks)
    pub sstatus: usize,
    /// x86_64: user RIP (entry point for new tasks; return addr for syscalls)
    pub sepc: usize,
    /// x86_64: CR2 on #PF, 0 otherwise
    pub stval: usize,
    /// x86_64: interrupt vector number
    pub scause: usize,
}

/// Returns `(0, 0)` — x86_64 has no RISC-V GP/TP registers.
#[inline(always)]
pub fn get_gp_tp() -> (usize, usize) { (0, 0) }
```

### Updated `hal/arch/x86/src/x86_64.rs` — add `pub mod trap; pub mod arch`

```rust
#[cfg(target_arch = "x86_64")] pub mod trap;

// ...existing module declarations...

/// Mirrors the rv64 `pub mod arch { ... }` used by kernel/src/task*.rs
#[cfg(target_arch = "x86_64")]
pub mod arch {
    pub use super::context::CpuContext as Context;
    pub use super::trap::{ViTrapFrame, get_gp_tp};
    pub use super::set_kernel_stack;

    /// Called by scheduler::init() — delegates to the full x86_64 HAL init sequence.
    /// NOTE: this runs EARLY (before paging). Must NOT access MMIO (no HPET here).
    /// HPET + LAPIC calibration are deferred to `init_timers()` called post-paging.
    pub fn init() {
        super::gdt::init();
        super::idt::init();
        super::syscall::init();
        // LAPIC basic init (hardcoded count for now; calibrated in init_timers)
        super::apic::init_lapic();
    }

    pub fn enable_interrupts() {
        unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
    }

    extern "C" {
        /// Assembly trampoline that bootstraps new x86_64 threads.
        /// Defined in boot.rs global_asm! (Phase 02).
        pub fn thread_trampoline();
    }
}
```

### Existing `Arch::init()` on `X86_64Arch` — update to ONLY do GDT/IDT/syscall/basic LAPIC

The `Arch` trait `init()` (called from `kmain` step 1, before paging) must not do HPET.
Current `x86_64.rs:Arch::init()` calls `gdt::init(); idt::init(); syscall::init(); apic::init_lapic()` — this is fine (LAPIC 0xFEE00000 is accessible via the identity map that Limine provides at boot, before our own paging activation). Leave as is.

### New `pub fn init_timers()` in `x86_64.rs`

```rust
/// Post-paging timer init: HPET init + LAPIC calibration.
/// Must be called AFTER paging activation so 0xFED0_0000 (HPET) is mapped.
#[cfg(target_arch = "x86_64")]
pub fn init_timers() {
    unsafe {
        hpet::init(0xFED0_0000);
        let ticks_per_ms = hpet::calibrate_lapic();
        apic::init_lapic_calibrated(ticks_per_ms);
    }
}
```

(Defined in this phase as a stub; fully implemented in Phase 04.)

### `task.rs` — add x86_64 `BOOT_CONTEXT` arm

```rust
#[cfg(target_arch = "x86_64")]
static mut BOOT_CONTEXT: crate::hal::arch::Context = crate::hal::arch::Context {
    r15: 0, r14: 0, r13: 0, r12: 0, rbx: 0, rbp: 0, rsp: 0, rip: 0,
};
```

(Must match the `CpuContext` struct layout in `context.rs` — verify field names.)

### `task.rs` — x86_64 arms for task spawn trap-frame setup

Lines 386 and 1185 set RISC-V-specific `trap_frame.sstatus`. Add x86_64 guards:

```rust
// Line 386 area:
#[cfg(not(target_arch = "x86_64"))]
{ task.trap_frame.sstatus = 0x6020; }
#[cfg(target_arch = "x86_64")]
{ task.trap_frame.sstatus = 0x202; }  // RFLAGS: IF=1, Reserved=1

// Lines 396-400 area: add x86_64 arm after aarch64 arm
#[cfg(target_arch = "x86_64")]
{ task.context.rip = __trap_exit as *const () as usize; }
// rsp already set by: task.context.sp = tf_ptr as _;
```

---

## Related Code Files

| Action | File |
|--------|------|
| Create | `hal/arch/x86/src/x86_64/trap.rs` |
| Modify | `hal/arch/x86/src/x86_64.rs` — add `pub mod trap`, `pub mod arch { }`, `init_timers` stub |
| Modify | `kernel/src/task.rs` — add x86_64 arm to `BOOT_CONTEXT` + trap_frame setup |

---

## Implementation Steps

1. Create `hal/arch/x86/src/x86_64/trap.rs` with `ViTrapFrame` struct + `get_gp_tp()`

2. Add to `x86_64.rs`:
   - `#[cfg(target_arch = "x86_64")] pub mod trap;`
   - `pub mod arch { ... }` with all exports listed above
   - `pub fn init_timers()` stub (calls HPET init — HPET module not yet exists, stub for now)

3. Add x86_64 arm to `BOOT_CONTEXT` in `kernel/src/task.rs:38-49`

4. Add x86_64 arms to `task.trap_frame.sstatus` assignments and context setup
   (lines 386, 396-400, and 1185, 1192-1197)

5. Run `cargo check -p vicell-kernel --target x86_64-unknown-none`
   - Expected: zero `hal::arch` errors; may have remaining errors from `puts` / `FALLBACK_BOOT_INFO`

---

## Success Criteria

- `cargo check` shows zero `E0432`/`E0433` errors about `hal::arch` or `BOOT_CONTEXT`
- `crate::hal::arch::ViTrapFrame`, `Context`, `get_gp_tp`, `thread_trampoline` all resolve
- Remaining errors (if any) are in `main.rs` / `boot.rs` — handled in Phase 01 and 03

---

## Risk Assessment

- **MED** — `CpuContext` field names: `context.rs` defines the x86_64 context with callee-saved regs.
  Field list must match exactly for `BOOT_CONTEXT` literal. Read `context.rs` before writing the literal.
- **LOW** — `__trap_exit` external fn (`task.rs:23-25`): on x86_64 this will be a stub asm that
  does `iretq`; needs to exist in `boot.rs` or `trap.rs`. Add a `global_asm!`
  stub: `__trap_exit: iretq` in Phase 02's `boot.rs`.
- **LOW** — `init_timers()` stub: must not call `hpet::init` until Phase 04 creates `hpet.rs`.
  For Phase 00, body is `unimplemented!()` or `/* TODO: Phase 04 */`. Phase 04 implements it.

---

## Security Considerations

- `ViTrapFrame` must be `#[repr(C)]` to guarantee layout matches what the asm stub writes.
- `BOOT_CONTEXT` is `static mut` — only ever accessed in single-threaded early-boot context.
