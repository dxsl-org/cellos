//! Platform peripheral discovery via DTB.
//!
//! Call `platform::init(dtb_ptr)` at `kmain` before any driver init. All drivers
//! read MMIO addresses via `platform::with(|p| p.xxx)`. Falls back to QEMU virt
//! defaults when the DTB is absent or a compatible node is not found.
//!
//! Call ordering invariant: `init` → `uart::init` → `hal::ARCH.init` (which calls
//! `plic::init` internally). Any call to `with` before `init` panics.

mod boot_once;

use boot_once::BootOnce;
#[cfg(target_arch = "riscv64")]
use hal_soc_riscv::{RiscvSocProfile, RtcAccessPolicy, UartAccessPolicy, VirtioMmioPolicy};

// ── Public types ───────────────────────────────────────────────────────────────

/// A VirtIO MMIO device found in the DTB.
#[derive(Clone, Copy)]
pub struct VirtioEntry {
    pub base: usize,
    pub irq: u32,
}

/// Platform peripheral layout populated once from the DTB at early boot.
#[derive(Clone)]
pub struct PlatformInfo {
    pub uart_base: usize,
    pub uart_irq: u32,
    pub plic_base: usize,
    /// Mapped region size for identity-map range covering all PLIC registers.
    pub plic_size: usize,
    pub clint_base: usize,
    /// VirtIO MMIO slots from DTB (up to 8). `None` = slot unused.
    pub virtio_mmio: [Option<VirtioEntry>; 8],
    /// Goldfish RTC MMIO base (0 = not found in DTB, using default).
    pub rtc_base: usize,
}

impl PlatformInfo {
    /// Only called from `from_dtb`, which is riscv64-only (the DTB parser section
    /// below). Gated to avoid a dead-code warning on aarch64/x86_64.
    #[cfg(target_arch = "riscv64")]
    fn qemu_defaults() -> Self {
        let board = crate::board::selected();
        Self::from_board(board)
    }

    #[cfg(target_arch = "riscv64")]
    fn from_board(board: &cellos_boards::BoardDescriptor) -> Self {
        Self {
            uart_base: board.uart.base as usize,
            uart_irq: board.uart.irq.unwrap_or(0),
            plic_base: board.plic.base as usize,
            plic_size: board.plic.size as usize,
            clint_base: board.clint.base as usize,
            virtio_mmio: virtio_slots(board),
            rtc_base: board.rtc.base as usize,
        }
    }
}

// ── Storage ────────────────────────────────────────────────────────────────────

static PLATFORM: BootOnce<PlatformInfo> = BootOnce::new();

