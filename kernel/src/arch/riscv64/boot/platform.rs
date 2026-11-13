// SPDX-License-Identifier: MPL-2.0
//! riscv64 platform facts: the firmware DTB parsed against the selected SoC
//! profile, published once into `crate::platform`.

use crate::platform::{publish_boot, with, PlatformInfo, VirtioEntry};

use hal_soc_riscv::{RiscvSocProfile, RtcAccessPolicy, UartAccessPolicy, VirtioMmioPolicy};

pub const RISCV_PLIC_IRQ_CAPACITY: usize = 9;

impl PlatformInfo {
    /// Only called from `from_dtb` (the DTB parser below).
    fn board_defaults(profile: &'static RiscvSocProfile) -> Self {
        let fallback = profile.fallback_mmio;
        let uart = required_riscv_mmio(fallback.uart, "UART");
        Self {
            uart_base: uart.base,
            uart_irq: uart.irq.unwrap_or(0),
            plic_base: fallback.plic.base,
            plic_size: fallback.plic.size,
            clint_base: fallback.clint.base,
            virtio_mmio: riscv_virtio_slots(fallback.virtio),
            rtc_base: rtc_fallback_base(profile),
        }
    }

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

/// Parse the DTB and store platform info. Must be called exactly once by the
/// boot CPU before `uart::init`, interrupts, SMP, and `init_kernel_paging`.
/// Safe with `dtb_ptr == 0`.
pub fn init(sbi_dtb: usize) {
    let profile = crate::arch::board::soc();
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

pub fn riscv_plic_init_data() -> Option<(usize, [u32; RISCV_PLIC_IRQ_CAPACITY], usize)> {
    let context = riscv_plic_context_for_current_hart()?;
    let (irqs, len) = with(|platform| platform.riscv_plic_irqs());
    Some((context, irqs, len))
}

pub fn riscv_plic_context_for_current_hart() -> Option<usize> {
    let logical_hart = crate::sched::hart_local::current_hart_id();
    let physical_hart = crate::sched::smp::logical_to_physical(logical_hart)?;
    crate::arch::board::soc().plic_context_for_physical_hart(physical_hart)
}

pub fn virtio_mmio_base_for_irq(irq: u32) -> Option<usize> {
    with(|platform| platform.virtio_mmio_base_for_irq(irq))
}

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

fn virtio_mmio_entries_for_profile(
    fdt: &fdt::Fdt,
    profile: &'static RiscvSocProfile,
) -> [Option<VirtioEntry>; 8] {
    match profile.virtio_mmio {
        VirtioMmioPolicy::Discover => collect_virtio(fdt),
        VirtioMmioPolicy::Absent => [None; 8],
    }
}

fn from_dtb(dtb_ptr: usize, profile: &'static RiscvSocProfile) -> PlatformInfo {
    let board = crate::board::active();
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
    let defaults = profile.fallback_mmio;

    let requires_uart = board.boot.requires_firmware_dtb
        && (board.has_driver(cellos_boards::DriverId::UartNs16550a)
            || board.has_driver(cellos_boards::DriverId::UartDwApb));
    let uart_base = reg_base(&fdt, profile.uart_compatibles).unwrap_or_else(|| {
        if requires_uart {
            panic!("[platform] required firmware DTB is missing UART MMIO");
        }
        log::warn!("[platform] UART not in DTB");
        required_riscv_mmio(defaults.uart, "UART").base
    });
    let uart_irq = irq_first(&fdt, profile.uart_compatibles).unwrap_or_else(|| {
        if requires_uart {
            panic!("[platform] required firmware DTB is missing UART IRQ");
        }
        required_riscv_mmio(defaults.uart, "UART").irq.unwrap_or(0)
    });

    let (plic_base, plic_size) =
        reg_base_size(&fdt, profile.plic_compatibles).unwrap_or_else(|| {
            if board.boot.requires_firmware_dtb
                && board.has_driver(cellos_boards::DriverId::PlicSifive)
            {
                panic!("[platform] required firmware DTB is missing PLIC MMIO");
            }
            log::warn!("[platform] PLIC not in DTB");
            (defaults.plic.base, defaults.plic.size)
        });

    let clint_base = reg_base(&fdt, profile.clint_compatibles).unwrap_or_else(|| {
        if board.boot.requires_firmware_dtb
            && board.has_driver(cellos_boards::DriverId::ClintSifive)
        {
            panic!("[platform] required firmware DTB is missing CLINT MMIO");
        }
        log::warn!("[platform] CLINT not in DTB");
        defaults.clint.base
    });

    let virtio_mmio = if board.has_driver(cellos_boards::DriverId::VirtioMmio) {
        virtio_mmio_entries_for_profile(&fdt, profile)
    } else {
        [None; 8]
    };

    let rtc_base =
        reg_base(&fdt, profile.rtc_compatibles).unwrap_or_else(|| rtc_fallback_base(profile));

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

fn rtc_fallback_base(profile: &'static RiscvSocProfile) -> usize {
    match profile.rtc_access {
        RtcAccessPolicy::Mmio => required_riscv_mmio(profile.fallback_mmio.rtc, "RTC").base,
        RtcAccessPolicy::Unavailable => 0,
    }
}

fn reg_base(fdt: &fdt::Fdt, compat: &[&str]) -> Option<usize> {
    reg_base_size(fdt, compat).map(|(b, _)| b)
}

fn reg_base_size(fdt: &fdt::Fdt, compat: &[&str]) -> Option<(usize, usize)> {
    let node = fdt.find_compatible(compat)?;
    let r = node.reg()?.next()?;
    Some((r.starting_address as usize, r.size?))
}

/// Read the first cell of the `interrupts` property as a big-endian u32.
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

fn riscv_virtio_slots(regions: &[hal_soc_riscv::RiscvMmioRegion]) -> [Option<VirtioEntry>; 8] {
    let mut entries = [None, None, None, None, None, None, None, None];
    let mut index = 0;
    while index < regions.len() && index < entries.len() {
        let region = regions[index];
        entries[index] = Some(VirtioEntry {
            base: region.base,
            irq: region.irq.unwrap_or(0),
        });
        index += 1;
    }
    entries
}

fn required_riscv_mmio(
    region: Option<hal_soc_riscv::RiscvMmioRegion>,
    name: &'static str,
) -> hal_soc_riscv::RiscvMmioRegion {
    region.unwrap_or_else(|| panic!("[board] RISC-V fallback is missing {}", name))
}

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
