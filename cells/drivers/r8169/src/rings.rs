#![allow(dead_code)]
//! Realtek 16-byte descriptor layout.

pub const BUF_SIZE: usize = 2048;
pub(crate) const TX_SLOTS: usize = 16;
pub(crate) const RX_SLOTS: usize = 16;

pub(crate) const DESC_OWN: u32 = 1 << 31;
pub(crate) const DESC_EOP: u32 = 1 << 30;
pub(crate) const DESC_FS: u32 = 1 << 29;
pub(crate) const DESC_LS: u32 = 1 << 28;
pub(crate) const DESC_RING_END: u32 = 1 << 30;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RtlDesc {
    pub(crate) opts1: u32,
    pub(crate) opts2: u32,
    pub(crate) addr_lo: u32,
    pub(crate) addr_hi: u32,
}

impl RtlDesc {
    /// RX slot `index` of a `RX_SLOTS` ring handed to the controller with
    /// buffer `iova`; the last slot carries the ring-end mark.
    pub(crate) const fn rx_posted(index: usize, iova: u64) -> Self {
        let mut opts1 = DESC_OWN | (BUF_SIZE as u32);
        if index == RX_SLOTS - 1 {
            opts1 |= DESC_RING_END;
        }
        Self {
            opts1,
            opts2: 0,
            addr_lo: iova as u32,
            addr_hi: (iova >> 32) as u32,
        }
    }
}

const _: () = assert!(core::mem::size_of::<RtlDesc>() == 16);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_16_bytes() {
        assert_eq!(core::mem::size_of::<RtlDesc>(), 16);
    }
}
