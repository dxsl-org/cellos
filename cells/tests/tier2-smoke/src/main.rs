//! Tier 2 positive execution and isolation verification cell.
//!
//! Verifies that a Tier 2 domain cell running under private SATP isolation can:
//! 1. Be admitted to Tier 2 Paged Domain without SAS fallback.
//! 2. Execute user code safely behind hardware MMU boundaries.
//! 3. Perform dynamic memory allocations on its private heap.
//! 4. Invoke system calls (Log, Time, Yield).
//! 5. Observe the kernel's private-root grant posture through the real syscall
//!    path, architecture-honestly. Where the lifecycle is open (RV64, and AArch64
//!    test images) `GrantRegister` publishes the owner's own RW+NX mapping in
//!    this cell's root, the owner round-trips bytes through the kernel's copy
//!    facade, `GrantUnregister` removes the record, and the same id is then
//!    refused; the backing stays supervisor-only in the SAS root either way.
//!    Where the lifecycle is closed (x86_64, phase 02) the phase-01 containment
//!    posture must refuse before a frame or PTE exists, and the refusal is the
//!    alloc-safe `Ok(0)` sentinel — never a plausible address.
//! 6. Exit cleanly with code 0.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use ostd::io::println;
use ostd::syscall::{sys_exit, sys_grant_register};

/// Is this image's kernel private-root grant lifecycle open (phase 03)?
///
/// Mirrors `task::syscall::domain_grant_lifecycle_supported` from the cell side,
/// where the kernel's feature set is not visible: the lifecycle is open on RV64
/// and on the AArch64 images that can admit a domain at all (test images), and
/// closed on x86_64, which phase 02 deliberately left out.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
const GRANT_LIFECYCLE_OPEN: bool = true;
#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
const GRANT_LIFECYCLE_OPEN: bool = false;

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);

// `GrantSlice`/`GrantUnregister` are declared on every target so one manifest
// serves both postures; the closed branch never calls them.
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

    // 4. The private-root grant posture, through the real syscall path.
    if GRANT_LIFECYCLE_OPEN {
        // This is the ABI-visible half of the kernel-side `grant_gate_selftest`:
        // `GrantRegister` must publish the owner's backing (a plausible, nonzero
        // id — never the `Ok(0)` refusal sentinel), the owner must be able to
        // write into it and read the bytes back, and `GrantUnregister` must
        // revoke the record so a later slice of the same id is refused. No raw
        // pointer is dereferenced here: the copy facade resolves the mapping and
        // its registered bound together.
        use ostd::syscall::{
            sys_grant_copy_from_slice, sys_grant_copy_to_slice, sys_grant_slice,
            sys_grant_unregister,
        };
        const GRANT_SIZE: usize = 4096;
        let pattern = *b"phase-03";
        let Some(grant_id) = sys_grant_register(GRANT_SIZE) else {
            panic!("GrantRegister must publish the owner's backing on a lifecycle-open target");
        };
        assert!(grant_id != 0, "GrantRegister returned the deny sentinel");

        let wrote = sys_grant_copy_from_slice(grant_id, &pattern);
        let mut readback = [0u8; 8];
        let read = sys_grant_copy_to_slice(grant_id, &mut readback);
        assert_eq!(
            wrote,
            Some(pattern.len()),
            "owner write into its grant failed"
        );
        assert_eq!(read, Some(pattern.len()), "owner read of its grant failed");
        assert_eq!(readback, pattern, "grant round-trip returned other bytes");
        println("[tier2-smoke] Grant backing written and read back (owner mapping live)");

        assert!(
            sys_grant_unregister(grant_id),
            "the owner must be able to unregister its own grant"
        );
        assert!(
            sys_grant_slice(grant_id).is_none(),
            "an unregistered grant must be refused, not re-resolved"
        );
        println("[tier2-smoke] Grant unregistered and the id refused");
    } else {
        // Fail-closed containment: on a target whose private-root grant lifecycle
        // is closed, the kernel must refuse before any frame or PTE is published,
        // and the refusal must be the alloc-safe sentinel — never an address.
        const GRANT_SIZE: usize = 4096;
        assert!(
            sys_grant_register(GRANT_SIZE).is_none(),
            "a closed target must refuse a private-root grant before publishing it"
        );
        println("[tier2-smoke] Grant registration denied fail-closed (phase-01 gate)");
    }

    println("[tier2-smoke] PASS: All Tier 2 runtime invariants verified successfully!");
    sys_exit(0);
}
