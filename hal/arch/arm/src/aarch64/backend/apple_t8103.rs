// SPDX-License-Identifier: MPL-2.0

//! Apple M1 (t8103, Mac mini M1 J274) board backend under m1n1 (ADR-0032 §4–5).
//!
//! Provenance (Phase 04):
//! - AIC1 register definitions & interrupt dispatch: Class P (OpenBSD `sys/arch/arm64/dev/aplintc.c`, ISC license).
//! - S5L UART console registers: Class D (Linux `arch/arm64/boot/dts/apple/t8103.dtsi`, MIT license).
//! - FIQ timer & fast IPI routing: Class P (OpenBSD `aplintc.c`, ISC license).

use hal_arch_trait::{vi_ipi_service, vi_timer_tick};

use super::{
    ArmIrqController, ArmPlatform, ArmResources, ArmSoc, ArmTimer, DisplayFirmware, DtbPolicy,
    MmioMap, MmioSpan, MmioWindow, NoEl2Monitor, NoIommu, NoVcpuExt, PlatformFacts, SdhciFacts,
    SecondaryStart, VgicWindows,
};
use crate::aarch64::trap::TrapFrame;

// ── S5L UART (Apple SoC UART0 / m1n1 vUART) ──────────────────────────────────
pub const S5L_UART_BASE: usize = 0x2_3520_0000;
const S5L_UTRSTAT: usize = S5L_UART_BASE + 0x10;
const S5L_UTXH: usize = S5L_UART_BASE + 0x20;
const S5L_URXH: usize = S5L_UART_BASE + 0x24;

const UTRSTAT_RX_READY: u32 = 1 << 0;
const UTRSTAT_TX_EMPTY: u32 = 1 << 1;

// ── AIC1 (Apple Interrupt Controller v1) ─────────────────────────────────────
// AIC1 layout: TARGET_CPU at 0x3000 holds 0x400 words, so the four per-IRQ
// bitmaps follow at 0x4000 (SW_SET), 0x4080 (SW_CLR), 0x4100 (MASK_SET) and
// 0x4180 (MASK_CLR), 32 words each.
pub const AIC1_BASE: usize = 0x2_3b10_0000;
const AIC1_EVENT: usize = AIC1_BASE + 0x2004;
const AIC1_IPI_SEND: usize = AIC1_BASE + 0x2008;
const AIC1_IPI_ACK: usize = AIC1_BASE + 0x200c;
const AIC1_IPI_MASK_SET: usize = AIC1_BASE + 0x2024;
const AIC1_IPI_MASK_CLR: usize = AIC1_BASE + 0x2028;
const AIC1_MASK_SET: usize = AIC1_BASE + 0x4100;

/// `AIC_EVENT` type field (bits 23:16) value of an IPI.
const AIC_EVENT_TYPE_SHIFT: u32 = 16;
const AIC_EVENT_TYPE_IPI: u32 = 4;
/// `AIC_EVENT` number of an IPI raised by another core.
const AIC_EVENT_IPI_OTHER: u32 = 1;
/// IPI register bits: from another core / from this core.
const AIC_IPI_OTHER: u32 = 1 << 0;
const AIC_IPI_SELF: u32 = 1 << 31;

/// Generic-timer `CNT*_CTL` bits: ENABLE, IMASK, ISTATUS.
const TIMER_CTL_ENABLE: u64 = 1 << 0;
const TIMER_CTL_IMASK: u64 = 1 << 1;
const TIMER_CTL_ISTATUS: u64 = 1 << 2;

/// Whether a timer with control value `ctl` is asserting its (FIQ) output.
const fn timer_firing(ctl: u64) -> bool {
    (ctl & (TIMER_CTL_ENABLE | TIMER_CTL_IMASK | TIMER_CTL_ISTATUS))
        == (TIMER_CTL_ENABLE | TIMER_CTL_ISTATUS)
}

/// Apple Mac mini M1 (`apple_t8103`) machine backend.
pub struct AppleT8103;

impl ArmPlatform for AppleT8103 {
    type Iommu = NoIommu;

