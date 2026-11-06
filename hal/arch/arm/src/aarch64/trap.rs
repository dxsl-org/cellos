//! AArch64 exception vectors and trap handlers.
//!
//! At EL1 (default): installs `__vectors` into VBAR_EL1.
//! At EL2 (virtualization=on): installs `__vectors_el2` into VBAR_EL2.
//! The runtime dispatch is driven by `EL2_ACTIVE` (set in el2.rs at boot).

use core::arch::global_asm;
use hal_arch_trait::{
    vi_current_cell_id, vi_gpio_notify_irq, vi_ipi_service, vi_terminate_on_fault_aarch64,
    vi_timer_tick, ViCell_syscall_dispatch, ViTrapFrame,
};

use super::backend::{ArmIrqController, ArmPlatform, Board};
use super::gic::{self, GicV2Layout};

/// Saved register state on entry to a trap handler.
///
/// Field names use `_el1` suffixes matching the EL1 register names; at EL2
/// the assembly saves `elr_el2`/`spsr_el2`/`far_el2`/`esr_el2` into the same
/// offsets — the struct is a plain bag of u64 and the names are irrelevant at
/// the Rust level.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct TrapFrame {
    pub regs: [u64; 31],
    pub elr_el1: u64,   // offset 248 — holds ELR_EL2 at runtime when EL2 active
    pub spsr_el1: u64,  // offset 256
    pub far_el1: u64,   // offset 264
    pub esr_el1: u64,   // offset 272
    pub ttbr0_el1: u64, // offset 280
}

/// Bridge ARM64 SVC registers into the kernel's generic syscall dispatcher.
fn svc_dispatch(frame: &mut TrapFrame) {
    let mut vtf = ViTrapFrame::default();
    vtf.regs[17] = frame.regs[0] as usize; // syscall number (x0)
    vtf.regs[10] = frame.regs[1] as usize; // a0 (x1)
    vtf.regs[11] = frame.regs[2] as usize; // a1 (x2)
    vtf.regs[12] = frame.regs[3] as usize; // a2 (x3)
    vtf.regs[13] = frame.regs[4] as usize; // a3 (x4)
                                           // elr_el1 holds ELR_EL1 at EL1, or ELR_EL2 at EL2 — both are the
                                           // return address past the SVC instruction that the kernel needs.
    vtf.sepc = frame.elr_el1 as usize;
    ViCell_syscall_dispatch(&mut vtf);
    frame.regs[0] = vtf.regs[10] as u64; // return value → x0
}

/// Install the exception vector table.
///
/// At EL1: writes `__vectors` into VBAR_EL1.
/// At EL2: writes `__vectors_el2` into VBAR_EL2.
pub fn init() {
    extern "C" {
        static __vectors: u8;
        static __vectors_el2: u8;
    }
    if super::el2::is_el2() {
        let vbar = unsafe { &__vectors_el2 as *const u8 as u64 };
        // SAFETY: VBAR_EL2 is EL2-private; address is 2048-byte aligned
        // (enforced by `.balign 2048` in el2.rs global_asm).
        unsafe {
            core::arch::asm!("msr vbar_el2, {}", in(reg) vbar, options(nomem, nostack));
        }
    } else {
        let vbar = unsafe { &__vectors as *const u8 as u64 };
        // SAFETY: VBAR_EL1 is EL1-private; address is 2048-byte aligned.
        unsafe {
            core::arch::asm!("msr vbar_el1, {}", in(reg) vbar, options(nomem, nostack));
        }
    }
}

