//! Raspberry Pi 3 non-VHE EL2 monitor. The host and its Cells stay at EL1/EL0.
//! Only the boot CPU that actually entered EL2 may use this private HVC gateway.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use super::{
    stage2_regs,
    vcpu::{run_vcpu_impl, AArch64Vcpu},
};

static BOOTED_EL2: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
static VERIFIED: AtomicBool = AtomicBool::new(false);
static ENTRY_EL: AtomicU8 = AtomicU8::new(1);
// One-shot boot handshake diagnostics. MAX also means no HVC return observed;
// EL1 records the result itself after the HVC so failed initialization never
// relies on an uncached EL2 witness becoming visible.
static INIT_GATEWAY_RESULT: AtomicU64 = AtomicU64::new(u64::MAX);
static INIT_OP0_DISPATCHED: AtomicBool = AtomicBool::new(false);

/// EL2's Normal-WB view of one explicitly probed guest run. Values return in
/// HVC registers independently of the EL1 vCPU buffer; a broken writeback
/// path therefore cannot conceal itself behind that buffer's exit sentinels.
#[derive(Clone, Copy, Debug)]
pub struct GuestRunProbe {
    pub entry_pc: u64,
    pub stage2_l1: u64,
    pub stage2_l2: u64,
    pub stage2_l3: u64,
    pub instruction: u64,
    pub exit_esr: u64,
    pub exit_elr: u64,
    pub exit_is_irq: u64,
}

// Follow table pointers only inside Pi RAM; an invalid descriptor is evidence,
// not permission to dereference a potentially bogus or MMIO physical address.
const PI_RAM_END: u64 = 0x3f00_0000;

#[inline]
fn next_table(desc: u64) -> Option<*const u64> {
    let pa = desc & 0x0000_ffff_ffff_f000;
    if desc & 3 == 3 && pa >= 0x1000 && pa < PI_RAM_END {
        Some(pa as *const u64)
    } else {
        None
    }
}

/// Read a known smoke page's Stage-2 walk from EL2's coherent WB mapping.
/// An invalid/out-of-RAM intermediate descriptor stops the walk; it never
/// becomes an address for the next access.
unsafe fn probe_stage2(root_pa: u64, ipa: u64, guest_pa: u64, entry: u64) -> (u64, u64, u64, u64) {
    let l1_idx = ((ipa >> 30) & 0x3ff) as usize;
    let l2_idx = ((ipa >> 21) & 0x1ff) as usize;
    let l3_idx = ((ipa >> 12) & 0x1ff) as usize;
    let l1 = unsafe { core::ptr::read_volatile((root_pa as *const u64).add(l1_idx)) };
    let l2 = next_table(l1)
        .map(|table| unsafe { core::ptr::read_volatile(table.add(l2_idx)) })
        .unwrap_or(0);
    let l3 = next_table(l2)
        .map(|table| unsafe { core::ptr::read_volatile(table.add(l3_idx)) })
        .unwrap_or(0);
    let instruction = if entry >= ipa && entry - ipa <= 4096 - 4 && entry & 3 == 0 {
        // The caller provides a physically allocated page; do not trust the
        // descriptor PA when sampling code potentially still dirty in EL1.
        unsafe { core::ptr::read_volatile((guest_pa + entry - ipa) as *const u32) as u64 }
    } else {
        u64::MAX
    };
    (l1, l2, l3, instruction)
}

