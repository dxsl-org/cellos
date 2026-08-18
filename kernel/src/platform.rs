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

#[cfg(target_arch = "riscv64")]
pub const RISCV_PLIC_IRQ_CAPACITY: usize = 9;

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
    fn board_defaults(profile: &'static RiscvSocProfile) -> Self {
        let board = crate::board::selected();
        Self::from_board(board, profile)
    }

    #[cfg(target_arch = "riscv64")]
    fn from_board(
        board: &cellos_boards::BoardDescriptor,
        profile: &'static RiscvSocProfile,
    ) -> Self {
        let plic = required_riscv_mmio(board.plic, "PLIC");
        let clint = required_riscv_mmio(board.clint, "CLINT");
        Self {
            uart_base: board.uart.base as usize,
            uart_irq: board.uart.irq.unwrap_or(0),
            plic_base: plic.base as usize,
            plic_size: plic.size as usize,
            clint_base: clint.base as usize,
            virtio_mmio: virtio_slots(board),
            rtc_base: rtc_fallback_base(board, profile),
        }
    }

    #[cfg(target_arch = "riscv64")]
    fn riscv_plic_irqs(&self) -> ([u32; RISCV_PLIC_IRQ_CAPACITY], usize) {
        let mut irqs = [0; RISCV_PLIC_IRQ_CAPACITY];
        let mut len = 0;

        if self.riscv_irq_owner_count(self.uart_irq) == 1 {
            push_irq(&mut irqs, &mut len, self.uart_irq);
        }
        let mut index = 0;
        while index < self.virtio_mmio.len() {
            if let Some(entry) = self.virtio_mmio[index] {
                if self.riscv_irq_owner_count(entry.irq) == 1 {
                    push_irq(&mut irqs, &mut len, entry.irq);
                }
            }
            index += 1;
        }

        (irqs, len)
    }

    #[cfg(target_arch = "riscv64")]
    fn virtio_mmio_base_for_irq(&self, irq: u32) -> Option<usize> {
        let mut found = None;
        let mut index = 0;
        while index < self.virtio_mmio.len() {
            if let Some(entry) = self.virtio_mmio[index] {
                if entry.irq == irq {
                    if found.is_some() {
                        return None;
                    }
                    found = Some(entry.base);
                }
            }
            index += 1;
        }
        found
    }

    #[cfg(target_arch = "riscv64")]
    pub(crate) fn riscv_irq_owner_count(&self, irq: u32) -> usize {
        if irq == 0 {
            return 0;
        }

        let mut owners = usize::from(self.uart_irq == irq);
        let mut index = 0;
        while index < self.virtio_mmio.len() {
            if self.virtio_mmio[index].is_some_and(|entry| entry.irq == irq) {
                owners += 1;
            }
            index += 1;
        }
        owners
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
    let profile = crate::board::selected_riscv64_soc();
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
    let board = crate::board::selected_rpi3();
    let soc = hal_soc_bcm27xx::BCM2837;
    // RPi 3 has no Goldfish/PL031 RTC — uptime only via ARM counter, epoch=0.
    let info = PlatformInfo {
        uart_base: board.uart.base as usize,
        uart_irq: board.uart.irq.unwrap_or(0),
        plic_base: board.plic.map_or(0, |region| region.base as usize),
        plic_size: board.plic.map_or(0, |region| region.size as usize),
        clint_base: board.clint.map_or(0, |region| region.base as usize),
        virtio_mmio: virtio_slots(board),
        rtc_base: board.rtc.map_or(0, |region| region.base as usize),
    };
    // SAFETY: `kmain` invokes this once during single-core boot.
    unsafe { publish_boot(info) };
    log::info!(
        "[platform] RPi 3 BCM2837: UART={:#x} periph={:#x} fallback-end={:#x}",
        board.uart.base,
        soc.mmio.peripheral_base,
        board.fallback_memory[1].base + board.fallback_memory[1].size
    );
}

// ── QEMU ARM virt defaults (aarch64) ─────────────────────────────────────────
// QEMU ARM virt: 32 VirtIO MMIO slots at 0x0a000000, 512 bytes each, SPI 16+i.
// Goldfish RTC at 0x0902_0000 on ARM virt; UART (PL011) at 0x0900_0000.
#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
pub fn init(_dtb_ptr: usize) {
    let board = crate::board::selected_qemu_arm_virt();
    hal::rtc::init_default();
    let info = PlatformInfo {
        uart_base: board.uart.base as usize,
        uart_irq: board
            .uart
            .irq
            .expect("validated QEMU ARM descriptor requires UART IRQ"),
        plic_base: 0,
        plic_size: 0,
        clint_base: 0,
        virtio_mmio: virtio_slots_limited(board, 4),
        rtc_base: board.rtc.map_or(0, |rtc| rtc.base as usize),
    };
    // SAFETY: `kmain` invokes this once during single-core boot.
    unsafe { publish_boot(info) };
}

