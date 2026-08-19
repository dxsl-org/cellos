use super::c2c_broker_oracle_wire::ClientSummary;
use alloc::format;
use ostd::io::println;
use service_net_broker::bench_oracle::OracleSnapshot;

pub const MAX_CLIENTS: usize = 16;
pub const P99_CEILING_NS: u64 = 147_000;
pub const SOAK_CALLS: u16 = 10_000;
pub const SWEEP_LEVELS: [usize; 5] = [1, 2, 4, 8, 16];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapshotDelta {
    pub accepted: u64,
    pub completed: u64,
    pub busy: u64,
    pub terminal: u64,
    pub indeterminate: u64,
    pub duplicate: u64,
    pub stale_reply: u64,
    pub heartbeat_miss: u64,
    pub watchdog_expired: u64,
    pub network_progress: u64,
}

pub fn snapshot_delta(before: OracleSnapshot, after: OracleSnapshot) -> SnapshotDelta {
    SnapshotDelta {
        accepted: after
            .accepted
            .saturating_sub(before.accepted)
            .saturating_sub(1),
        completed: after
            .completed
            .saturating_sub(before.completed)
            .saturating_sub(1),
        busy: after.busy.saturating_sub(before.busy),
        terminal: after.terminal.saturating_sub(before.terminal),
        indeterminate: after.indeterminate.saturating_sub(before.indeterminate),
        duplicate: after.duplicate.saturating_sub(before.duplicate),
        stale_reply: after.stale_reply.saturating_sub(before.stale_reply),
        heartbeat_miss: after.heartbeat_miss.saturating_sub(before.heartbeat_miss),
        watchdog_expired: after
            .watchdog_expired
            .saturating_sub(before.watchdog_expired),
        network_progress: after
            .network_progress
            .saturating_sub(before.network_progress),
    }
}

pub fn percentile_pair(samples: &mut [u64], len: usize) -> (u64, u64) {
    samples[..len].sort_unstable();
    let p50_idx = len.saturating_sub(1) / 2;
    let p99_idx = ((len * 99).saturating_sub(1) / 100).min(len.saturating_sub(1));
    (samples[p50_idx], samples[p99_idx])
}

pub fn aggregate(summaries: &[ClientSummary]) -> ClientSummary {
    let mut total = ClientSummary::default();
    for summary in summaries {
        total.attempted = total.attempted.saturating_add(summary.attempted);
        total.success = total.success.saturating_add(summary.success);
        total.busy = total.busy.saturating_add(summary.busy);
        total.indeterminate = total.indeterminate.saturating_add(summary.indeterminate);
        total.correlation = total.correlation.saturating_add(summary.correlation);
    }
    total
}

pub fn print_role_gate(pass: bool, broker_tid: usize) {
    println(&format!(
        "[c2c-broker-oracle] role_gate={} broker_tid={}",
        if pass { "PASS" } else { "BLOCKED" },
        broker_tid
    ));
}

pub fn print_sweep_line(n: usize, total: ClientSummary, delta: SnapshotDelta, p50: u64, p99: u64) {
    let ceiling = if p99 <= P99_CEILING_NS
        && total.busy == 0
        && total.indeterminate == 0
        && total.correlation == 0
    {
        "PASS"
    } else {
        "FAIL"
    };
    println(&format!(
        "[c2c-broker-oracle] sweep n={} attempted={} success={} busy={} indeterminate={} duplicate={} stale={} correlation={} p50_ns={} p99_ns={} ceiling={} network_progress_delta={} heartbeat_miss_delta={} watchdog_expired_delta={}",
        n,
        total.attempted,
        total.success,
        total.busy,
        total.indeterminate,
        delta.duplicate,
        delta.stale_reply,
        total.correlation,
        p50,
        p99,
        ceiling,
        delta.network_progress,
        delta.heartbeat_miss,
        delta.watchdog_expired
    ));
}

pub fn print_soak_line(total: ClientSummary, delta: SnapshotDelta, snapshot: OracleSnapshot) {
    let silent_drop = delta
        .accepted
        .saturating_sub(delta.completed)
        .saturating_sub(delta.terminal);
    println(&format!(
        "[c2c-broker-oracle] soak attempted={} success={} busy={} indeterminate={} duplicate={} stale={} correlation={} accepted_delta={} completed_delta={} terminal_delta={} silent_drop={} peak_bytes={} static_footprint_bytes={}",
        total.attempted,
        total.success,
        total.busy,
        total.indeterminate,
        delta.duplicate,
        delta.stale_reply,
        total.correlation,
        delta.accepted,
        delta.completed,
        delta.terminal,
        silent_drop,
        snapshot.peak_bytes,
        snapshot.static_footprint_bytes
    ));
}

pub fn print_overflow_line(status: &str, total: ClientSummary, delta: SnapshotDelta, peak: u64) {
    println(&format!(
        "[c2c-broker-oracle] overflow status={} attempted={} success={} busy={} indeterminate={} correlation={} queue_peak={} network_progress_delta={} heartbeat_miss_delta={} watchdog_expired_delta={} busy_frame=0x7f01",
        status,
        total.attempted,
        total.success,
        total.busy,
        total.indeterminate,
        total.correlation,
        peak,
        delta.network_progress,
        delta.heartbeat_miss,
        delta.watchdog_expired
    ));
}
