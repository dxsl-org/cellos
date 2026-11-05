#[cfg(all(not(test), target_os = "none"))]
use crate::syscall::sys_log;
#[cfg(all(not(test), target_os = "none"))]
use core::panic::PanicInfo;

/// Root `_start` against `--gc-sections`.
///
/// `_start` lives in `.text.boot`, which cell linker scripts pull without
/// `KEEP`. Nothing in Rust code *references* the symbol — it is only named by
/// the linker script's `ENTRY(_start)`, which is NOT a GC root — so it was
/// collected out of every cell, and the ELF entry silently fell through to
/// `main`. This `#[used]` pointer creates a real relocation to `_start` from an
/// object that is always linked (this module holds the `#[panic_handler]`),
/// keeping it against GC. Belt-and-suspenders alongside `ENTRY(_start)`.
#[cfg(any(target_os = "none", target_os = "cellos"))]
#[used]
static _KEEP_START: unsafe extern "C" fn() -> ! = _start;

#[cfg(any(target_os = "none", target_os = "cellos"))]
mod libc_hook;

/// Cell entry point — sets up gp/sp and jumps into the ostd runtime.
///
/// Sequence: `__cellos_libc_start(entry_sp)` (ostd's weak default runs
/// `.init_array`; a C library may replace it, see `startup/libc_hook.rs`),
/// then `main`, then `Exit(0)`. `entry_sp` is the kernel's Cell initial stack
/// (argc, argv, envp, auxv; CP1 cell-initial-stack-auxv, AX-2).
///
/// # Safety
/// Never call from Rust: the kernel loader jumps here exactly once on a fresh
/// stack; the naked asm assumes registers per the cell spawn ABI.
#[cfg(any(target_os = "none", target_os = "cellos"))]
#[no_mangle]
#[unsafe(naked)]
#[link_section = ".text.boot"]
pub unsafe extern "C" fn _start() -> ! {
    #[cfg(target_arch = "riscv64")]
    core::arch::naked_asm!(
        ".option push",
        ".option norelax",
        "la gp, __global_pointer$",
        ".option pop",
        "mv a0, sp",
        "andi sp, sp, -16",
        "call __cellos_libc_start",
        "call main",
        "li a7, 60", // ViSyscall::Exit
        "li a0, 0",  // exit code = 0 in a0 (ViCell ABI: syscall nr in a7, arg in a0)
        "ecall",
        "1: j 1b"
    );
    // ViCell ARM64 ABI: x0=syscall_nr, x1=a0 (exit code).
    // Stack is kernel-aligned on entry; skip re-alignment to avoid clobbering sp.
    // Every address is formed PC-relatively (`bl`, adrp+add in the hook): an
    // R_AARCH64_ABS64 literal is rejected by rust-lld under -pie (Spec 17 §8).
    #[cfg(target_arch = "aarch64")]
    core::arch::naked_asm!(
        "mov  x0, sp",
        "bl   __cellos_libc_start",
        "bl   main",
        "mov  x0, #60", // ViSyscall::Exit
        "mov  x1, #0",  // exit code = 0
        "svc  #0",
        "1: b 1b"
    );
    // ViCell x86_64 ABI: RAX=syscall_nr, RDI=a0.
    // The kernel-supplied stack is 16-byte-aligned at entry; AND to guarantee it
    // before the CALL so the SysV AMD64 ABI requirement is always satisfied.
    // sys_exit(0): ViSyscall::Exit = 60, exit code in RDI = 0.
    #[cfg(target_arch = "x86_64")]
    core::arch::naked_asm!(
        "mov rdi, rsp",
        "and rsp, -16",
        "call __cellos_libc_start",
        "call main",
        "mov rax, 60",  // ViSyscall::Exit
        "xor rdi, rdi", // exit code = 0
        "syscall",
        "2: jmp 2b",
    );
}

// User applications must define `fn main() -> !` or `fn main()`.
// Since we don't have a standardized `main` signature yet in ostd macro,
// we will assume the app defines `no_mangle pub extern "C" fn main()`.
extern "C" {
    fn main();
}

#[no_mangle]
pub extern "C" fn generic_main() -> ! {
    unsafe {
        main();
    }
    // If main returns, we exit
    crate::syscall::sys_exit(0);
}

#[cfg(all(not(test), target_os = "none"))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Log panic
    // We don't have a proper writer yet, so just stringify manually?
    // Or just sys_log("PANIC!");
    let _ = sys_log("PANIC: Application crashed!\n");
    if let Some(location) = info.location() {
        // simple formatting
        let _ = sys_log("Location: ");
        let _ = sys_log(location.file());
    }

    // Exit
    crate::syscall::sys_exit(1);
}