    const PL011_BASE: usize = 0;
    const SECONDARY_START: SecondaryStart = SecondaryStart::SpinTable;
    const CONSOLE_RX_IRQ: bool = false;
    const PI_MONITOR_BOOT: bool = false;
    const DTB_POLICY: DtbPolicy = DtbPolicy::Required;
    const IDLE_PROBES: usize = 0;
    type VcpuExt = NoVcpuExt;
    type El2Monitor = NoEl2Monitor;
    const LINUX_IMAGE_HEADER: bool = true;

    #[inline(always)]
    fn console_init() {}

    #[inline(always)]
    fn early_putc(byte: u8) {
        Self::s5l_putc(byte);
    }

    #[inline(always)]
    fn early_puts(s: &str) {
        for &b in s.as_bytes() {
            Self::early_putc(b);
        }
    }

    #[inline(always)]
    fn console_write_byte(byte: u8, _service_rx: impl Fn() + Copy) {
        Self::early_putc(byte);
    }

    #[inline(always)]
    fn console_poll_rx() -> Option<u8> {
        Self::s5l_poll_rx()
    }

    #[inline(always)]
    fn console_enable_rx_irq() {}

    #[inline(always)]
    fn diag_probe(_byte: u8) {}

    #[inline(always)]
    fn irq_snapshot_probe() {}

    #[inline(always)]
    fn probe_uncategorized_trap(_frame: &TrapFrame, _vector_kind: u8) {}
}

impl AppleT8103 {
    #[inline(always)]
    fn s5l_putc(byte: u8) {
        unsafe {
            let utrstat = S5L_UTRSTAT as *const u32;
            let utxh = S5L_UTXH as *mut u32;
            while core::ptr::read_volatile(utrstat) & UTRSTAT_TX_EMPTY == 0 {
                core::hint::spin_loop();
            }
            core::ptr::write_volatile(utxh, byte as u32);
        }
    }

    #[inline(always)]
    fn s5l_poll_rx() -> Option<u8> {
        unsafe {
            let utrstat = S5L_UTRSTAT as *const u32;
            let urxh = S5L_URXH as *const u32;
            if core::ptr::read_volatile(utrstat) & UTRSTAT_RX_READY != 0 {
                Some((core::ptr::read_volatile(urxh) & 0xFF) as u8)
            } else {
                None
            }
        }
    }

    #[inline(always)]
    fn aic_write(addr: usize, val: u32) {
        // SAFETY: `addr` is one of the AIC1 registers above, inside the
        // identity-mapped AIC window in `KERNEL_MMIO_MAPS`.
        unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
    }

    /// Control register of the host tick timer (`timer::reset_generic`'s timer).
    #[inline(always)]
    fn host_timer_ctl() -> u64 {
        let ctl: u64;
        // SAFETY: reads of the current EL's own timer control register.
        unsafe {
            if crate::aarch64::el2::is_el2() {
                core::arch::asm!("mrs {}, cnthp_ctl_el2", out(reg) ctl, options(nomem, nostack));
            } else {
                core::arch::asm!("mrs {}, cntp_ctl_el0", out(reg) ctl, options(nomem, nostack));
            }
        }
        ctl
    }
}

impl ArmIrqController for AppleT8103 {
    const HAS_IPI: bool = true;
    const USB_ONE_SHOT_IRQ: Option<u32> = None;

    #[inline(always)]
    fn init() {
        // Mask every hardware IRQ; reading AIC_EVENT auto-masks again later.
        for i in 0..32 {
            Self::aic_write(AIC1_MASK_SET + i * 4, 0xFFFF_FFFF);
        }
        Self::init_cpu();
    }

    #[inline(always)]
    fn init_cpu() {
        // Clear stale IPIs, keep self-IPIs masked, take IPIs from other cores.
        Self::aic_write(AIC1_IPI_ACK, AIC_IPI_SELF | AIC_IPI_OTHER);
        Self::aic_write(AIC1_IPI_MASK_SET, AIC_IPI_SELF);
        Self::aic_write(AIC1_IPI_MASK_CLR, AIC_IPI_OTHER);
        // The virtual timer is not the host tick; a firmware-armed one would
        // otherwise raise an FIQ nothing acknowledges.
        // SAFETY: CNTV_CTL_EL0 is this core's own virtual timer control.
        unsafe {
            core::arch::asm!(
                "mrs {t}, cntv_ctl_el0",
                "orr {t}, {t}, #2",
                "msr cntv_ctl_el0, {t}",
                t = out(reg) _,
                options(nomem, nostack),
            );
        }
    }