/// Synchronous trap dispatcher — called from both EL1 and EL2 trampolines.
#[no_mangle]
pub extern "C" fn vi_aarch64_trap_handler(frame: &mut TrapFrame) {
    let esr = frame.esr_el1; // field holds ESR_EL2 at EL2; naming is irrelevant here
    let ec = (esr >> 26) & 0x3F;
    let vector_kind = super::el2::take_lower_vector_kind() as usize;
    if ec == 0 {
        Board::probe_uncategorized_trap(frame, vector_kind as u8);
    }
    match ec {
        // EC 0x15 = SVC instruction from AArch64.
        // ViCell ARM64 syscall ABI: x0=syscall_nr, x1=a0, x2=a1, x3=a2, x4=a3.
        0x15 => {
            svc_dispatch(frame);
        }
        // EC 0x20 = Instruction Abort from lower EL (EL0 cell).
        // EC 0x24 = Data Abort from lower EL (EL0 cell).
        // With HCR_EL2.TGE=1 all EL0 exceptions trap to EL2 — these ECs only
        // arrive from EL0 in our setup (there is no EL1 guest).
        0x20 | 0x24 => {
            // A Cell's access to a sub-page MMIO window it owns (the BCM2837
            // mailbox shares its page with the IRQ controller, so that page is
            // never USER-mapped): the kernel performs the one access and the
            // Cell resumes after the instruction. Every other abort is a fault.
            // SAFETY: #[no_mangle] in the kernel resource registry; the lower-EL
            // EC proves the frame belongs to the interrupted EL0 context.
            if ec == 0x24
                && unsafe {
                    hal_arch_trait::vi_emulate_el0_mmio_aarch64(
                        esr as usize,
                        frame.far_el1 as usize,
                        frame.elr_el1 as usize,
                        &mut frame.regs,
                    )
                }
            {
                frame.elr_el1 += 4;
                return;
            }
            // Lower-EL EC proves the privilege origin, but a recoverable Cell
            // fault also needs a live attribution.  Never manufacture a
            // deferred record for an inconsistent unowned EL0 trap.
            let cell_id = unsafe { vi_current_cell_id() };
            if cell_id != 0 {
                // SAFETY: vi_terminate_on_fault_aarch64 is #[no_mangle] in
                // kernel::task; this lower-EL trap has proved the origin.
                unsafe {
                    vi_terminate_on_fault_aarch64(
                        esr as usize,
                        frame.elr_el1 as usize,
                        frame.far_el1 as usize,
                        frame.spsr_el1 as usize,
                        vector_kind,
                    );
                }
            } else {
                panic!(
                    "[aarch64] unowned lower-EL trap ec=0x{:X} esr=0x{:X} elr=0x{:X} far=0x{:X} spsr=0x{:X}",
                    ec, esr, frame.elr_el1, frame.far_el1, frame.spsr_el1
                );
            }
        }
        // Every other EC: a cell must never be able to panic the kernel.
        //
        // Mirrors the RISC-V dispatcher (hal-riscv rv64/trap.rs, `_` arm). The
        // ECs above are lower-EL-only encodings, so reaching them at all proves
        // a cell raised them. These do not have that property — EC 0x22 (PC
        // alignment) and 0x26 (SP alignment) carry the same value whether EL0 or
        // EL2 raised them — so the originating EL has to come from SPSR.M[3:0]
        // (0b0000 = EL0t). A cell that corrupts its PC to a misaligned address
        // previously landed here and panicked the whole kernel; it now dies
        // alone, exactly as the 0x20 (branch-to-unmapped) case already did.
        //
        // Checking cell_id != 0 alone is insufficient, for the reason the
        // RISC-V side documents: the kernel faulting while servicing a cell's
        // syscall still has a non-zero CURRENT_CELL_ID but is executing at EL2,
        // and misreading that as a cell fault would silently kill the cell and
        // bury the kernel bug.
        _ => {
            if matches!(ec, 0x21 | 0x25) {
                let mut vtf = ViTrapFrame {
                    stval: frame.far_el1 as usize,
                    sepc: frame.elr_el1 as usize,
                    scause: esr as usize,
                    sstatus: frame.spsr_el1 as usize,
                    ..ViTrapFrame::default()
                };
                if unsafe { hal_arch_trait::vi_user_copy_guard_fault(&mut vtf) } {
                    frame.elr_el1 = vtf.sepc as u64;
                    return;
                }
            }
            // SAFETY: both are #[no_mangle] in kernel::task and linked via
            // extern "Rust"; see the 0x20 | 0x24 arm for the teardown contract.
            let cell_id = unsafe { vi_current_cell_id() };
            // Which EL raised this is *not* readable from `spsr_el1` here. On QEMU
            // virt the host runs at EL2 with HCR_EL2.TGE=1, so this field holds
            // SPSR_EL2 — the host's own resume state — and it reads 0 whenever the
            // host was about to `eret` to EL0, which is indistinguishable from a
            // genuine EL0 trap's SPSR. A host trap was read that way and killed an
            // innocent cell: `ec=0x0 esr=0x2000000 elr=0x400823a4 far=0 spsr=0`
            // (ELR inside `__trap_exit`) terminated the current cell instead of
            // reporting a kernel bug, which is what this arm's own contract asks
            // for. The lower-EL wrapper's marker is the proof that a cell raised
            // the trap — it is zero for every host vector, and nonzero (1..4) for
            // the EL0 ones — so it is what decides here.
            let from_lower_el = vector_kind != 0;
            if from_lower_el && cell_id != 0 {
                // SAFETY: as above — switches away from this (now dead) cell.
                unsafe {
                    vi_terminate_on_fault_aarch64(
                        esr as usize,
                        frame.elr_el1 as usize,
                        frame.far_el1 as usize,
                        frame.spsr_el1 as usize,
                        vector_kind,
                    );
                }
            } else {
                panic!(
                    "[aarch64] kernel trap ec=0x{:X} esr=0x{:X} elr=0x{:X} far=0x{:X} spsr=0x{:X} vector={}",
                    ec, esr, frame.elr_el1, frame.far_el1, frame.spsr_el1, vector_kind
                );
            }
        }
    }
}

