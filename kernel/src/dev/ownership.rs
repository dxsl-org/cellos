// SPDX-License-Identifier: MPL-2.0
//! Per-device ownership records (architecture remediation Phase 18).
//!
//! One [`DeviceRecord`] per device a Driver Cell can own: a PCI function, a
//! VirtIO MMIO slot, or the DWC2 USB aperture. Each names the device's owner,
//! its claimable windows, its interrupts and its DMA domain, so MMIO
//! (`RequestMmio`), IRQ (435, `WaitIrq`, the MSI and VirtIO ISRs), DMA
//! (`GrantDma`, DMA export) and route (416/417) checks answer from one record,
//! and one release ([`release_for`]) frees it.
//!
//! The owner is the claiming task id, the key every check used before. A dead
//! owner's records stay owned (quarantined) until its DMA teardown is
//! acknowledged and its windows are released, so no replacement can claim the
//! device, its windows or its vectors while hardware may still reach the old
//! domain.
//!
//! The table holds a record for every device the boot scan, the Platform Cell
//! or a claim names, however many there are: records are appended only at
//! registration and claim (boot and syscall context, charged to the kernel)
//! and never removed, so release, teardown (which may run in the timer/IPI
//! ISR) and every check only touch records in place and never allocate.

use crate::sync::Spinlock;
use abi::block_route::{DeviceKey, BLOCK_DEVICE_MMIO, BLOCK_DEVICE_PCI};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    /// The tid of the NIC route bound to this device, while it has a
    /// provider ([`set_nic_route`]); its lines are NIC lines while that tid
    /// also owns the device.
    nic_route: Option<usize>,
}

