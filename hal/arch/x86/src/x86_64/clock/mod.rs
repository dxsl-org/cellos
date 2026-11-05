//! x86_64 timekeeping: one monotonic clocksource and the LAPIC clockevent.
//!
//! **Clocksource**, chosen once on the BSP by [`select`], in this order:
//! 1. TSC, when it is invariant or the hypervisor publishes its frequency.
//!    The frequency comes from CPUID 0x15, the hypervisor leaf 0x40000010, or
//!    a calibration against HPET or the PM timer.
//! 2. kvmclock, under KVM with a host-declared stable TSC.
//! 3. HPET.
//! 4. ACPI PM timer (port I/O).
//!
//! When none is available the timer gate stays closed. [`now_ns`] starts near
//! 0 at selection, and a global `fetch_max` keeps it non-decreasing across
//! CPUs and across a source demotion.
//!
//! **Clockevent** (scheduler tick, 100 Hz, vector 0x20): TSC-deadline when the
//! CPU supports it and the TSC frequency is known; otherwise the periodic LAPIC
//! timer calibrated against the clocksource.
//!
//! The G1 private lane does not use this module (`init_timers_observed` keeps
//! its HPET-only contract).

pub mod kvmclock;
pub mod pmtimer;
pub mod tsc;

use super::{apic, hpet};
use core::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, AtomicUsize, Ordering};

/// The selected monotonic time base.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSource {
    None = 0,
    Tsc = 1,
    Kvmclock = 2,
    Hpet = 3,
    PmTimer = 4,
}

impl ClockSource {
    fn from_u8(raw: u8) -> Self {
        match raw {
            1 => Self::Tsc,
            2 => Self::Kvmclock,
            3 => Self::Hpet,
            4 => Self::PmTimer,
            _ => Self::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Tsc => "tsc",
            Self::Kvmclock => "kvmclock",
            Self::Hpet => "hpet",
            Self::PmTimer => "pmtimer",
        }
    }
}

/// The tick clockevent in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clockevent {
    /// TSC-deadline mode; re-armed every tick with this many TSC ticks.
    TscDeadline { period_ticks: u64 },
    /// Periodic LAPIC timer with this calibrated rate.
    LapicPeriodic { ticks_per_ms: u64 },
}

const TICK_NS: u64 = 10_000_000; // 100 Hz scheduler tick
const CALIBRATION_NS: u64 = 10_000_000;
const TSC_CALIBRATION_NS: u64 = 50_000_000;
/// AP TSC behind the published clock by more than this demotes the TSC.
const TSC_SKEW_LIMIT_NS: u64 = 1_000;

static SOURCE: AtomicU8 = AtomicU8::new(ClockSource::None as u8);
static OFFSET_NS: AtomicI64 = AtomicI64::new(0);
static LAST_NS: AtomicU64 = AtomicU64::new(0);
static DEADLINE_PERIOD: AtomicU64 = AtomicU64::new(0);
static PVCLOCK_VIRT: AtomicUsize = AtomicUsize::new(0);
static PVCLOCK_PHYS: AtomicU64 = AtomicU64::new(0);
static TSC_DEMOTIONS: AtomicU64 = AtomicU64::new(0);

const IA32_TSC_DEADLINE: u32 = 0x6E0;

/// Bytes of the kvmclock page handed over by [`set_pvclock_area`]: one
/// [`kvmclock::AREA_BYTES`] slot per logical CPU.
const PVCLOCK_PAGE_BYTES: usize = 4096;
const _: () = assert!(super::syscall::MAX_CPUS * kvmclock::AREA_BYTES <= PVCLOCK_PAGE_BYTES);

/// Offer a zeroed, dedicated 4 KiB page (HHDM `virt`, physical `phys`) for
/// kvmclock. Call before [`select`]; ignored when KVM is absent. Logical CPU
/// `n` owns bytes `n * 32 .. n * 32 + 32` of it: the boot CPU registers slot 0
/// in [`select`], each AP its own slot in [`start_secondary`].
pub fn set_pvclock_area(virt: usize, phys: u64) {
    PVCLOCK_VIRT.store(virt, Ordering::Relaxed);
    PVCLOCK_PHYS.store(phys, Ordering::Relaxed);
}

fn raw_ns(source: ClockSource) -> u64 {
    match source {
        ClockSource::None => 0,
        ClockSource::Tsc => tsc::now_ns(),
        ClockSource::Kvmclock => kvmclock::now_ns(),
        ClockSource::Hpet => hpet::now_ns(),
        ClockSource::PmTimer => pmtimer::now_ns(),
    }
}

