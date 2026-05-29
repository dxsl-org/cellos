#![no_std]
#![no_main]
// Note: #[no_mangle] on main() is required by the ViOS ELF loader and
// triggers unsafe_attr, so we cannot use #![forbid(unsafe_code)] here.
// All benchmark logic in framework/ and scenarios/ is unsafe-free.

extern crate alloc;

mod framework;
mod scenarios;

use api::benchmark::ViBenchmark;
use framework::{report, runner};
use ostd::io::println;
use scenarios::{
    context_switch::ContextSwitchBench,
    ipc_send_recv::IpcSendRecvBench,
    memory_footprint::MemoryFootprintBench,
    syscall_yield::SyscallYieldBench,
};

/// PDR performance targets (nanoseconds).  All checked against p99.
const TARGET_CTX_SWITCH_NS:  u64 = 100_000; //  100 µs
const TARGET_IPC_NS:         u64 =  50_000; //   50 µs
const TARGET_SYSCALL_NS:     u64 =  10_000; //   10 µs
const TARGET_FOOTPRINT_BYTES: u64 = 10 * 1024 * 1024; // 10 MB

#[no_mangle]
pub fn main() {
    println("[bench] ViOS Performance Benchmark Suite v0.1");
    println("[bench] PDR targets: ctx<100µs  ipc<50µs  syscall<10µs  mem<10MB");
    println("");

    let mut passed = 0u32;
    let mut failed = 0u32;

    // ── 1. Context switch ─────────────────────────────────────────────────────
    {
        let r = runner::run_default(&mut ContextSwitchBench);
        report::print_report(&r);
        report::print_json(&r);
        if r.meets_target(TARGET_CTX_SWITCH_NS) {
            passed += 1;
            println("[bench] context_switch PASS");
        } else {
            failed += 1;
            println("[bench] context_switch FAIL (p99 exceeds 100µs target)");
        }
    }

    // ── 2. IPC send/recv ──────────────────────────────────────────────────────
    {
        let r = runner::run_default(&mut IpcSendRecvBench::new());
        report::print_report(&r);
        report::print_json(&r);
        if r.meets_target(TARGET_IPC_NS) {
            passed += 1;
            println("[bench] ipc_send_recv PASS");
        } else {
            failed += 1;
            println("[bench] ipc_send_recv FAIL (p99 exceeds 50µs target)");
        }
    }

    // ── 3. Syscall yield ─────────────────────────────────────────────────────
    {
        let r = runner::run_default(&mut SyscallYieldBench);
        report::print_report(&r);
        report::print_json(&r);
        if r.meets_target(TARGET_SYSCALL_NS) {
            passed += 1;
            println("[bench] syscall_yield PASS");
        } else {
            failed += 1;
            println("[bench] syscall_yield FAIL (p99 exceeds 10µs target)");
        }
    }

    // ── 4. Memory footprint ───────────────────────────────────────────────────
    {
        let mut mb = MemoryFootprintBench::new();
        let _ = mb.run_once();
        let r = mb.footprint_report();
        report::print_report(&r);
        report::print_json(&r);
        if r.p50 <= TARGET_FOOTPRINT_BYTES {
            passed += 1;
            println("[bench] memory_footprint PASS");
        } else {
            failed += 1;
            println("[bench] memory_footprint FAIL (exceeds 10 MB target)");
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    println("");
    println(&alloc::format!(
        "[bench] Results: {}/{} PASS  {}/{} FAIL",
        passed, passed + failed, failed, passed + failed
    ));

    if failed == 0 {
        println("[bench] ALL BENCHMARKS PASS");
    } else {
        println("[bench] BENCHMARK FAILURES DETECTED");
    }
}
