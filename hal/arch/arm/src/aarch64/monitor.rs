//! Raspberry Pi 3 non-VHE EL2 monitor. The host and its Cells stay at EL1/EL0.
//! Only the boot CPU that actually entered EL2 may use this private HVC gateway.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use super::{
    stage2_regs,
    vcpu::{run_vcpu_impl, AArch64Vcpu},
};

static BOOTED_EL2: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
static VERIFIED: AtomicBool = AtomicBool::new(false);
static ENTRY_EL: AtomicU8 = AtomicU8::new(1);

/// Read by the guest-entry assembly; set on the monitor CPU before each ERET.
#[no_mangle]
pub static mut PI_GUEST_VI: u64 = 0;

#[no_mangle]
pub extern "C" fn el2_mark_monitor_boot(entry_el: u64) {
    ENTRY_EL.store(entry_el as u8, Ordering::Release);
    BOOTED_EL2.store(true, Ordering::Release);
}

/// Original `CurrentEL` observed at `_start` (not the host's current EL1).
pub fn entry_el() -> u8 {
    ENTRY_EL.load(Ordering::Acquire)
}

/// True only once an HVC actually entered the monitor and installed its vectors.
#[inline]
pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// Boot-time guest smoke is required before HypervisorCap can be granted.
pub fn is_verified() -> bool {
    is_ready() && VERIFIED.load(Ordering::Acquire)
}

pub fn mark_verified() {
    if is_ready() {
        VERIFIED.store(true, Ordering::Release);
    }
}

/// Called after EL1 page-table activation. EL1-only firmware never executes HVC.
pub fn init() {
    if BOOTED_EL2.load(Ordering::Acquire) {
        if gateway(0, 0, 0, 0, 0) != 0 {
            // Never advertise virtualization after a failed monitor handshake.
            READY.store(false, Ordering::Release);
        }
    }
}

/// Operations: 1 bind/flush VMID, 2 unbind/flush, 3 flush all, 4 flush IPA,
/// 5 run vCPU. The immediate is private to the EL1 kernel; while a vCPU runs,
/// TPIDR_EL2 directs *all* lower-EL exceptions into the guest-exit trampoline.
#[inline(never)]
pub(crate) fn gateway(op: u64, a: u64, b: u64, c: u64, d: u64) -> u64 {
    if !BOOTED_EL2.load(Ordering::Acquire) {
        return u64::MAX;
    }
    let result: u64;
    // SAFETY: called only after entering EL2 at boot and installing the private
    // vector. Guest EL1 cannot invoke this path because TPIDR_EL2 is nonzero.
    unsafe {
        asm!("hvc #0xca11", inout("x0") op => result, in("x1") a,
             in("x2") b, in("x3") c, in("x4") d,
             lateout("x5") _, lateout("x6") _, lateout("x7") _,
             lateout("x8") _, lateout("x9") _, lateout("x10") _,
             lateout("x11") _, lateout("x12") _, lateout("x13") _,
             lateout("x14") _, lateout("x15") _, lateout("x16") _,
             lateout("x17") _, options(nostack));
    }
    result
}

/// Enter a guest using its own Stage-2 root. Returns with the vCPU's exit state
/// populated, and HCR_EL2.VM/VI cleared before restoring the EL1 host.
pub unsafe fn run(vcpu: &mut AArch64Vcpu, vmid: u16, root_pa: u64, vi: bool) {
    assert!(is_ready(), "Pi EL2 monitor not initialized");
    // Keep the host's physical C1 timer routed: HCR.IMO makes each 10 ms
    // tick exit an uncooperative guest and return the CPU to its EL1 host.
    let status = gateway(
        5,
        vcpu as *mut AArch64Vcpu as u64,
        vmid as u64,
        root_pa,
        vi as u64,
    );
    assert_eq!(status, 0, "Pi EL2 guest run failed");
}

