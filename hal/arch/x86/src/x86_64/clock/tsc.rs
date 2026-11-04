//! Time-stamp counter as a clocksource and as the TSC-deadline time base.

use core::arch::x86_64::{__cpuid, __cpuid_count, _mm_lfence, _rdtsc};
use core::sync::atomic::{AtomicU64, Ordering};

/// TSC frequency in Hz; 0 = unknown.
static HZ: AtomicU64 = AtomicU64::new(0);
/// `ns = tsc * MULT >> 32`, precomputed from `HZ`.
static MULT: AtomicU64 = AtomicU64::new(0);

/// Read the TSC, ordered after earlier loads.
#[inline]
pub fn read() -> u64 {
    // SAFETY: `lfence`/`rdtsc` are unprivileged and always present on x86_64.
    unsafe {
        _mm_lfence();
        _rdtsc()
    }
}

/// CPUID.80000007H:EDX[8]: the TSC rate does not change with P/C-states.
pub fn invariant() -> bool {
    let max_ext = __cpuid(0x8000_0000).eax;
    max_ext >= 0x8000_0007 && __cpuid(0x8000_0007).edx & (1 << 8) != 0
}

/// CPUID.1:ECX[24]: the LAPIC timer supports TSC-deadline mode.
pub fn deadline_supported() -> bool {
    __cpuid(1).ecx & (1 << 24) != 0
}

/// Frequency from CPUID leaf 0x15 when the crystal clock is enumerated.
pub fn hz_from_leaf_15() -> Option<u64> {
    if __cpuid(0).eax < 0x15 {
        return None;
    }
    let leaf = __cpuid_count(0x15, 0);
    hz_from_crystal(leaf.eax, leaf.ebx, leaf.ecx)
}

/// `crystal_hz * numerator / denominator`; `None` when any term is 0.
pub(super) fn hz_from_crystal(denominator: u32, numerator: u32, crystal_hz: u32) -> Option<u64> {
    if denominator == 0 || numerator == 0 || crystal_hz == 0 {
        return None;
    }
    Some(crystal_hz as u64 * numerator as u64 / denominator as u64)
}

/// Frequency published by the hypervisor timing leaf 0x40000010 (EAX = kHz),
/// implemented by VMware and by QEMU/KVM when the frequency is known.
pub fn hz_from_hypervisor_leaf() -> Option<u64> {
    if __cpuid(1).ecx & (1 << 31) == 0 || __cpuid(0x4000_0000).eax < 0x4000_0010 {
        return None;
    }
    let khz = __cpuid(0x4000_0010).eax;
    (khz != 0).then_some(khz as u64 * 1000)
}

/// Measure the TSC against `reference_ns` over `window_ns`.
pub fn calibrate(reference_ns: impl Fn() -> u64, window_ns: u64) -> Option<u64> {
    let start_ns = reference_ns();
    let start_tsc = read();
    let mut now = start_ns;
    while now.wrapping_sub(start_ns) < window_ns {
        core::hint::spin_loop();
        now = reference_ns();
    }
    let end_tsc = read();
    let elapsed = now.checked_sub(start_ns).filter(|ns| *ns != 0)?;
    let ticks = end_tsc.checked_sub(start_tsc)?;
    Some((ticks as u128 * 1_000_000_000 / elapsed as u128) as u64)
}

pub fn set_hz(hz: u64) {
    HZ.store(hz, Ordering::Relaxed);
    let mult = if hz == 0 {
        0
    } else {
        ((1_000_000_000u128 << 32) / hz as u128) as u64
    };
    MULT.store(mult, Ordering::Release);
}

pub fn hz() -> u64 {
    HZ.load(Ordering::Relaxed)
}

/// Nanoseconds since TSC reset; 0 when the frequency is unknown.
pub fn now_ns() -> u64 {
    ticks_to_ns(read(), MULT.load(Ordering::Acquire))
}

pub(super) fn ticks_to_ns(ticks: u64, mult: u64) -> u64 {
    ((ticks as u128 * mult as u128) >> 32) as u64
}

/// TSC ticks covering `ns` at the current frequency.
pub fn ns_to_ticks(ns: u64) -> u64 {
    (ns as u128 * HZ.load(Ordering::Relaxed) as u128 / 1_000_000_000) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crystal_frequency_needs_every_term() {
        // 24 MHz crystal, ratio 188/2 → 2.256 GHz.
        assert_eq!(hz_from_crystal(2, 188, 24_000_000), Some(2_256_000_000));
        assert_eq!(hz_from_crystal(2, 188, 0), None);
        assert_eq!(hz_from_crystal(0, 188, 24_000_000), None);
    }

    #[test]
    fn tick_conversion_matches_the_frequency() {
        let mult = ((1_000_000_000u128 << 32) / 2_000_000_000u128) as u64;
        assert_eq!(ticks_to_ns(2_000_000_000, mult), 1_000_000_000);
        assert_eq!(ticks_to_ns(u64::MAX, mult), u64::MAX / 2);
    }
}
