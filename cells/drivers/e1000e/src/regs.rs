//! 82574 register map and the register-file seam.
//!
//! Offsets and bits are from the Intel 82574 GbE Controller Family Datasheet,
//! chapter 10 (register descriptions). Only the registers this cell touches are
//! named. Where the 82574 differs from the 82540EM that `/bin/e1000` drives:
//!
//! * `EERD` carries the word address at bits 15:2 and signals done at bit 1
//!   (8254x: address at 15:8, done at bit 4);
//! * MSI-X causes are routed per queue through `IVAR` and only reach a vector
//!   with `CTRL_EXT.PBA_SUPPORT` set;
//! * `ICR` reports the per-queue MSI-X causes `RXQ0`/`TXQ0` and the summary
//!   `OTHER` cause (bits 20, 22, 24).
//!
//! [`RegisterFile`] (`nic_driver::regs`) is the only path to the device:
//! `MmioRegion` implements it for the cell, and the host tests implement it
//! with a fake register file.

use types::{ViError, ViResult};

// ── Offsets (BAR0, 128 KiB) ──────────────────────────────────────────────────

pub(crate) const CTRL: usize = 0x0_0000;
pub(crate) const STATUS: usize = 0x0_0008;
pub(crate) const EERD: usize = 0x0_0014;
pub(crate) const CTRL_EXT: usize = 0x0_0018;
pub(crate) const MDIC: usize = 0x0_0020;
pub(crate) const ICR: usize = 0x0_00C0;
pub(crate) const IMS: usize = 0x0_00D0;
pub(crate) const IMC: usize = 0x0_00D8;
pub(crate) const EIAC: usize = 0x0_00DC;
pub(crate) const IVAR: usize = 0x0_00E4;
pub(crate) const RCTL: usize = 0x0_0100;
pub(crate) const TCTL: usize = 0x0_0400;
pub(crate) const TIPG: usize = 0x0_0410;
pub(crate) const RDBAL: usize = 0x0_2800;
pub(crate) const RDBAH: usize = 0x0_2804;
pub(crate) const RDLEN: usize = 0x0_2808;
pub(crate) const RDH: usize = 0x0_2810;
pub(crate) const RDT: usize = 0x0_2818;
pub(crate) const TDBAL: usize = 0x0_3800;
pub(crate) const TDBAH: usize = 0x0_3804;
pub(crate) const TDLEN: usize = 0x0_3808;
pub(crate) const TDH: usize = 0x0_3810;
pub(crate) const TDT: usize = 0x0_3818;
pub(crate) const MTA: usize = 0x0_5200;
pub(crate) const MTA_ENTRIES: usize = 128;
pub(crate) const RAL0: usize = 0x0_5400;
pub(crate) const RAH0: usize = 0x0_5404;

/// Highest register this cell touches (`RAH0`) plus one word.
pub(crate) const MMIO_MIN: usize = RAH0 + 4;

// ── Bits ─────────────────────────────────────────────────────────────────────

pub(crate) const CTRL_GIO_MASTER_DISABLE: u32 = 1 << 2;
pub(crate) const CTRL_ASDE: u32 = 1 << 5;
pub(crate) const CTRL_SLU: u32 = 1 << 6;
pub(crate) const CTRL_RST: u32 = 1 << 26;
pub(crate) const STATUS_LU: u32 = 1 << 1;
pub(crate) const STATUS_GIO_MASTER_ENABLE: u32 = 1 << 19;
pub(crate) const EERD_START: u32 = 1 << 0;
pub(crate) const EERD_DONE: u32 = 1 << 1;
pub(crate) const EERD_ADDR_SHIFT: u32 = 2;
pub(crate) const EERD_DATA_SHIFT: u32 = 16;
pub(crate) const CTRL_EXT_PBA_SUPPORT: u32 = 1 << 31;
pub(crate) const ICR_LSC: u32 = 1 << 2;
pub(crate) const ICR_TXQ0: u32 = 1 << 22;
pub(crate) const ICR_OTHER: u32 = 1 << 24;
pub(crate) const RCTL_EN: u32 = 1 << 1;
pub(crate) const RCTL_UPE: u32 = 1 << 3;
pub(crate) const RCTL_MPE: u32 = 1 << 4;
pub(crate) const RCTL_BAM: u32 = 1 << 15;
pub(crate) const RCTL_SECRC: u32 = 1 << 26;
pub(crate) const TCTL_EN: u32 = 1 << 1;
pub(crate) const TCTL_PSP: u32 = 1 << 3;
pub(crate) const TCTL_CT: u32 = 0x0F << 4;
pub(crate) const TCTL_COLD: u32 = 0x40 << 12;
/// IEEE 802.3 IPG for copper (IPGT 8, IPGR1 8, IPGR2 6).
pub(crate) const TIPG_COPPER: u32 = 0x0060_200A;
pub(crate) const RAH_AV: u32 = 1 << 31;

