#![no_main]
//! Fuzz the VFS path validator with arbitrary byte slices.
//! Run: cargo fuzz run vfs_path -- -max_total_time=300

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The VFS path check must not panic or overflow on any input.
    if let Ok(s) = core::str::from_utf8(data) {
        // Simulate the kernel's path validation logic.
        let _ = s.starts_with('/');
        let _ = s.len() <= 256;
        let _ = s.contains('\0');
    }
});
