// SPDX-License-Identifier: MPL-2.0
//! Per-device ownership records (architecture remediation Phase 18).
//!
//! One [`DeviceRecord`] per device a Driver Cell can own: a PCI function, a
//! VirtIO MMIO slot, or the DWC2 USB aperture. Each names the device's owner,
//! its claimable windows, its interrupts and its DMA domain, so MMIO
//! (`RequestMmio`), IRQ (435, `WaitIrq`, the MSI and VirtIO ISRs), DMA
//! (`GrantDma`, DMA export) and route (416/417) checks answer from one record,
//! and one teardown releases it.
//!
//! The owner is the claiming task id, the key every check used before. A dead
//! owner's PCI record stays owned (quarantined) until its DMA teardown is
//! acknowledged, so no replacement can claim the function, its windows or its
//! vectors while hardware may still reach the old domain.
//!
//! The table is a fixed array: teardown and the ISR readers never allocate.

use crate::sync::Spinlock;
use abi::block_route::{DeviceKey, BLOCK_DEVICE_MMIO, BLOCK_DEVICE_PCI};

/// Records the table holds: every scanned or registered PCI function, every
/// VirtIO slot and the DWC2 aperture.
#[cfg(cellos_wide)]
pub(crate) const MAX_DEVICES: usize = 256;
#[cfg(not(cellos_wide))]
pub(crate) const MAX_DEVICES: usize = 16;

/// Windows one record may hold: a PCI function's six BARs.
const MAX_BARS: usize = 6;

/// What a record describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(target_arch = "riscv64", target_arch = "aarch64")),
    allow(dead_code)
)]
pub(crate) enum DeviceKind {
    /// A PCI function; its class code `class << 16 | subclass << 8 | prog_if`
    /// once the scan or a registration reported it.
    Pci { class: Option<u32> },
    /// A VirtIO MMIO transport slot.
    VirtioSlot,
    /// The DWC2 USB host aperture (the RPi3 LAN9514 sits behind it).
    Dwc2,
}

/// One device's ownership record.
#[derive(Clone, Copy)]
pub(crate) struct DeviceRecord {
    key: DeviceKey,
    kind: DeviceKind,
    /// The claiming task; `None` when free.
    owner: Option<usize>,
    /// Claimable windows `(base, len)`; `len == 0` is an empty slot.
    bars: [(usize, usize); MAX_BARS],
}

impl DeviceRecord {
    const fn new(key: DeviceKey, kind: DeviceKind) -> Self {
        Self {
            key,
            kind,
            owner: None,
            bars: [(0, 0); MAX_BARS],
        }
    }

    /// Whether `[base, base + len)` is one of this record's windows: a BAR or
    /// VirtIO slot claimed from its exact base, or any window inside the DWC2
    /// aperture.
    fn has_window(&self, base: usize, len: usize) -> bool {
        len != 0
            && self.bars.iter().any(|&(bar, bar_len)| {
                bar_len != 0
                    && match self.kind {
                        DeviceKind::Dwc2 => {
                            base >= bar
                                && base
                                    .checked_add(len)
                                    .is_some_and(|end| end <= bar + bar_len)
                        }
                        _ => base == bar && len <= bar_len,
                    }
            })
    }

    fn add_window(&mut self, base: usize, len: usize) -> bool {
        if self.bars.contains(&(base, len)) {
            return true;
        }
        match self.bars.iter_mut().find(|bar| bar.1 == 0) {
            Some(slot) => {
                *slot = (base, len);
                true
            }
            None => false,
        }
    }
}

struct Devices {
    records: [Option<DeviceRecord>; MAX_DEVICES],
}

impl Devices {
    fn find(&mut self, key: DeviceKey) -> Option<&mut DeviceRecord> {
        self.records
            .iter_mut()
            .flatten()
            .find(|record| record.key == key)
    }

    /// The record of `key`, created as `kind` when absent; `None` only when
    /// the table is full.
    fn find_or_insert(&mut self, key: DeviceKey, kind: DeviceKind) -> Option<&mut DeviceRecord> {
        let index = match self
            .records
            .iter()
            .position(|record| record.is_some_and(|record| record.key == key))
        {
            Some(index) => index,
            None => {
                let Some(index) = self.records.iter().position(Option::is_none) else {
                    log::warn!(
                        "[devices] table full ({MAX_DEVICES}); {} has no ownership record",
                        crate::dev::driver_cell::DeviceName(key)
                    );
                    return None;
                };
                self.records[index] = Some(DeviceRecord::new(key, kind));
                index
            }
        };
        self.records[index].as_mut()
    }
}

