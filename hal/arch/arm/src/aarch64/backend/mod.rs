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

/// The Raspberry Pi non-VHE EL2 monitor. Both BCM boards install its vectors
/// and stack at boot (`boot.rs` `.el2_init`); only the BCM2837 backend uses it.
#[cfg(any(feature = "board-rpi3", feature = "board-rpi4"))]
pub mod monitor;

/// The backend selected for this build.
#[cfg(all(feature = "board-rpi4", not(feature = "board-rpi3")))]
pub type Board = bcm2711::Bcm2711;
/// The backend selected for this build.
#[cfg(feature = "board-rpi3")]
pub type Board = bcm2837::Bcm2837;
/// The backend selected for this build.
#[cfg(not(any(feature = "board-rpi3", feature = "board-rpi4")))]
pub type Board = qemu_virt::QemuVirt;

/// The selected board's EL2 monitor ([`NoEl2Monitor`] unless the host stays
/// at EL1).
pub type El2Monitor = <Board as ArmPlatform>::El2Monitor;

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

    /// Boot installs the Pi EL2 monitor's vectors and stack, then drops the
    /// host to EL1 (`boot.rs` `.el2_init`), instead of keeping an EL2 host.
    const PI_MONITOR_BOOT: bool;

    /// How the kernel treats the firmware device tree's memory description.
    const DTB_POLICY: DtbPolicy;

    /// Idle-loop progress probes emitted once IRQs are enabled (0 = none).
    const IDLE_PROBES: usize;

    /// Per-vCPU state the board's world switch saves beyond the shared ABI.
    type VcpuExt: ArmVcpuExt;

    /// The EL2 monitor of a host that itself stays at EL1.
    type El2Monitor: ArmEl2Monitor;

    /// The regime the host kernel runs in when firmware enters at EL2.
    /// Firmware that enters at EL1 always yields an EL1 host, whatever this
    /// says. The Pi monitor boards run an EL1 host, everything else a VHE EL2
    /// host that falls back to EL1 on a CPU without FEAT_VHE.
    const HOST_REGIME: HostRegime = if Self::PI_MONITOR_BOOT {
        HostRegime::El1
    } else {
        HostRegime::El2Vhe
    };

    /// `_start` carries a Linux arm64 `Image` header (Linux
    /// `Documentation/arch/arm64/booting.rst`), so an `Image` loader can boot
    /// the raw image with the FDT address in `x0`. Off by default: the Pi
    /// boards keep their entry byte for byte.
    const LINUX_IMAGE_HEADER: bool = false;

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

    /// Emit the board's one-shot IRQ-state probe right after IRQs are first
    /// enabled. No-op on boards without one.
    fn irq_snapshot_probe();

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

/// The host kernel's exception level and EL2 translation regime.
///
/// There is no non-VHE EL2 host: under `HCR_EL2.{E2H,TGE} = {0,1}` the
/// EL1&0 regime treats `SCTLR_EL1.M` as 0 for EL0 (ARM ARM
/// `AArch64.S1Enabled`), so EL0 Cells would run untranslated, VA = PA, with
/// no AP/UXN checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostRegime {
    /// EL2 host with `HCR_EL2.{E2H,TGE} = {1,1}` (FEAT_VHE). The kernel and EL0
    /// Cells share the EL2&0 regime (`TTBR0_EL2`, `TCR_EL2` in the EL1
    /// layout); a guest's EL1 bank is reached through the `_EL12`/`_EL02`
    /// encodings. A CPU without FEAT_VHE ignores the `E2H` write; the boot
    /// path then drops the host to EL1 instead (`boot.rs` `.el2_init`).
    El2Vhe,
    /// EL1 host; EL2, when present, is a private monitor.
    El1,
}

/// `HCR_EL2.E2H`.
pub const HCR_EL2_E2H: u64 = 1 << 34;
/// `HCR_EL2.TGE`.
pub const HCR_EL2_TGE: u64 = 1 << 27;

/// Whether an EL0 Cell may run under `hcr`: `{E2H,TGE} = {0,1}` disables EL0
/// stage-1 translation, every other combination keeps it.
pub const fn hcr_translates_el0(hcr: u64) -> bool {
    hcr & HCR_EL2_TGE == 0 || hcr & HCR_EL2_E2H != 0
}

