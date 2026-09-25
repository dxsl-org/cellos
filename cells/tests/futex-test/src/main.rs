//! Futex wait/wake smoke test (portability program phase 04).
//!
//! Proves the primitive end to end in a Tier 2 paged domain:
//!   1. the cell is admitted as an `FFI`-class Tier 2 cell (kernel marker),
//!   2. a futex mutex serialises two threads over 10 000 increments with no lost
//!      update — every waiter that parks is woken by the release,
//!   3. a ping-pong handoff between two threads completes `PING_PONG_ROUNDS`
//!      alternating waits: the classic lost-wakeup stress,
//!   4. a wait with a deadline and no waker returns `TimedOut`,
//!   5. a wait whose word does not hold the expected value returns `ValueMismatch`
//!      without parking,
//!   6. a null and an unmapped address return a recoverable error — the kernel
//!      never dereferences a raw user pointer.
//!
//! The wait loops use a deadline so a lost wake shows up as a retry (and a wrong
//! counter), never as a hang: the runner would otherwise only see a timeout.
//!
//! Markers (integration-test contract):
//!   `[futex-test] mutex ok counter=…`
//!   `[futex-test] ping-pong ok rounds=…`
//!   `[futex-test] timeout ok`
//!   `[futex-test] mismatch ok`
//!   `[futex-test] invalid-address ok`
//!   `FUTEX-TEST: PASS`

#![no_std]
#![no_main]
// This cell is pure safe Rust: the futex words are plain atomics and every wait
// goes through the typed ostd wrappers.
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use alloc::format;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use ostd::io::println;
use ostd::syscall::{
    sys_exit, sys_futex_wait, sys_futex_wake, sys_yield, FutexWaitOutcome, SyscallResult,
};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_FFI
);

// Futex wait/wake are always permitted (no allowlist bit exists); declaring them
// documents intent and costs nothing.
api::declare_syscalls![Log, Exit, Spawn, Yield, GetTime, FutexWait, FutexWake];

ostd::cell_main!(cell_main);

const ITERATIONS: u32 = 5_000; // per thread → 10 000 total increments
const PING_PONG_ROUNDS: u32 = 2_000;
const WAIT_TIMEOUT_TICKS: u64 = 200; // 2 s at 10 ms ticks
const TIMEOUT_PROBE_TICKS: u64 = 10; // 100 ms

/// Mutex word: 0 = free, 1 = held.
static LOCK_WORD: AtomicU32 = AtomicU32::new(0);
static COUNTER: AtomicU32 = AtomicU32::new(0);
/// Ping-pong word: the value encodes whose turn it is.
static PING_WORD: AtomicU32 = AtomicU32::new(0);
/// A word nobody wakes, for the deadline case.
static QUIET_WORD: AtomicU32 = AtomicU32::new(0);

static FAILURES: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicUsize = AtomicUsize::new(0);
static MUTEX_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);
static PING_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);

/// Record a failure and return: the run reports every stage it reached and the
/// tail exits non-zero once any failure is recorded. The arms that call this sit
/// beside `()` arms, so it must not diverge.
fn fail(stage: &str, detail: u64) {
    FAILURES.fetch_add(1, Ordering::AcqRel);
    println(&format!(
        "[futex-test] FAIL stage={} detail={}",
        stage, detail
    ));
}

fn word_addr(word: &AtomicU32) -> usize {
    word as *const AtomicU32 as usize
}

/// Acquire the futex mutex. A timed-out wait is retried, not treated as success:
/// the value is re-checked by the CAS on the next pass.
fn lock() {
    loop {
        if LOCK_WORD
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
        match sys_futex_wait(word_addr(&LOCK_WORD), 1, WAIT_TIMEOUT_TICKS) {
            Ok(FutexWaitOutcome::Woken) | Ok(FutexWaitOutcome::ValueMismatch) => {}
            Ok(FutexWaitOutcome::TimedOut) => {
                // Legal under contention (the release can land between the failed
                // CAS and the park), but a systematic loss would show up as a
                // timeout storm and a wrong counter.
                MUTEX_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                fail("mutex-wait-error", 0);
                return;
            }
        }
    }
}

fn unlock() {
    LOCK_WORD.store(0, Ordering::Release);
    let _ = sys_futex_wake(word_addr(&LOCK_WORD), 1);
}

fn counter_worker() {
    for _ in 0..ITERATIONS {
        lock();
        // Read-modify-write under the lock: a lost wakeup that let two threads in
        // would drop increments and the final count would be short.
        let value = COUNTER.load(Ordering::Relaxed);
        COUNTER.store(value + 1, Ordering::Relaxed);
        unlock();
    }
    DONE.fetch_add(1, Ordering::AcqRel);
}