/// Every device record. Lock order: after `SCHEDULER`, the grant tables and
/// `ROLE_PUBLICATION`, before `RESOURCE_REGISTRY` and `PCIE_MSI_STATE`.
static DEVICES: Spinlock<Devices> = Spinlock::ranked(
    Devices {
        records: [None; MAX_DEVICES],
    },
    crate::sync::rank::DEVICES,
);

fn pci_key(bdf: u32) -> Option<DeviceKey> {
    DeviceKey::new(BLOCK_DEVICE_PCI, bdf as u64)
}

fn mmio_key(base: usize) -> Option<DeviceKey> {
    DeviceKey::new(BLOCK_DEVICE_MMIO, base as u64)
}

/// Record PCI function `bdf` with its class code (boot scan, 236).
pub(crate) fn register_pci_function(bdf: u32, class: u32) {
    let Some(key) = pci_key(bdf) else {
        return;
    };
    if let Some(record) = DEVICES
        .lock()
        .find_or_insert(key, DeviceKind::Pci { class: Some(class) })
    {
        record.kind = DeviceKind::Pci { class: Some(class) };
    }
}

/// Add BAR window `[base, base + len)` to PCI function `bdf`'s record (boot
/// scan, 235, 236). Only a bounded conventional BAR is admitted
/// ([`valid_pcie_bar_window`](crate::resource_registry::valid_pcie_bar_window)).
pub(crate) fn register_pci_bar(bdf: u32, base: usize, len: usize) {
    if !crate::resource_registry::valid_pcie_bar_window(base, len) {
        log::warn!(
            "[pcie] rejected invalid BAR window base={:#x} len={:#x}",
            base,
            len
        );
        return;
    }
    let Some(key) = pci_key(bdf) else {
        log::warn!("[pcie] BAR {base:#x}+{len:#x} names no function (bdf={bdf:#x}); not recorded");
        return;
    };
    let mut devices = DEVICES.lock();
    let Some(record) = devices.find_or_insert(key, DeviceKind::Pci { class: None }) else {
        return;
    };
    if !record.add_window(base, len) {
        log::warn!(
            "[pcie] {}: no BAR slot left for {base:#x}+{len:#x}",
            crate::dev::driver_cell::DeviceName(key)
        );
    }
}

/// Record the MMIO device at `base` (a VirtIO slot or the DWC2 aperture)
/// with its claimable window.
#[cfg_attr(
    not(any(target_arch = "riscv64", target_arch = "aarch64")),
    allow(dead_code)
)]
pub(crate) fn register_mmio_device(base: usize, len: usize, kind: DeviceKind) {
    let Some(key) = mmio_key(base) else {
        return;
    };
    if let Some(record) = DEVICES.lock().find_or_insert(key, kind) {
        record.add_window(base, len);
    }
}

/// PCI function `bdf`'s class code, once the scan or a registration reported it.
pub(crate) fn pci_class(bdf: u32) -> Option<u32> {
    let key = pci_key(bdf)?;
    match DEVICES.lock().find(key)?.kind {
        DeviceKind::Pci { class } => class,
        DeviceKind::VirtioSlot | DeviceKind::Dwc2 => None,
    }
}

/// Claim PCI function `bdf` for `tid` (the 418/424 handout). Repeated claims
/// by the owner are idempotent; a live or quarantined owner is never
/// displaced. A function without a record gets one.
pub(crate) fn claim_pci(bdf: u32, tid: usize) -> bool {
    let Some(key) = pci_key(bdf) else {
        return false;
    };
    let mut devices = DEVICES.lock();
    let Some(record) = devices.find_or_insert(key, DeviceKind::Pci { class: None }) else {
        return false;
    };
    match record.owner {
        Some(owner) => owner == tid,
        None => {
            record.owner = Some(tid);
            true
        }
    }
}

/// The task that owns PCI function `bdf`, if any.
pub(crate) fn pci_owner(bdf: u32) -> Option<usize> {
    let key = pci_key(bdf)?;
    DEVICES.lock().find(key).and_then(|record| record.owner)
}

/// Whether `tid` owns PCI function `bdf`: the gate of DMA grants (`GrantDma`,
/// DMA export), 434 and 435. The function's DMA domain is its owner's.
pub(crate) fn owns_pci(tid: usize, bdf: u32) -> bool {
    pci_owner(bdf) == Some(tid)
}

