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

/// The interrupt lines a record holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IrqLines {
    /// The fixed line of a VirtIO slot or of the DWC2 aperture.
    #[cfg_attr(
        not(any(target_arch = "riscv64", target_arch = "aarch64", test)),
        allow(dead_code)
    )]
    Line(u8),
    /// The MSI/MSI-X vectors 435 allocated to the owner.
    #[cfg(target_arch = "x86_64")]
    Msi(abi::pcie_function::PcieIrqAlloc),
}

impl IrqLines {
    fn contains(self, irq: u8) -> bool {
        match self {
            Self::Line(line) => line == irq,
            #[cfg(target_arch = "x86_64")]
            Self::Msi(alloc) => irq >= alloc.first_irq && (irq - alloc.first_irq) < alloc.count,
        }
    }

    /// Every line, in ascending order.
    fn lines(self) -> impl Iterator<Item = u8> {
        let (first, count) = match self {
            Self::Line(line) => (line, 1),
            #[cfg(target_arch = "x86_64")]
            Self::Msi(alloc) => (alloc.first_irq, alloc.count),
        };
        (0..count).map(move |offset| first.wrapping_add(offset))
    }
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
    /// The device's interrupts: a VirtIO slot's or the DWC2 line, fixed at
    /// registration; on x86_64 a PCI function's MSI vectors once 435
    /// allocated them to its owner.
    irq: Option<IrqLines>,
}

impl DeviceRecord {
    const fn new(key: DeviceKey, kind: DeviceKind) -> Self {
        Self {
            key,
            kind,
            owner: None,
            bars: [(0, 0); MAX_BARS],
            irq: None,
        }
    }

    fn owned_by(&self, tid: usize) -> bool {
        self.owner == Some(tid)
    }