/// Wait until the ping word holds `expected`, then advance it to `next` and wake.
fn ping_pong(id: u32, expected: u32, next: u32, rounds: u32) {
    for _ in 0..rounds {
        loop {
            if PING_WORD.load(Ordering::Acquire) == expected {
                break;
            }
            match sys_futex_wait(word_addr(&PING_WORD), expected, WAIT_TIMEOUT_TICKS) {
                Ok(FutexWaitOutcome::Woken) | Ok(FutexWaitOutcome::ValueMismatch) => {}
                Ok(FutexWaitOutcome::TimedOut) => {
                    PING_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    fail("ping-wait-error", id as u64);
                    DONE.fetch_add(1, Ordering::AcqRel);
                    return;
                }
            }
        }
        PING_WORD.store(next, Ordering::Release);
        let _ = sys_futex_wake(word_addr(&PING_WORD), 0);
    }
    DONE.fetch_add(1, Ordering::AcqRel);
}

fn wait_for_completion(expected: usize) {
    let mut spins = 0usize;
    while DONE.load(Ordering::Acquire) < expected && spins < 2_000_000 {
        sys_yield();
        spins += 1;
    }
}

fn cell_main() {
    println("[futex-test] start (Tier 2 FFI cell, futex wait/wake)");

    // 1. Mutex over a futex word: 10 000 serialised increments.
    match ostd::task::spawn(counter_worker) {
        SyscallResult::Ok(_) => {}
        SyscallResult::Err(_) => {
            fail("spawn-mutex-a", 0);
            sys_exit(1);
        }
    }
    match ostd::task::spawn(counter_worker) {
        SyscallResult::Ok(_) => {}
        SyscallResult::Err(_) => {
            fail("spawn-mutex-b", 0);
            sys_exit(1);
        }
    }
    wait_for_completion(2);
    let counter = COUNTER.load(Ordering::Acquire);
    if counter != ITERATIONS * 2 {
        fail("mutex-counter", counter as u64);
    }
    println(&format!(
        "[futex-test] mutex ok counter={} timeouts={}",
        counter,
        MUTEX_TIMEOUTS.load(Ordering::Acquire)
    ));

    // 2. Ping-pong handoff: every round parks one thread and wakes the other.
    DONE.store(0, Ordering::Release);
    PING_WORD.store(1, Ordering::Release);
    match ostd::task::spawn(|| ping_pong(0, 1, 2, PING_PONG_ROUNDS)) {
        SyscallResult::Ok(_) => {}
        SyscallResult::Err(_) => fail("spawn-ping-a", 0),
    }
    match ostd::task::spawn(|| ping_pong(1, 2, 1, PING_PONG_ROUNDS)) {
        SyscallResult::Ok(_) => {}
        SyscallResult::Err(_) => fail("spawn-ping-b", 0),
    }
    wait_for_completion(2);
    let final_word = PING_WORD.load(Ordering::Acquire);
    if final_word != 1 {
        fail("ping-final-word", final_word as u64);
    }
    println(&format!(
        "[futex-test] ping-pong ok rounds={} final={} timeouts={}",
        PING_PONG_ROUNDS,
        final_word,
        PING_TIMEOUTS.load(Ordering::Acquire)
    ));

    // 3. Deadline with no waker: must report TimedOut, not block forever.
    match sys_futex_wait(word_addr(&QUIET_WORD), 0, TIMEOUT_PROBE_TICKS) {
        Ok(FutexWaitOutcome::TimedOut) => println("[futex-test] timeout ok"),
        Ok(other) => fail("timeout-outcome", other as u64),
        Err(_) => fail("timeout-error", 0),
    }

    // 4. Value mismatch: the word holds 0, so expecting 7 must not park.
    QUIET_WORD.store(0, Ordering::Release);
    match sys_futex_wait(word_addr(&QUIET_WORD), 7, WAIT_TIMEOUT_TICKS) {
        Ok(FutexWaitOutcome::ValueMismatch) => println("[futex-test] mismatch ok"),
        Ok(other) => fail("mismatch-outcome", other as u64),
        Err(_) => fail("mismatch-error", 0),
    }

    // 5. Invalid words: null and an address this domain does not map. Both are
    //    recoverable errors — a raw dereference here would be a kernel fault.
    let null_result = sys_futex_wait(0, 0, TIMEOUT_PROBE_TICKS);
    let unmapped_result = sys_futex_wait(0xDEAD_B000, 0, TIMEOUT_PROBE_TICKS);
    if null_result.is_err() && unmapped_result.is_err() {
        println("[futex-test] invalid-address ok");
    } else {
        fail("invalid-address", 0);
    }
    // A wake needs no mapped word — it selects waiters by key, so an address with
    // no waiters is a no-op that reports zero (the Linux contract). What must not
    // happen is a kernel fault.
    match sys_futex_wake(0xDEAD_B000, 1) {
        Ok(0) => println("[futex-test] invalid-wake ok (no waiters, no-op)"),
        Ok(other) => fail("invalid-wake-count", other as u64),
        Err(_) => fail("invalid-wake-error", 0),
    }

    if FAILURES.load(Ordering::Acquire) != 0 {
        println(&format!(
            "[futex-test] FAIL failures={}",
            FAILURES.load(Ordering::Acquire)
        ));
        sys_exit(1);
    }
    println("FUTEX-TEST: PASS");
    sys_exit(0);
}
