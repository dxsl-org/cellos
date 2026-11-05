//! Quiesce, global reset and the station address.
//!
//! The 82574 auto-loads `RAL0`/`RAH0` from the NVM after reset (datasheet
//! §5.6, §10.2.5.13); `EERD` words 0..3 are the fallback when that entry is not
//! valid. The address is written back so the exact-match filter is real even
//! though `RCTL` also opens the promiscuous paths for the DHCP broadcast.

use crate::regs::*;
use types::{ViError, ViResult};

/// Bus-master quiesce and reset self-clear polls.
const RESET_POLLS: u32 = 1_000_000;

/// Spin until `(read(offset) & mask == 0) == clear`, bounded.
fn poll_bits(regs: &impl RegisterFile, offset: usize, mask: u32, clear: bool) -> ViResult<bool> {
    for _ in 0..RESET_POLLS {
        if (regs.rd32(offset)? & mask == 0) == clear {
            return Ok(true);
        }
        core::hint::spin_loop();
    }
    Ok(false)
}

/// Quiesce, reset and mask the controller, then return its station address.
pub(crate) fn reset_and_read_mac(regs: &impl RegisterFile) -> ViResult<[u8; 6]> {
    // Stop new master requests so the reset cannot cut a DMA in half. A part
    // that never reports quiesced is still reset (the datasheet's fallback).
    regs.wr32(CTRL, regs.rd32(CTRL)? | CTRL_GIO_MASTER_DISABLE)?;
    let _ = poll_bits(regs, STATUS, STATUS_GIO_MASTER_ENABLE, true)?;
    regs.wr32(IMC, u32::MAX)?;
    regs.wr32(CTRL, regs.rd32(CTRL)? | CTRL_RST)?;
    if !poll_bits(regs, CTRL, CTRL_RST, true)? {
        return Err(ViError::IO);
    }
    regs.wr32(IMC, u32::MAX)?;
    let _ = regs.rd32(ICR)?;

    let mac = match mac_from_receive_address(regs.rd32(RAL0)?, regs.rd32(RAH0)?) {
        Some(mac) => mac,
        None => mac_from_nvm_words([nvm_read(regs, 0)?, nvm_read(regs, 1)?, nvm_read(regs, 2)?]),
    };
    // All zeros means neither source produced an address; binding would publish
    // an unusable NIC route, so fail before any DMA.
    if mac == [0u8; 6] {
        return Err(ViError::IO);
    }
    let (ral, rah) = receive_address(mac);
    regs.wr32(RAL0, ral)?;
    regs.wr32(RAH0, rah)?;
    for index in 0..MTA_ENTRIES {
        regs.wr32(MTA + index * 4, 0)?;
    }
    Ok(mac)
}

/// The station address the controller auto-loaded into `RAL0`/`RAH0`, or `None`
/// when the entry is not valid (`RAH.AV` clear) or the address is all zeros.
pub(crate) fn mac_from_receive_address(ral: u32, rah: u32) -> Option<[u8; 6]> {
    if rah & RAH_AV == 0 {
        return None;
    }
    let lo = ral.to_le_bytes();
    let hi = rah.to_le_bytes();
    let mac = [lo[0], lo[1], lo[2], lo[3], hi[0], hi[1]];
    (mac != [0u8; 6]).then_some(mac)
}

/// The station address from NVM words 0..3 (little-endian byte pairs).
pub(crate) fn mac_from_nvm_words(words: [u16; 3]) -> [u8; 6] {
    let [a, b] = words[0].to_le_bytes();
    let [c, d] = words[1].to_le_bytes();
    let [e, f] = words[2].to_le_bytes();
    [a, b, c, d, e, f]
}

/// `RAL0`/`RAH0` values that program `mac` as a valid exact-match entry.
pub(crate) fn receive_address(mac: [u8; 6]) -> (u32, u32) {
    let ral = u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]);
    let rah = u32::from(u16::from_le_bytes([mac[4], mac[5]])) | RAH_AV;
    (ral, rah)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeRegs;

    #[test]
    fn receive_address_round_trips_and_rejects_invalid_entries() {
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let (ral, rah) = receive_address(mac);
        assert_eq!(ral, 0x1200_5452);
        assert_eq!(rah, RAH_AV | 0x5634);
        assert_eq!(mac_from_receive_address(ral, rah), Some(mac));
        assert_eq!(mac_from_receive_address(ral, rah & !RAH_AV), None);
        assert_eq!(mac_from_receive_address(0, RAH_AV), None);
    }

    #[test]
    fn reset_quiesces_masks_and_prefers_the_auto_loaded_address() {
        let regs = FakeRegs::new();
        let (ral, rah) = receive_address([0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF]);
        regs.wr32(RAL0, ral).unwrap();
        regs.wr32(RAH0, rah).unwrap();
        regs.wr32(STATUS, STATUS_GIO_MASTER_ENABLE).unwrap();
        assert_eq!(
            reset_and_read_mac(&regs),
            Ok([0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF])
        );
        let writes = regs.writes.borrow();
        let first_master = writes
            .iter()
            .position(|&(o, v)| o == CTRL && v & CTRL_GIO_MASTER_DISABLE != 0);
        let reset = writes
            .iter()
            .position(|&(o, v)| o == CTRL && v & CTRL_RST != 0);
        assert!(
            first_master < reset,
            "bus mastering is quiesced before the reset"
        );
        assert!(writes.iter().any(|&(o, v)| o == IMC && v == u32::MAX));
        assert!(
            !writes.iter().any(|&(o, _)| o == EERD),
            "valid RAL0/RAH0 needs no NVM read"
        );
        assert_eq!(regs.get(MTA + 4 * (MTA_ENTRIES - 1)), 0);
    }

    #[test]
    fn reset_falls_back_to_nvm_and_refuses_an_empty_address() {
        let mut regs = FakeRegs::new();
        regs.nvm[..3].copy_from_slice(&[0x5452, 0x1200, 0x5634]);
        assert_eq!(
            reset_and_read_mac(&regs),
            Ok([0x52, 0x54, 0x00, 0x12, 0x34, 0x56])
        );
        assert_eq!(regs.get(RAH0), RAH_AV | 0x5634);
        regs.nvm[..3].copy_from_slice(&[0, 0, 0]);
        regs.wr32(RAH0, 0).unwrap();
        assert_eq!(reset_and_read_mac(&regs), Err(ViError::IO));
    }
}
