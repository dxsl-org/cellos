//! Broadcom BCM57xx descriptor formats and ring operations.

#![allow(dead_code)]

use ostd::dma::DmaBuf;

pub const BUF_SIZE: usize = 2048;

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

pub(crate) fn clear(buf: &DmaBuf) {
    unsafe {
        core::ptr::write_bytes(buf.virt(), 0, buf.size());
    }
}

pub(crate) fn write_tx_desc(buf: &DmaBuf, index: usize, desc: BcmTxDesc) {
    let ptr = (buf.virt() as *mut BcmTxDesc).wrapping_add(index);
    unsafe { core::ptr::write_volatile(ptr, desc) }
}

pub(crate) fn write_rx_desc(buf: &DmaBuf, index: usize, desc: BcmRxDesc) {
    let ptr = (buf.virt() as *mut BcmRxDesc).wrapping_add(index);
    unsafe { core::ptr::write_volatile(ptr, desc) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_sizes_are_16_bytes() {
        assert_eq!(core::mem::size_of::<BcmTxDesc>(), 16);
        assert_eq!(core::mem::size_of::<BcmRxDesc>(), 16);
    }
}
