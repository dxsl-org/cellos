#![allow(dead_code)]
//! Realtek 16-byte descriptor layout and accessors.

use ostd::dma::DmaBuf;

pub const BUF_SIZE: usize = 2048;

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

const _: () = assert!(core::mem::size_of::<RtlDesc>() == 16);

pub(crate) fn clear(buf: &DmaBuf) {
    unsafe {
        core::ptr::write_bytes(buf.virt(), 0, buf.size());
    }
}

pub(crate) fn read_desc(buf: &DmaBuf, index: usize) -> RtlDesc {
    let ptr = (buf.virt() as *const RtlDesc).wrapping_add(index);
    unsafe { core::ptr::read_volatile(ptr) }
}

pub(crate) fn write_desc(buf: &DmaBuf, index: usize, desc: RtlDesc) {
    let ptr = (buf.virt() as *mut RtlDesc).wrapping_add(index);
    unsafe { core::ptr::write_volatile(ptr, desc) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_16_bytes() {
        assert_eq!(core::mem::size_of::<RtlDesc>(), 16);
    }
}
