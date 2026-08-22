//! AArch64 exception vectors and trap handlers.
//!
//! At EL1 (default): installs `__vectors` into VBAR_EL1.
//! At EL2 (virtualization=on): installs `__vectors_el2` into VBAR_EL2.
//! The runtime dispatch is driven by `EL2_ACTIVE` (set in el2.rs at boot).

use core::arch::global_asm;
#[cfg(feature = "board-rpi3")]
use hal_arch_trait::vi_handle_uart_irq;
#[cfg(all(not(feature = "board-rpi3"), not(feature = "board-rpi4")))]
use hal_arch_trait::vi_handle_virtio_irq;
use hal_arch_trait::{
    vi_current_cell_id, vi_gpio_notify_irq, vi_terminate_on_fault_aarch64, vi_timer_tick,
    ViCell_syscall_dispatch, ViTrapFrame,
};

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
    pub elr_el1: u64,  // offset 248 — holds ELR_EL2 at runtime when EL2 active
    pub spsr_el1: u64, // offset 256
    pub far_el1: u64,  // offset 264
    pub esr_el1: u64,  // offset 272
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

/// Snapshot both translation regimes and the controlling EL1/EL2 registers at
/// an uncategorized RPi3 Cell exception. This is observation-only: `AT` records
/// a result in `PAR_EL1` and never raises the translation fault it discovers.
#[cfg(feature = "board-rpi3")]
fn probe_uncategorized_el2_fault(frame: &TrapFrame, vector_kind: u8) {
    if !super::el2::is_el2() {
        return;
    }

    macro_rules! read_sysreg {
        ($name:literal) => {{
            let value: u64;
            // SAFETY: this probe runs at EL2; all named registers are readable
            // there and reads have no architectural side effects.
            unsafe {
                core::arch::asm!(
                    concat!("mrs {}, ", $name),
                    out(reg) value,
                    options(nomem, nostack)
                );
            }
            value
        }};
    }

    let pc = frame.elr_el1;
    let par_s1e0r: u64;
    let par_s1e0r_tge0: u64;
    let par_s1e2r: u64;
    // SAFETY: EL2 may query both stage-1 regimes. Clearing TGE is bounded by
    // this block and cannot expose an EL0 exception because the probe itself
    // remains at EL2; restoring HCR before UART output preserves fault routing.
    unsafe {
        core::arch::asm!(
            "isb",
            "at s1e0r, {pc}",
            "isb",
            "mrs {par_tge1}, par_el1",
            "mrs {hcr_saved}, hcr_el2",
            "bic {hcr_tge0}, {hcr_saved}, #(1 << 27)",
            "msr hcr_el2, {hcr_tge0}",
            "isb",
            "at s1e0r, {pc}",
            "isb",
            "mrs {par_tge0}, par_el1",
            "msr hcr_el2, {hcr_saved}",
            "isb",
            pc = in(reg) pc,
            par_tge1 = out(reg) par_s1e0r,
            par_tge0 = out(reg) par_s1e0r_tge0,
            hcr_saved = out(reg) _,
            hcr_tge0 = out(reg) _,
            options(nomem, nostack),
        );
        core::arch::asm!(
            "isb",
            "at s1e2r, {pc}",
            "isb",
            "mrs {par}, par_el1",
            pc = in(reg) pc,
            par = out(reg) par_s1e2r,
            options(nomem, nostack),
        );
    }

    let put_value = |name: &str, value: u64| {
        super::uart_bcm_mini::puts(name);
        super::uart_bcm_mini::probe_put(b'=');
        for shift in (0..16).rev() {
            let nibble = ((value >> (shift * 4)) & 0xf) as u8;
            super::uart_bcm_mini::probe_put(if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + nibble - 10
            });
        }
        super::uart_bcm_mini::probe_put(b' ');
    };

    super::uart_bcm_mini::puts("\nFS0 ");
    put_value("vec", vector_kind as u64);
    put_value("x19", frame.regs[19]);
    put_value("x20", frame.regs[20]);
    put_value("hcr", read_sysreg!("hcr_el2"));
    put_value("cptr", read_sysreg!("cptr_el2"));
    put_value("mdcr", read_sysreg!("mdcr_el2"));
    put_value("isr", read_sysreg!("isr_el1"));
    put_value("daif", read_sysreg!("daif"));
    super::uart_bcm_mini::puts("\nFS1 ");
    put_value("sctlr", read_sysreg!("sctlr_el1"));
    put_value("tcr", read_sysreg!("tcr_el1"));
    put_value("ttbr0", read_sysreg!("ttbr0_el1"));
    put_value("mair", read_sysreg!("mair_el1"));
    put_value("cpacr", read_sysreg!("cpacr_el1"));
    put_value("mdscr", read_sysreg!("mdscr_el1"));
    put_value("par", par_s1e0r);
    super::uart_bcm_mini::puts("\nFS2 ");
    put_value("sctlr", read_sysreg!("sctlr_el2"));
    put_value("tcr", read_sysreg!("tcr_el2"));
    put_value("ttbr0", read_sysreg!("ttbr0_el2"));
    put_value("mair", read_sysreg!("mair_el2"));
    put_value("par", par_s1e2r);
    super::uart_bcm_mini::puts("\nFS3 ");
    put_value("par_tge0", par_s1e0r_tge0);
    super::uart_bcm_mini::probe_put(b'\n');
}