#[cfg(all(target_arch = "aarch64", feature = "board-rpi4"))]
pub fn init(_dtb_ptr: usize) {
    let board = crate::board::selected_rpi4();
    let info = PlatformInfo {
        uart_base: board.uart.base as usize,
        uart_irq: board.uart.irq.unwrap_or(0),
        plic_base: 0,
        plic_size: 0,
        clint_base: 0,
        virtio_mmio: [None; 8],
        rtc_base: 0,
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

#[cfg(target_arch = "riscv64")]
pub fn riscv_plic_init_data() -> Option<(usize, [u32; RISCV_PLIC_IRQ_CAPACITY], usize)> {
    let context = riscv_plic_context_for_current_hart()?;
    let (irqs, len) = with(|platform| platform.riscv_plic_irqs());
    Some((context, irqs, len))
}

#[cfg(target_arch = "riscv64")]
pub fn riscv_plic_context_for_current_hart() -> Option<usize> {
    let logical_hart = crate::task::hart_local::current_hart_id();
    let physical_hart = crate::task::smp::logical_to_physical(logical_hart)?;
    crate::board::selected_riscv64_soc().plic_context_for_physical_hart(physical_hart)
}

#[cfg(target_arch = "riscv64")]
pub fn virtio_mmio_base_for_irq(irq: u32) -> Option<usize> {
    with(|platform| platform.virtio_mmio_base_for_irq(irq))
}

// ── DTB parser (riscv64 only) ──────────────────────────────────────────────────

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
    let board = crate::board::selected();
    if dtb_ptr == 0 {
        if board.boot.requires_firmware_dtb {
            panic!("[platform] selected board requires a firmware DTB");
        }
        log::warn!("[platform] dtb_ptr=0, using board defaults");
        return PlatformInfo::board_defaults(profile);
    }
    // SAFETY: dtb_ptr is the FDT physical address passed by OpenSBI (a1) or
    // retrieved from a Limine DtbResponse. fdt::Fdt::from_ptr validates FDT
    // magic before any further parsing.
    let fdt = match unsafe { fdt::Fdt::from_ptr(dtb_ptr as *const u8) } {
        Ok(f) => f,
        Err(e) => {
            if board.boot.requires_firmware_dtb {
                panic!("[platform] required firmware DTB is invalid: {:?}", e);
            }
            log::warn!("[platform] DTB parse error ({:?}), using board defaults", e);
            return PlatformInfo::board_defaults(profile);
        }
    };
    let defaults = board;

    let uart_base = reg_base(&fdt, profile.uart_compatibles).unwrap_or_else(|| {
        log::warn!("[platform] UART not in DTB");
        defaults.uart.base as usize
    });
    let uart_irq =
        irq_first(&fdt, profile.uart_compatibles).unwrap_or(defaults.uart.irq.unwrap_or(0));

    let (plic_base, plic_size) =
        reg_base_size(&fdt, profile.plic_compatibles).unwrap_or_else(|| {
            log::warn!("[platform] PLIC not in DTB");
            let plic = required_riscv_mmio(defaults.plic, "PLIC");
            (plic.base as usize, plic.size as usize)
        });

    let clint_base = reg_base(&fdt, profile.clint_compatibles).unwrap_or_else(|| {
        log::warn!("[platform] CLINT not in DTB");
        required_riscv_mmio(defaults.clint, "CLINT").base as usize
    });

    let virtio_mmio = virtio_mmio_entries_for_profile(&fdt, profile);

    let rtc_base = reg_base(&fdt, profile.rtc_compatibles)
        .unwrap_or_else(|| rtc_fallback_base(defaults, profile));

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
fn rtc_fallback_base(
    board: &cellos_boards::BoardDescriptor,
    profile: &'static RiscvSocProfile,
) -> usize {
    match profile.rtc_access {
        RtcAccessPolicy::Mmio => required_riscv_mmio(board.rtc, "RTC").base as usize,
        RtcAccessPolicy::Unavailable => 0,
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

#[cfg(any(
    target_arch = "riscv64",
    all(target_arch = "aarch64", feature = "board-rpi3")
))]
fn virtio_slots(board: &cellos_boards::BoardDescriptor) -> [Option<VirtioEntry>; 8] {
    virtio_slots_limited(board, 8)
}

#[cfg(any(
    target_arch = "riscv64",
    all(target_arch = "aarch64", feature = "board-rpi3")
))]
fn virtio_slots_limited(
    board: &cellos_boards::BoardDescriptor,
    limit: usize,
) -> [Option<VirtioEntry>; 8] {
    let mut entries = [None, None, None, None, None, None, None, None];
    let mut index = 0;
    while index < board.virtio_mmio.len() && index < entries.len() && index < limit {
        let region = board.virtio_mmio[index];
        entries[index] = Some(VirtioEntry {
            base: region.base as usize,
            irq: region.irq.unwrap_or(0),
        });
        index += 1;
    }
    entries
}

#[cfg(target_arch = "riscv64")]
fn required_riscv_mmio(
    region: Option<cellos_boards::MmioRegion>,
    name: &'static str,
) -> cellos_boards::MmioRegion {
    region.unwrap_or_else(|| panic!("[board] RISC-V fallback is missing {}", name))
}

#[cfg(target_arch = "riscv64")]
fn push_irq(irqs: &mut [u32; RISCV_PLIC_IRQ_CAPACITY], len: &mut usize, irq: u32) {
    if irq == 0 {
        return;
    }

    let mut index = 0;
    while index < *len {
        if irqs[index] == irq {
            return;
        }
        index += 1;
    }

    if *len < irqs.len() {
        irqs[*len] = irq;
        *len += 1;
    }
}
