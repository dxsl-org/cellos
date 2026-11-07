#![allow(dead_code)]
//! Intel i40e descriptor formats and ring operations.

use ostd::dma::DmaBuf;

pub const BUF_SIZE: usize = 2048;

pub(crate) const I40E_TXD_QW1_CMD_EOP: u64 = 1 << 28;
pub(crate) const I40E_TXD_QW1_CMD_RS: u64 = 1 << 29;
pub(crate) const I40E_TXD_QW1_DTYPE_DATA: u64 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct TxDesc {
    pub(crate) buffer_addr: u64,
    pub(crate) cmd_type_offset_bsz: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RxDesc {
    pub(crate) pkt_addr: u64,
    pub(crate) hdr_addr: u64,
}

const _: () = assert!(core::mem::size_of::<TxDesc>() == 16);
const _: () = assert!(core::mem::size_of::<RxDesc>() == 16);

pub(crate) fn clear(buf: &DmaBuf) {
    unsafe {
        core::ptr::write_bytes(buf.virt(), 0, buf.size());
    }
}

pub(crate) fn write_tx_desc(buf: &DmaBuf, index: usize, desc: TxDesc) {
    let ptr = (buf.virt() as *mut TxDesc).wrapping_add(index);
    unsafe { core::ptr::write_volatile(ptr, desc) }
}

pub(crate) fn write_rx_desc(buf: &DmaBuf, index: usize, desc: RxDesc) {
    let ptr = (buf.virt() as *mut RxDesc).wrapping_add(index);
    unsafe { core::ptr::write_volatile(ptr, desc) }
}

pub(crate) fn read_rx_desc(buf: &DmaBuf, index: usize) -> RxDesc {
    let ptr = (buf.virt() as *const RxDesc).wrapping_add(index);
    unsafe { core::ptr::read_volatile(ptr) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_sizes_are_16_bytes() {
        assert_eq!(core::mem::size_of::<TxDesc>(), 16);
        assert_eq!(core::mem::size_of::<RxDesc>(), 16);
    }
}