/// Synchronous trap dispatcher — called from both EL1 and EL2 trampolines.
#[no_mangle]
pub extern "C" fn vi_aarch64_trap_handler(frame: &mut TrapFrame) {
    let esr = frame.esr_el1; // field holds ESR_EL2 at EL2; naming is irrelevant here
    let ec = (esr >> 26) & 0x3F;
    let vector_kind = super::el2::take_lower_vector_kind() as usize;
    #[cfg(feature = "board-rpi3")]
    if ec == 0 {
        probe_uncategorized_el2_fault(frame, vector_kind as u8);
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
            // SAFETY: both are #[no_mangle] in kernel::task and linked via
            // extern "Rust"; see the 0x20 | 0x24 arm for the teardown contract.
            let cell_id = unsafe { vi_current_cell_id() };
            let from_el0 = (frame.spsr_el1 & 0xF) == 0;
            if from_el0 && cell_id != 0 {
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
                    "[aarch64] kernel trap ec=0x{:X} esr=0x{:X} elr=0x{:X} far=0x{:X} spsr=0x{:X}",
                    ec, esr, frame.elr_el1, frame.far_el1, frame.spsr_el1
                );
            }
        }
    }
}

/// GIC ID for the PL061 GPIO controller on QEMU ARM virt (SPI 7 = GIC ID 39).
#[cfg(all(not(feature = "board-rpi3"), not(feature = "board-rpi4")))]
const GPIO_GIC_ID: u32 =
    hal_soc_arm_virt::ArmVirtProfile::gic_id_for_spi(hal_soc_arm_virt::QEMU_ARM_VIRT.gpio.spi);
#[cfg(feature = "board-rpi4")]
const GPIO_GIC_ID: u32 = u32::MAX;

