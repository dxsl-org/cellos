//! ARM Generic Timer driver.
//!
//! EL1 (default):  CNTP_TVAL_EL0 / CNTP_CTL_EL0.
//! EL2 host:       CNTHP_TVAL_EL2 / CNTHP_CTL_EL2.
//!
//! Runtime dispatch via `el2::is_el2()` (set before `kmain` in boot.rs).
//!
//! On QEMU virt: CNTFRQ_EL0 = 62.5 MHz, TICKS_PER_QUANTUM = 625_000.
//! On RPi 3 (BCM2837): CNTFRQ_EL0 = 19.2 MHz, TICKS_PER_QUANTUM = 192_000.
//! `ticks_per_quantum()` reads CNTFRQ_EL0 at runtime to handle both.

#[cfg(not(feature = "board-rpi3"))]
/// Fallback ticks-per-quantum for QEMU virt (~10 ms @ 62.5 MHz).
const TICKS_PER_QUANTUM_QEMU: u64 = 625_000;
#[cfg(feature = "board-rpi3")]
const RPI3_UART_IO: usize = hal_soc_bcm27xx::BCM2837.mmio.mini_uart_io;
#[cfg(feature = "board-rpi3")]
const RPI3_SYSTIMER_BASE: usize = hal_soc_bcm27xx::BCM2837.mmio.system_timer_base;
#[cfg(feature = "board-rpi3")]
const RPI3_LEGACY_IRQ_BASE: usize = hal_soc_bcm27xx::BCM2837.mmio.legacy_irq_base;
#[cfg(feature = "board-rpi3")]
const RPI3_SYSTIMER_IRQ: u32 = hal_soc_bcm27xx::BCM2837.irq.system_timer_c1;

/// The system counter (`CNTPCT_EL0`), in its own units.
///
/// Used by bounded probes that must not wait on an interrupt — it advances
/// whether or not IRQs are unmasked in the caller's context. `CNTFRQ_EL0`
/// gives the units-per-second.
pub fn counter_now() -> u64 {
    let value: u64;
    // SAFETY: CNTPCT_EL0 is readable from EL1 and modifies nothing.
    unsafe {
        core::arch::asm!("mrs {}, cntpct_el0", out(reg) value, options(nomem, nostack));
    }
    value
}

/// The system counter's frequency in Hz (`CNTFRQ_EL0`).
///
/// Absent firmware that reports a frequency (0) falls back to QEMU virt's
/// 62.5 MHz: the callers turn this into a *bounded* budget, never an exact
/// interval, so a wrong frequency shortens or lengthens a probe rather than
/// changing what is correct.
pub fn counter_frequency_hz() -> u64 {
    let freq: u64;
    // SAFETY: CNTFRQ_EL0 is a read-only system register at every exception level.
    unsafe {
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack));
    }
    if freq == 0 {
        62_500_000
    } else {
        freq
    }
}

#[cfg(not(feature = "board-rpi3"))]
/// Compute 10 ms quantum by reading the actual timer frequency from CNTFRQ_EL0.
pub fn ticks_per_quantum() -> u64 {
    let freq: u64;
    // SAFETY: CNTFRQ_EL0 is read-only from EL1/EL2; no state modified.
    unsafe {
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack));
    }
    if freq == 0 {
        return TICKS_PER_QUANTUM_QEMU;
    }
    freq / 100 // 10 ms = 1/100 s
}