pub fn source() -> ClockSource {
    ClockSource::from_u8(SOURCE.load(Ordering::Acquire))
}

/// Monotonic nanoseconds since clock selection; 0 before selection.
pub fn now_ns() -> u64 {
    let source = source();
    if source == ClockSource::None {
        return 0;
    }
    let shifted = (raw_ns(source) as i64).wrapping_add(OFFSET_NS.load(Ordering::Acquire));
    let value = shifted.max(0) as u64;
    LAST_NS.fetch_max(value, Ordering::AcqRel).max(value)
}

/// Install `source` so that [`now_ns`] continues from its last value.
fn install(source: ClockSource) {
    let last = LAST_NS.load(Ordering::Acquire);
    let raw = raw_ns(source) as i64;
    OFFSET_NS.store((last as i64).wrapping_sub(raw), Ordering::Release);
    SOURCE.store(source as u8, Ordering::Release);
}

/// Choose the clocksource. `hpet_ready` means `hpet::init` ran on a
/// validated base; a PM timer counts only if `pmtimer::init` ran.
pub fn select(hpet_ready: bool) -> ClockSource {
    let reference = if hpet_ready {
        Some(hpet::now_ns as fn() -> u64)
    } else if pmtimer::present() {
        Some(pmtimer::now_ns as fn() -> u64)
    } else {
        None
    };
    let hypervisor_hz = tsc::hz_from_hypervisor_leaf();
    let known_hz = tsc::hz_from_leaf_15().or(hypervisor_hz);
    let calibrated = || reference.and_then(|now| tsc::calibrate(now, TSC_CALIBRATION_NS));

    let tsc_reliable = tsc::invariant() || hypervisor_hz.is_some();
    let chosen = if let Some(hz) = tsc_reliable.then(|| known_hz.or_else(calibrated)).flatten() {
        tsc::set_hz(hz);
        ClockSource::Tsc
    } else if pvclock_ready() {
        ClockSource::Kvmclock
    } else if hpet_ready {
        ClockSource::Hpet
    } else if pmtimer::present() {
        ClockSource::PmTimer
    } else {
        ClockSource::None
    };
    if chosen == ClockSource::None {
        return chosen;
    }
    // The deadline clockevent needs the TSC rate even when TSC is not the
    // clocksource; measure it against the chosen source.
    if tsc::hz() == 0 && tsc::deadline_supported() {
        let chosen_now = move || raw_ns(chosen);
        if let Some(hz) = known_hz.or_else(|| tsc::calibrate(chosen_now, TSC_CALIBRATION_NS)) {
            tsc::set_hz(hz);
        }
    }
    install(chosen);
    chosen
}

fn pvclock_ready() -> bool {
    let virt = PVCLOCK_VIRT.load(Ordering::Relaxed);
    // SAFETY: `set_pvclock_area` hands over a dedicated zeroed area.
    virt != 0 && unsafe { kvmclock::init(virt, PVCLOCK_PHYS.load(Ordering::Relaxed)) }
}

/// Start the BSP tick. The LAPIC must already be enabled.
pub fn start_clockevent() -> Clockevent {
    if tsc::deadline_supported() && tsc::hz() != 0 {
        let period_ticks = tsc::ns_to_ticks(TICK_NS).max(1);
        DEADLINE_PERIOD.store(period_ticks, Ordering::Release);
        arm_deadline_mode(period_ticks);
        return Clockevent::TscDeadline { period_ticks };
    }
    let ticks_per_ms = calibrate_lapic();
    apic::init_lapic_calibrated(ticks_per_ms);
    Clockevent::LapicPeriodic { ticks_per_ms }
}

fn arm_deadline_mode(period_ticks: u64) {
    apic::set_tsc_deadline_mode();
    // SDM 11.5.4.1: order the LVT write before the deadline MSR write.
    core::sync::atomic::fence(Ordering::SeqCst);
    // SAFETY: TSC-deadline mode is supported (CPUID.1:ECX[24]) and selected.
    unsafe { super::msr::write(IA32_TSC_DEADLINE, tsc::read().wrapping_add(period_ticks)) };
}

/// Per-tick work from the timer interrupt, after EOI: re-arm the deadline and
/// keep the PM timer's wrap extension current.
pub fn on_tick() {
    let period = DEADLINE_PERIOD.load(Ordering::Relaxed);
    if period != 0 {
        // SAFETY: deadline mode was armed by `start_clockevent`/`start_secondary`.
        unsafe { super::msr::write(IA32_TSC_DEADLINE, tsc::read().wrapping_add(period)) };
    }
    if source() == ClockSource::PmTimer {
        let _ = pmtimer::ticks();
    }
}

