//! Kernel-mediated EL0 access to sub-page MMIO grants (H-18).
//!
//! A grant reaches EL0 through the MMU only as the whole pages that lie inside
//! the granted bytes ([`user_pages`]): the kernel never sets USER on a page
//! that also holds registers the Cell was not granted. The BCM2837 VideoCore
//! mailbox is that case — its 0x40 bytes share a page with the legacy IRQ
//! controller and ARM timer the kernel tick runs on.
//!
//! An EL0 load/store to such a page takes a permission fault. A stage-1
//! abort taken to EL1 carries no instruction syndrome (ISV = 0 on Armv8.0),
//! so the kernel decodes the faulting instruction itself; when it is one
//! plain register load/store and the access lies entirely inside a window the
//! current task owns, the kernel performs exactly that access and the trap
//! resumes past the instruction. Anything else stays a Cell fault.

use core::ops::Range;

const PAGE: usize = 4096;

/// Whole pages inside `[base, base + len)`: the only pages a grant may make
/// EL0-reachable. Empty for a sub-page window or an overflowing range.
pub(crate) fn user_pages(base: usize, len: usize) -> Range<usize> {
    let (Some(end), Some(start)) = (base.checked_add(len), base.checked_add(PAGE - 1)) else {
        return 0..0;
    };
    let (start, end) = (start & !(PAGE - 1), end & !(PAGE - 1));
    if start < end {
        start..end
    } else {
        0..0
    }
}

#[cfg(any(target_arch = "aarch64", test))]
/// One EL0 data access decoded from the faulting instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Access {
    pub(crate) write: bool,
    /// Bytes: 1, 2, 4 or 8.
    pub(crate) size: usize,
    /// Transfer register; 31 is the zero register for loads and stores.
    pub(crate) reg: usize,
    pub(crate) sign_extend: bool,
    /// Load destination is an X register, else a W register.
    pub(crate) wide: bool,
}

#[cfg(any(target_arch = "aarch64", test))]
impl Access {
    /// The register value a load of `raw` produces.
    pub(crate) fn extend(self, raw: u64) -> u64 {
        let bits = self.size * 8;
        let value = if self.sign_extend && bits < 64 {
            (((raw << (64 - bits)) as i64) >> (64 - bits)) as u64
        } else {
            raw
        };
        if self.wide {
            value
        } else {
            value & 0xFFFF_FFFF
        }
    }
}

#[cfg(any(target_arch = "aarch64", test))]
/// Decode a lower-EL data abort (`esr`) raised by the A64 instruction `insn`.
///
/// The abort must be a permission fault (the page is mapped kernel-only) with
/// a valid FAR, from a 32-bit instruction, not a cache-maintenance or
/// stage-1-walk fault. The instruction must be a single general-purpose
/// LDR/STR (unsigned-offset, register-offset or unscaled form) whose direction
/// matches the syndrome's WnR: writeback, pair, exclusive, unprivileged,
/// SIMD/FP and prefetch forms are refused.
pub(crate) fn decode(esr: u64, insn: u32) -> Option<Access> {
    const FNV_CM_S1PTW: u64 = (1 << 10) | (1 << 8) | (1 << 7);
    let permission_fault = (esr >> 26) & 0x3F == 0x24
        && esr & (1 << 25) != 0
        && esr & FNV_CM_S1PTW == 0
        && matches!(esr & 0x3F, 0x0D..=0x0F);
    // bits[29:24]: 0b111001 unsigned offset; 0b111000 with bit 21 and
    // bits[11:10] selecting unscaled (0, 00) or register offset (1, 10).
    let (form, bit21, mode) = ((insn >> 24) & 0x3F, (insn >> 21) & 1, (insn >> 10) & 3);
    let plain =
        (form == 0b11_1000 && matches!((bit21, mode), (0, 0) | (1, 2))) || form == 0b11_1001;
    if !permission_fault || !plain {
        return None;
    }
    let size = 1usize << (insn >> 30);
    let (write, sign_extend, wide) = match ((insn >> 22) & 3, size) {
        (0, _) => (true, false, false),
        (1, _) => (false, false, size == 8),
        (2, 1..=4) => (false, true, true),
        (3, 1..=2) => (false, true, false),
        _ => return None,
    };
    if write != (esr & (1 << 6) != 0) {
        return None;
    }
    Some(Access {
        write,
        size,
        reg: (insn & 0x1F) as usize,
        sign_extend,
        wide,
    })
}

