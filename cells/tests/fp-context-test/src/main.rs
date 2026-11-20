//! Per-task floating-point register file regression test.
//!
//! Two threads of one Cell alternate on the same hart. Each round a thread
//! loads its own bit pattern into `f0`-`f31` and `fcsr`, yields (so the other
//! thread runs and loads *its* pattern), and reads the registers back. Every
//! word must come back unchanged: the kernel owes each task its own FP state
//! across a context switch. Before `__switch` saved FP state on RV64, the first
//! round already read the peer's pattern.
//!
//! Markers (integration-test contract):
//!   `[fp-context-test] thread 0 ok rounds=…`
//!   `[fp-context-test] FAIL thread=… round=… reg=… expected=0x… observed=0x…`
//!   `FP-CONTEXT-TEST: PASS`

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

mod regs;

use alloc::format;
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::io::println;
use ostd::syscall::{sys_exit, sys_yield, SyscallResult};

abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_FFI
);

abi::declare_syscalls![Log, Exit, Spawn, Yield, GetTime];

ostd::cell_main!(cell_main);

const THREADS: usize = 2;
const ROUNDS: u64 = 200;

static DONE: AtomicUsize = AtomicUsize::new(0);
static FAILURES: AtomicUsize = AtomicUsize::new(0);

/// A pattern no other (thread, round) produces: the thread id and round sit in
/// the high bits, the register index in the low byte. `fcsr` uses only its
/// architected bits (`frm` 5-7, `fflags` 0-4); `frm` stays a valid mode (0-4).
fn pattern(thread: usize, round: u64) -> regs::FpFile {
    let mut file = [0u64; 33];
    for (index, word) in file.iter_mut().take(32).enumerate() {
        *word = 0x7FF0_0000_0000_0000
            | ((thread as u64 + 1) << 44)
            | ((round & 0xFFFF_FFFF) << 8)
            | index as u64;
    }
    let frm = ((thread as u64 + round) % 5) << 5;
    let fflags = (round + thread as u64 * 7) & 0x1F;
    file[32] = frm | fflags;
    file
}

fn thread_body(id: usize) {
    let mut observed = [0u64; 33];
    for round in 0..ROUNDS {
        let expected = pattern(id, round);
        regs::yield_with_pattern(&expected, &mut observed);
        if let Some(reg) = (0..33).find(|&i| observed[i] != expected[i]) {
            FAILURES.fetch_add(1, Ordering::AcqRel);
            let name: alloc::string::String = if reg == 32 {
                "fcsr".into()
            } else {
                format!("f{}", reg)
            };
            println(&format!(
                "[fp-context-test] FAIL thread={} round={} reg={} expected=0x{:X} observed=0x{:X}",
                id, round, name, expected[reg], observed[reg]
            ));
            DONE.fetch_add(1, Ordering::AcqRel);
            return;
        }
    }
    println(&format!(
        "[fp-context-test] thread {} ok rounds={}",
        id, ROUNDS
    ));
    DONE.fetch_add(1, Ordering::AcqRel);
}

fn cell_main() {
    if !regs::SUPPORTED {
        println("[fp-context-test] skipped: no per-task FP register file on this architecture");
        sys_exit(0);
    }
    println("[fp-context-test] start (two threads, f0-f31 + fcsr across yields)");

    for id in 0..THREADS {
        if let SyscallResult::Err(_) = ostd::task::spawn(move || thread_body(id)) {
            println(&format!("[fp-context-test] FAIL spawn thread={}", id));
            sys_exit(1);
        }
    }

    // No join primitive: yield until both threads report, bounded so a lost
    // thread fails the run instead of hanging it.
    let mut spins = 0usize;
    while DONE.load(Ordering::Acquire) < THREADS && spins < 200_000 {
        sys_yield();
        spins += 1;
    }
    let done = DONE.load(Ordering::Acquire);
    let failures = FAILURES.load(Ordering::Acquire);
    if done < THREADS || failures != 0 {
        println(&format!(
            "[fp-context-test] FAIL done={} failures={}",
            done, failures
        ));
        sys_exit(1);
    }
    println("FP-CONTEXT-TEST: PASS");
    sys_exit(0);
}
