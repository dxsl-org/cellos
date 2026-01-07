use core::panic::PanicInfo;
use crate::syscall::sys_log;

#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    // 1. Initialize BSS? (Already done by ELF Loader if zeroed)
    // 2. Call main
    core::arch::naked_asm!(
        ".option push",
        ".option norelax",
        "la gp, __global_pointer$",
        ".option pop",
        "andi sp, sp, -16", // Align stack to 16 bytes
        "call main",
        "li a0, 0",
        "li a7, 93", // Exit
        "ecall"
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
    unsafe { main(); }
    // If main returns, we exit
    crate::syscall::sys_exit(0);
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write;
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