    #[inline(always)]
    fn send_ipi(target_cpu: u32) {
        // AIC1 addresses at most 32 cores, one IPI_SEND bit each.
        if target_cpu < 32 {
            Self::aic_write(AIC1_IPI_SEND, 1 << target_cpu);
        }
    }

    #[inline(always)]
    fn unmask_usb_irq() {}

    #[inline(always)]
    fn handle_irq() {
        loop {
            // SAFETY: reading AIC_EVENT acknowledges (and masks) the event.
            let event = unsafe { core::ptr::read_volatile(AIC1_EVENT as *const u32) };
            if event == 0 {
                break;
            }
            let kind = (event >> AIC_EVENT_TYPE_SHIFT) & 0xFF;
            if kind == AIC_EVENT_TYPE_IPI && event & 0xFFFF == AIC_EVENT_IPI_OTHER {
                Self::aic_write(AIC1_IPI_ACK, AIC_IPI_OTHER);
                // SAFETY: vi_ipi_service is `#[no_mangle]` in kernel::task.
                unsafe { vi_ipi_service() };
                Self::aic_write(AIC1_IPI_MASK_CLR, AIC_IPI_OTHER);
            }
            // Hardware IRQ events: no device driver is wired on this board, so
            // the auto-masked line stays masked.
        }
    }

    #[inline(always)]
    fn handle_fiq() {
        // Apple routes the core timers straight to FIQ, with no controller
        // acknowledge: re-arming clears ISTATUS, which deasserts the FIQ, and
        // must happen before vi_timer_tick context-switches away.
        if timer_firing(Self::host_timer_ctl()) {
            crate::aarch64::timer::reset();
            // SAFETY: vi_timer_tick is `#[no_mangle]` in kernel/src/task.rs.
            unsafe { vi_timer_tick() };
        }
    }
}

impl ArmTimer for AppleT8103 {
    #[inline(always)]
    fn init() {
        crate::aarch64::timer::reset_generic();
        // SAFETY: enables this EL's own timer (ENABLE = 1, IMASK = 0).
        unsafe {
            if crate::aarch64::el2::is_el2() {
                core::arch::asm!("msr cnthp_ctl_el2, {}", in(reg) TIMER_CTL_ENABLE, options(nomem, nostack));
            } else {
                core::arch::asm!("msr cntp_ctl_el0, {}", in(reg) TIMER_CTL_ENABLE, options(nomem, nostack));
            }
        }
    }

    #[inline(always)]
    fn reset() {
        crate::aarch64::timer::reset_generic();
    }
}

impl ArmResources for AppleT8103 {
    const SOC: ArmSoc = ArmSoc::AppleT8103;

    const PLATFORM: PlatformFacts = PlatformFacts {
        uart_base: S5L_UART_BASE,
        uart_irq: 0,
        rtc_base: None,
        published_virtio_slots: 0,
        announce: None,
    };

    const KERNEL_MMIO_MAPS: &'static [MmioMap] = &[
        // S5L UART (vUART) MMIO window (64 KiB page)
        MmioMap {
            base: S5L_UART_BASE,
            end: S5L_UART_BASE + 0x10000,
        },
        // AIC1 MMIO window (64 KiB page)
        MmioMap {
            base: AIC1_BASE,
            end: AIC1_BASE + 0x10000,
        },
    ];

    const VIRTIO_MMIO: Option<hal_soc_arm_virt::VirtioMmioLayout> = None;
    const VGIC: Option<VgicWindows> = None;
    const GPIO_MMIO_BASE: usize = 0;
    const DWC2_WINDOW: Option<MmioWindow> = None;
    const DISPLAY: Option<DisplayFirmware> = None;
    const SDHCI: Option<SdhciFacts> = None;
    const BOARD_PINMUX_GPIO_BASE: Option<usize> = None;
    const SUPERVISOR_DEVICE_MMIO: &'static [MmioSpan] = &[];
}
