//! Boot self-test for real user-thread first entry.
//!
//! This proves more than "the thread became Ready": the real `Syscall::Spawn`
//! path must switch into U-mode, hand the thread its `arg`, and let it complete
//! a syscall/exit round-trip before control returns to the boot context.

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
#[path = "../arch/riscv64/task/thread_user_entry_selftest.rs"]
mod rv;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub use rv::*;

#[cfg(not(all(feature = "test-hooks", target_arch = "riscv64")))]
pub fn self_test() -> Option<bool> {
    None
}