/// IRQ handler — dispatches timer, GPIO, and VirtIO MMIO interrupts.
///
/// On `board-rpi3`: reads BCM2836 local IRQ source; dispatches timer PPIs
/// (nCNTPNSIRQ / nCNTHPIRQ) and GPU (BCM2835 peripheral) interrupts.
///
/// On QEMU virt (default): uses GIC claim/complete cycle.
/// Timer PPIs: EL1 = GIC ID 30 (CNTP), EL2 = GIC ID 26 (CNTHP).
/// GPIO PL061: GIC ID 39 (SPI 7); VirtIO MMIO: GIC IDs 48..79 (SPI 16..47).
#[no_mangle]
pub extern "C" fn vi_aarch64_irq_handler(_frame: &mut TrapFrame) {
    #[cfg(feature = "board-rpi3")]
    {
        let src = super::bcm2836_irq::irq_source();
        // Timer: non-secure physical (EL1) or hypervisor physical (EL2).
        let timer_bits =
            super::bcm2836_irq::IRQ_SRC_TIMER_NS | super::bcm2836_irq::IRQ_SRC_TIMER_HP;
        // Fallback: check ARM generic timer ISTATUS directly.
        // QEMU raspi3b may not update CORE0_IRQ_SOURCE even when the timer fires,
        // routing the interrupt directly to the CPU IRQ line.  The timer CTL ISTATUS
        // bit (bit 2) is hardware-authoritative: set iff the timer condition is met.
        // Use CNTP_CTL_EL0 at EL1, CNTHP_CTL_EL2 at EL2.
        let timer_fired_by_status: bool = if super::el2::is_el2() {
            unsafe {
                let ctl: u64;
                core::arch::asm!("mrs {}, cnthp_ctl_el2", out(reg) ctl, options(nomem, nostack));
                ctl & (1 << 2) != 0
            }
        } else {
            unsafe {
                let ctl: u64;
                core::arch::asm!("mrs {}, cntp_ctl_el0", out(reg) ctl, options(nomem, nostack));
                ctl & (1 << 2) != 0
            }
        };
        let aux_pending = super::bcm2835_legacy_irq::is_aux_irq_pending();
        if src & timer_bits != 0 || timer_fired_by_status {
            super::timer::reset();
            if aux_pending {
                // The mini UART has only an eight-symbol RX FIFO. Drain it after
                // acknowledging the timer but before the scheduler tick can run.
                unsafe {
                    vi_handle_uart_irq();
                }
            }
            // SAFETY: vi_timer_tick is #[no_mangle] in kernel/src/task.rs.
            unsafe {
                vi_timer_tick();
            }
            return;
        }
        // GPU pass-through: BCM2835 peripheral interrupts (system timer, GPIO, …).
        // Check BCM2835 systimer C1 FIRST — both when CORE0_IRQ_SOURCE bit 8 is set
        // (normal path) AND unconditionally as a fallback.  QEMU raspi3b may deliver
        // the BCM2835 IRQ to the CPU nIRQ line without setting bit 8 in
        // CORE0_IRQ_SOURCE (the GPU routing register may not be fully emulated).
        let systimer_pending = super::bcm2835_systimer::is_c1_pending();
        if systimer_pending {
            super::timer::reset(); // ack C1 + re-arm
            if aux_pending {
                // Preserve the same FIFO-first contract for the BCM system timer.
                unsafe {
                    vi_handle_uart_irq();
                }
            }
            // SAFETY: vi_timer_tick is #[no_mangle] in kernel/src/task.rs.
            unsafe {
                vi_timer_tick();
            }
            return;
        }
        // Like the system timer above, check the legacy pending bit directly:
        // some raspi3 environments deliver nIRQ without reflecting GPU routing
        // in CORE0_IRQ_SOURCE.
        if aux_pending {
            // Draining AUX_MU_IO deasserts the level-triggered RX interrupt.
            // SAFETY: vi_handle_uart_irq is #[no_mangle] in kernel UART drivers.
            unsafe {
                vi_handle_uart_irq();
            }
            return;
        }
        // GPU peripheral IRQs that are not the systimer.
        if src & super::bcm2836_irq::IRQ_SRC_GPU != 0 {
            // GPIO banks via BCM2835 IRQ controller.
            if super::bcm2835_legacy_irq::identify_gpio_irq().is_some() {
                // SAFETY: vi_gpio_notify_irq is #[no_mangle] in kernel/src/task/drivers/gpio_irq.rs.
                unsafe {
                    vi_gpio_notify_irq();
                }
                return;
            }
            // Other GPU sources (UART, SPI, I2C) — fall through to spurious.
        }
        // Spurious or unhandled.
        return;
    }

    #[cfg(not(feature = "board-rpi3"))]
    {
        let irq = super::gic::claim();
        let timer_irq = if super::el2::is_el2() { 26 } else { 30 };
        if irq == timer_irq {
            // Rearm the hardware countdown first.
            super::timer::reset();
            // Send EOI (priority drop) BEFORE calling vi_timer_tick().
            // vi_timer_tick() calls yield_cpu() which context-switches away.
            // GICv2: until GICC_EOIR is written, the IRQ stays "active" and the
            // GIC priority preemption logic blocks all same/lower priority IRQs.
            // EOI first → priority drop → new timer ticks can fire on any task.
            super::gic::complete(irq);
            // SAFETY: vi_timer_tick is #[no_mangle] in kernel/src/task.rs.
            unsafe {
                vi_timer_tick();
            }
            return;
        } else if irq == GPIO_GIC_ID {
            // GPIO PL061 edge: EOI before notify so the next GPIO edge can fire
            // as soon as the cell re-enables it (GPIOIE write from userspace).
            super::gic::complete(irq);
            // SAFETY: vi_gpio_notify_irq is #[no_mangle] in kernel/src/task/drivers/gpio_irq.rs.
            unsafe {
                vi_gpio_notify_irq();
            }
            return;
        } else if irq >= 32 && irq != 0x3FF {
            // SPI range (GIC ID ≥ 32): dispatch VirtIO; convert GIC ID → SPI number.
            // SAFETY: vi_handle_virtio_irq is #[no_mangle] in kernel/src/task/drivers.
            #[cfg(not(feature = "board-rpi4"))]
            unsafe {
                vi_handle_virtio_irq(irq - hal_soc_arm_virt::ArmVirtProfile::GIC_SPI_OFFSET);
            }
        }
        if irq != 0x3FF {
            super::gic::complete(irq);
        }
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
    mov  x9,  #0
    msr  spsr_el2, x9         // EL0t — Cells stay at EL0
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
    add  sp, sp, #(36 * 8)
    eret
"#
);