/// Start this AP's tick the same way as the BSP's. Returns `false` when the
/// BSP has no clockevent to copy. Runs on the AP after `syscall::install_cpu`
/// (its logical id is readable through GS).
pub fn start_secondary(lapic_id: u32) -> bool {
    register_secondary_pvclock();
    check_secondary_tsc();
    let period = DEADLINE_PERIOD.load(Ordering::Acquire);
    if period != 0 {
        if !apic::init_secondary_lapic(lapic_id) {
            return false;
        }
        arm_deadline_mode(period);
        return true;
    }
    apic::init_secondary(lapic_id)
}

/// Register this AP's own pvclock slot when the boot CPU's kvmclock
/// registration is active (otherwise kvmclock is unused on every CPU).
///
/// The boot CPU's area is what [`now_ns`] reads, which is valid only while the
/// host declares the TSC stable on every vCPU. An AP whose own area lacks that
/// declaration therefore makes kvmclock unusable system-wide: an active
/// kvmclock clocksource is demoted (as a skewed TSC is), and the boot CPU's
/// registration is retired from reads ([`kvmclock::retire`]) so that no later
/// fallback — a TSC demotion on another AP — can pick kvmclock again. The one
/// exception is an active kvmclock with nothing to demote to: it stays, since
/// retiring it would stall [`now_ns`] (the `fetch_max` guard keeps it
/// monotonic either way).
fn register_secondary_pvclock() {
    if kvmclock::now_ns() == 0 {
        return;
    }
    let logical = super::syscall::current_cpu_id();
    let offset = logical * kvmclock::AREA_BYTES;
    let virt = PVCLOCK_VIRT.load(Ordering::Relaxed);
    if logical == 0 || virt == 0 || offset + kvmclock::AREA_BYTES > PVCLOCK_PAGE_BYTES {
        return;
    }
    let phys = PVCLOCK_PHYS.load(Ordering::Relaxed) + offset as u64;
    // SAFETY: `set_pvclock_area` handed over a dedicated zeroed page; this
    // logical CPU owns its slot and registers it once, on itself.
    if unsafe { kvmclock::register(virt + offset, phys) } {
        return;
    }
    if source() == ClockSource::Kvmclock {
        let Some(next) = fallback_source(false) else {
            return;
        };
        install(next);
    }
    kvmclock::retire();
}

/// The source a demotion falls back to, best first; `None` when nothing better
/// exists (the `fetch_max` guard then keeps time monotonic).
fn fallback_source(kvmclock_usable: bool) -> Option<ClockSource> {
    if kvmclock_usable && kvmclock::now_ns() != 0 {
        Some(ClockSource::Kvmclock)
    } else if hpet::present() {
        Some(ClockSource::Hpet)
    } else if pmtimer::present() {
        Some(ClockSource::PmTimer)
    } else {
        None
    }
}

/// Demote the TSC when this CPU's TSC is behind the published clock: a
/// skewed TSC would make `now_ns` stall on this CPU.
fn check_secondary_tsc() {
    if source() != ClockSource::Tsc {
        return;
    }
    let mine = (tsc::now_ns() as i64).wrapping_add(OFFSET_NS.load(Ordering::Acquire));
    let published = LAST_NS.load(Ordering::Acquire) as i64;
    if mine + (TSC_SKEW_LIMIT_NS as i64) < published {
        TSC_DEMOTIONS.fetch_add(1, Ordering::Relaxed);
        if let Some(next) = fallback_source(true) {
            install(next);
        }
    }
}

/// How many times an AP's TSC skew demoted the TSC.
pub fn tsc_demotions() -> u64 {
    TSC_DEMOTIONS.load(Ordering::Relaxed)
}

/// LAPIC ticks per millisecond, measured against the clocksource.
fn calibrate_lapic() -> u64 {
    const START: u32 = u32::MAX;
    // SAFETY: the LAPIC is enabled and identity-mapped (init_timers order).
    unsafe { apic::start_oneshot(START) };
    let start = now_ns();
    let mut now = start;
    while now - start < CALIBRATION_NS {
        core::hint::spin_loop();
        now = now_ns();
    }
    let elapsed_ticks = (START - apic::read_current_count()) as u64;
    elapsed_ticks * 1_000_000 / (now - start)
}
