//! BCM2837 trap paths: IRQ dispatch through the BCM2836 local and BCM2835
//! legacy controllers, and the uncategorized-exception EL2 probe.

use crate::aarch64::trap::TrapFrame;
use hal_arch_trait::{vi_gpio_notify_irq, vi_handle_uart_irq, vi_signal_usb_irq, vi_timer_tick};

/// Snapshot both translation regimes and the controlling EL1/EL2 registers at
/// an uncategorized RPi3 Cell exception. This is observation-only: `AT` records
/// a result in `PAR_EL1` and never raises the translation fault it discovers.
pub fn probe_uncategorized_el2_fault(frame: &TrapFrame, vector_kind: u8) {
    if !crate::aarch64::el2::is_el2() {
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

/// IRQ handler body for the BCM2837.
///
/// Reads the BCM2836 local IRQ source; dispatches timer PPIs (nCNTPNSIRQ /
/// nCNTHPIRQ) and GPU (BCM2835 peripheral) interrupts.
#[inline(always)]
pub fn handle_irq() {
    let src = super::bcm2836_irq::irq_source();
    // Timer: non-secure physical (EL1) or hypervisor physical (EL2).
    let timer_bits = super::bcm2836_irq::IRQ_SRC_TIMER_NS | super::bcm2836_irq::IRQ_SRC_TIMER_HP;
    // Fallback: check ARM generic timer ISTATUS directly.
    // QEMU raspi3b may not update CORE0_IRQ_SOURCE even when the timer fires,
    // routing the interrupt directly to the CPU IRQ line.  The timer CTL ISTATUS
    // bit (bit 2) is hardware-authoritative: set iff the timer condition is met.
    // Use CNTP_CTL_EL0 at EL1, CNTHP_CTL_EL2 at EL2.
    let timer_fired_by_status: bool = if crate::aarch64::el2::is_el2() {
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
        crate::aarch64::timer::reset();
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
        crate::aarch64::timer::reset(); // ack C1 + re-arm
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
    if src & super::bcm2836_irq::IRQ_SRC_GPU != 0 || super::bcm2835_legacy_irq::is_usb_irq_pending()
    {
        // USB DWC2 interrupt via BCM2835 IRQ controller (legacy IRQ 9).
        if super::bcm2835_legacy_irq::is_usb_irq_pending() {
            // ONE-SHOT PROTOCOL:
            // Mask IRQ 9 immediately to prevent interrupt storm while driver cell runs.
            super::bcm2835_legacy_irq::disable_irq(super::bcm2835_legacy_irq::USB_IRQ);
            // Signal WaitIrq waiter for USB_IRQ
            unsafe {
                vi_signal_usb_irq();
            }
            return;
        }
        // GPIO banks via BCM2835 IRQ controller.
        if super::bcm2835_legacy_irq::identify_gpio_irq().is_some() {
            // SAFETY: vi_gpio_notify_irq is #[no_mangle] in kernel/src/task/drivers/gpio_irq.rs.
            unsafe {
                vi_gpio_notify_irq();
            }
        }
        // Other GPU sources (UART, SPI, I2C) — fall through to spurious.
    }
    // Spurious or unhandled.
}
