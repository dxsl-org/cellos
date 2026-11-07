//! Advanced 16-byte descriptor ring layout for I225/I226.

use ostd::dma::DmaBuf;

pub const BUF_SIZE: usize = 2048;

pub(crate) const ADVTXD_DCMD_DEXT: u32 = 1 << 29;
pub(crate) const ADVTXD_DTYP_DATA: u32 = 0x0030_0000;
pub(crate) const TXD_CMD_EOP: u32 = 1 << 24;
pub(crate) const TXD_CMD_IFCS: u32 = 1 << 25;
pub(crate) const TXD_CMD_RS: u32 = 1 << 27;
pub(crate) const TXD_STAT_DD: u32 = 1 << 0;
pub(crate) const RXD_STAT_DD: u32 = 1 << 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct TxDesc {
    pub(crate) buffer_addr: u64,
    pub(crate) cmd_type_len: u32,
    pub(crate) olinfo_status: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RxDesc {
    pub(crate) buffer_addr: u64,
    pub(crate) status_error: u32,
    pub(crate) length: u16,
    pub(crate) vlan: u16,
}

const _: () = assert!(core::mem::size_of::<TxDesc>() == 16);
const _: () = assert!(core::mem::size_of::<RxDesc>() == 16);

pub(crate) fn clear(buf: &DmaBuf) {
    unsafe {
        core::ptr::write_bytes(buf.virt(), 0, buf.size());
    }
}

pub(crate) fn read_rx_desc(buf: &DmaBuf, index: usize) -> RxDesc {
    let ptr = (buf.virt() as *const RxDesc).wrapping_add(index);
    unsafe { core::ptr::read_volatile(ptr) }
}

pub(crate) fn write_rx_desc(buf: &DmaBuf, index: usize, desc: RxDesc) {
    let ptr = (buf.virt() as *mut RxDesc).wrapping_add(index);
    unsafe { core::ptr::write_volatile(ptr, desc) }
}

pub(crate) fn read_tx_desc(buf: &DmaBuf, index: usize) -> TxDesc {
    let ptr = (buf.virt() as *const TxDesc).wrapping_add(index);
    unsafe { core::ptr::read_volatile(ptr) }
}

pub(crate) fn write_tx_desc(buf: &DmaBuf, index: usize, desc: TxDesc) {
    let ptr = (buf.virt() as *mut TxDesc).wrapping_add(index);
    unsafe { core::ptr::write_volatile(ptr, desc) }
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
