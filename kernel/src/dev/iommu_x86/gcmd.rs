//! VT-d global command/status protocol (Intel VT-d spec §§10.4.4–10.4.5).
//!
//! GCMD is write-only and every write restates all persistent enables, so each
//! command is built from the current GSTS.

use super::invalidate::POLL_MAX;

pub const VTD_GCMD: usize = 0x18;
pub const VTD_GSTS: usize = 0x1C;

/// GSTS bits that mirror persistent GCMD state. The rest (RTPS, FLS, WBFS,
/// IRTPS) report one-shot commands; writing them back would re-issue them.
const GSTS_PERSISTENT: u32 = 0x96FF_FFFF;

fn status(base: usize) -> u32 {
    // SAFETY: `base` is the unit's identity-mapped register page.
    unsafe { core::ptr::read_volatile((base + VTD_GSTS) as *const u32) }
}

fn command(base: usize, value: u32) {
    // SAFETY: `base` is the unit's identity-mapped register page.
    unsafe { core::ptr::write_volatile((base + VTD_GCMD) as *mut u32, value) }
}

/// Issue one GCMD command `bit` while preserving the persistent enables.
pub fn issue_gcmd(base: usize, bit: u32) {
    command(base, (status(base) & GSTS_PERSISTENT) | bit);
}

/// Clear persistent enable `bit` while preserving the others.
pub fn clear_gcmd(base: usize, bit: u32) {
    command(base, status(base) & GSTS_PERSISTENT & !bit);
}

/// Whether GSTS reports `bit` set.
pub fn status_set(base: usize, bit: u32) -> bool {
    status(base) & bit != 0
}

/// Poll GSTS until `bit` reads `want`; `false` (logged as `what`) on timeout.
pub fn wait_status(base: usize, bit: u32, want: bool, what: &str) -> bool {
    for _ in 0..POLL_MAX {
        if status_set(base, bit) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    log::warn!(
        "[vtd] unit {:#x} GSTS.{} did not become {} (timeout)",
        base,
        what,
        want as u8
    );
    false
}

/// Clear persistent enable `bit` and wait until GSTS drops it.
pub fn disable(base: usize, bit: u32, what: &str) -> bool {
    if !status_set(base, bit) {
        return true;
    }
    clear_gcmd(base, bit);
    wait_status(base, bit, false, what)
}
