//! VT-d context-cache, IOTLB and interrupt-entry-cache invalidation (Intel
//! VT-d spec §§6.5, 10.4.7–10.4.8).
//!
//! [`VtdUnit`] picks the mechanism: the invalidation queue once it is on
//! (register-based invalidation is then illegal), the command registers
//! before that or on units without queued invalidation.

use super::gcmd::{issue_gcmd, wait_status};
use super::qi::{context_desc, iec_desc, iotlb_desc};
use super::unit::VtdUnit;
use core::sync::atomic::fence;
use core::sync::atomic::Ordering;

/// Which cached translations an invalidation covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    /// Everything tagged with this domain id.
    Domain(u16),
}

/// `GCAP.RWBF`: table stores sit in a write buffer until flushed.
const GCAP_RWBF: u64 = 1 << 4;
/// `GCAP.DWD` / `GCAP.DRD`: IOTLB invalidation can drain writes / reads.
const GCAP_DWD: u64 = 1 << 54;
const GCAP_DRD: u64 = 1 << 55;
const VTD_GCMD_WBF: u32 = 1 << 27;

pub const VTD_CCMD: usize = 0x28; // 64-bit context command
pub const CCMD_ICC: u64 = 1u64 << 63;
pub const CCMD_GLOBAL: u64 = 1u64 << 61;
pub const CCMD_DOMAIN: u64 = 2u64 << 61;

pub const IOTLB_IVT: u64 = 1u64 << 63;
pub const IOTLB_GLOBAL: u64 = 1u64 << 60;
pub const IOTLB_DOMAIN: u64 = 2u64 << 60;
pub const IOTLB_DRD: u64 = 1u64 << 49;
pub const IOTLB_DWD: u64 = 1u64 << 48;

pub const POLL_MAX: u64 = 1_000_000;

fn ctx_global_command() -> u64 {
    CCMD_ICC | CCMD_GLOBAL
}

fn ctx_domain_command(did: u16) -> u64 {
    CCMD_ICC | CCMD_DOMAIN | (did as u64)
}

fn iotlb_global_command() -> u64 {
    IOTLB_IVT | IOTLB_GLOBAL | IOTLB_DRD | IOTLB_DWD
}

fn iotlb_domain_command(did: u16) -> u64 {
    IOTLB_IVT | IOTLB_DOMAIN | IOTLB_DRD | IOTLB_DWD | ((did as u64) << 32)
}

fn ctx_command(scope: Scope) -> u64 {
    match scope {
        Scope::Global => ctx_global_command(),
        Scope::Domain(did) => ctx_domain_command(did),
    }
}

fn iotlb_command(scope: Scope) -> u64 {
    match scope {
        Scope::Global => iotlb_global_command(),
        Scope::Domain(did) => iotlb_domain_command(did),
    }
}

impl VtdUnit {
    /// Flush the unit's write buffer when `GCAP.RWBF` requires it, so table
    /// stores reach memory before an invalidation makes hardware re-read them.
    fn flush_write_buffer(&self) {
        if self.gcap & GCAP_RWBF == 0 {
            return;
        }
        issue_gcmd(self.base, VTD_GCMD_WBF);
        // WBFS reads 1 while the flush is in progress.
        wait_status(self.base, VTD_GCMD_WBF, false, "WBFS");
    }

    /// Invalidate the context cache for `scope`.
    pub fn invalidate_context(&self, scope: Scope) -> bool {
        self.flush_write_buffer();
        if let Some(queue) = self.qi.lock().as_mut() {
            return queue.submit(self.base, &[context_desc(scope)]);
        }
        flush_context(self.base, ctx_command(scope))
    }