/// IRQ handler — dispatches through the board backend's interrupt controller.
#[no_mangle]
pub extern "C" fn vi_aarch64_irq_handler(_frame: &mut TrapFrame) {
    <Board as ArmIrqController>::handle_irq();
}

/// GICv2 claim/dispatch/complete cycle, shared by the GICv2 backends.
///
/// Timer PPIs: EL1 = GIC ID 30 (CNTP), EL2 = GIC ID 26 (CNTHP). The cross-hart
/// IPI is [`gic::SGI_IPI`]; the GPIO controller is `G::GPIO_GIC_ID`; other SPIs
/// go to `G::dispatch_spi` (QEMU virt: VirtIO MMIO, GIC IDs 48..79).
#[inline(always)]
pub fn handle_gic_irq<G: GicV2Layout>() {
    let irq = gic::claim::<G>();
    let timer_irq = if super::el2::is_el2() { 26 } else { 30 };
    if irq == gic::SGI_IPI {
        // The kernel's cross-hart IPI: a remote hart asked this one to
        // invalidate its TLB and publish the epoch, or to answer a retiring
        // generation's request. It is *not* a preemption request, so it does
        // not enter the timer path: a hart parked in WFI takes its whole life
        // in `smp_aarch64_secondary_main`'s loop and has no boot context to be
        // switched to, and a hart running a task must not re-enter the
        // scheduler between a reader's load and its use.
        // SAFETY: vi_ipi_service is `#[no_mangle]` in kernel::task.
        unsafe {
            vi_ipi_service();
        }
        gic::complete::<G>(irq);
        return;
    }
    if irq == timer_irq {
        // Rearm the hardware countdown first.
        super::timer::reset();
        // Send EOI (priority drop) BEFORE calling vi_timer_tick().
        // vi_timer_tick() calls yield_cpu() which context-switches away.
        // GICv2: until GICC_EOIR is written, the IRQ stays "active" and the
        // GIC priority preemption logic blocks all same/lower priority IRQs.
        // EOI first → priority drop → new timer ticks can fire on any task.
        gic::complete::<G>(irq);
        // SAFETY: vi_timer_tick is #[no_mangle] in kernel/src/task.rs.
        unsafe {
            vi_timer_tick();
        }
        return;
    } else if irq == G::GPIO_GIC_ID {
        // GPIO PL061 edge: EOI before notify so the next GPIO edge can fire
        // as soon as the cell re-enables it (GPIOIE write from userspace).
        gic::complete::<G>(irq);
        // SAFETY: vi_gpio_notify_irq is #[no_mangle] in kernel/src/task/drivers/gpio_irq.rs.
        unsafe {
            vi_gpio_notify_irq();
        }
        return;
    } else if irq >= 32 && irq != 0x3FF {
        // SPI range (GIC ID ≥ 32): the board's device dispatch.
        G::dispatch_spi(irq);
    }
    if irq != 0x3FF {
        gic::complete::<G>(irq);
    }
}

/// Noop: ARM64 uses SP_EL0 via context switch, not an sscratch-style CSR.
pub fn set_kernel_stack(_top: usize) {}

/// Unmask IRQs by clearing DAIF.I.
pub fn enable_interrupts() {
    // SAFETY: msr daifclr from EL1/EL2 is always permitted.
    unsafe {
        core::arch::asm!("msr daifclr, #2", options(nomem, nostack));
    }
}

/// ARM64 has no GP/TP registers — return zeroes so kernel spawn paths compile.
pub fn get_gp_tp() -> (usize, usize) {
    (0, 0)
}

global_asm!(
    r#"
    .section .text
    .global thread_trampoline
    .balign 4
thread_trampoline:
    msr daifclr, #2          // enable IRQ (I bit cleared)
    mov x0, x19              // arg  (s0-equiv stored in x19 by spawn setup)
    br  x20                  // entry (s1-equiv stored in x20 by spawn setup)
"#
);