/// Every PCI function `tid` owns.
pub(crate) fn pci_functions_of(tid: usize) -> alloc::vec::Vec<u32> {
    DEVICES
        .lock()
        .records
        .iter()
        .flatten()
        .filter(|record| record.key.kind == BLOCK_DEVICE_PCI && record.owner == Some(tid))
        .map(|record| record.key.id as u32)
        .collect()
}

/// Free every PCI function `tid` owns. Only after its DMA teardown was
/// acknowledged.
pub(crate) fn release_pci_for(tid: usize) {
    for record in DEVICES.lock().records.iter_mut().flatten() {
        if record.key.kind == BLOCK_DEVICE_PCI && record.owner == Some(tid) {
            record.owner = None;
        }
    }
}

/// Whether `[base, base + len)` is a window of some device record (the
/// `RequestMmio` PCIe path).
pub(crate) fn is_device_window(base: usize, len: usize) -> bool {
    DEVICES
        .lock()
        .records
        .iter()
        .flatten()
        .any(|record| record.kind != DeviceKind::Dwc2 && record.has_window(base, len))
}

/// Record `tid` as the owner of the MMIO device whose window `[base, base +
/// len)` it was just granted.
pub(crate) fn note_mmio_claim(tid: usize, base: usize, len: usize) {
    for record in DEVICES.lock().records.iter_mut().flatten() {
        if record.key.kind == BLOCK_DEVICE_MMIO && record.has_window(base, len) {
            record.owner = Some(tid);
        }
    }
}

/// Free every MMIO device `tid` owns (its windows were released).
pub(crate) fn release_mmio_devices_for(tid: usize) {
    for record in DEVICES.lock().records.iter_mut().flatten() {
        if record.key.kind == BLOCK_DEVICE_MMIO && record.owner == Some(tid) {
            record.owner = None;
        }
    }
}

/// Free the MMIO devices `tid` owns through one of the released `windows`
/// (a capability revoke releases windows by class while the task lives).
pub(crate) fn release_mmio_devices_in(tid: usize, windows: &[(usize, usize)]) {
    for record in DEVICES.lock().records.iter_mut().flatten() {
        if record.key.kind == BLOCK_DEVICE_MMIO
            && record.owner == Some(tid)
            && windows
                .iter()
                .any(|&(base, len)| record.has_window(base, len))
        {
            record.owner = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claim_holds_until_its_owner_is_released() {
        const BDF: u32 = 0x00_FE_ED;
        const FIRST: usize = 0xA001;
        const COMPETING: usize = 0xA002;
        release_pci_for(FIRST);
        release_pci_for(COMPETING);
        assert!(claim_pci(BDF, FIRST));
        assert!(claim_pci(BDF, FIRST), "idempotent");
        assert!(!claim_pci(BDF, COMPETING));
        assert_eq!(pci_owner(BDF), Some(FIRST));
        assert_eq!(pci_functions_of(FIRST), alloc::vec![BDF]);

        release_pci_for(FIRST);
        assert_eq!(pci_owner(BDF), None);
        assert!(claim_pci(BDF, COMPETING));
        assert_eq!(pci_owner(BDF), Some(COMPETING));
        release_pci_for(COMPETING);
        assert!(!claim_pci(0x1_0000, FIRST), "not a BDF");
    }

    #[test]
    fn windows_are_matched_by_exact_base_except_inside_the_dwc2_aperture() {
        register_pci_bar(0x00_FE_E0, 0xFE00_0000, 0x2_0000);
        register_pci_bar(0x00_FE_E0, 0xFE00_0000, 0x3000);
        assert!(is_device_window(0xFE00_0000, 0x2_0000));
        assert!(is_device_window(0xFE00_0000, 0x1000));
        assert!(!is_device_window(0xFE00_1000, 0x1000), "interior base");
        assert!(!is_device_window(0xFE00_0000, 0x4_0000), "past the BAR");
        assert!(!is_device_window(0xFE00_0000, 0), "empty");

        let mut dwc2 = DeviceRecord::new(mmio_key(0x3F98_0000).unwrap(), DeviceKind::Dwc2);
        assert!(dwc2.add_window(0x3F98_0000, 0x1_0000));
        assert!(dwc2.has_window(0x3F98_4000, 0x1000));
        assert!(!dwc2.has_window(0x3F98_F000, 0x2000));
    }
}
