//! VT-d root and context table encoding (Intel VT-d spec §9.3).

use crate::dev::iommu_pt::virt_to_phys;
use alloc::alloc::{alloc_zeroed, Layout};
use core::sync::atomic::{fence, Ordering};

pub const CTX_AW_39BIT_HI: u64 = 0b001; // hi[2:0]
pub const CTX_PRESENT: u64 = 1;

/// P01 lifetime kind of a root table.
pub const TABLE_KIND_ROOT: u64 = 5;
/// P01 lifetime kind of a per-bus context table.
pub const TABLE_KIND_CONTEXT: u64 = 6;

#[derive(Clone, Copy, Debug)]
pub struct ContextTable {
    pub virt: usize,
    pub phys: u64,
    /// The root entry naming this table was globally invalidated; until then
    /// the table is not handed out, so a timeout stays retryable.
    pub publication_flushed: bool,
}

/// Allocate a zeroed 4 KiB table page of P01 lifetime `_kind`. Panics on OOM.
pub fn alloc_table(_kind: u64) -> (usize, u64) {
    #[cfg(feature = "p01-trace")]
    let _p01_activity = crate::p01_noncpu::activity();
    let layout = Layout::from_size_align(4096, 4096).expect("table layout");
    // SAFETY: layout is non-zero and 4 KiB aligned.
    let virt = {
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::DRIVER_SHARED,
        );
        unsafe { alloc_zeroed(layout) as usize }
    };
    if virt == 0 {
        panic!("[vtd] out of memory allocating table");
    }
    let phys = virt_to_phys(virt);
    #[cfg(feature = "p01-trace")]
    crate::p01_noncpu::created_heap(phys, _kind, virt);
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

/// `[lo, hi]` of a present legacy context entry: untranslated requests walk
/// `slpt_phys` (3-level, 39-bit AW) under domain `did`.
#[inline]
pub fn context_entry(slpt_phys: u64, did: u16) -> [u64; 2] {
    [
        (slpt_phys & !0xFFF) | CTX_PRESENT,
        ((did as u64) << 8) | CTX_AW_39BIT_HI,
    ]
}

/// # Safety
/// Caller must ensure `slot` points to a valid context entry slot.
pub unsafe fn read_ctx_slot(slot: usize) -> [u64; 2] {
    // SAFETY: the caller guarantees both 8-byte words lie in the entry.
    unsafe {
        [
            core::ptr::read_volatile(slot as *const u64),
            core::ptr::read_volatile((slot + 8) as *const u64),
        ]
    }
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
/// Caller must ensure `ctx_virt` points to an allocated context table page
/// and that the entry for `bdf` is not present (hardware never sees a torn
/// present entry).
pub unsafe fn write_ctx_entry(ctx_virt: usize, bdf: u16, slpt_phys: u64, did: u16) {
    let slot = context_slot(ctx_virt, bdf);
    let [lo, hi] = context_entry(slpt_phys, did);
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
        let slot = context_slot(ctx_virt, bdf);
        assert_eq!(
            unsafe { read_ctx_slot(slot) },
            context_entry(0x3000, 0xFFFF)
        );
        unsafe {
            clear_ctx_slot(slot);
        }
        assert_eq!(&context[510..], &[0, 0]);
    }
}
