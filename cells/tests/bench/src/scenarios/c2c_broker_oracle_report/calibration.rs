use super::{
    ClientSummary, SnapshotDelta, BROKER_CALIBRATION_SAMPLES, BROKER_CALIBRATION_WARMUP,
    DIRECT_IPC_REFERENCE_P99_NS,
};
use alloc::format;
use ostd::io::println;

pub fn print_calibration_line(summary: ClientSummary, delta: SnapshotDelta) {
    let expected_calls = BROKER_CALIBRATION_WARMUP as u64 + summary.attempted as u64;
    let measured = summary.attempted == BROKER_CALIBRATION_SAMPLES
        && summary.success == summary.attempted
        && summary.busy == 0
        && summary.indeterminate == 0
        && summary.correlation == 0
        && summary.warmup_failures == 0
        && summary.timing_invalid == 0
        && delta.accepted == expected_calls
        && delta.completed == expected_calls
        && delta.terminal == 0
        && delta.indeterminate == 0
        && delta.duplicate == 0
        && delta.stale_reply == 0
        && delta.heartbeat_miss == 0
        && delta.watchdog_expired == 0;
    println(&format!(
        "[c2c-broker-oracle] baseline warmup={} samples={} success={} busy={} indeterminate={} correlation={} warmup_failures={} timing_invalid={} p50_ns={} p99_ns={} send_p99_ns={} reply_wait_p99_ns={} worker_p99_ns={} reply_pump_p99_ns={} client_wake_p99_ns={} calibration={} direct_ipc_ref_ns={} accepted_delta={} completed_delta={} network_progress_delta={} heartbeat_miss_delta={} watchdog_expired_delta={}",
        BROKER_CALIBRATION_WARMUP,
        summary.attempted,
        summary.success,
        summary.busy,
        summary.indeterminate,
        summary.correlation,
        summary.warmup_failures,
        summary.timing_invalid,
        summary.latency_p50_ns,
        summary.latency_ns,
        summary.send_latency_ns,
        summary.reply_wait_ns,
        summary.worker_latency_ns,
        summary.reply_pump_latency_ns,
        summary.client_wake_latency_ns,
        if measured { "MEASURED" } else { "BLOCKED" },
        DIRECT_IPC_REFERENCE_P99_NS,
        delta.accepted,
        delta.completed,
        delta.network_progress,
        delta.heartbeat_miss,
        delta.watchdog_expired,
    ));
}
