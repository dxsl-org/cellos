//! PCIe function query (434), MSI/MSI-X allocation (435) and the MSI-X table
//! carve-out (ADR-0031, CP1 §3–§4). x86_64 only.
//!
//! Lifetime: an allocation belongs to the BDF owner and is held in the
//! function's device record ([`crate::dev::ownership`]) until that task's DMA
//! authority is torn down. Teardown quiesces the function first (MSI off,
//! MSI-X masked, Bus Master off), then [`release_for`] frees the vectors, so a
//! dead cell's device cannot fire into a reused vector.

mod config;
mod layout;
mod program;
mod record;
mod vectors;
mod virtio_gate;

pub use virtio_gate::dma_bypasses_iommu;

use crate::dev::{irq_wait, pcie_ecam};
use crate::sync::Spinlock;
use abi::pcie_function::{
    PcieIrqAlloc, ViPcieFunctionV1, IRQ_KIND_MSI, IRQ_KIND_MSIX, IRQ_MODE_ALLOW_MSI,
    IRQ_MODE_ALLOW_MSIX, IRQ_WANT_MAX,
};
use alloc::vec::Vec;
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

/// The vector pool. Allocations live in the device records; lock order
/// `DEVICES -> PCIE_MSI_STATE`.
struct State {
    pool: VectorPool,
}

static STATE: Spinlock<State> = Spinlock::ranked(
    State {
        pool: VectorPool::new(),
    },
    crate::sync::rank::PCIE_MSI_STATE,
);
static READY: AtomicBool = AtomicBool::new(false);
static FIRMWARE_FORBIDS_MSI: AtomicBool = AtomicBool::new(false);

/// ISR: the pending flag first, then the early-sweep hint (one atomic store
/// each), so a sweep that takes the hint sees the flag. A vector of a live NIC
/// route's function also signals NET_RX; that check is one atomic load of the
/// NIC line set (`ownership::NIC_LINES`), with no lock taken.
pub(crate) fn on_msi(vector: u8) {
    irq_wait::signal_irq(vector);
    irq_wait::hint_msi_wake();
    if crate::dev::driver_cell::owns_registered_nic_irq(vector as u32) {
        crate::task::waker::signal_net_rx();
    }
}

/// Install the MSI handler. `firmware_forbids_msi` is FADT IAPC_BOOT_ARCH
/// bit 3; when set, 435 refuses every request.
pub fn init(firmware_forbids_msi: bool) {
    FIRMWARE_FORBIDS_MSI.store(firmware_forbids_msi, Ordering::Release);
    hal::x86_64::msi::set_handler(on_msi);
    READY.store(true, Ordering::Release);
    log::info!(
        "[msi] vectors={:#x}-{:#x} firmware_forbids={} interrupt_remapping={}",
        hal::x86_64::msi::VECTOR_FIRST,
        hal::x86_64::msi::VECTOR_LAST,
        firmware_forbids_msi as u8,
        crate::arch::dev::iommu_x86::ir::is_ir_active() as u8
    );
}