/// The identity root is supplied by EL1 in HVC registers, not inferred from
/// TTBR0_EL1 (which can be a private Cell root). Refuse the MMU transition if
/// its code/stack leaves lack identity or WB/shareable attributes, or sampled
/// register pages in either physical MMIO aperture are not Device mappings.
fn identity_leaf(root: u64, va: u64) -> Option<u64> {
    if root & 0xfff != 0 || root < 0x1000 || root >= PI_RAM_END {
        return None;
    }
    let l1 = unsafe {
        core::ptr::read_volatile((root as *const u64).add(((va >> 30) & 0x1ff) as usize))
    };
    let l2 = next_table(l1)?;
    let l2 = unsafe { core::ptr::read_volatile(l2.add(((va >> 21) & 0x1ff) as usize)) };
    let l3 = next_table(l2)?;
    let leaf = unsafe { core::ptr::read_volatile(l3.add(((va >> 12) & 0x1ff) as usize)) };
    ((leaf & (3 | (1 << 10)) == (3 | (1 << 10)))
        && (leaf & 0x0000_ffff_ffff_f000 == va & !0xfff))
        .then_some(leaf)
}

fn monitor_root_valid(root: u64) -> bool {
    extern "C" {
        static __pi_monitor_vectors: u8;
        static __pi_monitor_stack: u8;
        static __pi_monitor_stack_top: u8;
    }
    let normal = |va: u64, execute: bool| {
        va < PI_RAM_END
            && identity_leaf(root, va).is_some_and(|leaf| {
                leaf & (7 << 2) == 1 << 2
                    && leaf & (3 << 8) == 3 << 8
                    && (leaf & (1 << 54) == 0 || !execute)
            })
    };
    let vector = &raw const __pi_monitor_vectors as u64;
    let stack = &raw const __pi_monitor_stack as u64;
    let stack_top = &raw const __pi_monitor_stack_top as u64;
    if stack_top <= stack || stack_top & 0xf != 0 || vector & 0x7ff != 0 {
        return false;
    }
    if !normal(root, false)
        || !normal(pi_monitor_dispatch as *const () as u64, true)
        || !normal(super::el2::pi_monitor_mmu_init as *const () as u64, true)
        || !normal(vector, true)
        || !normal(vector + 2047, true)
    {
        return false;
    }
    let mut page = stack;
    while page < stack_top {
        if !normal(page, false)
            || !identity_leaf(root, page).is_some_and(|leaf| leaf & (1 << 7) == 0)
        {
            return false;
        }
        page += 4096;
    }
    if !normal(stack_top - 1, false)
        || !identity_leaf(root, stack_top - 1).is_some_and(|leaf| leaf & (1 << 7) == 0)
    {
        return false;
    }
    // BCM2837 peripheral window (including the mini UART) and local IRQ
    // controller must not turn into speculative/cached RAM under EL2.
    [0x3f21_5000, 0x4000_0000].into_iter().all(|va| {
        identity_leaf(root, va)
            .is_some_and(|leaf| leaf & ((7 << 2) | (3 << 8)) == 0)
    })
}

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
/// Boot provenance, raw op0 return (MAX if not observed or failed that way),
/// and EL2 validated-op0 dispatch witness. Diagnostics never grant HypervisorCap.
/// `hvc_result`: 0=coherent MMU active, 1=invalid identity root,
/// 2=firmware WXN disallows the shared writable kernel image, MAX=no return
/// or unexpected operation. Neither rejection sets READY.
pub fn probe_status() -> (bool, u64, bool) {
    (
        BOOTED_EL2.load(Ordering::Acquire),
        INIT_GATEWAY_RESULT.load(Ordering::Acquire),
        INIT_OP0_DISPATCHED.load(Ordering::Acquire),
    )
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

/// Called after EL1 has enabled its Normal-WB identity mapping. No EL1 access
/// to the EL2-private stack occurs during the HVC transition; EL2 does not
/// publish READY until its own WB/Inner-shareable regime is live.
pub fn init(root_pa: u64) {
    if BOOTED_EL2.load(Ordering::Acquire) {
        let result = gateway(0, root_pa, 0, 0, 0);
        INIT_GATEWAY_RESULT.store(result, Ordering::Release);
        if result != 0 {
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

/// Run exactly one guest interval with a register-returned EL2 visibility
/// probe. This is diagnostic only; it does not mark the smoke verified.
///
/// # Safety
/// `root_pa` must identify the live, 8 KiB Stage-2 root, and `guest_pa` a
/// mapped, allocated 4 KiB guest page corresponding to page-aligned
/// `guest_ipa`. All must remain allocated until the HVC returns.
pub unsafe fn run_probed(
    vcpu: &mut AArch64Vcpu,
    vmid: u16,
    root_pa: u64,
    vi: bool,
    guest_ipa: u64,
    guest_pa: u64,
) -> GuestRunProbe {
    assert!(is_ready(), "Pi EL2 monitor not initialized");
    let status: u64;
    let (entry_pc, stage2_l1, stage2_l2, stage2_l3, instruction, exit_esr, exit_elr, exit_is_irq):
        (u64, u64, u64, u64, u64, u64, u64, u64);
    unsafe {
        asm!(
            "hvc #0xca11",
            inout("x0") 6u64 => status,
            inout("x1") vcpu as *mut AArch64Vcpu as u64 => entry_pc,
            inout("x2") vmid as u64 => stage2_l1,
            inout("x3") root_pa => stage2_l2,
            inout("x4") vi as u64 => stage2_l3,
            inout("x5") guest_ipa => instruction,
            inout("x6") guest_pa => exit_esr,
            lateout("x7") exit_elr, lateout("x8") exit_is_irq,
            lateout("x9") _, lateout("x10") _, lateout("x11") _,
            lateout("x12") _, lateout("x13") _, lateout("x14") _,
            lateout("x15") _, lateout("x16") _, lateout("x17") _,
            options(nostack),
        );
    }
    assert_eq!(status, 0, "Pi EL2 guest run probe failed");
    GuestRunProbe {
        entry_pc, stage2_l1, stage2_l2, stage2_l3, instruction,
        exit_esr, exit_elr, exit_is_irq,
    }
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
        0 if !is_ready() => {
            let sctlr: u64;
            unsafe {
                asm!("mrs {sctlr}, sctlr_el2", sctlr = out(reg) sctlr,
                     options(nomem, nostack));
            }
            if sctlr & (1 << 19) != 0 {
                // Kernel-image leaves are WRITE|EXECUTE today. Never clear a
                // firmware WXN policy to make the monitor executable.
                2
            } else if !monitor_root_valid(regs[1]) {
                1
            } else {
                // The root and table frames were built before EL1 enabled
                // caching. HVC masks interrupts; the frame/stack are EL2-
                // private during the transient mixed-attribute interval.
                unsafe {
                    super::el2::pi_monitor_mmu_init(regs[1]);
                    asm!("msr tpidr_el2, xzr", "mov {hcr}, #(1 << 31)",
                         "msr hcr_el2, {hcr}", "isb", hcr = out(reg) _,
                         options(nomem, nostack));
                }
                INIT_OP0_DISPATCHED.store(true, Ordering::Release);
                READY.store(true, Ordering::Release);
                0
            }
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
        op @ (5 | 6) if is_ready() && regs[2] != 0 && regs[1] != 0 => {
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
                let probe = if op == 6 {
                    let entry = core::ptr::read_volatile(&vcpu.g_elr_el2);
                    let (l1, l2, l3, instruction) =
                        probe_stage2(regs[3], regs[5], regs[6], entry);
                    Some((entry, l1, l2, l3, instruction))
                } else {
                    None
                };
                let _exit = run_vcpu_impl(vcpu);
                if let Some((entry, l1, l2, l3, instruction)) = probe {
                    regs[1] = entry;
                    regs[2] = l1;
                    regs[3] = l2;
                    regs[4] = l3;
                    regs[5] = instruction;
                    regs[6] = core::ptr::read_volatile(&vcpu.exit_esr);
                    regs[7] = core::ptr::read_volatile(&vcpu.exit_elr);
                    regs[8] = core::ptr::read_volatile(&vcpu.exit_is_irq);
                }
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