impl DeviceRecord {
    const fn new(key: DeviceKey, kind: DeviceKind) -> Self {
        Self {
            key,
            kind,
            owner: None,
            bars: [(0, 0); MAX_BARS],
            irq: None,
            nic_route: None,
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
    /// Append-only: a record's index never changes and none is removed.
    records: Vec<DeviceRecord>,
}

impl Devices {
    fn find(&mut self, key: DeviceKey) -> Option<&mut DeviceRecord> {
        self.records.iter_mut().find(|record| record.key == key)
    }

    /// The record of `key`, appended as `kind` when absent. Boot or syscall
    /// context only: the append may grow the table, charged to the kernel.
    /// `None` only when the heap refuses that growth.
    fn find_or_insert(&mut self, key: DeviceKey, kind: DeviceKind) -> Option<&mut DeviceRecord> {
        let index = match self.records.iter().position(|record| record.key == key) {
            Some(index) => index,
            None => {
                let _kernel = crate::memory::heap::ChargeScope::enter(
                    crate::memory::cell_quota::Charge::KERNEL,
                );
                #[cfg(feature = "p01-trace")]
                let _semantic = crate::memory::heap::p01_owner::creation(
                    None,
                    crate::memory::heap::p01_owner::DRIVER_SHARED,
                );
                self.records.try_reserve(1).ok()?;
                self.records.push(DeviceRecord::new(key, kind));
                self.records.len() - 1
            }
        };
        self.records.get_mut(index)
    }

    /// Rewrite [`NIC_LINES`] from the records: every line of a device whose
    /// owner is the provider of its NIC route. Called, under `DEVICES`, by
    /// every change of an owner, a line or a NIC route provider, so the
    /// words always describe the records. Never allocates (teardown may run
    /// in an ISR).
    fn publish_nic_lines(&self) {
        let mut words = [0u64; NIC_LINE_WORDS];
        for record in &self.records {
            let Some(lines) = record.irq else {
                continue;
            };
            if record.owner.is_some() && record.owner == record.nic_route {
                lines
                    .lines()
                    .for_each(|line| words[usize::from(line / 64)] |= 1 << (line % 64));
            }
        }
        for (word, bits) in NIC_LINES.iter().zip(words) {
            word.store(bits, Ordering::Release);
        }
    }
}

/// Every device record. Lock order: after `SCHEDULER`, the grant tables,
/// `ROLE_PUBLICATION`, `NIC_ROUTES` and `BLOCK_ROUTES` (route publication and
/// clearing check ownership under them), before `RESOURCE_REGISTRY` and
/// `PCIE_MSI_STATE`.
static DEVICES: Spinlock<Devices> = Spinlock::ranked(
    Devices {
        records: Vec::new(),
    },
    crate::sync::rank::DEVICES,
);

const NIC_LINE_WORDS: usize = 256 / 64;

/// The interrupt lines (x86_64 MSI vectors, riscv64/aarch64 VirtIO lines) of
/// the devices behind a live NIC route, one bit per `u8` line: written only
/// by [`Devices::publish_nic_lines`] under `DEVICES`, read lock-free by the
/// MSI and VirtIO ISRs ([`is_nic_line`]).
static NIC_LINES: [AtomicU64; NIC_LINE_WORDS] = [const { AtomicU64::new(0) }; NIC_LINE_WORDS];

fn pci_key(bdf: u32) -> Option<DeviceKey> {
    DeviceKey::new(BLOCK_DEVICE_PCI, bdf as u64)
}

fn mmio_key(base: usize) -> Option<DeviceKey> {
    DeviceKey::new(BLOCK_DEVICE_MMIO, base as u64)
}

/// Logged, after `DEVICES` is released, when the heap refused `key` a record.
fn no_record(key: DeviceKey) {
    log::warn!(
        "[devices] out of memory; {} has no ownership record",
        crate::dev::driver_cell::DeviceName(key)
    );
}

/// Record PCI function `bdf` with its class code (boot scan, 236).
pub(crate) fn register_pci_function(bdf: u32, class: u32) {
    let Some(key) = pci_key(bdf) else {
        return;
    };
    let kind = DeviceKind::Pci { class: Some(class) };
    let recorded = DEVICES
        .lock()
        .find_or_insert(key, kind)
        .map(|record| record.kind = kind)
        .is_some();
    if !recorded {
        no_record(key);
    }
}

/// Add BAR window `[base, base + len)` to PCI function `bdf`'s record (boot
/// scan, 236; 235 through [`register_platform_bar`]). Only a bounded
/// conventional BAR clear of RAM, the kernel image and ECAM is admitted
/// ([`pcie_bar_admitted`](crate::resource_registry::pcie_bar_admitted)).
pub(crate) fn register_pci_bar(bdf: u32, base: usize, len: usize) {
    if !crate::resource_registry::pcie_bar_admitted(base, len) {
        log::warn!(
            "[pcie] rejected BAR window base={:#x} len={:#x}: invalid, or it reaches RAM, the kernel image or ECAM",
            base,
            len
        );
        return;
    }
    let Some(key) = pci_key(bdf) else {
        log::warn!("[pcie] BAR {base:#x}+{len:#x} names no function (bdf={bdf:#x}); not recorded");
        return;
    };
    let added = DEVICES
        .lock()
        .find_or_insert(key, DeviceKind::Pci { class: None })
        .map(|record| record.add_window(base, len));
    match added {
        Some(true) => {}
        Some(false) => log::warn!(
            "[pcie] {}: no BAR slot left for {base:#x}+{len:#x}",
            crate::dev::driver_cell::DeviceName(key)
        ),
        None => no_record(key),
    }
}

/// A Platform Cell BAR registration (235): the window goes to `bdf`'s record
/// when it has one, and creates a record only when `answers()` reports that
/// the function answers in config space, so records track real functions.
/// `false`, with nothing recorded, otherwise. Records are never removed, so a
/// record found here is still there when the window is added.
pub(crate) fn register_platform_bar(
    bdf: u32,
    base: usize,
    len: usize,
    answers: impl FnOnce() -> bool,
) -> bool {
    let Some(key) = pci_key(bdf) else {
        return false;
    };
    let known = DEVICES.lock().find(key).is_some();
    if !known && !answers() {
        return false;
    }
    register_pci_bar(bdf, base, len);
    true
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
    let recorded = {
        let mut devices = DEVICES.lock();
        let recorded = devices
            .find_or_insert(key, kind)
            .map(|record| {
                record.add_window(base, len);
                record.irq = line.map(IrqLines::Line);
            })
            .is_some();
        devices.publish_nic_lines();
        recorded
    };
    if !recorded {
        no_record(key);
    }
}

/// Claim PCI function `bdf` for `tid` (the 418/424 handout). Repeated claims
/// by the owner are idempotent; a live or quarantined owner is never
/// displaced. A function without a record gets one.
pub(crate) fn claim_pci(bdf: u32, tid: usize) -> bool {
    let Some(key) = pci_key(bdf) else {
        return false;
    };
    let claimed = {
        let mut devices = DEVICES.lock();
        let claimed = devices
            .find_or_insert(key, DeviceKind::Pci { class: None })
            .map(|record| match record.owner {
                Some(owner) => owner == tid,
                None => {
                    record.owner = Some(tid);
                    true
                }
            });
        devices.publish_nic_lines();
        claimed
    };
    claimed.unwrap_or_else(|| {
        no_record(key);
        false
    })
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
        .filter(|record| record.owned_by(tid))
        .map(|record| (record.key, record.kind))
        .collect()
}

/// Whether `[base, base + len)` is a window of a PCI function or VirtIO slot
/// record (the `RequestMmio` PCIe path).
pub(crate) fn is_device_window(base: usize, len: usize) -> bool {
    DEVICES
        .lock()
        .records
        .iter()
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
    let claim = devices.claim_window(tid, base, len, dwc2);
    if matches!(claim, WindowClaim::Claimed(_)) {
        devices.publish_nic_lines();
    }
    claim
}

impl Devices {
    fn claim_window(&mut self, tid: usize, base: usize, len: usize, dwc2: bool) -> WindowClaim {
        let mut matching = self.records.iter_mut().filter(|record| {
            (record.kind == DeviceKind::Dwc2) == dwc2 && record.has_window(base, len)
        });
        let Some(record) = matching.next() else {
            return WindowClaim::NoDevice;
        };
        // Two records naming one window (a 235 registration repeating a
        // scanned BAR under another BDF) answer for whichever the caller owns.
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
}

/// Undo a [`WindowClaim::Claimed`] whose window grant failed: free `key`'s
/// record if `tid` still owns it.
pub(crate) fn unclaim(tid: usize, key: DeviceKey) {
    let mut devices = DEVICES.lock();
    if let Some(record) = devices.find(key).filter(|record| record.owned_by(tid)) {
        record.owner = None;
        devices.publish_nic_lines();
    }
}

/// Which of an owner's records [`release_for`] frees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Every record: the owner retired, its DMA teardown was acknowledged and
    /// its windows were released.
    Retired,
    /// The records `PcieDriverCap` claims (PCI functions through 418/424,
    /// VirtIO slots through `RequestMmio`): a runtime revoke of that
    /// capability, after its DMA teardown and the release of its windows.
    PcieDriver,
}

/// Free the records `tid` owns in `scope`: the one release of device
/// ownership. A freed record keeps its windows and fixed line for the next
/// claimant; MSI vectors were already taken out by `x86::release_msis`.
pub(crate) fn release_for(tid: usize, scope: Scope) {
    let mut devices = DEVICES.lock();
    for record in devices.records.iter_mut() {
        if record.owned_by(tid) && (scope == Scope::Retired || record.kind != DeviceKind::Dwc2) {
            record.owner = None;
        }
    }
    devices.publish_nic_lines();
}

/// Free the MMIO devices `tid` owns through one of the released `windows`
/// (a capability revoke releases windows by class while the task lives).
pub(crate) fn release_mmio_devices_in(tid: usize, windows: &[(usize, usize)]) {
    let mut devices = DEVICES.lock();
    for record in devices.records.iter_mut() {
        if record.key.kind == BLOCK_DEVICE_MMIO
            && record.owner == Some(tid)
            && windows
                .iter()
                .any(|&(base, len)| record.has_window(base, len))
        {
            record.owner = None;
        }
    }
    devices.publish_nic_lines();
}

/// Whether `irq` is a line of a device `tid` owns: an MSI vector 435
/// allocated for its function, or the fixed line of its VirtIO slot or DWC2
/// aperture. The `WaitIrq` gate on every architecture.
pub(crate) fn irq_owned_by(tid: usize, irq: u8) -> bool {
    DEVICES
        .lock()
        .records
        .iter()
        .any(|record| record.owned_by(tid) && record.holds_irq(irq))
}

/// Name `provider` the tid of the NIC route bound to device `key`, or `None`
/// when the route lost its provider: the route half of [`NIC_LINES`]. Called
/// under `NIC_ROUTES` by every bind and clear of a route provider. A key
/// without a record proves no line and is left alone.
pub(crate) fn set_nic_route(key: DeviceKey, provider: Option<usize>) {
    let mut devices = DEVICES.lock();
    if let Some(record) = devices.find(key) {
        record.nic_route = provider;
        devices.publish_nic_lines();
    }
}

/// Whether `irq` is a line of the device of a live NIC route, owned by that
/// route's provider: the proof the MSI and VirtIO ISRs signal `NET_RX` on.
/// Lock-free: one atomic load.
pub(crate) fn is_nic_line(irq: u8) -> bool {
    NIC_LINES[usize::from(irq / 64)].load(Ordering::Acquire) & (1 << (irq % 64)) != 0
}

/// Unbind every device from its NIC route; host tests only, under
/// `TEST_STATE_LOCK`, with the route table reset.
#[cfg(test)]
pub(crate) fn test_clear_nic_routes() {
    let mut devices = DEVICES.lock();
    for record in devices.records.iter_mut() {
        record.nic_route = None;
    }
    devices.publish_nic_lines();
}

/// Every line of every device `tid` owns: the snapshot a
/// `WaitCompletion(IRQ)` waits on.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub(crate) fn lines_owned_by(tid: usize) -> crate::dev::irq_wait::IrqLineSet {
    let mut set = crate::dev::irq_wait::IrqLineSet::EMPTY;
    for record in DEVICES.lock().records.iter() {
        if let Some(lines) = record.irq.filter(|_| record.owned_by(tid)) {
            lines.lines().for_each(|line| set.insert(line));
        }
    }
    set
}

/// Consume `irq`'s pending flag only while `tid` still owns the line.
///
/// The ownership check and the swap share one `DEVICES` hold, and
/// `x86::release_msis` drops the flags under it too, so a revoked (and possibly
/// re-allocated) line is never consumed by its former owner.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub(crate) fn take_owned_line(tid: usize, irq: u8) -> bool {
    let devices = DEVICES.lock();
    let owned = devices
        .records
        .iter()
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
        .any(|record| record.owned_by(tid) && record.holds_irq(irq))
    {
        crate::dev::irq_wait::restore_pending(irq);
    }
}

