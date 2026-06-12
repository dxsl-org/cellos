//! AArch64 exception vectors and trap handlers.
use core::arch::global_asm;

/// Saved register state on entry to a trap handler.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct TrapFrame {
    pub regs: [u64; 31],
    pub elr_el1:  u64,
    pub spsr_el1: u64,
    pub far_el1:  u64,
    pub esr_el1:  u64,
}

/// Mirror of `hal_riscv::rv64::trap::ViTrapFrame` — same `#[repr(C)]` layout.
/// Needed because hal-arm does not depend on hal-riscv; both are `#[repr(C)]`
/// so the binary call to `ViCell_syscall_dispatch` is well-defined by layout.
#[derive(Default, Clone, Copy)]
#[repr(C)]
struct ViTrapFrameBridge {
    pub regs:    [usize; 32],
    pub sstatus: usize,
    pub sepc:    usize,
    pub stval:   usize,
    pub scause:  usize,
}

/// Bridge ARM64 SVC registers into the kernel's generic syscall dispatcher.
fn svc_dispatch(frame: &mut TrapFrame) {
    extern "Rust" {
        fn ViCell_syscall_dispatch(frame: &mut ViTrapFrameBridge);
    }
    let mut vtf = ViTrapFrameBridge::default();
    vtf.regs[17] = frame.regs[0] as usize; // syscall number (x0)
    vtf.regs[10] = frame.regs[1] as usize; // a0 (x1)
    vtf.regs[11] = frame.regs[2] as usize; // a1 (x2)
    vtf.regs[12] = frame.regs[3] as usize; // a2 (x3)
    vtf.regs[13] = frame.regs[4] as usize; // a3 (x4)
    vtf.sepc     = frame.elr_el1 as usize;
    // SAFETY: ViTrapFrameBridge is layout-identical to hal_riscv::ViTrapFrame
    // (both #[repr(C)], same fields and order). The kernel side is #[no_mangle]
    // extern "Rust" and will be resolved to the same symbol at link time.
    unsafe { ViCell_syscall_dispatch(&mut vtf); }
    frame.regs[0] = vtf.regs[10] as u64; // return value → x0
}

/// Install the vector table.
pub fn init() {
    extern "C" { static __vectors: u8; }
    let vbar = unsafe { &__vectors as *const u8 as u64 };
    // SAFETY: VBAR_EL1 is EL1-private; address is 2048-byte aligned (enforced by .balign).
    unsafe {
        core::arch::asm!("msr vbar_el1, {}", in(reg) vbar, options(nomem, nostack));
    }
}

/// Synchronous trap dispatcher.
#[no_mangle]
pub extern "C" fn vi_aarch64_trap_handler(frame: &mut TrapFrame) {
    let esr = frame.esr_el1;
    let ec  = (esr >> 26) & 0x3F;
    match ec {
        // EC 0x15 = SVC instruction from AArch64.
        // ViCell ARM64 syscall ABI: x0=syscall_nr, x1=a0, x2=a1, x3=a2, x4=a3.
        // We bridge to ViCell_syscall_dispatch which expects a ViTrapFrame where
        // regs[17]=syscall_nr, regs[10..13]=a0..a3, regs[10]=return value.
        // ELR_EL1 already points past the SVC on return — no manual advance needed.
        0x15 => {
            svc_dispatch(frame);
        }
        _ => {
            panic!("[aarch64] trap ec=0x{:X} esr=0x{:X} elr=0x{:X} far=0x{:X}", ec, esr, frame.elr_el1, frame.far_el1);
        }
    }
}

/// IRQ handler.
#[no_mangle]
pub extern "C" fn vi_aarch64_irq_handler(_frame: &mut TrapFrame) {
    extern "Rust" { fn vi_timer_tick(); }
    let irq = super::gic::claim();
    if irq == 30 {
        // Generic Timer SPI 30: advance the scheduler tick and re-arm.
        // timer::reset() reloads CNTP_TVAL so the level-triggered interrupt
        // deasserts before gic::complete() — prevents an immediate re-entry.
        // SAFETY: vi_timer_tick is #[no_mangle] in kernel/src/task.rs; called
        // from EL1 IRQ context with all callee-saved registers live.
        unsafe { vi_timer_tick(); }
        super::timer::reset();
    }
    if irq != 0x3FF { super::gic::complete(irq); }
}

/// Noop: ARM64 uses SP_EL0 via context switch, not an sscratch-style CSR.
pub fn set_kernel_stack(_top: usize) {}

