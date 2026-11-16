//! Broadcom BCM57xx descriptor formats.

#![allow(dead_code)]

pub const BUF_SIZE: usize = 2048;
pub(crate) const TX_SLOTS: usize = 16;
pub(crate) const RX_SLOTS: usize = 16;

pub(crate) const BCMD_FLAGS_END: u32 = 0x0004_0000;
pub(crate) const BCMD_FLAGS_PACKET_END: u32 = 0x0008_0000;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct BcmTxDesc {
    pub(crate) addr_hi: u32,
    pub(crate) addr_lo: u32,
    pub(crate) len_flags: u32,
    pub(crate) vlan_tag: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct BcmRxDesc {
    pub(crate) addr_hi: u32,
    pub(crate) addr_lo: u32,
    pub(crate) idx_len: u32,
    pub(crate) type_flags: u32,
}

const _: () = assert!(core::mem::size_of::<BcmTxDesc>() == 16);
const _: () = assert!(core::mem::size_of::<BcmRxDesc>() == 16);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_sizes_are_16_bytes() {
        assert_eq!(core::mem::size_of::<BcmTxDesc>(), 16);
        assert_eq!(core::mem::size_of::<BcmRxDesc>(), 16);
    }
}
