//! Phase 03 latency reports from `bench-counters` serial milestones.
//!
//! A kernel built with `bench-counters` prints
//! `[bench-ts] <event> tid=T ns=N exit_ns=E|unknown` on the `GetTime(0)` clock
//! (kernel/src/bench_counters.rs). Default kernels print none, and every report
//! here is then `None`.
//!
//! - P08 driver retirement: `dma-retired` once the dead driver's PCI functions
//!   are quiesced, its IOMMU translations revoked and its BDFs released
//!   (`exit_ns` = its Exit syscall), then `block-ready` when the restarted
//!   incarnation registers as block driver after its controller reset.
//! - P09 planned replacement: `state-stashed` by the old provider,
//!   `replacement-sealed` when the supervisor's replacement is spawned and the
//!   stash sealed to it, `state-restored` and `hotswap-ready` by the replacement.

/// One `[bench-ts]` line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Milestone {
    event: String,
    tid: u64,
    ns: u64,
    exit_ns: Option<u64>,
}

fn parse(line: &str) -> Option<Milestone> {
    let rest = line.split("[bench-ts] ").nth(1)?;
    let mut words = rest.split_whitespace();
    let event = words.next()?.to_string();
    let mut field = |name: &str| -> Option<&str> { words.next()?.strip_prefix(name)?.strip_prefix('=') };
    let tid = field("tid")?.parse().ok()?;
    let ns = field("ns")?.parse().ok()?;
    let exit_ns = match field("exit_ns")? {
        "unknown" => None,
        value => Some(value.parse().ok()?),
    };
    Some(Milestone { event, tid, ns, exit_ns })
}

/// `to - from` in ns, or `"unknown"` (never zero) when `from` is missing.
fn span(from: Option<u64>, to: u64) -> String {
    match from {
        Some(from) if to >= from => (to - from).to_string(),
        _ => "\"unknown\"".to_string(),
    }
}

fn milestones(log: &str) -> Vec<Milestone> {
    log.lines().filter_map(parse).collect()
}

/// P08: the first retirement in `log` and the first block-ready registration
/// after it. Exit-based spans are `"unknown"` when the kernel lost the exit
/// record (a concurrent exit overwrote it).
pub fn retire_report(driver: &str, accel: &str, log: &str) -> Option<String> {
    let all = milestones(log);
    let at = all.iter().position(|m| m.event == "dma-retired")?;
    let retired = &all[at];
    let ready = all[at + 1..].iter().find(|m| m.event == "block-ready")?;
    Some(format!(
        "{{\"name\":\"driver_retire_latency.{driver}\",\"accelerator\":\"{accel}\",\"unit\":\"ns\",\
         \"dead_tid\":{},\"new_tid\":{},\"exit_to_dma_retired\":{},\"exit_to_block_ready\":{},\
         \"dma_retired_to_block_ready\":{}}}",
        retired.tid,
        ready.tid,
        span(retired.exit_ns, retired.ns),
        span(retired.exit_ns, ready.ns),
        span(Some(retired.ns), ready.ns),
    ))
}

