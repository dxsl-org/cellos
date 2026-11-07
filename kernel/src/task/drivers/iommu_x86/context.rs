//! VT-d root and context table encoding (Intel VT-d spec §9.3).

use alloc::alloc::{alloc_zeroed, Layout};
use core::sync::atomic::{fence, Ordering};

pub const CTX_AW_39BIT_HI: u64 = 0b001; // hi[2:0]
pub const CTX_PRESENT: u64 = 1;

#[derive(Clone, Copy, Debug)]
pub struct ContextTable {
    pub virt: usize,
    pub phys: u64,
    pub publication_flushed: bool,
}

pub fn alloc_table(alignment_bits: usize) -> (usize, u64) {
    let size = 4096;
    let align = 1usize << alignment_bits.max(12);
    let layout = Layout::from_size_align(size, align).expect("table layout");
    // SAFETY: layout is 4 KiB aligned and zero-initialized.
    let virt = unsafe { alloc_zeroed(layout) as usize };
    if virt == 0 {
        panic!("[vtd] out of memory allocating table");
    }
    let phys = virt as u64; // kernel identity-mapped
    (virt, phys)
}

#[inline]
pub fn canonical_bdf(bus: u8, dev: u8, func: u8) -> u16 {
    ((bus as u16) << 8) | ((dev as u16) << 3) | (func as u16)
}

#[inline]
pub fn context_slot_offset(dev: u8, func: u8) -> usize {
    ((dev as usize) * 8 + (func as usize)) * 16
}

#[inline]
pub fn context_slot(ctx_virt: usize, bdf: u16) -> usize {
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let func = (bdf & 0x07) as u8;
    ctx_virt + context_slot_offset(dev, func)
}

/// # Safety
/// Caller must ensure `root_virt` points to an allocated root table page.
pub unsafe fn write_root_entry(root_virt: usize, bus: u8, ctx_phys: u64) {
    let slot = (root_virt + (bus as usize) * 16) as *mut u64;
    unsafe {
        core::ptr::write_volatile(slot.add(1), 0);
        fence(Ordering::Release);
        core::ptr::write_volatile(slot, (ctx_phys & !0xFFF) | 1);
    }
}

/// # Safety
/// Caller must ensure `ctx_virt` points to an allocated context table page.
pub unsafe fn write_ctx_entry(ctx_virt: usize, bdf: u16, slpt_phys: u64, did: u16) {
    let slot = context_slot(ctx_virt, bdf);
    let lo = (slpt_phys & !0xFFF) | CTX_PRESENT;
    let hi = ((did as u64) << 8) | CTX_AW_39BIT_HI;
    unsafe {
        core::ptr::write_volatile((slot + 8) as *mut u64, hi);
        fence(Ordering::Release);
        core::ptr::write_volatile(slot as *mut u64, lo);
    }
}

/// # Safety
/// Caller must ensure `slot` points to a valid context entry slot.
pub unsafe fn clear_ctx_slot(slot: usize) {
    unsafe {
        core::ptr::write_volatile(slot as *mut u64, 0);
        fence(Ordering::Release);
        core::ptr::write_volatile((slot + 8) as *mut u64, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_slot_offsets_cover_all_devfn() {
        assert_eq!(context_slot_offset(0, 0), 0);
        assert_eq!(context_slot_offset(7, 3), (7 * 8 + 3) * 16);
        assert_eq!(context_slot_offset(31, 7), 4096 - 16);
    }

    #[test]
    fn root_and_context_entry_encoding() {
        let mut root = [0u64; 512];
        let mut context = [0u64; 512];
        let root_virt = root.as_mut_ptr() as usize;
        let ctx_virt = context.as_mut_ptr() as usize;
        let bdf = canonical_bdf(255, 31, 7);
        unsafe {
            write_root_entry(root_virt, 255, 0x2000);
            write_ctx_entry(ctx_virt, bdf, 0x3000, 0xFFFF);
        }
        assert_eq!(&root[510..], &[0x2001, 0]);
        assert_eq!(&context[510..], &[0x3001, 0x00FF_FF01]);
        unsafe {
            clear_ctx_slot(context_slot(ctx_virt, bdf));
        }
        assert_eq!(&context[510..], &[0, 0]);
    }
}
