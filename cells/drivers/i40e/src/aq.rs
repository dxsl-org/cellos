//! Intel X710/XL710 Admin Queue (AQ) command and descriptor definitions.
//!
//! Intel Ethernet Controller X710 Datasheet §7.1; FreeBSD `i40e_adminq_cmd.h`.

#![allow(dead_code)]

use crate::regs::{RegisterFile, PF_ATQT};
use nic_driver::ring::DescRing;
use types::{ViError, ViResult};

pub(crate) const OPC_GET_VERSION: u16 = 0x0001;
pub(crate) const OPC_MAC_ADDRESS_READ: u16 = 0x0107;
pub(crate) const OPC_GET_SWITCH_CONFIG: u16 = 0x0200;
pub(crate) const OPC_GET_LINK_STATUS: u16 = 0x0600;

pub(crate) const I40E_AQ_FLAG_DD: u16 = 0x0001;
pub(crate) const I40E_AQ_FLAG_CMP: u16 = 0x0002;
pub(crate) const I40E_AQ_FLAG_ERR: u16 = 0x0004;
pub(crate) const I40E_AQ_FLAG_BUF: u16 = 0x1000;

const AQ_POLLS: u32 = 100_000;
/// Admin queue depth (send and receive).
pub(crate) const AQ_SLOTS: usize = 16;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AqDesc {
    pub(crate) flags: u16,
    pub(crate) opcode: u16,
    pub(crate) datalen: u16,
    pub(crate) retval: u16,
    pub(crate) cookie_high: u32,
    pub(crate) cookie_low: u32,
    pub(crate) param0: u32,
    pub(crate) param1: u32,
    pub(crate) addr_high: u32,
    pub(crate) addr_low: u32,
}

const _: () = assert!(core::mem::size_of::<AqDesc>() == 32);

pub(crate) fn format_get_version() -> AqDesc {
    AqDesc {
        opcode: OPC_GET_VERSION,
        ..Default::default()
    }
}

pub(crate) fn format_mac_read(buf_iova: u64, buf_len: u16) -> AqDesc {
    AqDesc {
        flags: I40E_AQ_FLAG_BUF,
        opcode: OPC_MAC_ADDRESS_READ,
        datalen: buf_len,
        addr_high: (buf_iova >> 32) as u32,
        addr_low: buf_iova as u32,
        ..Default::default()
    }
}

pub(crate) fn format_get_switch_config(buf_iova: u64, buf_len: u16) -> AqDesc {
    AqDesc {
        flags: I40E_AQ_FLAG_BUF,
        opcode: OPC_GET_SWITCH_CONFIG,
        datalen: buf_len,
        addr_high: (buf_iova >> 32) as u32,
        addr_low: buf_iova as u32,
        ..Default::default()
    }
}

pub(crate) fn format_get_link_status() -> AqDesc {
    AqDesc {
        opcode: OPC_GET_LINK_STATUS,
        param0: 1, // notify = 1
        ..Default::default()
    }
}

/// Admin send queue: [`AQ_SLOTS`] descriptors in one authorized DMA page.
pub(crate) type AdminQueue = DescRing<AqDesc, AQ_SLOTS>;

pub(crate) fn post_aq_command(
    regs: &impl RegisterFile,
    ring: &AdminQueue,
    head: usize,
    desc: AqDesc,
) -> ViResult<AqDesc> {
    ring.write(head, desc);
    let next = (head + 1) % AQ_SLOTS;
    regs.wr32(PF_ATQT, next as u32)?;

    for _ in 0..AQ_POLLS {
        let resp = ring.read(head);
        if resp.flags & I40E_AQ_FLAG_DD != 0 {
            if resp.retval != 0 {
                return Err(ViError::IO);
            }
            return Ok(resp);
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aq_descriptor_is_32_bytes() {
        assert_eq!(core::mem::size_of::<AqDesc>(), 32);
    }

    #[test]
    fn format_get_version_encodes_opcode() {
        let cmd = format_get_version();
        assert_eq!(cmd.opcode, OPC_GET_VERSION);
        assert_eq!(cmd.flags, 0);
    }

    #[test]
    fn format_mac_read_encodes_buffer_iova() {
        let cmd = format_mac_read(0x1_2345_6780, 24);
        assert_eq!(cmd.opcode, OPC_MAC_ADDRESS_READ);
        assert_ne!(cmd.flags & I40E_AQ_FLAG_BUF, 0);
        assert_eq!(cmd.datalen, 24);
        assert_eq!(cmd.addr_high, 1);
        assert_eq!(cmd.addr_low, 0x2345_6780);
    }
}
