//! Helpers shared by the DMA remappers' page tables.
//!
//! The tables themselves live with their backend (VT-d
//! `iommu_x86::slpt::VtdSlpt`, RISC-V `iommu_riscv::sv39::Sv39IommuPt`). Both
//! map IOVA == PA for explicitly registered DMA ranges, so any other IOVA
//! faults at the unit. This module holds what they share: the table page
//! allocator, kernel-heap address translation, and the boot-time teardown
//! probe.

// Architectures with no DMA remapper (`NoRemapper`) build no page table.
#![cfg_attr(
    not(any(target_arch = "x86_64", target_arch = "riscv64")),
    allow(dead_code)
)]

use alloc::alloc::{alloc_zeroed, Layout};

/// One zeroed 4 KiB table page: `(kernel virtual, physical)`.
pub(crate) fn alloc_page() -> (usize, u64) {
    let layout = Layout::from_size_align(4096, 4096).expect("iommu_pt: page");
    // SAFETY: layout is non-zero and 4096-aligned.
    // The witness is the heap table allocation, including hardware-independent
    // boot probes, not the physical DMA frames later named by its leaf entries.
    let ptr = {
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::DRIVER_SHARED,
        );
        (unsafe { alloc_zeroed(layout) }) as usize
    };
    assert!(ptr != 0, "[iommu_pt] OOM allocating IOMMU page");
    (ptr, virt_to_phys(ptr))
}

/// Physical address of a kernel-heap pointer: the heap lies in the kernel's
/// physical-memory window (HHDM on x86_64, identity elsewhere).
#[inline]
pub(crate) fn virt_to_phys(virt: usize) -> u64 {
    (virt - crate::memory::frame::phys_to_virt(0)) as u64
}

/// Kernel-heap pointer of a table page's physical address.
#[inline]
pub(crate) fn phys_to_virt_inner(phys: u64) -> usize {
    crate::memory::frame::phys_to_virt(phys as usize)
}

// ── Teardown self-test ────────────────────────────────────────────────────────

/// What one page table's teardown probe observed.
#[cfg(feature = "test-hooks")]
pub struct TeardownProbe {
    /// A cleared leaf reads absent while its two neighbours in the same table stay.
    leaf: bool,
    /// Clearing the same page twice reports nothing cleared the second time.
    idempotent: bool,
    /// Unmapping a range that was never mapped allocates no table and clears nothing.
    lookup_only: bool,
    /// The cleared page can be mapped again (no table was freed by the unmap).
    remappable: bool,
}

/// Exercise `map_range` / `unmap_range` on one page table.
///
/// The three pages sit in one 4 KiB leaf table, so the neighbours prove the walk
/// clears the exact entry rather than the table, and the re-map proves the
/// intermediate tables survived.
#[cfg(feature = "test-hooks")]
pub(crate) fn probe_teardown<M, U, P, L>(map: M, unmap: U, present: P, level2: L) -> TeardownProbe
where
    M: Fn(u64, usize),
    U: Fn(u64, usize) -> usize,
    P: Fn(u64) -> bool,
    L: Fn(u64) -> bool,
{
    const BASE: u64 = 0x1234_5000;
    const PAGE: u64 = 0x1000;
    // Never mapped, and far outside the range above: any table it allocates would
    // be visible as a level-2 entry of its own.
    const UNMAPPED: u64 = 0x7FFF_0000;

    map(BASE, 3 * PAGE as usize);
    let mapped = (0..3).all(|i| present(BASE + i as u64 * PAGE));

    let cleared = unmap(BASE + PAGE, PAGE as usize);
    let leaf = mapped
        && cleared == 1
        && !present(BASE + PAGE)
        && present(BASE)
        && present(BASE + 2 * PAGE);

    let idempotent = unmap(BASE + PAGE, PAGE as usize) == 0 && !present(BASE + PAGE);

    let lookup_only =
        !level2(UNMAPPED) && unmap(UNMAPPED, 3 * PAGE as usize) == 0 && !level2(UNMAPPED);

    map(BASE + PAGE, PAGE as usize);
    let remappable = present(BASE + PAGE) && present(BASE) && present(BASE + 2 * PAGE);

    TeardownProbe {
        leaf,
        idempotent,
        lookup_only,
        remappable,
    }
}

/// Boot-time assertions for the teardown primitives of this architecture's
/// DMA remapper page table.
///
/// The page tables are plain memory, so this runs without IOMMU hardware: it is
/// the page-table half of a revoke (leaf exactly cleared, siblings untouched,
/// no allocation on the unmap path). The hardware half — that the invalidation
/// is acknowledged — is proven by the DMA-isolation QEMU lanes.
#[cfg(feature = "test-hooks")]
pub(crate) fn run_selftest() {
    use crate::dev::iommu::DmaRemapper;
    let Some(probe) = crate::arch::iommu::BACKEND.teardown_probe() else {
        log::info!("IOMMU-TEARDOWN: SKIP (no DMA remapper page table on this architecture)");
        return;
    };
    report("IOMMU-TEARDOWN-LEAF", probe.leaf);
    report("IOMMU-TEARDOWN-IDEMPOTENT", probe.idempotent);
    report("IOMMU-TEARDOWN-LOOKUP-ONLY", probe.lookup_only);
    report("IOMMU-TEARDOWN-REMAPPABLE", probe.remappable);
}

/// One PASS/FAIL line per teardown property; the markers are the QEMU evidence.
#[cfg(feature = "test-hooks")]
fn report(marker: &str, ok: bool) {
    if ok {
        log::info!("{marker}: PASS");
    } else {
        log::error!("{marker}: FAIL");
    }
}
