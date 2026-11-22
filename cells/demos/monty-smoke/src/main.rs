//! Tier 2 witness: Monty (pydantic/monty) running fixed Python in an
//! `UNTRUSTED` std cell on the CellOS rust-std PAL.
//!
//! Proves the port, not the sandbox: the Python here is trusted and there is no
//! `max_memory` cap (that needs an allocator hook). Boot it with
//! `scripts/x86/qemu-monty-smoke.sh`.
//!
//! Every compile uses `source_scan_threshold = 0`: on cellos ruff's parser has no
//! `stacker` (third_party/monty patch 0001), so Monty's nesting pre-scan is what
//! bounds parser recursion on this cell's native stack.
#![cfg_attr(not(test), no_main)]
#![deny(unsafe_code)]

extern crate ostd;

mod entropy;

use std::time::Duration;

use monty::MontyRun;
use monty_types::{CompileOptions, MontyObject, PrintWriter, ResourceLimits, ResourceTracker};

abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = abi::manifest::PROTECTION_CLASS_UNTRUSTED
);

// Monty runs single-threaded: no Spawn/Wait. The PAL still needs TLS, futex,
// time, yield and entropy.
abi::declare_syscalls![
    Log, Yield, GetTime, GetRandom, SetTlsBase, FutexWait, FutexWake, GetCpuCount
];

ostd::cell_main!(extern "C" cell_main);

/// Release builds accept up to this depth (`monty::parse::MAX_NESTING_DEPTH`).
const NESTING_LIMIT: usize = 200;

fn compile_options() -> CompileOptions {
    CompileOptions {
        source_scan_threshold: 0,
        ..CompileOptions::default()
    }
}

fn run(name: &str, code: String, limits: ResourceLimits) -> Result<MontyObject, String> {
    let mut runner =
        MontyRun::new(code, name, vec![], compile_options()).map_err(|e| e.to_string())?;
    runner
        .run(vec![], ResourceTracker::new(limits), PrintWriter::Stdout)
        .map_err(|e| e.to_string())
}

fn nested(depth: usize) -> String {
    format!("{}1{}", "(".repeat(depth), ")".repeat(depth))
}

/// One case: print its marker on success, the failure and `FAIL` otherwise.
fn check(case: &str, ok: Result<(), String>) -> bool {
    match ok {
        Ok(()) => {
            println!("[monty-smoke] {case} PASS");
            true
        }
        Err(why) => {
            println!("[monty-smoke] {case} FAIL: {why}");
            false
        }
    }
}

fn expect_value(code: &str, want: MontyObject) -> Result<(), String> {
    let got = run("case.py", code.to_owned(), ResourceLimits::default())?;
    if got == want {
        Ok(())
    } else {
        Err(format!("got {got:?}"))
    }
}

fn cell_main() {
    println!("[monty-smoke] start");
    let fib = "def fib(n):\n    return n if n <= 1 else fib(n - 1) + fib(n - 2)\nfib(10)";

    let results = [
        check("fib", expect_value(fib, MontyObject::int(55))),
        // ruff recurses on the native stack up to the limit; this must not fault.
        check(
            "nesting-at-limit",
            expect_value(&nested(NESTING_LIMIT), MontyObject::int(1)),
        ),
        check(
            "nesting-over-limit",
            match run("deep.py", nested(10_000), ResourceLimits::default()) {
                Err(e) if e.contains("too deeply nested") => Ok(()),
                other => Err(format!("expected SyntaxError, got {other:?}")),
            },
        ),
        // Wiring only: the test-hooks kernel serves GetRandom from dev-weak-rng.
        // The fail-closed path is covered by entropy's host tests.
        check(
            "random",
            expect_value(
                "import random\nr = random.random()\n0 <= r < 1",
                MontyObject::bool(true),
            ),
        ),
        // jiter's scalar paths (its SSE2 paths are compiled out on this target):
        // long digit runs, a float, a bigint and an escaped string.
        check(
            "json",
            expect_value(
                "import json\nv = json.loads('[123456789012345678901234567890, 2.5, \"a\\\\u00e9b\"]')\nv[0] == 123456789012345678901234567890 and v[1] == 2.5 and v[2] == 'a\\u00e9b'",
                MontyObject::bool(true),
            ),
        ),
        check(
            "feed-timeout",
            match run(
                "spin.py",
                "while True: pass".to_owned(),
                ResourceLimits {
                    max_feed_duration: Some(Duration::from_millis(500)),
                    ..ResourceLimits::default()
                },
            ) {
                Err(e) if e.contains("feed time limit exceeded") => Ok(()),
                other => Err(format!("expected TimeoutError, got {other:?}")),
            },
        ),
    ];

    if results.iter().all(|&ok| ok) {
        println!("[monty-smoke] PASS");
    } else {
        println!("[monty-smoke] FAIL");
    }
}
