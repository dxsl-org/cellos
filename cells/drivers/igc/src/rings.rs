//! Advanced 16-byte descriptor ring layout for I225/I226.

pub const BUF_SIZE: usize = 2048;
pub(crate) const TX_SLOTS: usize = 16;
pub(crate) const RX_SLOTS: usize = 16;

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

impl RxDesc {
    /// A descriptor handed to the controller with buffer `iova`.
    pub(crate) const fn posted(iova: u64) -> Self {
        Self {
            buffer_addr: iova,
            status_error: 0,
            length: 0,
            vlan: 0,
        }
    }
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
