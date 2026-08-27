#![no_std]
#![no_main]

use core::{arch::asm, panic::PanicInfo};

#[cfg(target_arch = "x86_64")]
const SYS_WRITE: usize = 1;
#[cfg(target_arch = "aarch64")]
const SYS_WRITE: usize = 64;

#[cfg(target_arch = "x86_64")]
const SYS_EXIT: usize = 60;
#[cfg(target_arch = "aarch64")]
const SYS_EXIT: usize = 93;

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".global _start",
    "_start:",
    "mov rdi, [rsp]",
    "lea rsi, [rsp + 8]",
    "jmp main"
);

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    ".global _start",
    "_start:",
    "mov x0, sp",
    "add x1, sp, #8",
    "b main"
);

#[no_mangle]
pub unsafe extern "C" fn main(argc: isize, argv: *const *const u8) -> ! {
    let mut mode = 0; // 0=all, 1=bounds, 2=desc, 3=backend, 4=reset, 5=budget
    if argc > 1 {
        let arg1_ptr = *argv.add(1);
        let arg1 = core::slice::from_raw_parts(arg1_ptr, 1);
        if arg1[0] == b'1' { mode = 1; }
        if arg1[0] == b'2' { mode = 2; }
        if arg1[0] == b'3' { mode = 3; }
        if arg1[0] == b'4' { mode = 4; }
        if arg1[0] == b'5' { mode = 5; }
    }

    print(b"Starting Hostile Probe...\n");

    if mode == 0 || mode == 1 {
        print(b"[HOSTILE_PROBE] BOUNDS_TEST_NOT_APPLICABLE\n");
    }

    if mode == 0 || mode == 2 {
        print(b"[HOSTILE_PROBE] DESC_TEST_NOT_APPLICABLE\n");
    }

    if mode == 0 || mode == 3 {
        print(b"[HOSTILE_PROBE] BACKEND_TEST_NOT_APPLICABLE\n");
    }

    if mode == 0 || mode == 4 {
        print(b"[HOSTILE_PROBE] RESET_TEST_CONTROLLED_BY_INIT\n");
    }

    if mode == 0 || mode == 5 {
        print(b"[HOSTILE_PROBE] BUDGET_TEST_STARTED\n");
        if mode == 5 {
            // PAUSE is surfaced as HLT; this loop must exercise budget preemption.
            loop {
                asm!("", options(nomem, nostack, preserves_flags));
            }
        }
    }
    
    exit(0);
}

fn print(message: &[u8]) {
    unsafe {
        syscall3(SYS_WRITE, 1, message.as_ptr() as usize, message.len());
    }
}

fn exit(code: usize) -> ! {
    unsafe {
        syscall3(SYS_EXIT, code, 0, 0);
    }
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(number: usize, a0: usize, a1: usize, a2: usize) -> isize {
    let result: usize;
    asm!(
        "syscall",
        in("rax") number,
        in("rdi") a0,
        in("rsi") a1,
        in("rdx") a2,
        out("rcx") _,
        out("r11") _,
        lateout("rax") result,
        options(nostack, preserves_flags)
    );
    result as isize
}

#[cfg(target_arch = "aarch64")]
unsafe fn syscall3(number: usize, a0: usize, a1: usize, a2: usize) -> isize {
    let result: usize;
    asm!(
        "svc #0",
        in("x8") number,
        in("x0") a0,
        in("x1") a1,
        in("x2") a2,
        lateout("x0") result,
        options(nostack, preserves_flags)
    );
    result as isize
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(127)
}
