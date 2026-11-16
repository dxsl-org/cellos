//! vmxnet3 register map and the command handshake.
//!
//! Offsets and command codes follow FreeBSD's `sys/dev/vmware/vmxnet3/
//! if_vmxreg.h` (ISC licence, attribution in `NOTICE`; H-S3-14). The device
//! has two register BARs:
//!
//! * BAR0 "PT" — per-vector interrupt masks and the TX/RX producer doorbells;
//! * BAR1 "VD" — revision selection, the driver-shared address, the command
//!   register, the MAC registers and the event-acknowledge register.
//!
//! A command is a 32-bit write of its code to `VD_CMD`; a query command's
//! result is the following read of `VD_CMD`.
//!
//! [`RegisterFile`] (`nic_driver::regs`) is the only path to the device:
//! `MmioRegion` implements it for the cell, and the host tests implement it
//! with a fake.

pub(crate) use nic_driver::regs::RegisterFile;
use types::ViResult;

// ── BAR0 (PT) ────────────────────────────────────────────────────────────────

/// Interrupt mask of vector `vector`: 1 masks, 0 unmasks.
pub(crate) const fn pt_imask(vector: u8) -> usize {
    vector as usize * 8
}
/// TX producer doorbell of queue 0 (`TXH`).
pub(crate) const PT_TXPROD: usize = 0x600;
/// RX ring 0 / ring 1 producer of queue 0 (`RXH1`/`RXH2`).
pub(crate) const PT_RXPROD: usize = 0x800;
pub(crate) const PT_RXPROD2: usize = 0xA00;
/// Smallest BAR0 that holds every PT register this cell writes.
pub(crate) const PT_MIN: usize = PT_RXPROD2 + 4;

// ── BAR1 (VD) ────────────────────────────────────────────────────────────────

pub(crate) const VD_VRRS: usize = 0x000;
pub(crate) const VD_UVRS: usize = 0x008;
pub(crate) const VD_DSL: usize = 0x010;
pub(crate) const VD_DSH: usize = 0x018;
pub(crate) const VD_CMD: usize = 0x020;
pub(crate) const VD_MACL: usize = 0x028;
pub(crate) const VD_MACH: usize = 0x030;
/// Event-cause acknowledge: writing bits clears them in `DriverShared.event`.
pub(crate) const VD_ECR: usize = 0x040;
/// Smallest BAR1 that holds every VD register this cell touches.
pub(crate) const VD_MIN: usize = VD_ECR + 4;

/// Revision 1 of the device interface (`VRRS`) and of UPT (`UVRS`).
pub(crate) const REVISION_1: u32 = 1;

// ── Commands ─────────────────────────────────────────────────────────────────

/// `ENABLE` in FreeBSD's naming: activate the device from `DriverShared`.
pub(crate) const CMD_ACTIVATE_DEV: u32 = 0xCAFE_0000;
pub(crate) const CMD_QUIESCE_DEV: u32 = 0xCAFE_0001;
pub(crate) const CMD_RESET_DEV: u32 = 0xCAFE_0002;
/// `SET_RXMODE`: apply `DriverShared.rxmode`.
pub(crate) const CMD_UPDATE_RX_MODE: u32 = 0xCAFE_0003;
/// `SET_FILTER`: apply the multicast table.
pub(crate) const CMD_UPDATE_MAC_FILTERS: u32 = 0xCAFE_0004;
/// `VLAN_FILTER`: apply `DriverShared.vlan_filter`.
pub(crate) const CMD_UPDATE_VLAN_FILTERS: u32 = 0xCAFE_0005;
/// Query: bit 0 link up, bits 31:16 speed in Mb/s.
pub(crate) const CMD_GET_LINK: u32 = 0xF00D_0002;
/// Queries: the permanent MAC, low four and high two bytes.
pub(crate) const CMD_GET_PERM_MAC_LO: u32 = 0xF00D_0003;
pub(crate) const CMD_GET_PERM_MAC_HI: u32 = 0xF00D_0004;

pub(crate) const LINK_UP: u32 = 1 << 0;

/// Event causes in `DriverShared.event` (acknowledged through [`VD_ECR`]).
pub(crate) const EVENT_RQERROR: u32 = 0x01;
pub(crate) const EVENT_TQERROR: u32 = 0x02;

/// Why the device cannot be driven.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    /// `VRRS` does not offer revision 1 (the value read).
    Revision(u32),
    /// `UVRS` does not offer UPT version 1 (the value read).
    Upt(u32),
}

/// Select device revision 1 and UPT version 1, the only ones this cell speaks.
/// An all-ones read is a BAR that does not decode, never an offer.
pub(crate) fn negotiate(vd: &impl RegisterFile) -> ViResult<Result<(), Refusal>> {
    let revisions = vd.rd32(VD_VRRS)?;
    if revisions == u32::MAX || revisions & REVISION_1 == 0 {
        return Ok(Err(Refusal::Revision(revisions)));
    }
    vd.wr32(VD_VRRS, REVISION_1)?;
    let upt = vd.rd32(VD_UVRS)?;
    if upt == u32::MAX || upt & REVISION_1 == 0 {
        return Ok(Err(Refusal::Upt(upt)));
    }
    vd.wr32(VD_UVRS, REVISION_1)?;
    Ok(Ok(()))
}

/// Issue a query command and return its result.
pub(crate) fn query(vd: &impl RegisterFile, command: u32) -> ViResult<u32> {
    vd.wr32(VD_CMD, command)?;
    vd.rd32(VD_CMD)
}

/// The permanent station address.
pub(crate) fn permanent_mac(vd: &impl RegisterFile) -> ViResult<[u8; 6]> {
    let low = query(vd, CMD_GET_PERM_MAC_LO)?.to_le_bytes();
    let high = query(vd, CMD_GET_PERM_MAC_HI)?.to_le_bytes();
    Ok([low[0], low[1], low[2], low[3], high[0], high[1]])
}

/// Program `mac` as the current station address (`MACL`, then `MACH`).
pub(crate) fn set_mac(vd: &impl RegisterFile, mac: [u8; 6]) -> ViResult<()> {
    vd.wr32(
        VD_MACL,
        u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]),
    )?;
    vd.wr32(VD_MACH, u32::from_le_bytes([mac[4], mac[5], 0, 0]))
}

/// Whether `mac` is a usable unicast station address.
pub(crate) fn mac_usable(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac[0] & 1 == 0
}

#[cfg(test)]
#[path = "regs_tests.rs"]
mod tests;