/// Unmask IRQs by clearing DAIF.I.
pub fn enable_interrupts() {
    // SAFETY: msr daifclr from EL1 is always permitted.
    unsafe { core::arch::asm!("msr daifclr, #2", options(nomem, nostack)); }
}

/// ARM64 has no GP/TP registers — return zeroes so kernel spawn paths compile.
pub fn get_gp_tp() -> (usize, usize) { (0, 0) }

global_asm!(r#"
    .section .text
    .global thread_trampoline
    .balign 4
thread_trampoline:
    msr daifclr, #2          // enable IRQ (I bit cleared)
    mov x0, x19              // arg  (s0-equiv stored in x19 by spawn setup)
    br  x20                  // entry (s1-equiv stored in x20 by spawn setup)
"#);

global_asm!(r#"
    // __trap_exit — restore ViTrapFrame from the kernel stack and eret to user mode.
    //
    // Called when a spawned task runs for the first time (context.x30 = __trap_exit).
    // On entry: sp → arch::ViTrapFrame (288 bytes, layout: regs[32], sstatus, sepc, stval, scause).
    //
    // Offsets: regs[N] = N*8; sstatus = 256; sepc = 264.
    // SPSR_EL1 = 0 (EL0t, all interrupts unmasked) — RISC-V sstatus values are not
    // directly applicable to ARM64 SPSR; hardcode EL0 entry for initial bring-up.
    .section .text
    .global __trap_exit
    .balign 4
__trap_exit:
    ldr  x9,  [sp, #264]     // sepc → ELR_EL1 (user entry point)
    msr  elr_el1, x9
    mov  x9,  #0
    msr  spsr_el1, x9         // EL0t, no interrupt masking
    ldr  x9,  [sp, #16]      // regs[2] = user sp
    msr  sp_el0, x9
    ldp  x0,  x1,  [sp, #0]
    ldp  x2,  x3,  [sp, #16]
    ldp  x4,  x5,  [sp, #32]
    ldp  x6,  x7,  [sp, #48]
    ldp  x8,  x9,  [sp, #64]
    ldp  x10, x11, [sp, #80]
    ldp  x12, x13, [sp, #96]
    ldp  x14, x15, [sp, #112]
    ldp  x16, x17, [sp, #128]
    ldp  x18, x19, [sp, #144]
    ldp  x20, x21, [sp, #160]
    ldp  x22, x23, [sp, #176]
    ldp  x24, x25, [sp, #192]
    ldp  x26, x27, [sp, #208]
    ldp  x28, x29, [sp, #224]
    ldr  x30,       [sp, #240]
    add  sp, sp, #288
    eret
"#);

global_asm!(
r#"
    // AArch64 vector table — ARM spec requires each entry at VBAR + N*0x80.
    // SAVE_REGS + branch + RESTORE_REGS + eret = ~188 bytes which overflows the
    // 128-byte (0x80) slot.  Use a single `b` per slot branching to out-of-line
    // trampolines that have no size constraint.
    .section .text.vectors
    .global __vectors
    .balign 2048
__vectors:
    // ── Current EL, SP_EL0 ──────────────────────────────────────────────────
    .balign 0x80; b vt_sync_sp0
    .balign 0x80; b vt_irq_sp0
    .balign 0x80; b vt_sync_sp0        // FIQ → treat as sync
    .balign 0x80; b vt_sync_sp0        // SError → treat as sync
    // ── Current EL, SP_ELx ──────────────────────────────────────────────────
    .balign 0x80; b vt_sync_spx
    .balign 0x80; b vt_irq_spx
    .balign 0x80; b vt_sync_spx
    .balign 0x80; b vt_sync_spx
    // ── Lower EL (AArch64) ───────────────────────────────────────────────────
    .balign 0x80; b vt_sync_el0
    .balign 0x80; b vt_irq_el0
    .balign 0x80; b vt_sync_el0
    .balign 0x80; b vt_sync_el0
    // ── Lower EL (AArch32) ── not supported ─────────────────────────────────
    .balign 0x80; b .
    .balign 0x80; b .
    .balign 0x80; b .
    .balign 0x80; b .

    // ── Out-of-line trampolines ──────────────────────────────────────────────
    // TrapFrame layout (35 * 8 = 280 bytes):
    //   x0..x30  at offsets 0..240 (each 8 bytes)
    //   elr_el1  at 248
    //   spsr_el1 at 256
    //   far_el1  at 264
    //   esr_el1  at 272
    .section .text
    .balign 4
vt_sync_sp0:
vt_sync_spx:
vt_sync_el0:
    sub  sp, sp, #(35 * 8)
    stp  x0,  x1,  [sp, #0]
    stp  x2,  x3,  [sp, #16]
    stp  x4,  x5,  [sp, #32]
    stp  x6,  x7,  [sp, #48]
    stp  x8,  x9,  [sp, #64]
    stp  x10, x11, [sp, #80]
    stp  x12, x13, [sp, #96]
    stp  x14, x15, [sp, #112]
    stp  x16, x17, [sp, #128]
    stp  x18, x19, [sp, #144]
    stp  x20, x21, [sp, #160]
    stp  x22, x23, [sp, #176]
    stp  x24, x25, [sp, #192]
    stp  x26, x27, [sp, #208]
    stp  x28, x29, [sp, #224]
    str  x30,       [sp, #240]
    mrs  x9,  elr_el1
    mrs  x10, spsr_el1
    mrs  x11, far_el1
    mrs  x12, esr_el1
    stp  x9,  x10, [sp, #248]
    stp  x11, x12, [sp, #264]
    mov  x0,  sp
    bl   vi_aarch64_trap_handler
    ldp  x9,  x10, [sp, #248]
    msr  elr_el1,  x9
    msr  spsr_el1, x10
    ldp  x0,  x1,  [sp, #0]
    ldp  x2,  x3,  [sp, #16]
    ldp  x4,  x5,  [sp, #32]
    ldp  x6,  x7,  [sp, #48]
    ldp  x8,  x9,  [sp, #64]
    ldp  x10, x11, [sp, #80]
    ldp  x12, x13, [sp, #96]
    ldp  x14, x15, [sp, #112]
    ldp  x16, x17, [sp, #128]
    ldp  x18, x19, [sp, #144]
    ldp  x20, x21, [sp, #160]
    ldp  x22, x23, [sp, #176]
    ldp  x24, x25, [sp, #192]
    ldp  x26, x27, [sp, #208]
    ldp  x28, x29, [sp, #224]
    ldr  x30,       [sp, #240]
    add  sp, sp, #(35 * 8)
    eret

    .balign 4
vt_irq_sp0:
vt_irq_spx:
vt_irq_el0:
    sub  sp, sp, #(35 * 8)
    stp  x0,  x1,  [sp, #0]
    stp  x2,  x3,  [sp, #16]
    stp  x4,  x5,  [sp, #32]
    stp  x6,  x7,  [sp, #48]
    stp  x8,  x9,  [sp, #64]
    stp  x10, x11, [sp, #80]
    stp  x12, x13, [sp, #96]
    stp  x14, x15, [sp, #112]
    stp  x16, x17, [sp, #128]
    stp  x18, x19, [sp, #144]
    stp  x20, x21, [sp, #160]
    stp  x22, x23, [sp, #176]
    stp  x24, x25, [sp, #192]
    stp  x26, x27, [sp, #208]
    stp  x28, x29, [sp, #224]
    str  x30,       [sp, #240]
    mrs  x9,  elr_el1
    mrs  x10, spsr_el1
    mrs  x11, far_el1
    mrs  x12, esr_el1
    stp  x9,  x10, [sp, #248]
    stp  x11, x12, [sp, #264]
    mov  x0,  sp
    bl   vi_aarch64_irq_handler
    ldp  x9,  x10, [sp, #248]
    msr  elr_el1,  x9
    msr  spsr_el1, x10
    ldp  x0,  x1,  [sp, #0]
    ldp  x2,  x3,  [sp, #16]
    ldp  x4,  x5,  [sp, #32]
    ldp  x6,  x7,  [sp, #48]
    ldp  x8,  x9,  [sp, #64]
    ldp  x10, x11, [sp, #80]
    ldp  x12, x13, [sp, #96]
    ldp  x14, x15, [sp, #112]
    ldp  x16, x17, [sp, #128]
    ldp  x18, x19, [sp, #144]
    ldp  x20, x21, [sp, #160]
    ldp  x22, x23, [sp, #176]
    ldp  x24, x25, [sp, #192]
    ldp  x26, x27, [sp, #208]
    ldp  x28, x29, [sp, #224]
    ldr  x30,       [sp, #240]
    add  sp, sp, #(35 * 8)
    eret
"#
);