#[cfg(any(target_arch = "aarch64", test))]
/// `[far, far + size)` is a naturally aligned access inside the window
/// `[base, base + len)` on a page the grant left kernel-only.
pub(crate) fn mediates(base: usize, len: usize, far: usize, size: usize) -> bool {
    let (Some(window_end), Some(access_end)) = (base.checked_add(len), far.checked_add(size))
    else {
        return false;
    };
    far % size == 0
        && far >= base
        && access_end <= window_end
        && !user_pages(base, len).contains(&(far & !(PAGE - 1)))
}

/// Complete a Cell's faulting access to a sub-page window it owns: `true`
/// once performed (the trap skips the instruction), else a Cell fault.
#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub extern "Rust" fn vi_emulate_el0_mmio_aarch64(
    esr: usize,
    far: usize,
    pc: usize,
    regs: &mut [u64; 31],
) -> bool {
    use core::ptr::{read_volatile, write_volatile};
    use core::sync::atomic::{AtomicBool, Ordering};
    static ANNOUNCED: AtomicBool = AtomicBool::new(false);

    // Read through the kernel's RAM alias of the frame the interrupted EL0
    // context just executed, as the Cell-fault reporter does.
    let Some(insn_phys) = crate::memory::paging::virt_to_phys(pc & !3) else {
        return false;
    };
    // SAFETY: the frame holding an instruction that just executed is mapped
    // RAM, and an aligned word inside it is in bounds of its kernel alias.
    let insn =
        unsafe { read_volatile(crate::memory::frame::phys_to_virt(insn_phys) as *const u32) };
    let Some(access) = decode(esr as u64, insn) else {
        return false;
    };
    let tid = crate::task::current_task_id();
    // Held across the device access so a concurrent release cannot hand the
    // window to another task between the ownership check and the access.
    let registry = super::REGISTRY.lock();
    let Some((&base, _)) = registry
        .range(..=far)
        .next_back()
        .filter(|(&base, region)| {
            region.owner_tid == tid && mediates(base, region.len, far, access.size)
        })
    else {
        return false;
    };
    // SAFETY: `far` lies inside a device window the current task owns, on a
    // page the kernel's identity map keeps as kernel-only device memory (the
    // fault was a permission fault, so the page is mapped). The access has the
    // Cell's own width and natural alignment and touches only granted bytes.
    unsafe {
        let value = regs.get(access.reg).copied().unwrap_or(0);
        match (access.write, access.size) {
            (true, 1) => write_volatile(far as *mut u8, value as u8),
            (true, 2) => write_volatile(far as *mut u16, value as u16),
            (true, 4) => write_volatile(far as *mut u32, value as u32),
            (true, _) => write_volatile(far as *mut u64, value),
            (false, size) => {
                let raw = match size {
                    1 => read_volatile(far as *const u8) as u64,
                    2 => read_volatile(far as *const u16) as u64,
                    4 => read_volatile(far as *const u32) as u64,
                    _ => read_volatile(far as *const u64),
                };
                if let Some(slot) = regs.get_mut(access.reg) {
                    *slot = access.extend(raw);
                }
            }
        }
    }
    drop(registry);
    if !ANNOUNCED.swap(true, Ordering::Relaxed) {
        log::info!("[mmio] kernel-mediated EL0 access: tid={tid} window={base:#x} far={far:#x}");
    }
    true
}

#[cfg(target_arch = "aarch64")]
const _: crate::hal::EmulateEl0MmioAarch64 = vi_emulate_el0_mmio_aarch64;

#[cfg(test)]
#[path = "resource_registry_mediation_tests.rs"]
mod tests;
