//! Per-task floating-point register file regression test.
//!
//! Two threads of one Cell alternate on the same hart. Each round a thread
//! loads its own bit pattern into the whole FP register file (RV64 `f0`-`f31`
//! and `fcsr`; AArch64 `q0`-`q31`, `FPCR` and `FPSR`), yields (so the other
//! thread runs and loads *its* pattern), and reads the registers back. Every
//! word must come back unchanged: the kernel owes each task its own FP state
//! across a context switch. Before `__switch` saved FP state (RV64 `__switch`,
//! AArch64 `__switch_el1`/`__switch_el2`), the first round already read the
//! peer's pattern.
//!
//! The RV64 lane runs it as an `FFI`-class Tier 2 domain; on AArch64 it is a
//! Tier 1 SAS Cell, the class std Cells run in, so the lane covers both the
//! EL1 and the EL2 (VHE) host, where Tier 2 admission is refused.
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

#[cfg(not(target_arch = "aarch64"))]
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_FFI
);
#[cfg(target_arch = "aarch64")]
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_STANDARD
);

abi::declare_syscalls![Log, Exit, Spawn, Yield, GetTime];

ostd::cell_main!(cell_main);

const THREADS: usize = 2;
const ROUNDS: u64 = 200;

static DONE: AtomicUsize = AtomicUsize::new(0);
static FAILURES: AtomicUsize = AtomicUsize::new(0);

fn thread_body(id: usize) {
    let mut observed = regs::FpFile::zeroed();
    for round in 0..ROUNDS {
        let expected = regs::pattern(id, round);
        regs::yield_with_pattern(&expected, &mut observed);
        if let Some(reg) = (0..regs::WORDS).find(|&i| observed.0[i] != expected.0[i]) {
            FAILURES.fetch_add(1, Ordering::AcqRel);
            println(&format!(
                "[fp-context-test] FAIL thread={} round={} reg={} expected=0x{:X} observed=0x{:X}",
                id,
                round,
                regs::reg_name(reg),
                expected.0[reg],
                observed.0[reg]
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
    println("[fp-context-test] start (two threads, whole FP register file across yields)");

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
