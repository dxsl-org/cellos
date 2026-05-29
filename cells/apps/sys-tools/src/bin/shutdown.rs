#![no_std]
#![no_main]
extern crate ostd;

/// shutdown — halt the system via SBI shutdown (RISC-V) or QEMU power-off.
#[no_mangle]
pub fn main() {
    ostd::io::println("System shutting down...");
    // SBI call to halt: ecall with a7=0x08 (SHUTDOWN) and a6=0.
    // This is the same path the kernel boot uses for fatal errors.
    // SAFETY: the inline asm issues a legal SBI ecall; the RISC-V SBI spec
    // guarantees SHUTDOWN does not return if the host accepts it.
    unsafe {
        core::arch::asm!(
            "li a7, 0x08",
            "li a6, 0",
            "ecall",
            options(noreturn)
        );
    }
}