global_asm!(
    r#"
    // __trap_exit — restore ViTrapFrame from the kernel stack and eret to EL0.
    //
    // Called when a spawned task runs for the first time (context.x30 = __trap_exit).
    // On entry: sp → arch::ViTrapFrame (288 bytes, layout: regs[32], sstatus, sepc, stval, scause).
    //
    // Offsets: regs[N] = N*8; sstatus = 256; sepc = 264.
    //
    // Runtime dispatch: reads EL2_ACTIVE (1 byte, AtomicBool) at boot-time-set address.
    // EL1 path: msr elr_el1, spsr_el1.
    // EL2 path: msr elr_el2, spsr_el2.
    .section .text
    .global __trap_exit
    .balign 4
__trap_exit:
    // NOTE: NO board-specific probes here — see the no-board-probes rule at
    // vt_sync_el0. This path runs on every first task entry on all boards.
    //
    // Runtime EL dispatch via EL2_ACTIVE flag.
    // SAFETY: EL2_ACTIVE is an AtomicBool (1 byte); ldrb loads it atomically
    // for reads (store-release in el2_mark_active provides the ordering guarantee).
    adrp  x9, EL2_ACTIVE
    add   x9, x9, :lo12:EL2_ACTIVE
    ldrb  w9, [x9]
    cbnz  w9, 1f

    // ── EL1 path ─────────────────────────────────────────────────────────────
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

    // ── EL2 path ─────────────────────────────────────────────────────────────
1:
    ldr  x9,  [sp, #264]     // sepc → ELR_EL2 (user entry point)
    msr  elr_el2, x9
    // HCR_EL2.TGE routes EL0 exceptions to EL2; it does not change the
    // exception level a Cell returns to. SPSR_EL2.M=0 is AArch64 EL0t.
    // Bit 4 selects AArch32, so 0x14 is an illegal return state on an
    // AArch64-only QEMU CPU (EC=0x0e at the first Cell entry).
    mov  x9,  #0
    msr  spsr_el2, x9
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
"#
);

global_asm!(
    r#"
    // AArch64 EL1 vector table — ARM spec requires each entry at VBAR + N*0x80.
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
    // TrapFrame payload is 35 * 8 = 280 bytes. Allocate 36 * 8 = 288 bytes so
    // SP stays 16-byte aligned across the Rust handler call; the final 8 bytes
    // are padding and all field offsets below remain unchanged.
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
    // NOTE: NO board-specific probes here. This vector is SHARED by all
    // aarch64 boards; a raw MMIO write to a board-private UART (e.g. the
    // BCM 0x3F215040 mini UART) faults inside the exception vector on any
    // other board/machine → recursive sync abort → silent hang. That exact
    // bug shipped once (RPi3 bring-up probes broke QEMU virt boot-to-shell).
    // Board debug probes belong behind #[cfg(feature = "board-…")] Rust
    // code, never in this shared assembly.
    sub  sp, sp, #(36 * 8)
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
    mrs  x9,  ttbr0_el1
    str  x9,  [sp, #280]
    adrp x10, VI_KERNEL_TTBR0
    add  x10, x10, :lo12:VI_KERNEL_TTBR0
    ldr  x10, [x10]
    cbz  x10, 1f
    cmp  x9,  x10
    b.eq 1f
    dsb  ishst
    msr  ttbr0_el1, x10
    isb
1:
    mov  x0,  sp
    bl   vi_aarch64_trap_handler
    ldr  x10, [sp, #280]
    cbz  x10, 2f
    mrs  x9,  ttbr0_el1
    cmp  x9,  x10
    b.eq 2f
    dsb  ishst
    msr  ttbr0_el1, x10
    isb
2:
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
    add  sp, sp, #(36 * 8)
    eret

    // ── IRQ vectors ─────────────────────────────────────────────────────────
    // Same no-board-probes rule as vt_sync_* above.
    .balign 4
vt_irq_sp0:
vt_irq_spx:
vt_irq_el0:
    sub  sp, sp, #(36 * 8)
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
    mrs  x9,  ttbr0_el1
    str  x9,  [sp, #280]
    adrp x10, VI_KERNEL_TTBR0
    add  x10, x10, :lo12:VI_KERNEL_TTBR0
    ldr  x10, [x10]
    cbz  x10, 1f
    cmp  x9,  x10
    b.eq 1f
    dsb  ishst
    msr  ttbr0_el1, x10
    isb
1:
    mov  x0,  sp
    bl   vi_aarch64_irq_handler
    ldr  x10, [sp, #280]
    cbz  x10, 2f
    mrs  x9,  ttbr0_el1
    cmp  x9,  x10
    b.eq 2f
    dsb  ishst
    msr  ttbr0_el1, x10
    isb
2:
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
    add  sp, sp, #(36 * 8)
    eret
"#
);
