// SPDX-License-Identifier: MPL-2.0
//! x86_64 items of `crate::dev::ownership`: the MSI/MSI-X allocations 435
//! stores in a PCI function's record. Compiled as that module's `x86` child
//! module (`#[path]`), so they keep reading its private records unchanged.

use super::*;

/// Run `allocate` on the MSI allocation of PCI function `bdf` while `tid`
/// owns it: the slot holds the existing allocation, if any, and keeps what
/// `allocate` leaves in it. `None` when `tid` does not own `bdf`. Lock order
/// `DEVICES -> PCIE_MSI_STATE`.
pub(crate) fn with_owned_msi<R>(
    tid: usize,
    bdf: u32,
    allocate: impl FnOnce(&mut Option<abi::pcie_function::PcieIrqAlloc>) -> R,
) -> Option<R> {
    let mut devices = DEVICES.lock();
    let record = devices
        .find(pci_key(bdf)?)
        .filter(|record| record.owned_by(tid))?;
    let mut msi = match record.irq {
        Some(IrqLines::Msi(alloc)) => Some(alloc),
        _ => None,
    };
    let result = allocate(&mut msi);
    record.irq = msi.map(IrqLines::Msi);
    devices.publish_nic_lines();
    Some(result)
}

/// Take every MSI allocation of the PCI functions `tid` owns out of their
/// records, handing each to `free` and dropping its lines' pending flags,
/// under one `DEVICES` hold. Returns how many. Call only after the functions
/// were quiesced.
pub(crate) fn release_msis(
    tid: usize,
    mut free: impl FnMut(abi::pcie_function::PcieIrqAlloc),
) -> usize {
    let mut released = 0;
    let mut devices = DEVICES.lock();
    for record in devices.records.iter_mut() {
        if !record.owned_by(tid) {
            continue;
        }
        if let Some(IrqLines::Msi(alloc)) = record.irq {
            record.irq = None;
            free(alloc);
            IrqLines::Msi(alloc).lines().for_each(|line| {
                let _ = crate::dev::irq_wait::take_pending(line);
            });
            released += 1;
        }
    }
    devices.publish_nic_lines();
    released
}

/// Make `tid` the owner of PCI function `bdf` holding `msi`, displacing any
/// owner; host tests only, under `TEST_STATE_LOCK`.
#[cfg(test)]
pub(crate) fn test_own_msi(tid: usize, bdf: u32, msi: abi::pcie_function::PcieIrqAlloc) {
    let mut devices = DEVICES.lock();
    let record = devices
        .find_or_insert(
            pci_key(bdf).expect("valid BDF"),
            DeviceKind::Pci { class: None },
        )
        .expect("heap for a test record");
    record.owner = Some(tid);
    record.irq = Some(IrqLines::Msi(msi));
    devices.publish_nic_lines();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The NIC line set follows a route's MSI allocation: set once 435 stores
    /// vectors for the route's owned function, cleared when they are released.
    #[test]
    fn nic_lines_follow_the_msi_allocation() {
        const TID: usize = 0x7B01;
        const BDF: u32 = 0x7B0;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let msi = abi::pcie_function::PcieIrqAlloc {
            first_irq: 0x90,
            count: 2,
            kind: abi::pcie_function::IRQ_KIND_MSIX,
        };
        assert!(claim_pci(BDF, TID));
        set_nic_route(pci_key(BDF).unwrap(), Some(TID));
        assert!(!is_nic_line(0x90), "no vector yet");
        assert_eq!(with_owned_msi(TID, BDF, |held| *held = Some(msi)), Some(()));
        assert!(is_nic_line(0x90) && is_nic_line(0x91) && !is_nic_line(0x92));
        assert_eq!(release_msis(TID, |_| {}), 1);
        assert!(!is_nic_line(0x90), "vectors freed");
        set_nic_route(pci_key(BDF).unwrap(), None);
        release_for(TID, Scope::Retired);
    }
}