/// Publish immutable platform data while the boot CPU is the only running core.
///
/// Spinlocks are forbidden on this path: ARM64 reaches it before the MMU has
/// assigned Normal memory attributes, so LL/SC may abort on real hardware.
unsafe fn publish_boot(info: PlatformInfo) {
    // SAFETY: every architecture calls `platform::init` once from `kmain`
    // before interrupts, paging, or secondary-core startup.
    unsafe { PLATFORM.initialize(info) };
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Parse the DTB and store platform info. Must be called exactly once by the
/// boot CPU before `uart::init`, interrupts, SMP, and `init_kernel_paging`.
/// Safe with `dtb_ptr == 0`.
#[cfg(target_arch = "riscv64")]
pub fn init(sbi_dtb: usize) {
    let profile = active_riscv_soc_profile();
    let info = apply_riscv_soc_access_policy(from_dtb(sbi_dtb, profile), profile);

    log::info!(
        "[platform] UART={:#x} irq={} PLIC={:#x}+{:#x} CLINT={:#x} RTC={:#x}",
        info.uart_base,
        info.uart_irq,
        info.plic_base,
        info.plic_size,
        info.clint_base,
        info.rtc_base
    );
    hal::common::rtc::init(info.rtc_base);
    // SAFETY: `kmain` invokes this once during single-core boot.
    unsafe { publish_boot(info) };
}

// ── Raspberry Pi 3 defaults (BCM2837, aarch64, board-rpi3) ───────────────────
// BCM2837: peripherals at 0x3F000000, no GIC, no VirtIO, no Goldfish RTC.
// Mini UART IO register at 0x3F215040 (AUX_MU_IO).
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
pub fn init(_dtb_ptr: usize) {
    // RPi 3 has no Goldfish/PL031 RTC — uptime only via ARM counter, epoch=0.
    let info = PlatformInfo {
        uart_base: 0x3F21_5040, // BCM mini UART IO register (AUX_MU_IO)
        uart_irq: 0,            // mini UART IRQ not used (polled I/O)
        plic_base: 0,
        plic_size: 0,
        clint_base: 0,
        virtio_mmio: [None; 8], // No VirtIO on RPi 3 — real hardware Driver Cells
        rtc_base: 0,            // No Goldfish RTC; epoch unknown without external RTC
    };
    // SAFETY: `kmain` invokes this once during single-core boot.
    unsafe { publish_boot(info) };
    log::info!("[platform] RPi 3 BCM2837: UART=0x3F215040 periph=0x3F000000 RAM=960MiB");
}

// ── QEMU ARM virt defaults (aarch64) ─────────────────────────────────────────
// QEMU ARM virt: 32 VirtIO MMIO slots at 0x0a000000, 512 bytes each, SPI 16+i.
// Goldfish RTC at 0x0902_0000 on ARM virt; UART (PL011) at 0x0900_0000.
#[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
pub fn init(_dtb_ptr: usize) {
    hal::rtc::init_default();
    let info = PlatformInfo {
        uart_base: 0x0900_0000,
        uart_irq: 1,
        plic_base: 0,
        plic_size: 0,
        clint_base: 0,
        virtio_mmio: [
            Some(VirtioEntry {
                base: 0x0a00_0000,
                irq: 16,
            }),
            Some(VirtioEntry {
                base: 0x0a00_0200,
                irq: 17,
            }),
            Some(VirtioEntry {
                base: 0x0a00_0400,
                irq: 18,
            }),
            Some(VirtioEntry {
                base: 0x0a00_0600,
                irq: 19,
            }),
            None,
            None,
            None,
            None,
        ],
        rtc_base: 0x0902_0000,
    };
    // SAFETY: `kmain` invokes this once during single-core boot.
    unsafe { publish_boot(info) };
}

#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
pub fn init(_dtb_ptr: usize) {}

/// Borrow the platform info. Panics if `init` was not called.
pub fn with<R>(f: impl FnOnce(&PlatformInfo) -> R) -> R {
    f(PLATFORM
        .get()
        .expect("[platform] platform::init not called before platform::with"))
}

// ── DTB parser (riscv64 only) ──────────────────────────────────────────────────

#[cfg(target_arch = "riscv64")]
fn active_riscv_soc_profile() -> &'static RiscvSocProfile {
    #[cfg(feature = "board-pioneer")]
    {
        &hal_soc_riscv::SG2042
    }
    #[cfg(all(not(feature = "board-pioneer"), feature = "board-vf2"))]
    {
        &hal_soc_riscv::JH7110
    }
    #[cfg(all(not(feature = "board-pioneer"), not(feature = "board-vf2")))]
    {
        &hal_soc_riscv::GENERIC_VIRT
    }
}

#[cfg(target_arch = "riscv64")]
fn apply_riscv_soc_access_policy(
    mut info: PlatformInfo,
    profile: &'static RiscvSocProfile,
) -> PlatformInfo {
    match profile.uart_access {
        UartAccessPolicy::Mmio => {}
        UartAccessPolicy::SbiDbcnOnly => {
            info.uart_base = 0;
            info.uart_irq = 0;
        }
    }

    match profile.rtc_access {
        RtcAccessPolicy::Mmio => {}
        RtcAccessPolicy::Unavailable => {
            info.rtc_base = 0;
        }
    }

    match profile.virtio_mmio {
        VirtioMmioPolicy::Discover => {}
        VirtioMmioPolicy::Absent => {
            info.virtio_mmio = [None; 8];
        }
    }

    info
}

#[cfg(target_arch = "riscv64")]
fn virtio_mmio_entries_for_profile(
    fdt: &fdt::Fdt,
    profile: &'static RiscvSocProfile,
) -> [Option<VirtioEntry>; 8] {
    match profile.virtio_mmio {
        VirtioMmioPolicy::Discover => collect_virtio(fdt),
        VirtioMmioPolicy::Absent => [None; 8],
    }
}

