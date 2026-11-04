//! ACPI PM timer: a 3.579545 MHz free-running 24- or 32-bit counter in port
//! I/O space (FADT `PM_TMR_BLK`).
//!
//! Wrap extension is lock-free: the 64-bit extended count lives in one atomic
//! and only moves forward through `fetch_max`. A raw value at most half a wrap
//! behind the extended count is a stale read from a racing CPU, not a wrap,
//! and leaves the count unchanged. This stays correct as long as something
//! reads the timer at least once per half wrap (≈2.3 s at 24 bits). The
//! scheduler tick does that through [`super::on_tick`].

use core::sync::atomic::{AtomicU16, AtomicU64, AtomicU8, Ordering};

pub const HZ: u64 = 3_579_545;

static PORT: AtomicU16 = AtomicU16::new(0);
static BITS: AtomicU8 = AtomicU8::new(0);
static EXTENDED: AtomicU64 = AtomicU64::new(0);

/// Install the timer port. `bits` is 24 or 32.
pub fn init(port: u16, bits: u8) {
    PORT.store(port, Ordering::Relaxed);
    BITS.store(if bits == 32 { 32 } else { 24 }, Ordering::Relaxed);
    EXTENDED.store(read_raw() as u64, Ordering::Release);
}

pub fn present() -> bool {
    PORT.load(Ordering::Relaxed) != 0
}

fn read_raw() -> u32 {
    let port = PORT.load(Ordering::Relaxed);
    let value: u32;
    // SAFETY: ring-0 port read of the firmware-declared PM timer port.
    unsafe {
        core::arch::asm!("in eax, dx", in("dx") port, out("eax") value,
            options(nomem, nostack, preserves_flags));
    }
    value
}

/// Fold a raw counter reading into the previous extended count.
pub(super) fn extend(previous: u64, raw: u32, bits: u8) -> u64 {
    let modulus = 1u64 << bits;
    let mask = modulus - 1;
    let delta = (raw as u64).wrapping_sub(previous) & mask;
    if delta >= modulus / 2 {
        previous // stale: another CPU already folded a newer reading
    } else {
        previous + delta
    }
}

/// Extended ticks since an arbitrary origin, never decreasing.
pub fn ticks() -> u64 {
    let candidate = extend(
        EXTENDED.load(Ordering::Acquire),
        read_raw(),
        BITS.load(Ordering::Relaxed),
    );
    EXTENDED
        .fetch_max(candidate, Ordering::AcqRel)
        .max(candidate)
}

pub fn now_ns() -> u64 {
    (ticks() as u128 * 1_000_000_000 / HZ as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::extend;

    #[test]
    fn a_24_bit_wrap_carries_into_the_extended_count() {
        assert_eq!(extend(0x00FF_FFF0, 0x10, 24), 0x0100_0010);
        assert_eq!(extend(0x0100_0010, 0x20, 24), 0x0100_0020);
        // Bits above the counter width in the raw read are ignored.
        assert_eq!(extend(0x10, 0xFF00_0011, 24), 0x11);
        assert_eq!(extend(0xFFFF_FFF0, 0x5, 32), 0x1_0000_0005);
    }

    #[test]
    fn a_stale_read_from_a_racing_cpu_is_not_a_wrap() {
        // CPU B already folded 0x20; CPU A's older raw 0x10 must not add a
        // whole wrap (a 4.7 s forward jump).
        assert_eq!(extend(0x0100_0020, 0x10, 24), 0x0100_0020);
        assert_eq!(extend(0x20, 0x10, 32), 0x20);
    }
}
