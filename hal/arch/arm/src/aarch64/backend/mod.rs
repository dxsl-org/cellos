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
pub mod bcm2837;
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
/// Console and diagnostic methods run on the boot path or in the console fast
/// path, so implementations stay allocation-free and forward straight to the
/// board's UART driver.
pub trait ArmPlatform: ArmIrqController + ArmTimer + ArmResources {
    /// DMA remapping unit in front of the board's bus masters.
    type Iommu: ArmIommu;

    /// MMIO base that the shared PL011 driver (`uart_pl011`) addresses.
    const PL011_BASE: usize;

    /// How secondary cores come online.
    const SECONDARY_START: SecondaryStart;

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

    /// Enable interrupt-driven console RX once the kernel RX ring exists.
    /// No-op on boards without [`CONSOLE_RX_IRQ`](Self::CONSOLE_RX_IRQ).
    fn console_enable_rx_irq();

    /// Emit a one-byte boot-progress probe. No-op on boards without probes.
    fn diag_probe(byte: u8);

    /// Board diagnostic for a synchronous exception with EC 0
    /// (uncategorized). No-op on boards without one.
    fn probe_uncategorized_trap(frame: &crate::aarch64::trap::TrapFrame, vector_kind: u8);
}

/// How a board brings its secondary cores online.
pub enum SecondaryStart {
    /// Firmware starts each core through `PSCI_CPU_ON`.
    Psci,
    /// The board stays single-hart; the reason is logged at boot.
    None(&'static str),
}

/// The board's interrupt controller, as the trap path and SMP use it.
pub trait ArmIrqController {
    /// Whether the controller can raise the kernel's cross-hart IPI.
    const HAS_IPI: bool;

    /// Legacy IRQ of a USB host controller run under the one-shot protocol:
    /// the ISR masks it and the driver's next `WaitIrq` unmasks it.
    const USB_ONE_SHOT_IRQ: Option<u32>;

    /// Bring up the controller on the boot core. Must precede the timer.
    fn init();

    /// Bring up this core's banked controller state on a secondary core.
    fn init_cpu();

    /// Raise the cross-hart IPI on `target_cpu`. Only called when
    /// [`HAS_IPI`](Self::HAS_IPI) is `true`.
    fn send_ipi(target_cpu: u32);

    /// Re-enable [`USB_ONE_SHOT_IRQ`](Self::USB_ONE_SHOT_IRQ). Only called
    /// when it is `Some`.
    fn unmask_usb_irq();

    /// Handle one IRQ exception: identify the source, acknowledge it and
    /// dispatch it to the kernel hooks.
    fn handle_irq();
}

/// The board's host tick source.
pub trait ArmTimer {
    /// Arm the tick. Runs after [`ArmIrqController::init`] on every core.
    fn init();

    /// Re-arm the tick for the next quantum. Called from the IRQ path.
    fn reset();
}

/// The board's platform map: the facts and windows the kernel publishes,
/// identity-maps and grants. All values come from the `hal/soc` profiles.
pub trait ArmResources {
    /// SoC identity. The kernel selects its MMIO authority table by it; the
    /// tables and their device-class bits stay kernel-side.
    const SOC: ArmSoc;

    /// Facts `platform::init` publishes.
    const PLATFORM: PlatformFacts;

    /// Kernel identity mappings of device MMIO, in mapping order.
    const KERNEL_MMIO_MAPS: &'static [MmioMap];

    /// VirtIO MMIO transport window, when the board has one.
    const VIRTIO_MMIO: Option<hal_soc_arm_virt::VirtioMmioLayout>;

    /// GPIO controller window whose owner receives GPIO IRQ notifications.
    const GPIO_MMIO_BASE: usize;

    /// DWC2 USB host aperture, when the board has one.
    const DWC2_WINDOW: Option<MmioWindow>;

    /// Firmware framebuffer path, when the board has one.
    const DISPLAY: Option<DisplayFirmware>;

    /// SDHCI controller the kernel probes, when the board has one.
    const SDHCI: Option<SdhciFacts>;

    /// BCM GPIO block whose pins the kernel routes from the board wiring at
    /// driver init, when the board needs it.
    const BOARD_PINMUX_GPIO_BASE: Option<usize>;
}

/// SoC identity of a board backend.
pub enum ArmSoc {
    /// QEMU `virt` machine.
    QemuArmVirt,
    /// BCM2837 (Raspberry Pi 3).
    Bcm2837,
    /// BCM2711 (Raspberry Pi 4).
    Bcm2711,
}

/// Platform facts published at boot.
pub struct PlatformFacts {
    /// Console UART register base.
    pub uart_base: usize,
    /// Console UART interrupt (0 when none is wired).
    pub uart_irq: u32,
    /// PL031 RTC base, when the board has one.
    pub rtc_base: Option<usize>,
    /// How many leading [`ArmResources::VIRTIO_MMIO`] slots are published.
    pub published_virtio_slots: usize,
    /// Boot log line naming the board, when it has one.
    pub announce: Option<PlatformAnnounce>,
}

/// The `[platform]` boot log line of a board.
pub struct PlatformAnnounce {
    /// Board label.
    pub label: &'static str,
    /// Peripheral aperture base.
    pub peripheral_base: usize,
}

/// One kernel identity mapping of device MMIO, `[base, end)`.
pub struct MmioMap {
    pub base: usize,
    pub end: usize,
    /// `true`: Cells may access the window (USER); `false`: kernel-only.
    pub cell_access: bool,
}

/// An MMIO window, `[base, base + size)`.
pub struct MmioWindow {
    pub base: usize,
    pub size: usize,
}

/// Firmware framebuffer path.
pub struct DisplayFirmware {
    /// Mailbox window the registering Cell must own exactly.
    pub mailbox: MmioWindow,
    /// A framebuffer must end at or below this address.
    pub framebuffer_limit: usize,
}

/// SDHCI controller facts.
pub struct SdhciFacts {
    pub base: usize,
    pub word_access_only: bool,
    pub minimum_write_spacing_us: u32,
    /// BCM GPIO block whose SD pins `mmc` routes before probing, when needed.
    pub pinmux_gpio_base: Option<usize>,
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
