//! Local APIC driver (xAPIC MMIO or x2APIC MSR) and ISA IRQ routing.
//!
//! Mode: the boot loader requests x2APIC (Limine MP flag), so a CPU that
//! supports it arrives in x2APIC mode. [`init_mode`] latches that, enabling
//! x2APIC itself when the CPU supports it but firmware left xAPIC. Every
//! register access then goes through MSRs `0x800 + reg/16`; otherwise through
//! the MMIO page at the MADT LAPIC base.
//!
//! Limine does NOT identity-map MMIO regions; it only maps physical RAM via the
//! HHDM. MMIO accesses use `HHDM_BASE + PHYS_ADDR`, and `HHDM_BASE` is reset to
//! 0 after `init_kernel_paging_x86` identity-maps MMIO. Physical bases remain
//! zero until validated ACPI MADT data is installed via `set_lapic_phys`.
//!
//! APIC IDs above 255 need interrupt remapping for IOAPIC/MSI destinations
//! (plan 261103-2218 phase 12); until then such CPUs are refused.
use super::msr;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};

static HHDM_BASE: AtomicU64 = AtomicU64::new(0);
static LAPIC_PHYS: AtomicUsize = AtomicUsize::new(0);
static X2APIC: AtomicBool = AtomicBool::new(false);
pub const IPI_VECTOR: u8 = 0xf0;
const UNREGISTERED: u32 = u32::MAX;
// One atomic publication prevents duplicate physical IDs even if startup races.
static CPU_APIC_IDS: AtomicU64 = AtomicU64::new(u64::MAX);
const _: () = assert!(super::syscall::MAX_CPUS == 2);
static TIMER_COUNT: AtomicU32 = AtomicU32::new(0);

const IA32_APIC_BASE: u32 = 0x1B;
const APIC_BASE_EXTD: u64 = 1 << 10;
const APIC_BASE_EN: u64 = 1 << 11;
const X2APIC_MSR_BASE: u32 = 0x800;

/// CPUID.1:ECX[21].
pub fn x2apic_supported() -> bool {
    core::arch::x86_64::__cpuid(1).ecx & (1 << 21) != 0
}

/// Latch this CPU's LAPIC mode, enabling x2APIC when supported. Must run on
/// every CPU before its first LAPIC register access. `true` = x2APIC.
pub fn init_mode() -> bool {
    if !x2apic_supported() {
        X2APIC.store(false, Ordering::Release);
        return false;
    }
    // SAFETY: ring 0; IA32_APIC_BASE exists, and setting EXTD with EN on a
    // CPU that enumerates x2APIC is the architectural enable sequence.
    unsafe {
        let base = msr::read(IA32_APIC_BASE);
        if base & APIC_BASE_EXTD == 0 {
            msr::write(IA32_APIC_BASE, base | APIC_BASE_EN | APIC_BASE_EXTD);
        }
    }
    X2APIC.store(true, Ordering::Release);
    true
}

pub fn x2apic_active() -> bool {
    X2APIC.load(Ordering::Acquire)
}

/// This CPU's APIC ID (32-bit in x2APIC mode).
pub fn current_id() -> u32 {
    // SAFETY: ring 0; IA32_APIC_BASE is architectural.
    if unsafe { msr::read(IA32_APIC_BASE) } & APIC_BASE_EXTD != 0 {
        // SAFETY: x2APIC is enabled, so the ID MSR exists.
        unsafe { msr::read(X2APIC_MSR_BASE + 2) as u32 }
    } else {
        core::arch::x86_64::__cpuid(1).ebx >> 24
    }
}

/// Publish a unique physical LAPIC destination before CPU admission.
pub fn register_cpu(logical: usize, lapic_id: u32) -> bool {
    if logical >= super::syscall::MAX_CPUS || lapic_id > 255 {
        return false;
    }
    let shift = logical * 32;
    let other_shift = (1 - logical) * 32;
    let mut ids = CPU_APIC_IDS.load(Ordering::Acquire);
    for _ in 0..64 {
        if (ids >> other_shift) as u32 == lapic_id {
            return false;
        }
        let previous = (ids >> shift) as u32;
        if previous != UNREGISTERED {
            return previous == lapic_id;
        }
        let updated = (ids & !((u32::MAX as u64) << shift)) | ((lapic_id as u64) << shift);
        match CPU_APIC_IDS.compare_exchange(ids, updated, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(current) => ids = current,
        }
    }
    false
}

pub fn cpu_apic_id(logical: usize) -> Option<u32> {
    if logical >= super::syscall::MAX_CPUS {
        return None;
    }
    let id = (CPU_APIC_IDS.load(Ordering::Acquire) >> (logical * 32)) as u32;
    (id != UNREGISTERED).then_some(id)
}

/// Virtual address of an identity-mapped (after `set_hhdm_base(0)`) MMIO page.
pub(super) fn mmio_virt(phys: usize) -> usize {
    (HHDM_BASE.load(Ordering::Relaxed) as usize) + phys
}

