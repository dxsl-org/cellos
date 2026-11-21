//! Guest virtual timer IRQ policy on the Pi. On Cortex-A53 EL1's CNTV registers
//! cannot be trapped by EL2; the monitor snapshots the architected timer on VM
//! exit. (QEMU virt: the kernel samples the same state before each entry.)

pub const VIRT_TIMER_PPI: u32 = 27;

pub fn guest_timer_asserted(ctl: u64, cval: u64) -> bool {
    let now: u64;
    // SAFETY: a register read with no memory access; Pi boot sets
    // CNTKCTL_EL1.EL0PCTEN so EL0 does not trap. CNTVOFF_EL2 = 0, so the host
    // physical count equals the guest virtual count.
    unsafe {
        core::arch::asm!("mrs {}, cntpct_el0", out(reg) now, options(nomem, nostack));
    }
    ctl & 3 == 1 && (now.wrapping_sub(cval) as i64) >= 0
}
