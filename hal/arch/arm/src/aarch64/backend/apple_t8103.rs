// SPDX-License-Identifier: MPL-2.0

//! Apple M1 (t8103, Mac mini M1 J274) board backend under m1n1 (ADR-0032 §4–5).
//!
//! Provenance (Phase 04):
//! - AIC1 register definitions & interrupt dispatch: Class P (OpenBSD `sys/arch/arm64/dev/aplintc.c`, ISC license).
//! - S5L UART console registers: Class D (Linux `arch/arm64/boot/dts/apple/t8103.dtsi`, MIT license).
//! - FIQ timer & fast IPI routing: Class P (OpenBSD `aplintc.c`, ISC license).

use core::sync::atomic::{compiler_fence, Ordering};

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
pub const AIC1_BASE: usize = 0x2_3b10_0000;
const AIC1_EVENT: usize = AIC1_BASE + 0x2004;
const AIC1_IPI_SEND: usize = AIC1_BASE + 0x2008;
const AIC1_MASK_SET: usize = AIC1_BASE + 0x4000;

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
}

impl ArmIrqController for AppleT8103 {
    const HAS_IPI: bool = true;
    const USB_ONE_SHOT_IRQ: Option<u32> = None;

    #[inline(always)]
    fn init() {
        // AIC1 initialization: mask interrupts on boot core
        unsafe {
            let mask_set = AIC1_MASK_SET as *mut u32;
            for i in 0..32 {
                core::ptr::write_volatile(mask_set.add(i), 0xFFFF_FFFF);
            }
        }
    }

    #[inline(always)]
    fn init_cpu() {}

    #[inline(always)]
    fn send_ipi(target_cpu: u32) {
        unsafe {
            let ipi_send = AIC1_IPI_SEND as *mut u32;
            core::ptr::write_volatile(ipi_send, 1 << target_cpu);
        }
    }

    #[inline(always)]
    fn unmask_usb_irq() {}

    #[inline(always)]
    fn handle_irq() {
        unsafe {
            let event_reg = AIC1_EVENT as *mut u32;
            let event = core::ptr::read_volatile(event_reg);
            if event != 0 {
                compiler_fence(Ordering::SeqCst);
                // Acknowledge interrupt event
                core::ptr::write_volatile(event_reg, event);
            }
        }
    }

    #[inline(always)]
    fn handle_fiq() {
        // Apple timer and IPIs are routed as FIQ
        compiler_fence(Ordering::SeqCst);
    }
}

impl ArmTimer for AppleT8103 {
    #[inline(always)]
    fn init() {}

    #[inline(always)]
    fn reset() {}
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
            cell_access: false,
        },
        // AIC1 MMIO window (64 KiB page)
        MmioMap {
            base: AIC1_BASE,
            end: AIC1_BASE + 0x10000,
            cell_access: false,
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
