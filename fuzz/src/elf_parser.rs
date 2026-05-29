#![no_main]
//! Fuzz the ELF parser with arbitrary byte slices.
//! Run: cargo fuzz run elf_parser -- -max_total_time=300

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Feed arbitrary bytes to the ELF magic check and header parse.
    // The kernel's ELF parser must never panic or UB on malformed input.
    if data.len() < 4 {
        return;
    }
    // Sanity: only proceed if it looks like an ELF (avoid false-positive crashes).
    if &data[..4] != b"\x7fELF" {
        return;
    }
    // Parse via xmas_elf — must not panic.
    let _ = xmas_elf::ElfFile::new(data);
});
