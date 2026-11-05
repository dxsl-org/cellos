//! PCIe function query (434), MSI/MSI-X allocation (435) and the MSI-X table
//! carve-out (ADR-0031, CP1 §3–§4). x86_64 only.
//!
//! Lifetime: an allocation belongs to the BDF owner and lives until that task's
//! DMA authority is torn down. Teardown quiesces the function first (MSI off,
//! MSI-X masked, Bus Master off), then [`release_for`] frees the vectors, so a
//! dead cell's device cannot fire into a reused vector.

mod config;
mod layout;
mod program;
mod record;
mod vectors;
mod virtio_gate;

pub use virtio_gate::dma_bypasses_iommu;

use crate::sync::Spinlock;
use crate::task::drivers::{irq_wait, pcie_ecam};
use alloc::vec::Vec;
use api::pcie_function::{
    PcieIrqAlloc, ViPcieFunctionV1, IRQ_KIND_MSI, IRQ_KIND_MSIX, IRQ_MODE_ALLOW_MSI,
    IRQ_MODE_ALLOW_MSIX, IRQ_WANT_MAX,
};
use config::{Config, CAP_ID_MSI, CAP_ID_MSIX};
use core::sync::atomic::{AtomicBool, Ordering};
use vectors::VectorPool;

/// Why 434/435 refused; the syscall layer maps each to its sentinel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcieFunctionError {
    InvalidInput,
    PermissionDenied,
    NotFound,
    NotSupported,
    NoVectors,
}

struct Allocation {
    bdf: u32,
    owner: usize,
    irq: PcieIrqAlloc,
}

struct State {
    pool: VectorPool,
    allocations: Vec<Allocation>,
}

static STATE: Spinlock<State> = Spinlock::new(State {
    pool: VectorPool::new(),
    allocations: Vec::new(),
});
static READY: AtomicBool = AtomicBool::new(false);
static FIRMWARE_FORBIDS_MSI: AtomicBool = AtomicBool::new(false);

/// ISR: the pending flag first, then the early-sweep hint (one atomic store
/// each, no lock), so a sweep that takes the hint sees the flag.
fn on_msi(vector: u8) {
    irq_wait::signal_irq(vector);
    irq_wait::hint_msi_wake();
}

/// Install the MSI handler. `firmware_forbids_msi` is FADT IAPC_BOOT_ARCH
/// bit 3; when set, 435 refuses every request.
pub fn init(firmware_forbids_msi: bool) {
    FIRMWARE_FORBIDS_MSI.store(firmware_forbids_msi, Ordering::Release);
    hal::x86_64::msi::set_handler(on_msi);
    READY.store(true, Ordering::Release);
    log::info!(
        "[msi] vectors={:#x}-{:#x} firmware_forbids={} no interrupt remapping",
        hal::x86_64::msi::VECTOR_FIRST,
        hal::x86_64::msi::VECTOR_LAST,
        firmware_forbids_msi as u8
    );
}

fn owned_device(owner: usize, bdf: u32) -> Result<pcie_ecam::PciDevice, PcieFunctionError> {
    if bdf > 0xFFFF {
        return Err(PcieFunctionError::InvalidInput);
    }
    if crate::resource_registry::owner_of_bdf(bdf) != Some(owner) {
        return Err(PcieFunctionError::PermissionDenied);
    }
    pcie_ecam::device(bdf).ok_or(PcieFunctionError::NotFound)
}

/// 434: the owned function's record.
pub fn query(owner: usize, bdf: u32) -> Result<ViPcieFunctionV1, PcieFunctionError> {
    let dev = owned_device(owner, bdf)?;
    record::build(&dev, FIRMWARE_FORBIDS_MSI.load(Ordering::Acquire))
        .ok_or(PcieFunctionError::NotFound)
}

