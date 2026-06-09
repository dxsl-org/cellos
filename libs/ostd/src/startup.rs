use crate::syscall::sys_log;
use core::panic::PanicInfo;

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
        "andi sp, sp, -16",
        "call main",
        "li a7, 60",   // ViSyscall::Exit
        "li a0, 0",    // exit code = 0 in a0 (ViCell ABI: syscall nr in a7, arg in a0)
        "ecall",
        "1: j 1b"
    );
    // ViCell ARM64 ABI: x0=syscall_nr, x1=a0 (exit code).
    // Stack is kernel-aligned on entry; skip re-alignment to avoid clobbering sp.
    #[cfg(target_arch = "aarch64")]
    core::arch::naked_asm!(
        "bl   main",
        "mov  x0, #60",   // ViSyscall::Exit
        "mov  x1, #0",    // exit code = 0
        "svc  #0",
        "1: b 1b"
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