/// Initialise and arm the timer.
///
/// On `board-rpi3`: arms BOTH the BCM2835 system timer (C1, 1 MHz) AND the
/// ARM generic timer (CNTP, read from CNTFRQ_EL0). QEMU raspi3b reliably
/// delivers CNTP IRQs via CORE0_TIMERS_IRQ bit 1 → Core 0 nIRQ; the BCM2835
/// GPU path (CORE0_IRQ_SOURCE bit 8) is less reliable in QEMU 10.x. Whichever
/// fires first delivers the tick; the handler detects both via `is_c1_pending()`
/// and `cntp_ctl_el0.ISTATUS`. Only one tick is recorded per quantum.
///
/// On QEMU virt: uses the EL2 hypervisor physical timer (CNTHP) when
/// `el2::is_el2()` is true, otherwise the EL1 physical timer (CNTP).
/// IRQ enable goes to the GIC.
pub fn init() {
    // board-rpi3: arm BCM2835 C1 (1 MHz path) AND CNTP (ARM generic timer path).
    #[cfg(feature = "board-rpi3")]
    {
        super::bcm2835_systimer::init();
        // Arm CNTP as a second timer source in case QEMU does not deliver the
        // BCM2835 GPU IRQ.  CORE0_TIMERS_IRQ bit 1 routes nCNTPNSIRQ to Core 0 IRQ.
        let freq: u64;
        // SAFETY: CNTFRQ_EL0 read-only; safe at EL1.
        unsafe {
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack));
        }
        if freq > 0 {
            let ticks = freq / 100; // 10 ms quantum
                                    // SAFETY: CNTP_* are EL1 non-secure physical timer registers (EL1-accessible).
            unsafe {
                core::arch::asm!(
                    "msr cntp_tval_el0, {val}",
                    "mov {ctl}, #1",          // ENABLE=1, IMASK=0
                    "msr cntp_ctl_el0,  {ctl}",
                    val = in(reg) ticks,
                    ctl = out(reg) _,
                    options(nomem, nostack),
                );
            }
            // Route CNTP PPI (nCNTPNSIRQ, bit 1) to Core 0 IRQ line.
            super::bcm2836_irq::add_timer_enable(super::bcm2836_irq::IRQ_SRC_TIMER_NS);
        }
    }

    // QEMU virt (and all non-rpi3 boards): ARM generic timer via GIC.
    #[cfg(not(feature = "board-rpi3"))]
    {
        let ticks = ticks_per_quantum();
        if super::el2::is_el2() {
            // SAFETY: CNTHP_* are EL2-private hypervisor physical timer registers.
            unsafe {
                core::arch::asm!(
                    "msr cnthp_tval_el2, {val}",
                    "mov {ctl}, #1",
                    "msr cnthp_ctl_el2,  {ctl}",
                    val = in(reg) ticks,
                    ctl = out(reg) _,
                    options(nomem, nostack),
                );
            }
            super::gic::enable_irq(26);
        } else {
            // SAFETY: CNTP_* system registers; EL1-private.
            unsafe {
                core::arch::asm!(
                    "msr cntp_tval_el0, {val}",
                    "mov {ctl}, #1",
                    "msr cntp_ctl_el0,  {ctl}",
                    val = in(reg) ticks,
                    ctl = out(reg) _,
                    options(nomem, nostack),
                );
            }
            super::gic::enable_irq(30);
        }
    }

    // ── board-rpi3 timer diagnostic ───────────────────────────────────────────
    // Runs with IRQs masked (DAIF.I=1 — set by boot stub, not cleared until
    // Arch::enable_interrupts). Writes 3 chars to BCM AUX mini UART (-serial stdio).
    //
    // Probe 'C': BCM2835 CLO counter advances → 1 MHz oscillator running.
    // Probe 'J': CLO stuck → BCM2835 systimer not emulated by this QEMU version.
    // Probe 'I': SYSTIMER_CS bit 1 set within ~1M poll iters → C1 fires correctly.
    // Probe 'O': SYSTIMER_CS bit 1 never set → C1 compare never matched.
    // Probe 'S': IRQ_PENDING1 bit 1 set → BCM2835 IRQ pending flag visible.
    // Probe 'U': IRQ_PENDING1 bit 1 clear → interrupt pending not propagated.
    // Good path: "CIS" → BCM2835 systimer fully works; expect G+M probes after init.
    #[cfg(feature = "board-rpi3")]
    rpi3_timer_diagnostic();
}