// Checked by the compiler on every aarch64 build: the shared core is never
// host-compiled, so these stand in for host tests. The last line pins the
// regime to the boot path: only the Pi monitor boot leaves the host at EL1,
// every other EL2 entry sets `E2H` alongside `TGE`.
const _: () = {
    assert!(hcr_translates_el0(1 << 31 | HCR_EL2_TGE | HCR_EL2_E2H));
    assert!(!hcr_translates_el0(1 << 31 | HCR_EL2_TGE));
    assert!(hcr_translates_el0(1 << 31));
    assert!(
        matches!(<Board as ArmPlatform>::HOST_REGIME, HostRegime::El1)
            == <Board as ArmPlatform>::PI_MONITOR_BOOT
    );
};

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

    /// GICv2 virtualization windows the EL2 host's vGIC drives, when the board
    /// has them and its host stays at EL2. `None`: no GICH list-register
    /// injection and no GICV passthrough (the Pi 3 monitor injects through
    /// `HCR_EL2.VI`; the Pi 4 has no guest vGIC yet).
    const VGIC: Option<VgicWindows>;

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

    /// Device MMIO the native-domain supervisor registers as shared-kernel
    /// ranges, in registration order.
    const SUPERVISOR_DEVICE_MMIO: &'static [MmioSpan];
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

/// GICv2 virtualization extension windows (GIC-400 GICH and GICV).
pub struct VgicWindows {
    /// Virtual interface control: list registers, VMCR, APR.
    pub gich_base: usize,
    /// Virtual CPU interface, passed through to the guest's GICC.
    pub gicv: MmioWindow,
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

/// An MMIO span, `[base, end)`.
pub struct MmioSpan {
    pub base: usize,
    pub end: usize,
}

/// How the kernel treats the firmware device tree's memory description.
pub enum DtbPolicy {
    /// The device tree is optional: RAM comes from the board's fallback span,
    /// bounded by the device tree's first memory region when one is present,
    /// with the kernel region sized from the linker end symbol.
    Optional,
    /// The device tree's memory map is authoritative when valid; otherwise a
    /// conservative map from the board descriptor, with the kernel region sized
    /// from the linker end symbol.
    Conservative,
    /// A valid device tree is required; the memory map is the board's static
    /// fallback.
    Required,
}

/// Board-specific vCPU state appended to [`AArch64Vcpu`](crate::aarch64::vcpu::AArch64Vcpu)
/// after every shared assembly offset, and the world-switch steps that save it.
pub trait ArmVcpuExt: Default + core::fmt::Debug {
    /// Host EL1 state saved around each `run_vcpu_impl` guest run.
    type HostBank: Default;

    /// The world-switch assembly saves and restores the eager FP bank and
    /// honours `PI_GUEST_VI` (the `.if {pi}` paths in `vcpu.rs`).
    const PI_WORLD_SWITCH: bool;

    /// Highest `VcpuRegs` mode; 2 adds the guest virtual-timer pair.
    const REGS_MAX_MODE: usize;

    /// `size_of::<AArch64Vcpu>()` for this build — the per-build ABI pin.
    const VCPU_SIZE: usize;

    /// Offset of this extension inside `AArch64Vcpu`.
    const VCPU_EXT_OFFSET: usize;

    /// Snapshot the extension's host sysregs before guest entry.
    fn save_host(bank: &mut Self::HostBank);

    /// Load the extension's guest sysregs before guest entry.
    fn restore_guest(&self);

    /// Save the extension's guest sysregs after guest exit.
    fn save_guest(&mut self);

    /// Restore the extension's host sysregs after guest exit.
    fn restore_host(bank: &Self::HostBank);
}

/// No board-specific vCPU state. A zero-length array keeps the empty
/// extension FFI-safe inside the `#[repr(C)]` vCPU (it adds no bytes).
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct NoVcpuExt([u64; 0]);

impl ArmVcpuExt for NoVcpuExt {
    type HostBank = ();

    const PI_WORLD_SWITCH: bool = false;
    const REGS_MAX_MODE: usize = 1;
    /// Shared ABI only: `g_sp_el0` at 528 is the last field.
    const VCPU_SIZE: usize = 536;
    const VCPU_EXT_OFFSET: usize = 536;

    #[inline(always)]
    fn save_host(_bank: &mut ()) {}

    #[inline(always)]
    fn restore_guest(&self) {}

    #[inline(always)]
    fn save_guest(&mut self) {}

