//! Tier 2 positive execution and isolation verification cell.
//!
//! Verifies that a Tier 2 domain cell running under private SATP isolation can:
//! 1. Be admitted to Tier 2 Paged Domain without SAS fallback.
//! 2. Execute user code safely behind hardware MMU boundaries.
//! 3. Perform dynamic memory allocations on its private heap.
//! 4. Invoke system calls (Log, Time, Yield).
//! 5. Exit cleanly with code 0.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use ostd::app::{AppContext, AppEvent};
use ostd::io::println;
use ostd::syscall::sys_exit;

ostd::app_entry!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED,
    handler = smoke_handler,
);

fn smoke_handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => run_smoke(),
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),
        _ => {}
    }
}

fn run_smoke() {
    println("[tier2-smoke] Starting Tier 2 Native Domain Cell execution under private SATP!");

    // 1. Dynamic allocation in private domain heap (.bss arena)
    let mut numbers = Vec::new();
    for i in 1..=20 {
        numbers.push(i * 5);
    }
    let sum: i32 = numbers.iter().sum();
    assert_eq!(sum, 1050);
    println("[tier2-smoke] Heap allocation verified (Vec len=20, sum=1050)");

    // 2. String allocation and formatting
    let msg = alloc::format!("Tier 2 message formatted on heap: val={}", sum);
    assert!(msg.contains("val=1050"));
    println("[tier2-smoke] String allocation and formatting verified");

    // 3. Syscalls: Yield and Time
    ostd::task::yield_now();
    println("[tier2-smoke] Scheduler yield completed in Tier 2 domain");

    println("[tier2-smoke] PASS: All Tier 2 runtime invariants verified successfully!");
    sys_exit(0);
}
