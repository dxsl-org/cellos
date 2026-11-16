#![allow(dead_code)]
//! Intel i40e descriptor formats.

pub const BUF_SIZE: usize = 2048;
pub(crate) const TX_SLOTS: usize = 16;
pub(crate) const RX_SLOTS: usize = 16;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_sizes_are_16_bytes() {
        assert_eq!(core::mem::size_of::<TxDesc>(), 16);
        assert_eq!(core::mem::size_of::<RxDesc>(), 16);
    }
}
