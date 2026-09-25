//! Per-task TLS base smoke test (portability program phase 03).
//!
//! Proves the `SetTlsBase` primitive end to end:
//!   1. the cell is admitted as an `FFI`-class Tier 2 cell (kernel marker),
//!   2. each thread sets its own user thread pointer and reads back its own
//!      previous value (the kernel's per-task record),
//!   3. on riscv64/aarch64 the *register itself* is read back, so the value is
//!      proven per-task rather than merely remembered,
//!   4. a sentinel written through the base survives forced context switches
//!      (the thread re-reads its own block after yielding repeatedly),
//!   5. a second thread's base never becomes visible to the first.
//!
//! Markers (integration-test contract):
//!   `[tls-test] thread 0 base=0x… previous=0x0`
//!   `[tls-test] thread 0 register-ok`
//!   `[tls-test] thread 0 sentinel-ok rounds=…`
//!   `TLS-TEST: PASS`

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

mod regs;

use alloc::boxed::Box;
use alloc::format;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::io::println;
use ostd::syscall::{sys_exit, sys_set_tls_base, sys_yield, SyscallResult};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_FFI
);

api::declare_syscalls![Log, Exit, Spawn, Yield, GetTime];

ostd::cell_main!(cell_main);

const THREADS: usize = 2;
const SWITCH_ROUNDS: usize = 64;
const SENTINEL_BASE: u64 = 0x7157_0000;

static DONE: AtomicUsize = AtomicUsize::new(0);
static FAILURES: AtomicUsize = AtomicUsize::new(0);
static OBSERVED_BASES: [AtomicUsize; THREADS] = [const { AtomicUsize::new(0) }; THREADS];

fn fail(stage: &str, detail: u64) {
    FAILURES.fetch_add(1, Ordering::AcqRel);
    println(&format!(
        "[tls-test] FAIL stage={} detail={}",
        stage, detail
    ));
}

fn thread_body(id: usize, block: usize) {
    let base = block;

    // 1. Claim the base. A thread inherits its creator's base (0 here), so the
    //    first set must report 0 — that is the kernel's per-task record speaking.
    let previous = match sys_set_tls_base(base) {
        SyscallResult::Ok(value) => value,
        SyscallResult::Err(_) => {
            fail("set-base", id as u64);
            DONE.fetch_add(1, Ordering::AcqRel);
            return;
        }
    };
    if previous != 0 {
        fail("inherited-nonzero", previous as u64);
    }
    println(&format!(
        "[tls-test] thread {} base=0x{:X} previous=0x{:X}",
        id, base, previous
    ));

    // 2. The register itself must agree (where the architecture lets user code
    //    read it). This is what separates "the kernel remembers a number" from
    //    "the hardware thread pointer is per-task".
    if regs::CAN_READ_REGISTER {
        let observed = regs::read_thread_pointer();
        if observed != base {
            fail("register-mismatch", observed as u64);
        } else {
            println(&format!("[tls-test] thread {} register-ok", id));
        }
    } else {
        println(&format!(
            "[tls-test] thread {} register-readback=unavailable",
            id
        ));
    }

    // 3. Write a sentinel through the base and keep re-reading it across forced
    //    context switches. If another thread's base were installed while this one
    //    was parked, the value would change under it.
    let sentinel = SENTINEL_BASE + id as u64;
    regs::write_own_sentinel(base, sentinel);
    for _ in 0..SWITCH_ROUNDS {
        sys_yield();
        let value = regs::read_own_sentinel(base);
        if value != sentinel {
            fail("sentinel-corrupted", value);
            break;
        }
        if regs::CAN_READ_REGISTER {
            let observed = regs::read_thread_pointer();
            if observed != base {
                fail("register-lost-after-switch", observed as u64);
                break;
            }
        }
        // A re-set must report this thread's own base, never a peer's.
        match sys_set_tls_base(base) {
            SyscallResult::Ok(value) if value == base => {}
            SyscallResult::Ok(value) => {
                fail("peer-base-visible", value as u64);
                break;
            }
            SyscallResult::Err(_) => {
                fail("reset-base", id as u64);
                break;
            }
        }
    }
    println(&format!(
        "[tls-test] thread {} sentinel-ok rounds={}",
        id, SWITCH_ROUNDS
    ));

    OBSERVED_BASES[id].store(base, Ordering::Release);
    DONE.fetch_add(1, Ordering::AcqRel);
}

fn cell_main() {
    println("[tls-test] start (Tier 2 FFI cell, per-task TLS base)");

    // Each thread owns a real TLS block; the kernel only carries the pointer.
    let mut handles = [0usize; THREADS];
    for id in 0..THREADS {
        let block = Box::new([0u8; 64]);
        let base = block.as_ptr() as usize;
        // Leak the block deliberately: the thread owns it for the cell's lifetime,
        // and the cell exits at the end of the test.
        core::mem::forget(block);
        match ostd::task::spawn(move || thread_body(id, base)) {
            SyscallResult::Ok(tid) => handles[id] = tid,
            SyscallResult::Err(_) => {
                fail("spawn", id as u64);
                return;
            }
        }
    }

    // Wait for both threads without blocking the scheduler: a cell has no join
    // primitive here, and the test only needs completion.
    let mut spins = 0usize;
    while DONE.load(Ordering::Acquire) < THREADS && spins < 200_000 {
        sys_yield();
        spins += 1;
    }

    if DONE.load(Ordering::Acquire) < THREADS {
        fail("threads-incomplete", DONE.load(Ordering::Acquire) as u64);
    }
    if FAILURES.load(Ordering::Acquire) != 0 {
        println(&format!(
            "[tls-test] FAIL failures={}",
            FAILURES.load(Ordering::Acquire)
        ));
        sys_exit(1);
    }

    // Distinct bases observed by both threads is the cross-thread claim: two
    // threads, two thread pointers, one cell.
    let first = OBSERVED_BASES[0].load(Ordering::Acquire);
    let second = OBSERVED_BASES[1].load(Ordering::Acquire);
    if first == 0 || second == 0 || first == second {
        fail("bases-not-distinct", 0);
        sys_exit(1);
    }
    println(&format!(
        "[tls-test] distinct bases 0x{:X} / 0x{:X}",
        first, second
    ));
    println("TLS-TEST: PASS");
    sys_exit(0);
}