/// P09: the first `replacement-sealed` in `log`, the replacement's following
/// `state-restored` and `hotswap-ready`, and the last `state-stashed` by any
/// task before the seal (the old provider's snapshot).
pub fn hotswap_report(scenario: &str, accel: &str, log: &str) -> Option<String> {
    let all = milestones(log);
    let at = all.iter().position(|m| m.event == "replacement-sealed")?;
    let sealed = &all[at];
    let after = &all[at + 1..];
    let restored = after.iter().find(|m| m.event == "state-restored" && m.tid == sealed.tid)?;
    let ready = after.iter().find(|m| m.event == "hotswap-ready" && m.tid == sealed.tid)?;
    let stashed = all[..at].iter().rev().find(|m| m.event == "state-stashed").map(|m| m.ns);
    Some(format!(
        "{{\"name\":\"hotswap_latency.{scenario}\",\"accelerator\":\"{accel}\",\"unit\":\"ns\",\
         \"new_tid\":{},\"stashed_to_sealed\":{},\"sealed_to_restored\":{},\"sealed_to_ready\":{},\
         \"stashed_to_ready\":{}}}",
        sealed.tid,
        span(stashed, sealed.ns),
        span(Some(sealed.ns), restored.ns),
        span(Some(sealed.ns), ready.ns),
        span(stashed, ready.ns),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RETIRE: &str = "\
[WARN] [bench-ts] block-ready tid=4 ns=100 exit_ns=unknown
[ahci] fault-inject: driver exiting at lba=9
[INFO] [pcie] quiesced 00:1f.2 for tid 4: cmd=0x0400 bme=0 intx_disable=1 msi=none msix=none
[WARN] [bench-ts] dma-retired tid=4 ns=5000 exit_ns=4000
Init: service restarted.
[WARN] [bench-ts] block-ready tid=9 ns=12000 exit_ns=unknown
";

    #[test]
    fn retirement_pairs_with_the_next_registration() {
        let json = retire_report("ahci", "tcg", RETIRE).expect("pair present");
        assert!(json.contains("\"dead_tid\":4,\"new_tid\":9"), "{json}");
        assert!(json.contains("\"exit_to_dma_retired\":1000"), "{json}");
        assert!(json.contains("\"exit_to_block_ready\":8000"), "{json}");
        assert!(json.contains("\"dma_retired_to_block_ready\":7000"), "{json}");
    }

    #[test]
    fn lost_exit_record_is_unknown_not_zero() {
        let log = RETIRE.replace("exit_ns=4000", "exit_ns=unknown");
        let json = retire_report("ahci", "tcg", &log).unwrap();
        assert!(json.contains("\"exit_to_dma_retired\":\"unknown\""), "{json}");
        assert!(json.contains("\"exit_to_block_ready\":\"unknown\""), "{json}");
        assert!(json.contains("\"dma_retired_to_block_ready\":7000"), "{json}");
    }

    #[test]
    fn default_kernel_or_missing_restart_yields_no_report() {
        assert_eq!(retire_report("ahci", "tcg", "[pcie] quiesced 00:1f.2 for tid 4"), None);
        let no_restart: String = RETIRE.lines().take(4).collect::<Vec<_>>().join("\n");
        assert_eq!(retire_report("ahci", "tcg", &no_restart), None);
    }

    const SWAP: &str = "\
[bench-ts] state-stashed tid=3 ns=900 exit_ns=unknown
[bench-ts] state-stashed tid=3 ns=1000 exit_ns=unknown
[bench-ts] replacement-sealed tid=7 ns=1500 exit_ns=unknown
[bench-ts] state-restored tid=5 ns=1600 exit_ns=unknown
[bench-ts] state-restored tid=7 ns=2500 exit_ns=unknown
[bench-ts] hotswap-ready tid=7 ns=4000 exit_ns=unknown
";

    #[test]
    fn hotswap_spans_follow_the_replacement_tid() {
        let json = hotswap_report("cli", "tcg", SWAP).expect("swap present");
        assert!(json.contains("\"new_tid\":7"), "{json}");
        assert!(json.contains("\"stashed_to_sealed\":500"), "{json}");
        assert!(json.contains("\"sealed_to_restored\":1000"), "{json}");
        assert!(json.contains("\"sealed_to_ready\":2500"), "{json}");
        assert!(json.contains("\"stashed_to_ready\":3000"), "{json}");
    }

    #[test]
    fn hotswap_without_ready_or_snapshot_is_partial_or_absent() {
        let no_ready: String = SWAP.lines().take(5).collect::<Vec<_>>().join("\n");
        assert_eq!(hotswap_report("cli", "tcg", &no_ready), None);
        let no_stash: String = SWAP.lines().skip(2).collect::<Vec<_>>().join("\n");
        let json = hotswap_report("cli", "tcg", &no_stash).unwrap();
        assert!(json.contains("\"stashed_to_sealed\":\"unknown\""), "{json}");
        assert!(json.contains("\"sealed_to_ready\":2500"), "{json}");
    }
}
