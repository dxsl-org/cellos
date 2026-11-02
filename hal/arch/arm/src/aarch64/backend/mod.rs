//! AArch64 board backends (ADR-0029 §6).
//!
//! The shared AArch64 core reaches board behaviour only through [`Board`]. Each
//! backend is a zero-sized type that implements [`ArmPlatform`] by wrapping the
//! existing `hal/soc/*` profile data, so dispatch is static: no `dyn`, no
//! runtime cost, and no second address table.
//!
//! Group 1 boards keep compile-time selection: the `board-rpi*` features pick
//! a backend type here. `board-rpi3` takes precedence over `board-rpi4` so this
//! crate still compiles with both, and the kernel's own conflict
//! `compile_error!` reports the combination.

#[cfg(all(feature = "board-rpi4", not(feature = "board-rpi3")))]
mod bcm2711;
#[cfg(feature = "board-rpi3")]
mod bcm2837;
#[cfg(not(any(feature = "board-rpi3", feature = "board-rpi4")))]
mod qemu_virt;

/// The backend selected for this build.
#[cfg(all(feature = "board-rpi4", not(feature = "board-rpi3")))]
pub type Board = bcm2711::Bcm2711;
/// The backend selected for this build.
#[cfg(feature = "board-rpi3")]
pub type Board = bcm2837::Bcm2837;
/// The backend selected for this build.
#[cfg(not(any(feature = "board-rpi3", feature = "board-rpi4")))]
pub type Board = qemu_virt::QemuVirt;

/// Board behaviour the shared AArch64 core and the kernel consume.
///
/// Every method runs on the boot path or in the console fast path, so
/// implementations stay allocation-free and forward straight to the board's
/// UART driver.
pub trait ArmPlatform {
    /// DMA remapping unit in front of the board's bus masters.
    type Iommu: ArmIommu;

    /// MMIO base that the shared PL011 driver (`uart_pl011`) addresses.
    const PL011_BASE: usize;

    /// Whether console RX is interrupt-driven.
    ///
    /// When `true`, the UART IRQ path (`vi_handle_uart_irq`) drains the
    /// console UART into the kernel RX ring, and the console reader consumes
    /// that ring before polling the UART directly. When `false`, the console
    /// reader only polls the UART.
    const CONSOLE_RX_IRQ: bool;

    /// Initialise the console UART. Must run before any other console call.
    fn console_init();

    /// Blocking write of one raw byte; no newline translation.
    fn early_putc(byte: u8);

    /// Blocking write of a string, with the console driver's newline policy.
    fn early_puts(s: &str);

    /// Write one byte of kernel console output.
    ///
    /// `service_rx` drains console RX into the kernel ring; boards with
    /// [`CONSOLE_RX_IRQ`](Self::CONSOLE_RX_IRQ) call it while TX waits.
    fn console_write_byte(byte: u8, service_rx: impl Fn() + Copy);

    /// Non-blocking console RX poll.
    fn console_poll_rx() -> Option<u8>;

    /// Emit a one-byte boot-progress probe. No-op on boards without probes.
    fn diag_probe(byte: u8);
}

/// Design hook for group 2/3 boards: a DMA remapping unit (SMMU) the kernel
/// would program. Shape only; no current backend has one.
pub trait ArmIommu {
    /// Whether the board has a DMA remapping unit.
    const PRESENT: bool;
}

/// No DMA remapping unit. Every current backend uses it.
pub struct NoIommu;

impl ArmIommu for NoIommu {
    const PRESENT: bool = false;
}
