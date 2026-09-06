use super::super::trap::ViTrapFrame;
use super::entry::EntryFrame;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use qemu_exit::QEMUExit;

pub(super) const PKRU_A: u32 = 0x5555_5550;
pub(super) const PKRU_B: u32 = 0x5555_5544;
const SYSCALL_TOKEN: u64 = 0x91;
const RDX_TOKEN: u64 = 0x1122_3344_5566_7788;
const A_ARM: u64 = 0xa110_a110_a110_a110;
const A_SPIN: u64 = 0x51a1;
const A_RESUME: u64 = 0x01d7_c0de;
const B_REPORT: u64 = 0xb110_b110_b110_b110;
const A_FINAL: u64 = 0xa440_a440_a440_a440;
const READY: u8 = 1;
const B_PARKED: u8 = 2;
const A_FRESH: u8 = 3;
const A_INT80_RETURNED: u8 = 4;
const A_TIMER_ENTERED: u8 = 5;
const B_SYSCALL_RETURNED: u8 = 6;
const A_TIMER_RETURNED: u8 = 7;
const COMPLETE: u8 = 8;

static STATE: AtomicU8 = AtomicU8::new(0);
static TIMER_COUNT: AtomicU8 = AtomicU8::new(0);
static B_RETURN_RIP: AtomicU64 = AtomicU64::new(0);

use hal_arch_trait::kernel_abi::{
    vi_x86_idt_cpl3_park_b, vi_x86_idt_cpl3_switch_to_a, vi_x86_idt_cpl3_wake_b,
};

fn transition(from: u8, to: u8) {
    if STATE
        .compare_exchange(from, to, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        super::probe::fail();
    }
}

pub(super) fn arm(code_base: usize, b_return_offset: usize) {
    B_RETURN_RIP.store((code_base + b_return_offset) as u64, Ordering::Release);
    transition(0, READY);
}

pub(super) fn handle_syscall(frame: &mut ViTrapFrame) -> bool {
    if STATE.load(Ordering::Acquire) != READY {
        return false;
    }
    let b_return = B_RETURN_RIP.load(Ordering::Acquire);
    if frame.regs[17] as u64 != SYSCALL_TOKEN
        || frame.regs[12] as u64 != RDX_TOKEN
        || frame.regs[18] as u64 != b_return
        || frame.regs[20] != frame.regs[2]
        || frame.sepc as u64 != b_return
        || !super::cpl3_platform::valid_kernel_state(PKRU_B)
    {
        super::probe::fail();
    }
    transition(READY, B_PARKED);
    vi_x86_idt_cpl3_park_b();
    if STATE.load(Ordering::Acquire) != A_TIMER_ENTERED {
        super::probe::fail();
    }
    true
}

pub(super) fn handle_entry(frame: &mut EntryFrame) -> bool {
    match (STATE.load(Ordering::Acquire), frame.vector) {
        (B_PARKED, 0x80)
            if super::cpl3_platform::valid_entry(frame, 0x80, PKRU_A)
                && frame.rax == A_ARM
                && frame.r15 == u64::from(PKRU_A) =>
        {
            transition(B_PARKED, A_FRESH);
            super::cpl3_platform::arm_timer();
            true
        }
        (A_FRESH, 0x20)
            if super::cpl3_platform::valid_entry(frame, 0x20, PKRU_A)
                && frame.rax == A_SPIN
                && frame.r13 == u64::from(PKRU_A)
                && frame.r15 == u64::from(PKRU_A) =>
        {
            if TIMER_COUNT.fetch_add(1, Ordering::AcqRel) != 0 {
                super::probe::fail();
            }
            unsafe { super::super::apic::start_oneshot(0) };
            transition(A_FRESH, A_INT80_RETURNED);
            transition(A_INT80_RETURNED, A_TIMER_ENTERED);
            frame.rax = A_RESUME;
            false
        }
        (A_TIMER_ENTERED, 0x80)
            if super::cpl3_platform::valid_entry(frame, 0x80, PKRU_B)
                && frame.rax == B_REPORT
                && frame.r15 == u64::from(PKRU_B)
                && frame.r13 == RDX_TOKEN
                && frame.r12 == B_RETURN_RIP.load(Ordering::Acquire) =>
        {
            transition(A_TIMER_ENTERED, B_SYSCALL_RETURNED);
            vi_x86_idt_cpl3_switch_to_a();
            super::probe::fail()
        }
        (B_SYSCALL_RETURNED, 0x80)
            if super::cpl3_platform::valid_entry(frame, 0x80, PKRU_A)
                && frame.rax == A_FINAL
                && frame.r15 == u64::from(PKRU_A)
                && frame.r13 == u64::from(PKRU_A)
                && TIMER_COUNT.load(Ordering::Acquire) == 1 =>
        {
            transition(B_SYSCALL_RETURNED, A_TIMER_RETURNED);
            transition(A_TIMER_RETURNED, COMPLETE);
            super::super::uart_16550::puts("\nX86-IDT-CPL3: PASS fresh=ok int80=ok timer=32 switch=syscall-resume gs=kernel/user pkru=0/55555550/55555544\n");
            unsafe { qemu_exit::X86::new(0xf4, 33) }.exit_success()
        }
        (0, _) => false,
        _ => super::probe::fail(),
    }
}

pub(super) fn timer_after_eoi() {
    if STATE.load(Ordering::Acquire) != A_TIMER_ENTERED {
        super::probe::fail();
    }
    vi_x86_idt_cpl3_wake_b();
}

pub(super) fn timer_after_callback() {
    if STATE.load(Ordering::Acquire) != B_SYSCALL_RETURNED {
        super::probe::fail();
    }
}