    fn holds_irq(&self, irq: u8) -> bool {
        self.irq.is_some_and(|lines| lines.contains(irq))
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
/// with its claimable window and its fixed interrupt line, if it has one.
#[cfg_attr(
    not(any(target_arch = "riscv64", target_arch = "aarch64")),
    allow(dead_code)
)]
pub(crate) fn register_mmio_device(base: usize, len: usize, kind: DeviceKind, line: Option<u8>) {
    let Some(key) = mmio_key(base) else {
        return;
    };
    if let Some(record) = DEVICES.lock().find_or_insert(key, kind) {
        record.add_window(base, len);
        record.irq = line.map(IrqLines::Line);
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

/// Whether `tid` owns device `key`: a PCI function it claimed through 418/424,
/// or a VirtIO slot or the DWC2 aperture it claimed through `RequestMmio`.
/// The ownership proof of a block route (416).
pub(crate) fn owns(tid: usize, key: DeviceKey) -> bool {
    DEVICES
        .lock()
        .find(key)
        .is_some_and(|record| record.owned_by(tid))
}

/// Every device `tid` owns, with its kind.
pub(crate) fn devices_of(tid: usize) -> alloc::vec::Vec<(DeviceKey, DeviceKind)> {
    DEVICES
        .lock()
        .records
        .iter()
        .flatten()
        .filter(|record| record.owned_by(tid))
        .map(|record| (record.key, record.kind))
        .collect()
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

/// Whether `[base, base + len)` is a window of a PCI function or VirtIO slot
/// record (the `RequestMmio` PCIe path).
pub(crate) fn is_device_window(base: usize, len: usize) -> bool {
    DEVICES
        .lock()
        .records
        .iter()
        .flatten()
        .any(|record| record.kind != DeviceKind::Dwc2 && record.has_window(base, len))
}

/// What a `RequestMmio` claim found in the device records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowClaim {
    /// No record of the path holds the window.
    NoDevice,
    /// The caller already owns the window's device.
    Owned,
    /// The caller now owns the window's MMIO device (its first claim of a
    /// VirtIO slot or the DWC2 aperture); [`unclaim`] undoes it when the
    /// window grant then fails.
    Claimed(DeviceKey),
    /// The window is a BAR of a PCI function the caller has not claimed
    /// through 418/424 (free, or another task's).
    NotOwner,
    /// The window's MMIO device is another task's, live or awaiting its
    /// teardown.
    Held,
}

/// Claim the device whose window `[base, base + len)` is, for `tid`, before
/// the window is granted: a PCI function's BAR only for its owner, a VirtIO
/// slot (`dwc2 == false`) or the DWC2 aperture (`dwc2 == true`) for its owner
/// or, when free, for its first claimant.
pub(crate) fn claim_window(tid: usize, base: usize, len: usize, dwc2: bool) -> WindowClaim {
    let mut devices = DEVICES.lock();
    let mut matching =
        devices.records.iter_mut().flatten().filter(|record| {
            (record.kind == DeviceKind::Dwc2) == dwc2 && record.has_window(base, len)
        });
    let Some(record) = matching.next() else {
        return WindowClaim::NoDevice;
    };
    // Two records naming one window (a 235 registration repeating a scanned
    // BAR under another BDF) answer for whichever the caller owns.
    if !record.owned_by(tid) && matching.any(|other| other.owned_by(tid)) {
        return WindowClaim::Owned;
    }
    match (record.owner, record.kind) {
        (Some(owner), _) if owner == tid => WindowClaim::Owned,
        (_, DeviceKind::Pci { .. }) => WindowClaim::NotOwner,
        (Some(_), _) => WindowClaim::Held,
        (None, _) => {
            record.owner = Some(tid);
            WindowClaim::Claimed(record.key)
        }
    }
}

/// Undo a [`WindowClaim::Claimed`] whose window grant failed: free `key`'s
/// record if `tid` still owns it.
pub(crate) fn unclaim(tid: usize, key: DeviceKey) {
    if let Some(record) = DEVICES.lock().find(key) {
        if record.owned_by(tid) {
            record.owner = None;
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

/// Whether `irq` is a line of a device `tid` owns: an MSI vector 435
/// allocated for its function, or the fixed line of its VirtIO slot or DWC2
/// aperture. The `WaitIrq` gate on every architecture.
pub(crate) fn irq_owned_by(tid: usize, irq: u8) -> bool {
    DEVICES
        .lock()
        .records
        .iter()
        .flatten()
        .any(|record| record.owned_by(tid) && record.holds_irq(irq))
}

/// Whether `irq` is a line of device `key` and `tid` owns it: the per-route
/// proof of a NIC route's interrupts (MSI and VirtIO ISRs).
pub(crate) fn irq_owned_for(tid: usize, irq: u8, key: DeviceKey) -> bool {
    DEVICES
        .lock()
        .find(key)
        .is_some_and(|record| record.owned_by(tid) && record.holds_irq(irq))
}

/// Every line of every device `tid` owns: the snapshot a
/// `WaitCompletion(IRQ)` waits on.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub(crate) fn lines_owned_by(tid: usize) -> crate::dev::irq_wait::IrqLineSet {
    let mut set = crate::dev::irq_wait::IrqLineSet::EMPTY;
    for record in DEVICES.lock().records.iter().flatten() {
        if let Some(lines) = record.irq.filter(|_| record.owned_by(tid)) {
            lines.lines().for_each(|line| set.insert(line));
        }
    }
    set
}

/// Consume `irq`'s pending flag only while `tid` still owns the line.
///
/// The ownership check and the swap share one `DEVICES` hold, and
/// [`release_msis`] drops the flags under it too, so a revoked (and possibly
/// re-allocated) line is never consumed by its former owner.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub(crate) fn take_owned_line(tid: usize, irq: u8) -> bool {
    let devices = DEVICES.lock();
    let owned = devices
        .records
        .iter()
        .flatten()
        .any(|record| record.owned_by(tid) && record.holds_irq(irq));
    owned && crate::dev::irq_wait::take_pending(irq)
}

/// Re-pend a line taken by [`take_owned_line`] whose record could not be
/// delivered; a line revoked meanwhile is left alone.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub(crate) fn repend_owned_line(tid: usize, irq: u8) {
    let devices = DEVICES.lock();
    if devices
        .records
        .iter()
        .flatten()
        .any(|record| record.owned_by(tid) && record.holds_irq(irq))
    {
        crate::dev::irq_wait::restore_pending(irq);
    }
}

/// Run `allocate` on the MSI allocation of PCI function `bdf` while `tid`
/// owns it: the slot holds the existing allocation, if any, and keeps what
/// `allocate` leaves in it. `None` when `tid` does not own `bdf`. Lock order
/// `DEVICES -> PCIE_MSI_STATE`.
#[cfg(target_arch = "x86_64")]
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
    Some(result)
}

/// Take every MSI allocation of the PCI functions `tid` owns out of their
/// records, handing each to `free` and dropping its lines' pending flags,
/// under one `DEVICES` hold. Returns how many. Call only after the functions
/// were quiesced.
#[cfg(target_arch = "x86_64")]
pub(crate) fn release_msis(
    tid: usize,
    mut free: impl FnMut(abi::pcie_function::PcieIrqAlloc),
) -> usize {
    let mut released = 0;
    for record in DEVICES.lock().records.iter_mut().flatten() {
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
    released
}

/// Make `tid` the owner of a VirtIO slot record at `base` with line `line`,
/// displacing any owner; host tests only, under `TEST_STATE_LOCK`.
#[cfg(test)]
pub(crate) fn test_own_mmio(tid: usize, base: usize, line: u8) {
    let mut devices = DEVICES.lock();
    let record = devices
        .find_or_insert(mmio_key(base).expect("valid base"), DeviceKind::VirtioSlot)
        .expect("room for a test record");
    record.add_window(base, 0x200);
    record.owner = Some(tid);
    record.irq = Some(IrqLines::Line(line));
}

/// Make `tid` the owner of PCI function `bdf` holding `msi`, displacing any
/// owner; host tests only, under `TEST_STATE_LOCK`.
#[cfg(all(test, target_arch = "x86_64"))]
pub(crate) fn test_own_msi(tid: usize, bdf: u32, msi: abi::pcie_function::PcieIrqAlloc) {
    let mut devices = DEVICES.lock();
    let record = devices
        .find_or_insert(
            pci_key(bdf).expect("valid BDF"),
            DeviceKind::Pci { class: None },
        )
        .expect("room for a test record");
    record.owner = Some(tid);
    record.irq = Some(IrqLines::Msi(msi));
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

    /// X2: a fixed line answers only for the owner of its device, on every
    /// architecture, and goes with the device.
    #[test]
    fn a_line_belongs_only_to_the_owner_of_its_device() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        test_own_mmio(0x7A01, 0x7A00_0000, 0x2A);
        assert!(irq_owned_by(0x7A01, 0x2A));
        assert!(!irq_owned_by(0x7A02, 0x2A), "another driver's line");
        assert!(!irq_owned_by(0x7A01, 0x2B));
        assert!(irq_owned_for(0x7A01, 0x2A, mmio_key(0x7A00_0000).unwrap()));
        assert!(lines_owned_by(0x7A01).contains(0x2A));
        release_mmio_devices_for(0x7A01);
        assert!(!irq_owned_by(0x7A01, 0x2A), "released with the device");
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