/// Diagnostic: verify BCM2835 system timer fires on QEMU raspi3b.
///
/// Runs immediately after timer::init(), with IRQs still masked (DAIF.I=1).
/// Writes three characters to the BCM AUX mini UART (`-serial stdio`):
///
/// | Char | Meaning |
/// |------|---------|
/// | 'C'  | BCM2835 CLO counter advances — 1 MHz oscillator running |
/// | 'J'  | CLO stuck — BCM2835 systimer oscillator not running |
/// | 'I'  | SYSTIMER_CS bit 1 set in ~1M poll iters — C1 compare fires |
/// | 'O'  | SYSTIMER_CS bit 1 never set — C1 compare never matched |
/// | 'S'  | IRQ_PENDING1 bit 1 set — BCM2835 IRQ pending flag works |
/// | 'U'  | IRQ_PENDING1 bit 1 clear — interrupt pending not visible |
///
/// Expected good path: `CIS` → BCM2835 system timer fully operational.
/// `JO` → BCM2835 CLO not counting (QEMU doesn't emulate BCM2835 systimer).
/// `COU` → C1 armed but compare never matched within poll budget.
#[cfg(feature = "board-rpi3")]
fn rpi3_timer_diagnostic() {
    let uart = RPI3_UART_IO as *mut u32;

    // --- Test 1: does BCM2835 CLO (free-running 1 MHz counter) advance? ---
    // SAFETY: SYSTIMER_CLO is identity-mapped MMIO.
    let t0 = unsafe { core::ptr::read_volatile((RPI3_SYSTIMER_BASE + 0x04) as *const u32) };
    for _ in 0..5_000u32 {
        unsafe {
            core::arch::asm!("nop", options(nomem, nostack));
        }
    }
    let t1 = unsafe { core::ptr::read_volatile((RPI3_SYSTIMER_BASE + 0x04) as *const u32) };
    // SAFETY: BCM AUX mini UART is identity-mapped IO.
    unsafe {
        core::ptr::write_volatile(uart, if t1 > t0 { b'C' } else { b'J' } as u32);
    }

    // --- Test 2: does SYSTIMER_CS bit 1 (C1 match) become set? ---
    // bcm2835_systimer::init() set C1 = CLO + 10_000 (10 ms at 1 MHz).
    // 1M poll iters at QEMU speed >> 10 ms — C1 must have fired by now.
    let mut c1_fired = false;
    for _ in 0..1_000_000u32 {
        // SAFETY: SYSTIMER_CS is identity-mapped MMIO.
        let cs = unsafe { core::ptr::read_volatile(RPI3_SYSTIMER_BASE as *const u32) };
        if cs & (1 << 1) != 0 {
            c1_fired = true;
            break;
        }
    }
    // SAFETY: same UART address.
    unsafe {
        core::ptr::write_volatile(uart, if c1_fired { b'I' } else { b'O' } as u32);
    }

    // --- Test 3: is IRQ 1 visible in BCM2835 IRQ_PENDING1? ---
    // After C1 fires the BCM2835 sets bit 1 of IRQ_PENDING1.
    // SAFETY: identity-mapped MMIO.
    let pending = unsafe { core::ptr::read_volatile((RPI3_LEGACY_IRQ_BASE + 0x04) as *const u32) };
    // SAFETY: same UART address.
    unsafe {
        core::ptr::write_volatile(
            uart,
            if pending & (1 << RPI3_SYSTIMER_IRQ) != 0 {
                b'S'
            } else {
                b'U'
            } as u32,
        );
    }
}

