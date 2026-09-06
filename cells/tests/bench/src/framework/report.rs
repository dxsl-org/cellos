//! Benchmark report: statistics computation and JSON/text emission.

extern crate alloc;
use super::timer::ticks_to_ns;
use alloc::vec::Vec;
use api::benchmark::BenchReport;
use ostd::io::println;

/// Compute a `BenchReport` from a raw (unsorted) tick-delta sample buffer.
///
/// Sorts `samples` in-place, converts each tick delta to nanoseconds, then
/// builds percentile stats.
pub fn build_report(name: &'static str, samples: &mut [u64]) -> BenchReport {
    samples.sort_unstable();
    let ns: Vec<u64> = samples.iter().map(|&t| ticks_to_ns(t)).collect();
    BenchReport::from_sorted(name, &ns)
}

/// Print a human-readable summary of a `BenchReport` to the serial console.
pub fn print_report(r: &BenchReport) {
    println(&format_report(r));
}

/// Print a machine-readable private JSON line for CI parsing.
///
/// Do not use the legacy `BenchReport::write_json` formatter here: the Phase01
/// evidence contract requires strict JSON and validates this retained raw line.
pub fn print_json(r: &BenchReport) {
    use alloc::format;
    println(&format!(
        "{{\"name\":\"{}\",\"n\":{},\"min\":{},\"p50\":{},\"p99\":{},\"max\":{}}}",
        r.name, r.n, r.min, r.p50, r.p99, r.max
    ));
}

/// Print the private footprint record without pretending bytes are latency.
pub fn print_memory_json(name: &str, bytes: u64) {
    use alloc::format;
    println(&format!(
        "{{\"name\":\"{}\",\"n\":1,\"bytes\":{}}}",
        name, bytes
    ));
}

/// Print a private scalar metric record.
pub fn print_value_json(name: &str, n: u32, value: u64) {
    use alloc::format;
    println(&format!(
        "{{\"name\":\"{}\",\"n\":{},\"value\":{}}}",
        name, n, value
    ));
}

/// Format a single-line human-readable report string.
fn format_report(r: &BenchReport) -> alloc::string::String {
    use alloc::format;
    format!(
        "[bench] {:20} n={:>6}  min={:>6}ns  p50={:>6}ns  p99={:>6}ns  max={:>6}ns",
        r.name, r.n, r.min, r.p50, r.p99, r.max
    )
}