fn owned_device(owner: usize, bdf: u32) -> Result<pcie_ecam::PciDevice, PcieFunctionError> {
    if bdf > 0xFFFF {
        return Err(PcieFunctionError::InvalidInput);
    }
    if !crate::dev::ownership::owns_pci(owner, bdf) {
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
    if !crate::dev::pcie_ecam::memory_bars_assigned(&dev.bars) {
        return Err(PcieFunctionError::NotSupported);
    }
    if !READY.load(Ordering::Acquire) || FIRMWARE_FORBIDS_MSI.load(Ordering::Acquire) {
        return Err(PcieFunctionError::NotSupported);
    }
    let cfg = Config::new(bdf);
    let msi_cap = cfg.find_capability(CAP_ID_MSI);
    // Plain MSI is the fallback whenever MSI-X cannot be used.
    let msi_allowed = mode & IRQ_MODE_ALLOW_MSI != 0 && msi_cap.is_some();
    let mut msix = record::carved_layout(&dev, &cfg)
        .filter(|_| mode & IRQ_MODE_ALLOW_MSIX != 0)
        .and_then(|(cap, layout)| {
            let base = dev.bars[layout.table_bar as usize].base_addr();
            (base != 0).then_some((cap, layout, base as usize + layout.table_offset as usize))
        });
    // The kernel writes the table through its own alias of it
    // (`paging::mmio_alias::kernel_mmio_alias`: supervisor-only, uncached), never through
    // the identity leaves a `RequestMmio` grant of the BAR may have made USER,
    // where SMAP would fault the write. Carved pages are not user-mapped, but
    // the table write does not rely on that. Alias before taking STATE: the
    // first alias of a table may issue a TLB shootdown, which must not wait on
    // CPUs spinning for STATE; later calls reuse it. A table that cannot be
    // aliased is never touched: this call falls back to plain MSI when the
    // caller allowed it, and otherwise fails with nothing programmed.
    if let Some((cap, layout, table_phys)) = msix {
        match crate::memory::paging::mmio_alias::kernel_mmio_alias(
            table_phys,
            layout.table_size as usize * 16,
        ) {
            Ok(table) => msix = Some((cap, layout, table)),
            Err(error) => {
                log::error!(
                    "[msi] {:02x}:{:02x}.{} MSI-X table {table_phys:#x} not mapped ({error:?}); {}",
                    bdf >> 8,
                    (bdf >> 3) & 0x1F,
                    bdf & 7,
                    if msi_allowed {
                        "trying MSI"
                    } else {
                        "no interrupts"
                    },
                );
                if !msi_allowed {
                    return Err(PcieFunctionError::NotSupported);
                }
                msix = None;
            }
        }
    }
    crate::dev::ownership::x86::with_owned_msi(owner, bdf, |held| {
        if let Some(existing) = *held {
            return Ok(existing);
        }
        let mut state = STATE.lock();
        let mut irq = None;
        if let Some((cap, layout, table)) = msix {
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
            if program::program_msix(&cfg, cap, &layout, table, first, count).is_some() {
                irq = Some(PcieIrqAlloc {
                    first_irq: first,
                    count: count as u8,
                    kind: IRQ_KIND_MSIX,
                });
            } else {
                // A block that failed partway leaves MSI-X enabled and
                // function-masked: turn it off, so the function is never left
                // with every vector masked and MSI disabled, then fall back to
                // MSI when the caller allowed it.
                state.pool.free(first, count);
                program::disable_msix(&cfg, cap);
                log::warn!(
                    "[msi] {:02x}:{:02x}.{} MSI-X programming failed; {}",
                    bdf >> 8,
                    (bdf >> 3) & 0x1F,
                    bdf & 7,
                    if msi_allowed {
                        "trying MSI"
                    } else {
                        "no interrupts"
                    },
                );
            }
        }
        let irq = match (irq, msi_cap) {
            (Some(irq), _) => irq,
            (None, Some(cap)) if msi_allowed => {
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
            }
            _ => return Err(PcieFunctionError::NotSupported),
        };
        for line in irq.first_irq..irq.first_irq + irq.count {
            let _ = irq_wait::take_pending(line);
        }
        *held = Some(irq);
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
    })
    .unwrap_or(Err(PcieFunctionError::PermissionDenied))
}

/// Record an allocation without touching config space; host tests only,
/// under `TEST_STATE_LOCK`.
#[cfg(test)]
pub(crate) fn allocate_for_test(owner: usize, bdf: u32, count: usize) -> PcieIrqAlloc {
    let first = STATE.lock().pool.alloc(count, 1).expect("test vectors");
    let irq = PcieIrqAlloc {
        first_irq: first,
        count: count as u8,
        kind: IRQ_KIND_MSIX,
    };
    for line in first..first + count as u8 {
        let _ = irq_wait::take_pending(line);
    }
    crate::dev::ownership::x86::test_own_msi(owner, bdf, irq);
    irq
}

/// Free every allocation `owner` holds; returns how many. Call only after the
/// owner's functions were quiesced. Pending flags are dropped so the next
/// owner of a reused vector never sees the dead owner's interrupt.
pub fn release_for(owner: usize) -> usize {
    crate::dev::ownership::x86::release_msis(owner, |irq| {
        STATE.lock().pool.free(irq.first_irq, irq.count as usize);
    })
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
    use crate::dev::ownership::{
        lines_owned_by, repend_owned_line as repend_owned, take_owned_line as take_owned,
    };

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

    #[test]
    fn on_msi_signals_net_rx_only_for_registered_nic() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        const NIC_TID: usize = 99_001;
        const OTHER_TID: usize = 99_002;
        let nic_alloc = allocate_for_test(NIC_TID, 0x0010, 1);
        let other_alloc = allocate_for_test(OTHER_TID, 0x0020, 1);

        // Clear any leftover pending NET_RX.
        let _ = crate::task::waker::consume_pending(abi::syscall::events::NET_RX);

        // MSI for non-NIC driver: signals IRQ but does NOT signal NET_RX.
        on_msi(other_alloc.first_irq);
        assert!(irq_wait::take_pending(other_alloc.first_irq));
        assert_eq!(
            crate::task::waker::consume_pending(abi::syscall::events::NET_RX),
            0
        );

        // Bind the NIC route of the function the vector belongs to.
        crate::dev::driver_cell::reset_nic_routes();
        let key = abi::block_route::DeviceKey::new(abi::block_route::BLOCK_DEVICE_PCI, 0x0010)
            .expect("valid BDF");
        assert!(
            crate::dev::driver_cell::publish_nic_route(key, (NIC_TID, 1, 1), || true).is_some()
        );

        // MSI for NIC driver: signals IRQ AND signals NET_RX.
        on_msi(nic_alloc.first_irq);
        assert!(irq_wait::take_pending(nic_alloc.first_irq));
        assert_eq!(
            crate::task::waker::consume_pending(abi::syscall::events::NET_RX),
            abi::syscall::events::NET_RX
        );

        // Clear the NIC route.
        crate::dev::driver_cell::deregister_all_for(NIC_TID);
        crate::dev::driver_cell::reset_nic_routes();
        release_for(NIC_TID);
        release_for(OTHER_TID);
    }
}