pub(crate) const IVAR_VALID: u32 = 1 << 3;
const IVAR_RXQ0_SHIFT: u32 = 0;
const IVAR_TXQ0_SHIFT: u32 = 8;
const IVAR_OTHER_SHIFT: u32 = 16;

/// NVM read poll budget.
const NVM_POLLS: u32 = 100_000;

/// The cell's only path to device registers.
pub(crate) use nic_driver::regs::RegisterFile;

/// The `EERD` command that reads NVM word `word` (82574 layout).
pub(crate) const fn eerd_command(word: u16) -> u32 {
    ((word as u32) << EERD_ADDR_SHIFT) | EERD_START
}

/// Read one 16-bit NVM word through `EERD`.
pub(crate) fn nvm_read(regs: &impl RegisterFile, word: u16) -> ViResult<u16> {
    regs.wr32(EERD, eerd_command(word))?;
    for _ in 0..NVM_POLLS {
        let value = regs.rd32(EERD)?;
        if value & EERD_DONE != 0 {
            return Ok((value >> EERD_DATA_SHIFT) as u16);
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

/// `IVAR` for `granted` MSI-X entries (1..=32): RX → 0, TX → 1, other → 2,
/// each clamped to the last granted entry.
pub(crate) const fn ivar(granted: u8) -> u32 {
    let last = if granted == 0 { 0 } else { granted as u32 - 1 };
    let tx = if 1 < last { 1 } else { last };
    let other = if 2 < last { 2 } else { last };
    (IVAR_VALID << IVAR_RXQ0_SHIFT)
        | ((IVAR_VALID | tx) << IVAR_TXQ0_SHIFT)
        | ((IVAR_VALID | other) << IVAR_OTHER_SHIFT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeRegs;

    #[test]
    fn eerd_uses_the_82574_layout() {
        assert_eq!(eerd_command(0), 0x1);
        assert_eq!(eerd_command(2), (2 << 2) | 1);
        // Not the 8254x layout (`addr << 8`, done at bit 4).
        assert_ne!(eerd_command(1), (1 << 8) | 1);
        assert_eq!(EERD_DONE, 0x2);
    }

    #[test]
    fn ivar_routes_rx_tx_other_to_entries_0_1_2() {
        let value = ivar(3);
        assert_eq!(value & 0xF, IVAR_VALID);
        assert_eq!((value >> 8) & 0xF, IVAR_VALID | 1);
        assert_eq!((value >> 16) & 0xF, IVAR_VALID | 2);
        assert_eq!(value & !0x000F_0F0F, 0, "RXQ1/TXQ1 entries stay invalid");
        assert_eq!(ivar(5), value);
    }

    #[test]
    fn ivar_folds_onto_fewer_granted_entries() {
        assert_eq!(ivar(1), IVAR_VALID | (IVAR_VALID << 8) | (IVAR_VALID << 16));
        assert_eq!(
            ivar(2),
            IVAR_VALID | ((IVAR_VALID | 1) << 8) | ((IVAR_VALID | 1) << 16)
        );
    }

    #[test]
    fn nvm_read_returns_the_word_and_times_out_when_wedged() {
        let mut regs = FakeRegs::new();
        regs.nvm[0] = 0x5452;
        regs.nvm[1] = 0x1200;
        regs.nvm[2] = 0x5634;
        assert_eq!(nvm_read(&regs, 0), Ok(0x5452));
        assert_eq!(nvm_read(&regs, 2), Ok(0x5634));
        regs.responsive = false;
        assert_eq!(nvm_read(&regs, 0), Err(ViError::IO));
    }

    #[test]
    fn register_offsets_match_the_datasheet() {
        assert_eq!((EERD, CTRL_EXT, MDIC), (0x14, 0x18, 0x20));
        assert_eq!((ICR, IMS, IMC, EIAC, IVAR), (0xC0, 0xD0, 0xD8, 0xDC, 0xE4));
        assert_eq!((RDBAL, TDBAL, RAL0, RAH0), (0x2800, 0x3800, 0x5400, 0x5404));
        assert_eq!((ICR_LSC, ICR_TXQ0, ICR_OTHER), (1 << 2, 1 << 22, 1 << 24));
    }
}
