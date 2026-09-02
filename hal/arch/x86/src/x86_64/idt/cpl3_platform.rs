use super::entry::EntryFrame;

const IA32_GS_BASE: u32 = 0xc000_0101;
const IA32_KERNEL_GS_BASE: u32 = 0xc000_0102;

unsafe extern "C" {
    static x86_idt_cpl3_user_start: u8;
    static x86_idt_cpl3_user_a: u8;
    static x86_idt_cpl3_user_b: u8;
    static x86_idt_cpl3_user_b_return: u8;
    static x86_idt_cpl3_user_end: u8;
}

fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { core::arch::asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi) };
    (u64::from(hi) << 32) | u64::from(lo)
}

fn rdpkru() -> u32 {
    let value: u32;
    unsafe { core::arch::asm!("rdpkru", in("ecx") 0_u32, out("eax") value, out("edx") _) };
    value
}

fn gs_pku() -> u32 {
    let value: u32;
    unsafe { core::arch::asm!("mov {0:e}, dword ptr gs:[16]", out(reg) value) };
    value
}

pub(super) fn valid_kernel_state(expected_pku: u32) -> bool {
    rdmsr(IA32_GS_BASE) == super::super::syscall::cpu_local_addr_for_test()
        && rdmsr(IA32_KERNEL_GS_BASE) == 0
        && rdpkru() == 0
        && gs_pku() == expected_pku
}

pub(super) fn valid_entry(frame: &EntryFrame, vector: u64, expected_pku: u32) -> bool {
    frame.vector == vector
        && frame.error == 0
        && frame.cs == 0x23
        && frame.old_ss() == Some(0x1b)
        && frame.old_rsp() == Some(frame.r14)
        && valid_kernel_state(expected_pku)
}

pub(super) fn arm_timer() {
    const LAPIC: usize = 0xfee0_0000;
    unsafe {
        core::ptr::write_volatile((LAPIC + 0x3e0) as *mut u32, 3);
        core::ptr::write_volatile((LAPIC + 0x320) as *mut u32, 0x20);
        core::ptr::write_volatile((LAPIC + 0x380) as *mut u32, 1_000_000);
    }
}

pub(super) fn require_pku() {
    if !super::super::pku::detect().pku || !super::probe::cpl0_complete() {
        super::probe::fail();
    }
    unsafe {
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4);
        cr4 |= 1 << 22;
        core::arch::asm!("mov cr4, {}", in(reg) cr4);
        core::arch::asm!(
            "wrpkru",
            in("eax") 0_u32,
            in("ecx") 0_u32,
            in("edx") 0_u32,
        );
        super::super::pku::PKU_ACTIVE = 1;
        core::arch::asm!("mov {}, cr4", out(reg) cr4);
        if cr4 & (1 << 22) == 0 {
            super::probe::fail();
        }
    }
    if rdpkru() != 0
        || rdmsr(IA32_GS_BASE) != super::super::syscall::cpu_local_addr_for_test()
        || rdmsr(IA32_KERNEL_GS_BASE) != 0
    {
        super::probe::fail();
    }
}

pub(super) fn user_image() -> (&'static [u8], usize, usize, usize) {
    let start = core::ptr::addr_of!(x86_idt_cpl3_user_start) as usize;
    let end = core::ptr::addr_of!(x86_idt_cpl3_user_end) as usize;
    let image = unsafe { core::slice::from_raw_parts(start as *const u8, end - start) };
    (
        image,
        core::ptr::addr_of!(x86_idt_cpl3_user_a) as usize - start,
        core::ptr::addr_of!(x86_idt_cpl3_user_b) as usize - start,
        core::ptr::addr_of!(x86_idt_cpl3_user_b_return) as usize - start,
    )
}