/// 435: allocate and program MSI-X (preferred) or MSI for the owned function.
pub fn allocate(
    owner: usize,
    bdf: u32,
    want: usize,
    mode: usize,
) -> Result<PcieIrqAlloc, PcieFunctionError> {
    let known = IRQ_MODE_ALLOW_MSIX | IRQ_MODE_ALLOW_MSI;
    if !(1..=IRQ_WANT_MAX).contains(&want) || mode & known == 0 || mode & !known != 0 {
        return Err(PcieFunctionError::InvalidInput);
    }
    let dev = owned_device(owner, bdf).map_err(|e| match e {
        PcieFunctionError::NotFound => PcieFunctionError::NotSupported,
        other => other,
    })?;
    if !READY.load(Ordering::Acquire) || FIRMWARE_FORBIDS_MSI.load(Ordering::Acquire) {
        return Err(PcieFunctionError::NotSupported);
    }
    let cfg = Config::new(bdf);
    let msi_cap = cfg.find_capability(CAP_ID_MSI);
    let msix = record::carved_layout(&dev, &cfg)
        .filter(|_| mode & IRQ_MODE_ALLOW_MSIX != 0)
        .and_then(|(cap, layout)| {
            let base = dev.bars[layout.table_bar as usize].base_addr();
            (base != 0).then_some((cap, layout, base as usize + layout.table_offset as usize))
        });
    // Map the table kernel-only before taking STATE: mapping may issue a TLB
    // shootdown, which must not wait on CPUs spinning for STATE. Carved pages
    // are never user-mapped, so an early (or repeated) mapping is harmless.
    if let Some((_, layout, table_phys)) = msix {
        crate::memory::paging::map_mmio_x86(table_phys, layout.table_size as usize * 16);
    }
    let mut state = STATE.lock();
    if let Some(existing) = state
        .allocations
        .iter()
        .find(|a| a.bdf == bdf && a.owner == owner)
    {
        return Ok(existing.irq);
    }
    let irq = if let Some((cap, layout, table_phys)) = msix {
        let count = want.min(layout.table_size as usize);
        let first = state
            .pool
            .alloc(count, 1)
            .ok_or(PcieFunctionError::NoVectors)?;
        if let Some(at) = msi_cap {
            let _ = cfg
                .read16(at as usize + 2)
                .map(|c| cfg.write16(at as usize + 2, c & !1));
        }
        if program::program_msix(&cfg, cap, &layout, table_phys, first, count).is_none() {
            state.pool.free(first, count);
            return Err(PcieFunctionError::NotSupported);
        }
        PcieIrqAlloc {
            first_irq: first,
            count: count as u8,
            kind: IRQ_KIND_MSIX,
        }
    } else if let Some(cap) = msi_cap.filter(|_| mode & IRQ_MODE_ALLOW_MSI != 0) {
        let control = cfg
            .read16(cap as usize + 2)
            .ok_or(PcieFunctionError::NotSupported)?;
        let count = program::msi_count(want, control);
        let first = state
            .pool
            .alloc(count, count)
            .ok_or(PcieFunctionError::NoVectors)?;
        if let Some(at) = cfg.find_capability(CAP_ID_MSIX) {
            let off = at as usize + 2;
            let _ = cfg
                .read16(off)
                .map(|c| cfg.write16(off, (c & !(1 << 15)) | (1 << 14)));
        }
        if program::program_msi(&cfg, cap, first, count).is_none() {
            state.pool.free(first, count);
            return Err(PcieFunctionError::NotSupported);
        }
        PcieIrqAlloc {
            first_irq: first,
            count: count as u8,
            kind: IRQ_KIND_MSI,
        }
    } else {
        return Err(PcieFunctionError::NotSupported);
    };
    for line in irq.first_irq..irq.first_irq + irq.count {
        let _ = irq_wait::take_pending(line);
    }
    state.allocations.push(Allocation { bdf, owner, irq });
    log::info!(
        "[msi] {:02x}:{:02x}.{} tid={} kind={} irq={:#x}+{}",
        bdf >> 8,
        (bdf >> 3) & 0x1F,
        bdf & 7,
        owner,
        if irq.kind == IRQ_KIND_MSIX {
            "msix"
        } else {
            "msi"
        },
        irq.first_irq,
        irq.count
    );
    Ok(irq)
}

/// Whether `irq` belongs to an allocation `owner` holds (x86 `WaitIrq` gate).
pub fn irq_owned_by(irq: u8, owner: usize) -> bool {
    holds(&STATE.lock(), owner, irq)
}

fn holds(state: &State, owner: usize, irq: u8) -> bool {
    state
        .allocations
        .iter()
        .any(|a| a.owner == owner && irq >= a.irq.first_irq && irq < a.irq.first_irq + a.irq.count)
}

/// Every line `owner` holds: the snapshot a `WaitCompletion(IRQ)` waits on.
/// Takes `STATE` alone; never call it with `SCHEDULER` held.
pub fn lines_owned_by(owner: usize) -> irq_wait::IrqLineSet {
    let mut set = irq_wait::IrqLineSet::EMPTY;
    for a in STATE.lock().allocations.iter().filter(|a| a.owner == owner) {
        for line in a.irq.first_irq..a.irq.first_irq + a.irq.count {
            set.insert(line);
        }
    }
    set
}

