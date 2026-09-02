use super::entry::EntryFrame;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use qemu_exit::QEMUExit;

const IDLE: u8 = 0;
const ARMED_BP: u8 = 1;
const SAW_BP: u8 = 2;
const VERIFIED_BP: u8 = 3;
const ARMED_GP: u8 = 4;
const SAW_GP: u8 = 5;
const VERIFIED_GP: u8 = 6;
const DF: u64 = 1 << 10;

const SENTINELS: [u64; 15] = [
    0x1111_1111_1111_1111,
    0x2222_2222_2222_2222,
    0x3333_3333_3333_3333,
    0x4444_4444_4444_4444,
    0x5555_5555_5555_5555,
    0x6666_6666_6666_6666,
    0x7777_7777_7777_7777,
    0x8888_8888_8888_8888,
    0x9999_9999_9999_9999,
    0xaaaa_aaaa_aaaa_aaaa,
    0xbbbb_bbbb_bbbb_bbbb,
    0xcccc_cccc_cccc_cccc,
    0xdddd_dddd_dddd_dddd,
    0xeeee_eeee_eeee_eeee,
    0xffff_ffff_ffff_ffff,
];

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub(super) struct ProbeCapture {
    regs: [u64; 15],
    rflags: u64,
}

const EMPTY_CAPTURE: ProbeCapture = ProbeCapture {
    regs: [0; 15],
    rflags: 0,
};

static STATE: AtomicU8 = AtomicU8::new(IDLE);
static EXPECTED_RIP: AtomicU64 = AtomicU64::new(0);
static RECOVERY_RIP: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub(super) static mut X86_IDT_BP_CAPTURE: ProbeCapture = EMPTY_CAPTURE;
#[no_mangle]
pub(super) static mut X86_IDT_GP_CAPTURE: ProbeCapture = EMPTY_CAPTURE;
#[no_mangle]
pub(super) static mut X86_IDT_SHIM_RSP: u64 = 0;
#[no_mangle]
pub(super) static mut X86_IDT_BP_CALLER_RSP: u64 = 0;
#[no_mangle]
pub(super) static mut X86_IDT_GP_CALLER_RSP: u64 = 0;

unsafe extern "C" {
    fn x86_idt_probe_bp();
    fn x86_idt_probe_gp();
}

pub(super) fn fail() -> ! {
    if !FAILED.swap(true, Ordering::AcqRel) {
        super::super::uart_16550::puts("\nX86-IDT-SELFTEST: FAIL\n");
    }
    unsafe { qemu_exit::X86::new(0xf4, 33) }.exit_failure()
}

fn transition(from: u8, to: u8) {
    if STATE
        .compare_exchange(from, to, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        fail();
    }
}

#[no_mangle]
extern "C" fn x86_idt_probe_arm_bp(expected_rip: u64) {
    EXPECTED_RIP.store(expected_rip, Ordering::Release);
    transition(IDLE, ARMED_BP);
}

#[no_mangle]
extern "C" fn x86_idt_probe_arm_gp(expected_rip: u64, recovery_rip: u64) {
    EXPECTED_RIP.store(expected_rip, Ordering::Release);
    RECOVERY_RIP.store(recovery_rip, Ordering::Release);
    transition(VERIFIED_BP, ARMED_GP);
}

fn registers(frame: &EntryFrame) -> [u64; 15] {
    [
        frame.rax, frame.rbx, frame.rcx, frame.rdx, frame.rbp, frame.rsi, frame.rdi, frame.r8,
        frame.r9, frame.r10, frame.r11, frame.r12, frame.r13, frame.r14, frame.r15,
    ]
}

fn live_rflags() -> u64 {
    let flags: u64;
    unsafe { core::arch::asm!("pushfq", "pop {}", out(reg) flags, options(preserves_flags)) };
    flags
}

fn validate_entry(frame: &EntryFrame, vector: u64, error: u64) {
    let base = frame as *const EntryFrame as u64;
    let shim_rsp = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(X86_IDT_SHIM_RSP)) };
    if registers(frame) != SENTINELS
        || frame.vector != vector
        || frame.error != error
        || frame.rip != EXPECTED_RIP.load(Ordering::Acquire)
        || frame.cs != 0x08
        || frame.rflags & DF == 0
        || live_rflags() & DF != 0
        || frame.old_rsp().is_some()
        || frame.old_ss().is_some()
        || frame.interrupted_rsp() != base + 160
        || (shim_rsp + 8) & 15 != 0
    {
        fail();
    }
}

pub(super) fn handle_entry(frame: &mut EntryFrame) -> bool {
    match STATE.load(Ordering::Acquire) {
        ARMED_BP => {
            validate_entry(frame, 3, 0);
            transition(ARMED_BP, SAW_BP);
            true
        }
        ARMED_GP => {
            validate_entry(frame, 13, 0xfffc);
            frame.rip = RECOVERY_RIP.load(Ordering::Acquire);
            transition(ARMED_GP, SAW_GP);
            true
        }
        _ => super::cpl3_probe::handle_entry(frame),
    }
}

fn validate_capture(capture: *const ProbeCapture, caller_rsp: *const u64) {
    let capture = unsafe { core::ptr::read_volatile(capture) };
    let caller_rsp = unsafe { core::ptr::read_volatile(caller_rsp) };
    if capture.regs != SENTINELS || capture.rflags & DF == 0 || caller_rsp & 15 != 8 {
        fail();
    }
}

pub(super) fn run_exception_probes() {
    unsafe { x86_idt_probe_bp() };
    if STATE.load(Ordering::Acquire) != SAW_BP {
        fail();
    }
    validate_capture(
        core::ptr::addr_of!(X86_IDT_BP_CAPTURE),
        core::ptr::addr_of!(X86_IDT_BP_CALLER_RSP),
    );
    transition(SAW_BP, VERIFIED_BP);

    unsafe { x86_idt_probe_gp() };
    if STATE.load(Ordering::Acquire) != SAW_GP {
        fail();
    }
    validate_capture(
        core::ptr::addr_of!(X86_IDT_GP_CAPTURE),
        core::ptr::addr_of!(X86_IDT_GP_CALLER_RSP),
    );
    transition(SAW_GP, VERIFIED_GP);
}
pub(super) fn cpl0_complete() -> bool {
    super::probe_timer::complete()
}

pub(super) fn timer_after_eoi() {
    super::probe_timer::after_eoi(STATE.load(Ordering::Acquire) == VERIFIED_GP);
}

pub(super) fn timer_after_callback() {
    super::probe_timer::after_callback();
}