#[inline]
fn lapic_base() -> usize {
    let phys = LAPIC_PHYS.load(Ordering::Relaxed);
    assert!(phys != 0, "LAPIC used before validated MADT setup");
    mmio_virt(phys)
}

fn lr(reg: usize) -> u32 {
    if x2apic_active() {
        // SAFETY: x2APIC mode is enabled; `reg` is an architectural register.
        unsafe { msr::read(X2APIC_MSR_BASE + (reg >> 4) as u32) as u32 }
    } else {
        // SAFETY: LAPIC is identity-mapped by init_kernel_paging_x86 at the parsed base.
        unsafe { core::ptr::read_volatile((lapic_base() + reg) as *const u32) }
    }
}

fn lw(reg: usize, v: u32) {
    if x2apic_active() {
        // SAFETY: x2APIC mode is enabled; `reg` is a writable register.
        unsafe { msr::write(X2APIC_MSR_BASE + (reg >> 4) as u32, v as u64) }
    } else {
        // SAFETY: LAPIC is identity-mapped by init_kernel_paging_x86 at the parsed base.
        unsafe { core::ptr::write_volatile((lapic_base() + reg) as *mut u32, v) }
    }
}

fn wait_icr_idle() -> bool {
    if x2apic_active() {
        return true; // x2APIC ICR writes have no delivery-status bit
    }
    for _ in 0..100_000 {
        if lr(0x300) & (1 << 12) == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

/// ICR value for a fixed IPI: `(high, low)` in xAPIC, one 64-bit MSR value in
/// x2APIC (destination in bits 63:32).
pub(super) fn icr_fixed(x2apic: bool, destination: u32, vector: u8) -> (u32, u32, u64) {
    if x2apic {
        (0, 0, (destination as u64) << 32 | vector as u64)
    } else {
        (destination << 24, vector as u32, 0)
    }
}

/// Send a directed fixed IPI. Delivery status is not an invalidate acknowledgement.
pub fn send_ipi(logical: usize) -> bool {
    let Some(id) = cpu_apic_id(logical) else {
        return false;
    };
    if LAPIC_PHYS.load(Ordering::Acquire) == 0 {
        return false;
    }
    let x2 = x2apic_active();
    let (high, low, wide) = icr_fixed(x2, id, IPI_VECTOR);
    let flags = super::context::save_and_disable_interrupts();
    let sent = if wait_icr_idle() {
        lw(0x280, 0);
        let _ = lr(0x280);
        if x2 {
            // SAFETY: x2APIC mode; the ICR MSR takes the whole 64-bit value.
            unsafe { msr::write(X2APIC_MSR_BASE + 0x30, wide) };
        } else {
            lw(0x310, high);
            lw(0x300, low);
        }
        let idle = wait_icr_idle();
        lw(0x280, 0);
        idle && lr(0x280) == 0
    } else {
        false
    };
    unsafe { super::context::restore_sstatus(flags) };
    sent
}

/// Enable only this AP's LAPIC (mode latch, software enable), with no timer
/// programmed. `false` when the LAPIC is unusable or not `lapic_id`.
pub fn init_secondary_lapic(lapic_id: u32) -> bool {
    init_mode();
    if LAPIC_PHYS.load(Ordering::Acquire) == 0 || current_id() != lapic_id {
        return false;
    }
    lw(0x0f0, 0x1ff);
    true
}

/// Configure only this AP's LAPIC using the BSP's calibrated periodic count.
pub fn init_secondary(lapic_id: u32) -> bool {
    let count = TIMER_COUNT.load(Ordering::Acquire);
    if count == 0 || !init_secondary_lapic(lapic_id) {
        return false;
    }
    lw(0x3e0, 0x3);
    lw(0x320, 0x20 | (1 << 17));
    lw(0x380, count);
    true
}

/// Put this CPU's LVT timer in TSC-deadline mode, vector 0x20, unmasked. The
/// caller arms it by writing IA32_TSC_DEADLINE.
pub fn set_tsc_deadline_mode() {
    lw(0x320, 0x20 | (0b10 << 17));
}

/// ISA IRQ → GSI override table from ACPI MADT type-2 entries.
/// Index = ISA IRQ (0–15); value = GSI. Identity-mapped by default.
static IRQ_OVERRIDES: [AtomicU32; 16] = {
    let mut table = [const { AtomicU32::new(0) }; 16];
    let mut irq = 0;
    while irq < 16 {
        table[irq] = AtomicU32::new(irq as u32);
        irq += 1;
    }
    table
};
/// MPS INTI flags of each override (polarity bits 0-1, trigger bits 2-3).
static IRQ_OVERRIDE_FLAGS: [AtomicU16; 16] = [const { AtomicU16::new(0) }; 16];

/// Store the HHDM base address so all LAPIC/IOAPIC helpers can use it.
/// Must be called before any other function in this module.
pub fn set_hhdm_base(offset: u64) {
    HHDM_BASE.store(offset, Ordering::Relaxed);
}

/// Install the LAPIC physical base from validated ACPI MADT data.
/// Call before `init_timers()`.
pub fn set_lapic_phys(base: u64) {
    LAPIC_PHYS.store(base as usize, Ordering::Relaxed);
}

/// Store MADT type-2 IRQ source overrides and their INTI flags. Call before
/// `init_input_irq()`.
pub fn set_irq_overrides(overrides: &[u32; 16], flags: &[u16; 16]) {
    for (i, (&gsi, &flag)) in overrides.iter().zip(flags).enumerate() {
        IRQ_OVERRIDES[i].store(gsi, Ordering::Relaxed);
        IRQ_OVERRIDE_FLAGS[i].store(flag, Ordering::Relaxed);
    }
}

/// Initialise LAPIC and configure periodic timer at ~100 Hz (vector 0x20).
pub fn init_lapic() {
    lw(0x0F0, 0x1FF); // SVR: enable LAPIC, spurious vector 0xFF
    lw(0x3E0, 0x3); // Timer divide-by-16
    lw(0x320, 0x20 | (1 << 17)); // LVT_TIMER: periodic mode, vector 0x20
    lw(0x380, 1_000_000 / 16); // Initial count (~100 Hz at 1 GHz LAPIC clock)
}

/// Signal End-of-Interrupt to the LAPIC.
pub fn eoi() {
    lw(0x0B0, 0);
}

/// Start LAPIC one-shot timer at `count` for calibration.
///
/// # Safety
/// LAPIC must be identity-mapped and enabled.
pub unsafe fn start_oneshot(count: u32) {
    lw(0x3E0, 0x3); // Timer divide-by-16
                    // bits 18:17 = 00 → one-shot mode; bit 16 = 1 → masked (silent calibration)
    lw(0x320, 0x20 | (1 << 16)); // LVT_TIMER: one-shot, masked, vector 0x20
    lw(0x380, count); // Initial count
}

/// Read the current LAPIC timer count (used by calibration).
pub fn read_current_count() -> u32 {
    lr(0x390)
}

/// Read the LVT Timer register (for debug/diagnostic only).
pub fn read_lvt_timer() -> u32 {
    lr(0x320)
}

/// Read the LAPIC initial count register (0x380).
pub fn read_initial_count() -> u32 {
    lr(0x380)
}

/// Re-initialise LAPIC timer with a calibrated ticks-per-ms value (~100 Hz).
///
/// Replaces the hardcoded initial count from `init_lapic`.
/// Safety: if calibration returned 0 or the 10× product overflows u32 (ticks_per_ms
/// > 429_496_729), fall back to a hardcoded estimate.
/// > Writing count=0 to the LAPIC initial-count register STOPS the timer entirely.
/// > A tiny count (< 10_000) means ~160 μs period → recursive ISR storm → crash.
pub fn init_lapic_calibrated(ticks_per_ms: u64) {
    // u32::MAX / 10 = 429_496_729.  If ticks_per_ms exceeds this, the cast wraps
    // to a tiny value (e.g. 429_496_730 × 10 = 4_294_967_300 → cast = 4) which
    // fires the timer every ~64 ns and causes an immediate recursive-ISR storm.
    const FALLBACK: u32 = 62_500; // ~10 ms at 100 MHz LAPIC / divide-16
    let safe_count = if ticks_per_ms == 0 || ticks_per_ms > (u32::MAX as u64 / 10) {
        FALLBACK
    } else {
        let c = (ticks_per_ms * 10) as u32;
        if c < 10_000 {
            FALLBACK
        } else {
            c
        }
    };
    lw(0x3E0, 0x3); // Timer divide-by-16
    lw(0x320, 0x20 | (1 << 17)); // LVT_TIMER: periodic, vector 0x20
    lw(0x380, safe_count);
    TIMER_COUNT.store(safe_count, Ordering::Release);
}

/// Route an ISA IRQ to an IDT vector on CPU 0, through the MADT override
/// table (GSI and polarity/trigger) and the owning I/O APIC.
pub fn ioapic_redirect(isa_irq: u8, vec: u8) -> bool {
    let index = isa_irq as usize;
    let (gsi, flags) = match (IRQ_OVERRIDES.get(index), IRQ_OVERRIDE_FLAGS.get(index)) {
        (Some(gsi), Some(flags)) => (gsi.load(Ordering::Relaxed), flags.load(Ordering::Relaxed)),
        _ => (isa_irq as u32, 0),
    };
    let destination =
        cpu_apic_id(0).expect("BSP APIC ID must be registered before IOAPIC redirection");
    super::ioapic::route_gsi(gsi, vec, destination, flags)
}

#[cfg(test)]
mod tests {
    use super::icr_fixed;

    #[test]
    fn icr_encoding_follows_the_apic_mode() {
        assert_eq!(icr_fixed(false, 3, 0xF0), (3 << 24, 0xF0, 0));
        assert_eq!(icr_fixed(true, 300, 0xF0), (0, 0, 300u64 << 32 | 0xF0));
    }
}