#[cfg(target_arch = "riscv64")]
fn from_dtb(dtb_ptr: usize, profile: &'static RiscvSocProfile) -> PlatformInfo {
    if dtb_ptr == 0 {
        log::warn!("[platform] dtb_ptr=0, using QEMU defaults");
        return PlatformInfo::qemu_defaults();
    }
    // SAFETY: dtb_ptr is the FDT physical address passed by OpenSBI (a1) or
    // retrieved from a Limine DtbResponse. fdt::Fdt::from_ptr validates FDT
    // magic before any further parsing.
    let fdt = match unsafe { fdt::Fdt::from_ptr(dtb_ptr as *const u8) } {
        Ok(f) => f,
        Err(e) => {
            log::warn!("[platform] DTB parse error ({:?}), using QEMU defaults", e);
            return PlatformInfo::qemu_defaults();
        }
    };
    let defaults = crate::board::selected();

    let uart_base = reg_base(&fdt, profile.uart_compatibles).unwrap_or_else(|| {
        log::warn!("[platform] UART not in DTB");
        defaults.uart.base as usize
    });
    let uart_irq =
        irq_first(&fdt, profile.uart_compatibles).unwrap_or(defaults.uart.irq.unwrap_or(0));

    let (plic_base, plic_size) =
        reg_base_size(&fdt, profile.plic_compatibles).unwrap_or_else(|| {
            log::warn!("[platform] PLIC not in DTB");
            (defaults.plic.base as usize, defaults.plic.size as usize)
        });

    let clint_base = reg_base(&fdt, profile.clint_compatibles).unwrap_or_else(|| {
        log::warn!("[platform] CLINT not in DTB");
        defaults.clint.base as usize
    });

    let virtio_mmio = virtio_mmio_entries_for_profile(&fdt, profile);

    let rtc_base = reg_base(&fdt, profile.rtc_compatibles).unwrap_or_else(|| {
        log::warn!("[platform] Goldfish RTC not in DTB, using default");
        defaults.rtc.base as usize
    });

    PlatformInfo {
        uart_base,
        uart_irq,
        plic_base,
        plic_size,
        clint_base,
        virtio_mmio,
        rtc_base,
    }
}

#[cfg(target_arch = "riscv64")]
fn reg_base(fdt: &fdt::Fdt, compat: &[&str]) -> Option<usize> {
    reg_base_size(fdt, compat).map(|(b, _)| b)
}

#[cfg(target_arch = "riscv64")]
fn reg_base_size(fdt: &fdt::Fdt, compat: &[&str]) -> Option<(usize, usize)> {
    let node = fdt.find_compatible(compat)?;
    let r = node.reg()?.next()?;
    Some((r.starting_address as usize, r.size.unwrap_or(0x1000)))
}

/// Read the first cell of the `interrupts` property as a big-endian u32.
#[cfg(target_arch = "riscv64")]
fn irq_first(fdt: &fdt::Fdt, compat: &[&str]) -> Option<u32> {
    let node = fdt.find_compatible(compat)?;
    let b = node.property("interrupts")?.value;
    if b.len() >= 4 {
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    } else {
        None
    }
}

/// Collect all `virtio,mmio` nodes (up to 8) in DTB traversal order.
#[cfg(target_arch = "riscv64")]
fn collect_virtio(fdt: &fdt::Fdt) -> [Option<VirtioEntry>; 8] {
    let mut entries = [None, None, None, None, None, None, None, None];
    let mut n = 0;
    for node in fdt.all_nodes() {
        if n >= 8 {
            break;
        }
        let is_v = node
            .compatible()
            .is_some_and(|c| c.all().any(|s| s == "virtio,mmio"));
        if !is_v {
            continue;
        }
        let base = node
            .reg()
            .and_then(|mut r| r.next())
            .map(|r| r.starting_address as usize);
        let irq = node.property("interrupts").and_then(|p| {
            let b = p.value;
            if b.len() >= 4 {
                Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            } else {
                None
            }
        });
        if let (Some(base), Some(irq)) = (base, irq) {
            entries[n] = Some(VirtioEntry { base, irq });
            n += 1;
        }
    }
    entries
}

#[cfg(target_arch = "riscv64")]
fn virtio_slots(board: &cellos_boards::BoardDescriptor) -> [Option<VirtioEntry>; 8] {
    let mut entries = [None, None, None, None, None, None, None, None];
    let mut index = 0;
    while index < board.virtio_mmio.len() && index < entries.len() {
        let region = board.virtio_mmio[index];
        entries[index] = Some(VirtioEntry {
            base: region.base as usize,
            irq: region.irq.unwrap_or(0),
        });
        index += 1;
    }
    entries
}