/// The EL2-only dispatcher receives a full saved EL1 host register frame.
/// Return x0 is written back into the frame by the entry assembly.
#[no_mangle]
extern "C" fn pi_monitor_dispatch(frame: *mut u64) {
    // SAFETY: private VBAR_EL2 frame is on our dedicated 16-byte-aligned stack.
    let regs = unsafe { &mut *(frame as *mut [u64; 36]) };
    let esr: u64;
    unsafe {
        asm!("mrs {esr}, esr_el2", esr = out(reg) esr, options(nomem, nostack));
    }
    if esr >> 26 != 0x16 || esr & 0xffff != 0xca11 {
        loop {
            core::hint::spin_loop();
        }
    }
    let result = match regs[0] {
        0 => {
            unsafe {
                asm!("msr tpidr_el2, xzr", "mov {hcr}, #(1 << 31)",
                     "msr hcr_el2, {hcr}", "isb", hcr = out(reg) _,
                     options(nomem, nostack));
            }
            READY.store(true, Ordering::Release);
            0
        }
        1 if is_ready() => {
            unsafe {
                stage2_regs::monitor_enable_stage2(regs[1] as u16, regs[2]);
            }
            0
        }
        2 if is_ready() => {
            unsafe {
                stage2_regs::monitor_disable_stage2();
            }
            0
        }
        3 if is_ready() => {
            unsafe {
                stage2_regs::monitor_s2_tlb_flush_all();
            }
            0
        }
        4 if is_ready() => {
            unsafe {
                stage2_regs::monitor_s2_tlb_flush_ipa(regs[1]);
            }
            0
        }
        5 if is_ready() && regs[2] != 0 && regs[1] != 0 => {
            // Save HVC's return context: run_vcpu_impl programs ELR/SPSR_EL2
            // for the guest and its trap overwrites them with guest exit values.
            let host_pc: u64;
            let host_psr: u64;
            unsafe {
                asm!("mrs {pc}, elr_el2", "mrs {psr}, spsr_el2",
                     pc = out(reg) host_pc, psr = out(reg) host_psr,
                     options(nomem, nostack));
                stage2_regs::monitor_enable_stage2(regs[2] as u16, regs[3]);
                core::ptr::write_volatile(&raw mut PI_GUEST_VI, regs[4] & 1);
                let vcpu = &mut *(regs[1] as *mut AArch64Vcpu);
                let _exit = run_vcpu_impl(vcpu);
                core::ptr::write_volatile(&raw mut PI_GUEST_VI, 0);
                asm!("msr elr_el2, {pc}", "msr spsr_el2, {psr}", "isb",
                     pc = in(reg) host_pc, psr = in(reg) host_psr,
                     options(nomem, nostack));
            }
            0
        }
        _ => u64::MAX,
    };
    regs[0] = result;
}

// SP_EL2 is never shared with the EL1 host stack; the monitor does not switch
// EL1 translation off until its register bank is saved in run_vcpu_impl.
global_asm!(
    r#"
    .section .bss
    .balign 16
    .global __pi_monitor_stack
__pi_monitor_stack:
    .skip 16384
    .global __pi_monitor_stack_top
__pi_monitor_stack_top:

    .section .text.vectors
    .balign 2048
    .global __pi_monitor_vectors
__pi_monitor_vectors:
    .rept 8
    .balign 0x80; b pi_monitor_fault
    .endr
    .balign 0x80; b pi_monitor_sync
    .balign 0x80; b pi_monitor_irq
    .balign 0x80; b pi_monitor_fault
    .balign 0x80; b pi_monitor_fault
    .rept 4
    .balign 0x80; b pi_monitor_fault
    .endr

    .section .text
pi_monitor_fault:
    b pi_monitor_fault
pi_monitor_sync:
    // TPIDR_EL2 can only be set by the monitor while a guest is live.
    sub sp, sp, #16
    stp x0, x1, [sp]
    mrs x0, tpidr_el2
    cbz x0, pi_monitor_host
    str xzr, [x0, #520]
    b vt_vcpu_trap
pi_monitor_irq:
    sub sp, sp, #16
    stp x0, x1, [sp]
    mrs x0, tpidr_el2
    cbz x0, pi_monitor_fault
    mov x1, #1
    str x1, [x0, #520]
    b vt_vcpu_trap
pi_monitor_host:
    ldp x0, x1, [sp]
    add sp, sp, #16
    sub sp, sp, #(36 * 8)
    stp x0, x1, [sp, #0]
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30, [sp, #240]
    mov x0, sp
    bl pi_monitor_dispatch
    ldp x0, x1, [sp, #0]
    ldp x2, x3, [sp, #16]
    ldp x4, x5, [sp, #32]
    ldp x6, x7, [sp, #48]
    ldp x8, x9, [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldr x30, [sp, #240]
    add sp, sp, #(36 * 8)
    eret
"#
);
