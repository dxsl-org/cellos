//! KVM paravirtual clock (pvclock, MSR_KVM_SYSTEM_TIME_NEW).
//!
//! The MSR is per vCPU, so every CPU registers its own time-info structure
//! ([`init`] on the boot CPU, [`register`] on each AP) and must see
//! `PVCLOCK_TSC_STABLE_BIT` in it. Reads use the boot CPU's structure: that is
//! valid only while the host sets the stable bit, which says every vCPU
//! computes the same time from it (the same rule the Linux vDSO uses). A CPU
//! whose own structure lacks the bit refuses the registration.

use core::arch::x86_64::__cpuid;
use core::sync::atomic::{fence, AtomicUsize, Ordering};

const MSR_KVM_SYSTEM_TIME_NEW: u32 = 0x4b56_4d01;
const KVM_FEATURE_CLOCKSOURCE2: u32 = 1 << 3;
const KVM_FEATURE_CLOCKSOURCE_STABLE_BIT: u32 = 1 << 24;
const PVCLOCK_TSC_STABLE_BIT: u8 = 1 << 0;

/// Bytes of one CPU's time-info area.
pub const AREA_BYTES: usize = core::mem::size_of::<TimeInfo>();
const _: () = assert!(AREA_BYTES == 32);

/// `struct pvclock_vcpu_time_info` (32 bytes, host-written).
#[repr(C)]
struct TimeInfo {
    version: u32,
    _pad0: u32,
    tsc_timestamp: u64,
    system_time: u64,
    tsc_to_system_mul: u32,
    tsc_shift: i8,
    flags: u8,
    _pad: [u8; 2],
}

/// Virtual address of the registered `TimeInfo`; 0 = not active.
static INFO: AtomicUsize = AtomicUsize::new(0);

/// `KVMKVMKVM\0\0\0` signature plus CLOCKSOURCE2 support.
pub fn available() -> bool {
    if __cpuid(1).ecx & (1 << 31) == 0 {
        return false;
    }
    let sig = __cpuid(0x4000_0000);
    let kvm = sig.ebx == u32::from_le_bytes(*b"KVMK")
        && sig.ecx == u32::from_le_bytes(*b"VMKV")
        && sig.edx == u32::from_le_bytes(*b"M\0\0\0");
    kvm && sig.eax >= 0x4000_0001 && __cpuid(0x4000_0001).eax & KVM_FEATURE_CLOCKSOURCE2 != 0
}

/// Register this CPU's 32-byte time-info area at `virt`/`phys` (zeroed,
/// 4-byte aligned, never reused) and make it the area [`now_ns`] reads.
/// Returns `false` when the host does not declare a stable TSC.
///
/// # Safety
/// As for [`register`].
pub unsafe fn init(virt: usize, phys: u64) -> bool {
    // SAFETY: forwarded caller contract.
    if !unsafe { register(virt, phys) } {
        return false;
    }
    INFO.store(virt, Ordering::Release);
    true
}

/// Register a 32-byte time-info area at `virt`/`phys` for the calling CPU only
/// (the MSR is per vCPU). Returns `false`, leaving the registration disabled,
/// when KVM is absent, the area is unusable, or the host does not declare a
/// stable TSC in this CPU's own area.
///
/// # Safety
/// `virt` must map `phys` writable for at least 32 bytes for the rest of the
/// boot, and nothing else (no other CPU's registration either) may use that
/// memory.
pub unsafe fn register(virt: usize, phys: u64) -> bool {
    if !available() || virt == 0 || phys & 3 != 0 {
        return false;
    }
    // SAFETY: the caller hands over this memory; enable bit 0.
    unsafe { super::super::msr::write(MSR_KVM_SYSTEM_TIME_NEW, phys | 1) };
    let declared_stable = __cpuid(0x4000_0001).eax & KVM_FEATURE_CLOCKSOURCE_STABLE_BIT != 0;
    // SAFETY: the host updates the area synchronously on the MSR write.
    let flags = unsafe { core::ptr::read_volatile(&(*(virt as *const TimeInfo)).flags) };
    if !declared_stable || flags & PVCLOCK_TSC_STABLE_BIT == 0 {
        // SAFETY: disable the registration so the host stops writing.
        unsafe { super::super::msr::write(MSR_KVM_SYSTEM_TIME_NEW, 0) };
        return false;
    }
    true
}

/// Physical address this CPU has registered with the host, if any (read back
/// from the per-vCPU MSR; `None` without KVM or when disabled).
pub fn registered_area() -> Option<u64> {
    if !available() {
        return None;
    }
    // SAFETY: the MSR exists whenever CLOCKSOURCE2 is advertised.
    let value = unsafe { super::super::msr::read(MSR_KVM_SYSTEM_TIME_NEW) };
    (value & 1 != 0).then_some(value & !1)
}

/// Host system time in nanoseconds; 0 when not active.
pub fn now_ns() -> u64 {
    let virt = INFO.load(Ordering::Acquire);
    if virt == 0 {
        return 0;
    }
    let info = virt as *const TimeInfo;
    // SAFETY (both closures): `init` validated the registration; the host
    // writes the area concurrently, so every field is read volatilely.
    let version = || unsafe { core::ptr::read_volatile(&(*info).version) };
    let (stamp, system, mul, shift, tsc) = seqlock_read(version, || unsafe {
        (
            core::ptr::read_volatile(&(*info).tsc_timestamp),
            core::ptr::read_volatile(&(*info).system_time),
            core::ptr::read_volatile(&(*info).tsc_to_system_mul),
            core::ptr::read_volatile(&(*info).tsc_shift),
            super::tsc::read(),
        )
    });
    scale(system, tsc.wrapping_sub(stamp), mul, shift)
}

/// pvclock version protocol: the host makes `version` odd while it writes
/// and bumps it after. Retry until a body read sits between two equal, even
/// versions.
pub(super) fn seqlock_read<T>(mut version: impl FnMut() -> u32, mut body: impl FnMut() -> T) -> T {
    loop {
        let before = version();
        fence(Ordering::Acquire);
        let value = body();
        fence(Ordering::Acquire);
        if before & 1 == 0 && before == version() {
            return value;
        }
        core::hint::spin_loop();
    }
}

/// `system + (delta << shift or >> -shift) * mul / 2^32`.
pub(super) fn scale(system: u64, delta: u64, mul: u32, shift: i8) -> u64 {
    let delta = if shift >= 0 {
        delta << shift
    } else {
        delta >> (-(shift as i16))
    };
    system.wrapping_add(((delta as u128 * mul as u128) >> 32) as u64)
}

#[cfg(test)]
mod tests {
    use super::{scale, seqlock_read};

    #[test]
    fn pvclock_scaling_follows_shift_and_multiplier() {
        // mul = 2^31 → ×0.5 per tick; shift +1 doubles first → 1 ns per tick.
        assert_eq!(scale(100, 1000, 1 << 31, 1), 1100);
        assert_eq!(scale(0, 1000, 1 << 31, -1), 250);
    }

    #[test]
    fn seqlock_retries_on_odd_or_changed_versions() {
        // Reads: 1 odd → retry (no second read); (2,4) changed → retry;
        // (4,4) → accept on the third body read.
        let versions = [1u32, 2, 4, 4, 4];
        let mut next_version = versions.iter().copied();
        let mut body_calls = 0;
        let value = seqlock_read(
            || next_version.next().expect("bounded"),
            || {
                body_calls += 1;
                body_calls
            },
        );
        assert_eq!((value, body_calls), (3, 3));
    }
}
