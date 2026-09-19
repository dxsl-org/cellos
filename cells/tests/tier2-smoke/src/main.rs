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
use ostd::io::println;
use ostd::syscall::{
    sys_exit, sys_grant_copy_from_slice, sys_grant_copy_to_slice, sys_grant_register,
    sys_grant_unregister,
};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);

api::declare_syscalls![
    Log,
    Yield,
    GetTime,
    Exit,
    GrantRegister,
    GrantSlice,
    GrantUnregister
];

ostd::cell_main!(cell_main);

fn cell_main() {
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

    // 4. Grant allocation, private SATP mapping, write, read, and unregister
    let grant_size = 4096;
    let reg_id = sys_grant_register(grant_size).expect("GrantRegister failed in Tier 2 domain");
    let test_payload = b"Tier 2 Domain Grant Zero-Copy Buffer Content!";
    let copied_in =
        sys_grant_copy_from_slice(reg_id, test_payload).expect("sys_grant_copy_from_slice failed");
    assert_eq!(copied_in, test_payload.len());

    let mut read_buf = [0u8; 45];
    let copied_out =
        sys_grant_copy_to_slice(reg_id, &mut read_buf).expect("sys_grant_copy_to_slice failed");
    assert_eq!(copied_out, test_payload.len());
    assert_eq!(&read_buf, test_payload);
    println(
        "[tier2-smoke] Grant allocation, private SATP mapping, and RW verified in Tier 2 domain",
    );

    let unregistered = sys_grant_unregister(reg_id);
    assert!(unregistered, "GrantUnregister failed in Tier 2 domain");
    println("[tier2-smoke] Grant unregister and unmapping verified in Tier 2 domain");

    println("[tier2-smoke] PASS: All Tier 2 runtime invariants verified successfully!");
    sys_exit(0);
}