    #[inline(always)]
    fn restore_host(_bank: &()) {}
}

/// The EL2 monitor of a host that itself stays at EL1 (non-VHE). Boards whose
/// host runs at EL2 use [`NoEl2Monitor`]; its methods are never reached because
/// every caller first checks [`PRESENT`](Self::PRESENT).
pub trait ArmEl2Monitor {
    /// Whether the host reaches EL2 through this monitor.
    const PRESENT: bool;

    /// Hand the monitor the identity root once EL1 paging is live.
    fn init(root_pa: u64);

    /// The monitor accepted its boot handshake.
    fn is_ready() -> bool;

    /// The boot-time guest smoke passed.
    fn is_verified() -> bool;

    /// Record a passed boot-time guest smoke.
    fn mark_verified();

    /// `CurrentEL` observed at `_start`.
    fn entry_el() -> u8;

    /// Boot provenance, raw handshake result, and op0 dispatch witness.
    fn probe_status() -> (bool, u64, bool);

    /// Private HVC gateway: 1 bind/flush VMID, 2 unbind/flush, 3 flush all,
    /// 4 flush IPA, 5 run vCPU.
    fn gateway(op: u64, a: u64, b: u64, c: u64, d: u64) -> u64;

    /// Run `vcpu` on Stage-2 root `root_pa` until it exits.
    ///
    /// # Safety
    /// See the monitor's `run`.
    unsafe fn run(vcpu: &mut crate::aarch64::vcpu::AArch64Vcpu, vmid: u16, root_pa: u64, vi: bool);

    /// Run one guest interval with an EL2 visibility probe.
    ///
    /// # Safety
    /// See the monitor's `run_probed`.
    unsafe fn run_probed(
        vcpu: &mut crate::aarch64::vcpu::AArch64Vcpu,
        vmid: u16,
        root_pa: u64,
        vi: bool,
        guest_ipa: u64,
        guest_pa: u64,
    ) -> GuestRunProbe;

    /// Bound the host tick for the boot smoke's WFI check.
    fn prepare_smoke_window();

    /// Arm the host tick ~1 ms out for the smoke's preemption step.
    fn arm_smoke_preemption();

    /// Restore the ordinary host tick cadence after the smoke.
    fn restore_host_tick();
}

/// EL2's Normal-WB view of one explicitly probed guest run. Values return in
/// HVC registers independently of the EL1 vCPU buffer; a broken writeback
/// path therefore cannot conceal itself behind that buffer's exit sentinels.
#[derive(Clone, Copy, Debug)]
pub struct GuestRunProbe {
    pub entry_pc: u64,
    pub stage2_l1: u64,
    pub stage2_l2: u64,
    pub stage2_l3: u64,
    pub instruction: u64,
    pub exit_esr: u64,
    pub exit_elr: u64,
    pub exit_is_irq: u64,
}

/// No EL2 monitor: the host runs at EL2 itself, or never uses EL2.
pub struct NoEl2Monitor;

impl ArmEl2Monitor for NoEl2Monitor {
    const PRESENT: bool = false;

    #[inline(always)]
    fn init(_root_pa: u64) {}

    #[inline(always)]
    fn is_ready() -> bool {
        false
    }

    #[inline(always)]
    fn is_verified() -> bool {
        false
    }

    #[inline(always)]
    fn mark_verified() {}

    fn entry_el() -> u8 {
        unreachable!("no EL2 monitor")
    }

    fn probe_status() -> (bool, u64, bool) {
        unreachable!("no EL2 monitor")
    }

    fn gateway(_op: u64, _a: u64, _b: u64, _c: u64, _d: u64) -> u64 {
        unreachable!("no EL2 monitor")
    }

    unsafe fn run(
        _vcpu: &mut crate::aarch64::vcpu::AArch64Vcpu,
        _vmid: u16,
        _root_pa: u64,
        _vi: bool,
    ) {
        unreachable!("no EL2 monitor")
    }

    unsafe fn run_probed(
        _vcpu: &mut crate::aarch64::vcpu::AArch64Vcpu,
        _vmid: u16,
        _root_pa: u64,
        _vi: bool,
        _guest_ipa: u64,
        _guest_pa: u64,
    ) -> GuestRunProbe {
        unreachable!("no EL2 monitor")
    }

    fn prepare_smoke_window() {
        unreachable!("no EL2 monitor")
    }

    fn arm_smoke_preemption() {
        unreachable!("no EL2 monitor")
    }

    fn restore_host_tick() {
        unreachable!("no EL2 monitor")
    }
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
