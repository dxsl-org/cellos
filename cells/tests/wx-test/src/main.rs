//! W^X violation test cell.
//!
//! Law 4 carve-out, same as `cfi-test`: this cell MUST use `unsafe` to write
//! into its own `.text` — that is the entire test. A cell that respects
//! `forbid(unsafe_code)` cannot prove a hardware page-permission bit.
//!
//! # What is being proved
//! The ELF loader maps every cell page WRITE so it can apply `.rela.dyn`
//! relocations, then lowers each page to the ELF's real `p_flags` before the
//! cell runs (`kernel/src/loader/wx.rs`). Without that lowering pass a single
//! `unsafe` block in ANY cell can rewrite `.text` of EVERY cell, because Cellos
//! runs all cells in one address space. With it, the store below faults.
//!
//! # Pass / fail is decided by the KERNEL log, not by this cell
//! - PASS: kernel logs `[fault] Cell … terminated` and keeps running. This cell
//!   never reaches its next line, so it prints no verdict — by design.
//! - FAIL: the store succeeds, this cell prints `wx-test: FAIL` and exits 1.
//! - Kernel panic instead of `[fault]`: also a failure, caught by the harness
//!   asserting the shell prompt returns afterwards.
//!
//! Run it from the shell as `wx-test`; the harness is
//! `tests/integration/tests/wx-text-write.rs`.
//!
//! Spawned UNSUPERVISED so init's never-die watchdog does not restart a cell
//! that is meant to die.

// Law 4 carve-out: deliberate W^X violation for hardware-enforcement testing.
#![allow(unsafe_code)]
#![no_std]
#![no_main]

use ostd::app::{AppContext, AppEvent};
use ostd::io::println;
use ostd::syscall::sys_exit;

ostd::app_entry!(handler = wx_handler);

fn wx_handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => run_test(),
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),
        _ => {}
    }
}

/// The write target. `#[inline(never)]` and `#[no_mangle]` keep it a real,
/// addressable symbol in `.text` that the optimiser cannot fold away — taking
/// its address is the only reason it exists.
#[inline(never)]
#[no_mangle]
extern "C" fn wx_victim() -> u32 {
    0xC0FF_EE00
}

fn run_test() {
    println("wx-test: probing W^X on this cell's own .text");

    let target = wx_victim as *const () as *mut u8;

    // Log BEFORE the store: on a correctly configured kernel this is the last
    // line this cell ever prints, and its presence in the log is what tells the
    // harness the cell got far enough to attempt the write.
    println("wx-test: storing to .text now — expect a fault and clean termination");

    // SAFETY: deliberate W^X violation. This is NOT a sound write — the whole
    // point is that the page must be read-only+execute, so the store is expected
    // to trap before it retires. `write_volatile` prevents the compiler from
    // eliding a store whose result is never read. If the kernel does fault, no
    // memory is modified and control never returns here; if it does NOT fault,
    // `wx_victim`'s first byte is corrupted, which is why nothing calls it
    // afterwards — the cell exits immediately on the failure path.
    unsafe {
        core::ptr::write_volatile(target, 0x00u8);
    }

    // Reaching this line means the page was still writable: the loader's W^X
    // pass did not run, ran too early, or missed this page.
    println("wx-test: FAIL: .text write succeeded — W^X is NOT enforced");
    sys_exit(1);
}
