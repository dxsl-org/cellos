//! Intel VT-d passthrough IOMMU driver for x86_64.
//!
//! Initialises VT-d in passthrough mode (TT=0b10): all 256 BDFs share one
//! context table where every entry passes DMA through unmodified (IOVA == PA).
//! This is safe for QEMU single-tenant use; real hardware requires Sv39x4
//! or VT-d page tables before G2 multi-tenant workloads.
//!
//! The VT-d MMIO at 0xFED90000 must be identity-mapped in `init_kernel_paging_x86`
//! before this function runs.

use alloc::alloc::{alloc_zeroed, Layout};

// Intel VT-d register offsets (VT-d spec §10.4)
const VTD_GCAP:   usize = 0x00; // 64-bit capabilities (read-only probe)
const VTD_GCMD:   usize = 0x18; // 32-bit command (write-only)
const VTD_GSTS:   usize = 0x1C; // 32-bit status (read-only)
const VTD_RTADDR: usize = 0x20; // 64-bit root table physical address

// GCMD / GSTS bit masks
const TE:   u32 = 1 << 31; // Translation Enable (command) / Status
const SRTP: u32 = 1 << 30; // Set Root Table Pointer (command) / RTPS (status)

// Context-entry field encoding (VT-d spec §9.3)
const TT_PASSTHROUGH: u64 = 0b10 << 2;  // Translation Type = passthrough
const AW_39BIT:       u64 = 0b010 << 4; // Address Width = 39-bit (Sv39)
const CTX_PRESENT:    u64 = 1;
const DID:            u64 = 0x0001u64 << 8; // Domain ID (in hi qword)

// QEMU q35 hardcoded VT-d MMIO base (ACPI DMAR DRHD register_base_address).
const VTD_BASE: usize = 0xFED9_0000;

const POLL_MAX: u64 = 1_000_000;

#[inline]
unsafe fn read64(base: usize, off: usize) -> u64 {
    // SAFETY: caller ensures base is identity-mapped VT-d MMIO.
    unsafe { core::ptr::read_volatile((base + off) as *const u64) }
}

#[inline]
unsafe fn read32(base: usize, off: usize) -> u32 {
    // SAFETY: caller ensures base is identity-mapped VT-d MMIO.
    unsafe { core::ptr::read_volatile((base + off) as *const u32) }
}

#[inline]
unsafe fn write32(base: usize, off: usize, val: u32) {
    // SAFETY: caller ensures base is identity-mapped VT-d MMIO.
    unsafe { core::ptr::write_volatile((base + off) as *mut u32, val) }
}

#[inline]
unsafe fn write64(base: usize, off: usize, val: u64) {
    // SAFETY: caller ensures base is identity-mapped VT-d MMIO.
    unsafe { core::ptr::write_volatile((base + off) as *mut u64, val) }
}

/// Translate a kernel heap VA to its DMA physical address (x86_64 HHDM).
#[inline]
fn heap_to_phys(virt: usize) -> u64 {
    let hhdm = crate::memory::frame::phys_to_virt(0);
    (virt - hhdm) as u64
}

/// Allocate a 4096-aligned, zeroed 4 KiB page for an IOMMU table.
///
/// Returns (virt_addr, phys_addr). Panics on OOM.
fn alloc_table() -> (usize, u64) {
    let layout = Layout::from_size_align(4096, 4096)
        .expect("VT-d table layout: size and align are valid");
    // SAFETY: layout is non-zero; alloc_zeroed returns a valid pointer or null.
    let ptr = unsafe { alloc_zeroed(layout) } as usize;
    assert!(ptr != 0, "[vtd] OOM allocating IOMMU table");
    (ptr, heap_to_phys(ptr))
}

/// Initialise Intel VT-d in passthrough mode.
///
/// Called from `iommu::init()` on x86_64 targets. Falls through silently when
/// VT-d is absent (QEMU launched without `-device intel-iommu`).
pub fn init_vtd() {
    // Probe GCAP: all-zeros or all-ones means VT-d not present.
    // SAFETY: VTD_BASE (0xFED90000) is identity-mapped by init_kernel_paging_x86.
    let gcap = unsafe { read64(VTD_BASE, VTD_GCAP) };
    if gcap == 0 || gcap == u64::MAX {
        log::info!("[vtd] Intel VT-d not present (GCAP={:#x})", gcap);
        return;
    }
    log::info!("[vtd] Intel VT-d found GCAP={:#x}", gcap);

    // Allocate root table (256 × 16 B = 4096 B, 4096-aligned).
    let (root_virt, root_phys) = alloc_table();
    // Allocate one shared passthrough context table (256 × 16 B = 4096 B).
    let (ctx_virt, ctx_phys) = alloc_table();

    // Fill context table: every BDF gets passthrough (TT=0b10, AW=39-bit).
    // Entry layout: lo qword = TT|AW|present; hi qword = DID.
    let lo = TT_PASSTHROUGH | AW_39BIT | CTX_PRESENT;
    let hi = DID;
    for i in 0usize..256 {
        let slot = ctx_virt + i * 16;
        // SAFETY: ctx_virt is a zeroed 4096-B heap allocation; i*16 < 4096.
        unsafe {
            core::ptr::write_volatile(slot as *mut u64, lo);
            core::ptr::write_volatile((slot + 8) as *mut u64, hi);
        }
    }

    // Fill root table: every bus entry points to the shared context table.
    // Entry layout: lo qword = ctx_phys|present; hi qword = 0.
    for i in 0usize..256 {
        let slot = root_virt + i * 16;
        // SAFETY: root_virt is a zeroed 4096-B heap allocation; i*16 < 4096.
        unsafe {
            core::ptr::write_volatile(slot as *mut u64, ctx_phys | CTX_PRESENT);
            core::ptr::write_volatile((slot + 8) as *mut u64, 0u64);
        }
    }

    // Enable VT-d translation.
    // Step 1: programme root table address.
    // SAFETY: VTD_BASE is identity-mapped; root_phys is 4096-aligned.
    unsafe { write64(VTD_BASE, VTD_RTADDR, root_phys); }

    // Step 2: GCMD.SRTP → poll GSTS.RTPS.
    // SAFETY: VTD_BASE is identity-mapped VT-d MMIO.
    unsafe { write32(VTD_BASE, VTD_GCMD, SRTP); }
    let mut n = 0u64;
    loop {
        // SAFETY: VTD_BASE is identity-mapped VT-d MMIO.
        if unsafe { read32(VTD_BASE, VTD_GSTS) } & SRTP != 0 { break; }
        n += 1;
        if n >= POLL_MAX {
            log::warn!("[vtd] GSTS.RTPS never set — aborting VT-d enable");
            return;
        }
        core::hint::spin_loop();
    }

    // Step 3: GCMD.(TE|SRTP) → poll GSTS.TES.
    // SAFETY: VTD_BASE is identity-mapped VT-d MMIO.
    unsafe { write32(VTD_BASE, VTD_GCMD, TE | SRTP); }
    let mut n = 0u64;
    loop {
        // SAFETY: VTD_BASE is identity-mapped VT-d MMIO.
        if unsafe { read32(VTD_BASE, VTD_GSTS) } & TE != 0 { break; }
        n += 1;
        if n >= POLL_MAX {
            log::warn!("[vtd] GSTS.TES never set — VT-d translation NOT active");
            return;
        }
        core::hint::spin_loop();
    }

    super::iommu::set_active();
    log::info!("[vtd] Intel VT-d passthrough enabled @ {:#x}", VTD_BASE);
}
