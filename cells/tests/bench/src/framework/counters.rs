//! Kernel benchmark-counter deltas around one scenario (raw opcode 504).
//!
//! `bench` reads the kernel's versioned counter record before and after each
//! scenario and prints one `<scenario>.counters` JSON line. The counters are
//! global (every task, not just this scenario's cells) and the delta spans the
//! whole scenario: setup, warmup, measurement, teardown and the closing read.
//! A kernel without the `bench-counters` feature refuses the opcode; every value
//! is then reported as `"unknown"`, never as zero.

use alloc::{format, string::String};
use ostd::{io::println, syscall::sys_bench_counters};

const RECORD_VERSION: u64 = 1;

/// Record words 1..=7 in kernel order (word 0 is the version).
const FIELDS: [&str; 7] = [
    "syscalls",
    "domain_syscall_cr3",
    "root_programmings",
    "context_switches",
    "ipc_bytes_copied",
    "heap_allocs",
    "heap_alloc_bytes",
];

/// Cost-accounting labels; the collector checks them against its contract.
const LABELS: &[(&str, &str, &str)] = &[
    ("context_switch", "sas", "sched"),
    ("ipc_send_recv", "sas", "copy"),
    ("ipc_send_recv_domain", "domain", "copy"),
    ("ipc_fastpath", "sas", "ring"),
    ("syscall_yield", "sas", "syscall"),
    ("memory_footprint", "sas", "footprint"),
    ("preempt_latency", "sas", "sched"),
    ("control_loop", "sas", "sched"),
    ("ipc_send_recv_idle", "sas", "copy"),
    ("syscall_yield_idle", "sas", "syscall"),
    ("ipc_send_recv_load", "sas", "copy"),
    ("syscall_yield_load", "sas", "syscall"),
    ("smp_spawn_rate", "sas", "spawn"),
    ("smp_ipc_throughput", "sas", "copy"),
    ("smp_work_distribution", "sas", "sched"),
    ("stage_encode_request_x1000", "sas", "codec"),
    ("stage_decode_reply_x1000", "sas", "codec"),
    ("stage_ecall_roundtrip_x1000", "sas", "syscall"),
    ("total_typed_roundtrip_x1000", "sas", "copy"),
];

/// One counter record, or `None` when the kernel refuses the opcode.
pub type Snapshot = Option<[u64; 8]>;

pub fn read() -> Snapshot {
    let mut record = [0u64; 8];
    match sys_bench_counters(&mut record) {
        Ok(()) if record[0] == RECORD_VERSION => Some(record),
        _ => None,
    }
}

/// Read the closing record and print `scenario`'s delta since `before`.
pub fn emit(scenario: &str, before: Snapshot) {
    let after = read();
    let (tier, path) = LABELS
        .iter()
        .find(|(name, _, _)| *name == scenario)
        .map_or(("unknown", "unknown"), |&(_, tier, path)| (tier, path));
    let mut line = format!(
        "{{\"name\":\"{}.counters\",\"tier\":\"{}\",\"path\":\"{}\"",
        scenario, tier, path
    );
    for (index, field) in FIELDS.iter().enumerate() {
        let delta = match (before, after) {
            (Some(start), Some(end)) => end[index + 1].checked_sub(start[index + 1]),
            _ => None,
        };
        let value = match delta {
            Some(value) => format!("{}", value),
            None => String::from("\"unknown\""),
        };
        line.push_str(&format!(",\"{}\":{}", field, value));
    }
    line.push('}');
    println(&line);
}
