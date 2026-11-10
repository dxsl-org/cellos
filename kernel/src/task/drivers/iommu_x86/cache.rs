//! CPU cache write-back for remapping structures read by a non-snooping unit
//! (`ECAP.C` = 0, Intel VT-d spec §10.4.3): such hardware reads memory, not
//! the CPU caches, so every table store must be flushed before the unit may
//! observe it.

/// `ECAP.C`: hardware table walks snoop the processor caches.
pub const ECAP_C: u64 = 1 << 0;

/// CLFLUSH line size from CPUID.01H:EBX[15:8] (in 8-byte units).
fn line_size() -> usize {
    let ebx = core::arch::x86_64::__cpuid(1).ebx;
    (((ebx >> 8) & 0xFF) as usize * 8).max(32)
}

/// Write back and invalidate every cache line of `[virt, virt + len)`.
/// Inline asm rather than the SSE2 intrinsics: the kernel target disables SSE
/// code generation, while MFENCE/CLFLUSH are baseline x86_64 instructions.
pub fn flush(virt: usize, len: usize) {
    if len == 0 {
        return;
    }
    let line = line_size();
    let mut at = virt & !(line - 1);
    // SAFETY: MFENCE/CLFLUSH only need a mapped address; the range is a live
    // allocation owned by the caller. The first MFENCE orders the preceding
    // table stores before the flushes, the last one completes them.
    unsafe {
        core::arch::asm!("mfence", options(nostack, preserves_flags));
        while at < virt + len {
            core::arch::asm!("clflush [{}]", in(reg) at, options(nostack, preserves_flags));
            at += line;
        }
        core::arch::asm!("mfence", options(nostack, preserves_flags));
    }
}

/// Flush `[virt, virt + len)` when the unit does not snoop (`ecap` lacks C).
pub fn sync_for(ecap: u64, virt: usize, len: usize) {
    if ecap & ECAP_C == 0 {
        flush(virt, len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_covers_unaligned_ranges_without_faulting() {
        let buf = [0u8; 300];
        flush(buf.as_ptr() as usize + 3, 250);
        sync_for(ECAP_C, buf.as_ptr() as usize, buf.len());
        sync_for(0, buf.as_ptr() as usize, buf.len());
        assert!(line_size().is_power_of_two());
    }
}