/// Consume `irq`'s pending flag only while `owner` still holds the line.
///
/// Ownership check and swap share one `STATE` hold, and [`release_for`]
/// clears flags under `STATE` too, so a revoked (and possibly re-allocated)
/// vector is never consumed by its former owner. `false` when the line is no
/// longer owned or nothing was pending.
pub fn take_owned(owner: usize, irq: u8) -> bool {
    let state = STATE.lock();
    holds(&state, owner, irq) && irq_wait::take_pending(irq)
}

/// Re-pend a line taken by [`take_owned`] whose record could not be
/// delivered; a line revoked meanwhile is left alone.
pub fn repend_owned(owner: usize, irq: u8) {
    let state = STATE.lock();
    if holds(&state, owner, irq) {
        irq_wait::restore_pending(irq);
    }
}

/// Record an allocation without touching config space; host tests only,
/// under `TEST_STATE_LOCK`.
#[cfg(test)]
pub(crate) fn allocate_for_test(owner: usize, bdf: u32, count: usize) -> PcieIrqAlloc {
    let mut state = STATE.lock();
    let first = state.pool.alloc(count, 1).expect("test vectors");
    let irq = PcieIrqAlloc {
        first_irq: first,
        count: count as u8,
        kind: IRQ_KIND_MSIX,
    };
    for line in first..first + count as u8 {
        let _ = irq_wait::take_pending(line);
    }
    state.allocations.push(Allocation { bdf, owner, irq });
    irq
}

/// Free every allocation `owner` holds; returns how many. Call only after the
/// owner's functions were quiesced. Pending flags are dropped so the next
/// owner of a reused vector never sees the dead owner's interrupt.
pub fn release_for(owner: usize) -> usize {
    let mut state = STATE.lock();
    let State { pool, allocations } = &mut *state;
    let before = allocations.len();
    allocations.retain(|a| {
        if a.owner != owner {
            return true;
        }
        pool.free(a.irq.first_irq, a.irq.count as usize);
        for line in a.irq.first_irq..a.irq.first_irq + a.irq.count {
            let _ = irq_wait::take_pending(line);
        }
        false
    });
    before - allocations.len()
}

/// Physical ranges of the PCIe BAR window `[base, base+len)` that may be
/// user-mapped: the window minus the MSI-X table/PBA pages of the function
/// whose BAR starts at `base`, when its layout passes the carve rule.
pub fn user_ranges(base: usize, len: usize) -> Vec<(usize, usize)> {
    let start = base as u64;
    let end = start.saturating_add(len as u64);
    let holes: Vec<(u64, u64)> = pcie_ecam::device_with_bar(start)
        .and_then(|(dev, index)| {
            let cfg = Config::new(dev.canonical_bdf());
            record::carved_layout(&dev, &cfg).map(|(_, layout)| {
                layout
                    .carved_ranges(index)
                    .map(|(s, e)| (start + s, start + e))
                    .collect()
            })
        })
        .unwrap_or_default();
    layout::subtract(start, end, &holes)
        .into_iter()
        .map(|(s, e)| (s as usize, (e - s) as usize))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_revoked_line_is_neither_taken_nor_repended_by_its_former_owner() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        const OLD: usize = 73_101;
        const NEW: usize = 73_102;
        let irq = allocate_for_test(OLD, 0x0010, 2);
        let (a, b) = (irq.first_irq, irq.first_irq + 1);
        let owned = lines_owned_by(OLD);
        assert!(owned.contains(a) && owned.contains(b));
        assert!(lines_owned_by(NEW).is_empty());

        irq_wait::restore_pending(a);
        assert!(!take_owned(NEW, a), "not the owner");
        assert!(take_owned(OLD, a));
        assert!(!take_owned(OLD, a), "consumed once");
        repend_owned(OLD, a);
        assert!(take_owned(OLD, a), "a re-pended line is taken again");

        irq_wait::restore_pending(b);
        assert_eq!(release_for(OLD), 1);
        assert!(lines_owned_by(OLD).is_empty());
        assert!(
            !take_owned(OLD, b),
            "revocation dropped the flag and the line"
        );

        let reused = allocate_for_test(NEW, 0x0018, 2);
        assert_eq!(reused.first_irq, a, "the freed block is handed out again");
        irq_wait::restore_pending(a);
        assert!(!take_owned(OLD, a), "the former owner cannot consume it");
        repend_owned(OLD, b);
        assert!(!irq_wait::take_pending(b), "repend after revoke is a no-op");
        assert!(take_owned(NEW, a));
        assert_eq!(release_for(NEW), 1);
    }
}