    /// Invalidate the IOTLB (and paging-structure caches) for `scope`.
    pub fn invalidate_iotlb(&self, scope: Scope) -> bool {
        self.flush_write_buffer();
        if let Some(queue) = self.qi.lock().as_mut() {
            let (dr, dw) = (self.gcap & GCAP_DRD != 0, self.gcap & GCAP_DWD != 0);
            return queue.submit(self.base, &[iotlb_desc(scope, dr, dw)]);
        }
        flush_iotlb(self.base, self.iva_off, iotlb_command(scope))
    }

    /// Invalidate the interrupt entry cache: globally for `None`, else the one
    /// entry. Only queued invalidation can do this; `false` without a queue.
    pub fn invalidate_iec(&self, index: Option<u16>) -> bool {
        self.flush_write_buffer();
        match self.qi.lock().as_mut() {
            Some(queue) => queue.submit(self.base, &[iec_desc(index)]),
            None => false,
        }
    }
}

pub fn invalidation_acknowledged(
    command: u64,
    result: u64,
    request_shift: u32,
    actual_shift: u32,
    busy_mask: u64,
) -> bool {
    let requested = (command >> request_shift) & 0x3;
    let actual = (result >> actual_shift) & 0x3;
    result & busy_mask == 0 && actual != 0 && actual <= requested
}

fn flush_context(base: usize, cmd: u64) -> bool {
    fence(Ordering::SeqCst);
    unsafe {
        core::ptr::write_volatile((base + VTD_CCMD) as *mut u64, cmd);
    }
    let mut n = 0u64;
    loop {
        let val = unsafe { core::ptr::read_volatile((base + VTD_CCMD) as *const u64) };
        if invalidation_acknowledged(cmd, val, 61, 59, CCMD_ICC) {
            return true;
        }
        n += 1;
        if n >= POLL_MAX {
            log::warn!(
                "[vtd] context invalidation timed out cmd={:#x} val={:#x}",
                cmd,
                val
            );
            return false;
        }
        core::hint::spin_loop();
    }
}

fn flush_iotlb(base: usize, iva_off: usize, cmd: u64) -> bool {
    if iva_off == 0 {
        return true;
    }
    let reg = base + iva_off + 8;
    fence(Ordering::SeqCst);
    unsafe {
        core::ptr::write_volatile(reg as *mut u64, cmd);
    }
    let mut n = 0u64;
    loop {
        let val = unsafe { core::ptr::read_volatile(reg as *const u64) };
        if invalidation_acknowledged(cmd, val, 60, 57, IOTLB_IVT) {
            return true;
        }
        n += 1;
        if n >= POLL_MAX {
            log::warn!(
                "[vtd] IOTLB invalidation timed out cmd={:#x} val={:#x}",
                cmd,
                val
            );
            return false;
        }
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidation_command_encodings() {
        assert_eq!(ctx_global_command(), (1u64 << 63) | (1u64 << 61));
        assert_eq!(
            ctx_domain_command(0x1234),
            (1u64 << 63) | (2u64 << 61) | 0x1234
        );
        assert_eq!(
            iotlb_global_command(),
            (1u64 << 63) | (1u64 << 60) | (1u64 << 49) | (1u64 << 48)
        );
        assert_eq!(
            iotlb_domain_command(0x1234),
            (1u64 << 63) | (2u64 << 60) | (1u64 << 49) | (1u64 << 48) | (0x1234u64 << 32)
        );
    }

    #[test]
    fn completion_check() {
        for (request_shift, actual_shift, busy) in [(61, 59, CCMD_ICC), (60, 57, IOTLB_IVT)] {
            for requested in 1..=3 {
                let command = busy | (requested << request_shift);
                for actual in 0..=3 {
                    let result = actual << actual_shift;
                    assert_eq!(
                        invalidation_acknowledged(
                            command,
                            result,
                            request_shift,
                            actual_shift,
                            busy
                        ),
                        actual != 0 && actual <= requested
                    );
                    assert!(!invalidation_acknowledged(
                        command,
                        result | busy,
                        request_shift,
                        actual_shift,
                        busy
                    ));
                }
            }
        }
    }
}