/// The x86_64 MSI items (`with_owned_msi`, `release_msis`), compiled as this
/// module's child so they keep reading the private records.
#[cfg(target_arch = "x86_64")]
#[path = "../arch/x86_64/dev/ownership.rs"]
pub(crate) mod x86;

/// Make `tid` the owner of a VirtIO slot record at `base` with line `line`,
/// displacing any owner; host tests only, under `TEST_STATE_LOCK`.
#[cfg(test)]
pub(crate) fn test_own_mmio(tid: usize, base: usize, line: u8) {
    let mut devices = DEVICES.lock();
    let record = devices
        .find_or_insert(mmio_key(base).expect("valid base"), DeviceKind::VirtioSlot)
        .expect("heap for a test record");
    record.add_window(base, 0x200);
    record.owner = Some(tid);
    record.irq = Some(IrqLines::Line(line));
    devices.publish_nic_lines();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claim_holds_until_its_owner_is_released() {
        const BDF: u32 = 0x00_FE_ED;
        const FIRST: usize = 0xA001;
        const COMPETING: usize = 0xA002;
        let _guard = crate::TEST_STATE_LOCK.lock();
        release_for(FIRST, Scope::Retired);
        release_for(COMPETING, Scope::Retired);
        assert!(claim_pci(BDF, FIRST));
        assert!(claim_pci(BDF, FIRST), "idempotent");
        assert!(!claim_pci(BDF, COMPETING));
        assert_eq!(pci_owner(BDF), Some(FIRST));
        assert!(devices_of(FIRST)
            .iter()
            .any(|&(key, _)| key == pci_key(BDF).unwrap()));

        release_for(FIRST, Scope::PcieDriver);
        assert_eq!(pci_owner(BDF), None);
        assert!(claim_pci(BDF, COMPETING));
        assert_eq!(pci_owner(BDF), Some(COMPETING));
        release_for(COMPETING, Scope::Retired);
        assert!(!claim_pci(0x1_0000, FIRST), "not a BDF");
    }

    /// A machine with more functions than the old fixed table (256) loses
    /// none: each keeps its record and class, and each is claimable.
    #[test]
    fn every_registered_function_keeps_a_claimable_record() {
        const TID: usize = 0xA0C0;
        const FUNCTIONS: u32 = 600;
        const FIRST_BDF: u32 = 0xB000;
        const CLASS: u32 = 0x02_00_00;
        let _guard = crate::TEST_STATE_LOCK.lock();
        for bdf in FIRST_BDF..FIRST_BDF + FUNCTIONS {
            register_pci_function(bdf, CLASS);
        }
        for bdf in FIRST_BDF..FIRST_BDF + FUNCTIONS {
            assert!(claim_pci(bdf, TID), "{bdf:#x} claimable");
        }
        let owned = devices_of(TID);
        assert_eq!(owned.len(), FUNCTIONS as usize);
        assert!(owned
            .iter()
            .all(|&(_, kind)| kind == DeviceKind::Pci { class: Some(CLASS) }));
        release_for(TID, Scope::Retired);
        assert!(devices_of(TID).is_empty());
    }

    /// A repeated function or BAR registration (a Platform Cell rescan, 235
    /// after 236) adds no record and no window.
    #[test]
    fn repeated_registrations_add_nothing() {
        const BDF: u32 = 0xB300;
        let key = pci_key(BDF).unwrap();
        let records_of = || {
            let devices = DEVICES.lock();
            let mut records = devices.records.iter().filter(|record| record.key == key);
            (
                records.clone().count(),
                records.next().map(|record| record.bars),
            )
        };
        let _guard = crate::TEST_STATE_LOCK.lock();
        register_pci_function(BDF, 0x01_08_02);
        register_pci_bar(BDF, 0xFD00_0000, 0x4000);
        let (count, windows) = records_of();
        assert_eq!(count, 1);
        for _ in 0..3 {
            register_pci_function(BDF, 0x01_08_02);
            register_pci_bar(BDF, 0xFD00_0000, 0x4000);
        }
        assert_eq!(records_of(), (1, windows));
    }

    /// 235 creates a record only for a function that answers; a function
    /// that already has one gains the window without a config-space probe.
    #[test]
    fn a_platform_bar_records_only_a_function_that_answers() {
        const SILENT: u32 = 0xB410;
        const ANSWERS: u32 = 0xB418;
        const KNOWN: u32 = 0xB420;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let has_record = |bdf| DEVICES.lock().find(pci_key(bdf).unwrap()).is_some();
        assert!(!register_platform_bar(SILENT, 0xFD10_0000, 0x1000, || {
            false
        }));
        assert!(!has_record(SILENT), "a silent function gets no record");
        assert!(register_platform_bar(ANSWERS, 0xFD11_0000, 0x1000, || true));
        assert!(has_record(ANSWERS));
        assert!(is_device_window(0xFD11_0000, 0x1000));

        register_pci_function(KNOWN, 0x02_00_00);
        let probed = core::cell::Cell::new(false);
        assert!(register_platform_bar(KNOWN, 0xFD12_0000, 0x1000, || {
            probed.set(true);
            false
        }));
        assert!(!probed.get(), "a known function is not probed");
        assert!(is_device_window(0xFD12_0000, 0x1000));
        assert!(
            !register_platform_bar(0x1_0000, 0xFD13_0000, 0x1000, || true),
            "not a BDF"
        );
    }

    /// A `PcieDriverCap` revoke frees what that capability claimed and
    /// leaves the DWC2 aperture, which `UsbDriverCap` claimed.
    #[test]
    fn a_pcie_driver_release_keeps_the_dwc2_aperture() {
        const TID: usize = 0xA0D2;
        const DWC2: usize = 0x7A10_0000;
        const SLOT: usize = 0x7A20_0000;
        let _guard = crate::TEST_STATE_LOCK.lock();
        register_mmio_device(DWC2, 0x1_0000, DeviceKind::Dwc2, Some(0x2C));
        assert_eq!(
            claim_window(TID, DWC2 + 0x1000, 0x1000, true),
            WindowClaim::Claimed(mmio_key(DWC2).unwrap())
        );
        test_own_mmio(TID, SLOT, 0x2D);
        release_for(TID, Scope::PcieDriver);
        assert!(irq_owned_by(TID, 0x2C), "DWC2 kept");
        assert!(!irq_owned_by(TID, 0x2D), "VirtIO slot freed");
        release_for(TID, Scope::Retired);
        assert!(!irq_owned_by(TID, 0x2C));
        assert_eq!(
            claim_window(TID + 1, DWC2, 0x1000, true),
            WindowClaim::Claimed(mmio_key(DWC2).unwrap()),
            "free for the next claimant"
        );
        release_for(TID + 1, Scope::Retired);
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
        assert!(lines_owned_by(0x7A01).contains(0x2A));
        release_for(0x7A01, Scope::Retired);
        assert!(!irq_owned_by(0x7A01, 0x2A), "released with the device");
    }

    /// The lock-free NIC line set follows every input under `DEVICES`: a line
    /// is set only while its device's owner is its NIC route's provider, and
    /// clears with the route or the owner (the x86_64 MSI allocation:
    /// `x86::tests`).
    #[test]
    fn nic_lines_follow_the_route_the_owner_and_the_lines() {
        const SLOT: usize = 0x7B00_0000;
        const LINE: u8 = 0xC3;
        const TID: usize = 0x7B01;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let key = mmio_key(SLOT).unwrap();
        test_own_mmio(TID, SLOT, LINE);
        assert!(!is_nic_line(LINE), "owned, no route");
        set_nic_route(key, Some(TID + 1));
        assert!(!is_nic_line(LINE), "route of a task that does not own it");
        set_nic_route(key, Some(TID));
        assert!(is_nic_line(LINE));
        assert!(!is_nic_line(LINE - 1) && !is_nic_line(LINE + 1));
        set_nic_route(key, None);
        assert!(!is_nic_line(LINE), "route lost its provider");
        set_nic_route(key, Some(TID));
        release_for(TID, Scope::PcieDriver);
        assert!(!is_nic_line(LINE), "device released");
        assert_eq!(
            claim_window(TID, SLOT, 0x200, false),
            WindowClaim::Claimed(key)
        );
        assert!(is_nic_line(LINE), "reclaimed by the route's provider");
        set_nic_route(key, None);
        release_for(TID, Scope::Retired);
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