/// Re-arm the timer for the next quantum.  Call from the IRQ handler.
///
/// board-rpi3: acknowledges and re-arms BCM2835 system timer C1.
/// Other boards: reloads CNTHP_TVAL_EL2 (EL2) or CNTP_TVAL_EL0 (EL1).
pub fn reset() {
    #[cfg(feature = "board-rpi3")]
    {
        super::bcm2835_systimer::ack_and_rearm();
        // Reload CNTP so it doesn't keep firing continuously after the first tick.
        let freq: u64;
        // SAFETY: read-only CSR.
        unsafe {
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack));
        }
        if freq > 0 {
            let ticks = freq / 100;
            // SAFETY: EL1 CNTP register, reload countdown.
            unsafe {
                core::arch::asm!(
                    "msr cntp_tval_el0, {val}",
                    val = in(reg) ticks,
                    options(nomem, nostack),
                );
            }
        }
        return;
    }
    #[cfg(not(feature = "board-rpi3"))]
    {
        let ticks = ticks_per_quantum();
        if super::el2::is_el2() {
            // SAFETY: same as init() EL2 branch.
            unsafe {
                core::arch::asm!(
                    "msr cnthp_tval_el2, {val}",
                    val = in(reg) ticks,
                    options(nomem, nostack),
                );
            }
        } else {
            // SAFETY: same as init() EL1 branch.
            unsafe {
                core::arch::asm!(
                    "msr cntp_tval_el0, {val}",
                    val = in(reg) ticks,
                    options(nomem, nostack),
                );
            }
        }
    }
}

/// Keep the host tick bounded while checking a tiny guest's WFI.
///
/// QEMU's `HELPER(wfi)` returns before the HCR_EL2.TWI trap whenever
/// `cpu_has_work()` is true (target/arm/tcg/op_helper.c), so the guest WFI is
/// only a trap when no physical interrupt is pending. Boot runs with DAIF
/// masked, so any IRQ source left asserted by an earlier guest preemption
/// would turn the smoke's WFI into a NOP forever. Deassert C1 and disable the
/// CNTP source here; `bcm2835_systimer::init()` + `timer::reset()` restore the
/// ordinary 10 ms cadence once the smoke completes.
#[cfg(feature = "board-rpi3")]
pub fn prepare_monitor_smoke_window() {
    super::bcm2835_systimer::arm_smoke_window();
    let freq: u64;
    unsafe {
        core::arch::asm!("mrs {freq}, cntfrq_el0", freq = out(reg) freq,
                         options(nomem, nostack));
        if freq != 0 {
            core::arch::asm!("msr cntp_tval_el0, {ticks}",
                             ticks = in(reg) freq / 10, options(nomem, nostack));
        }
        // CTL = 0: disabled and masked, so CNTP cannot assert the core IRQ
        // line while the smoke checks WFI. `timer::reset()` re-enables it.
        core::arch::asm!("msr cntp_ctl_el0, xzr", options(nomem, nostack));
    }
}

/// Arm both host tick sources ~1 ms out so the smoke can prove that a guest
/// which never yields is still preempted by the host timer.
#[cfg(feature = "board-rpi3")]
pub fn arm_monitor_smoke_preemption() {
    super::bcm2835_systimer::arm_smoke_window_ticks(1_000);
    let freq: u64;
    unsafe {
        core::arch::asm!("mrs {freq}, cntfrq_el0", freq = out(reg) freq,
                         options(nomem, nostack));
        if freq != 0 {
            core::arch::asm!(
                "msr cntp_tval_el0, {ticks}",
                "mov {ctl}, #1",           // ENABLE=1, IMASK=0
                "msr cntp_ctl_el0,  {ctl}",
                ticks = in(reg) freq / 1_000,
                ctl = out(reg) _,
                options(nomem, nostack),
            );
        }
    }
}

/// Read the current cycle counter (CNTPCT_EL0).
pub fn read_ticks() -> u64 {
    let val: u64;
    // SAFETY: CNTPCT_EL0 is read-only; no state modified.
    unsafe {
        core::arch::asm!("mrs {}, cntpct_el0", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Read the timer frequency (CNTFRQ_EL0) in Hz.
pub fn read_freq() -> u64 {
    let val: u64;
    // SAFETY: CNTFRQ_EL0 is read-only; no state modified.
    unsafe {
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) val, options(nomem, nostack));
    }
    val
}
