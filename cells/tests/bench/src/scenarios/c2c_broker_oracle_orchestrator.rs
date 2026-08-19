use super::c2c_broker_oracle_report::{
    aggregate, percentile_pair, print_overflow_line, print_role_gate, print_soak_line,
    print_sweep_line, snapshot_delta, MAX_CLIENTS, SOAK_CALLS, SWEEP_LEVELS,
};
use super::c2c_broker_oracle_wire::{
    decode_posted, decode_ready, decode_summary, encode_config, encode_drain, encode_start,
    ClientConfig, ClientMode, ClientSummary, CONFIG_BYTES, DRAIN_BYTES, POSTED_BYTES, READY_BYTES,
    START_BYTES, SUMMARY_BYTES,
};
use ostd::io::println;
use ostd::syscall::sys_send;
use ostd::task::yield_now;
use service_net_broker::bench_oracle::OracleSnapshot;

mod calibration;
mod support;
use support::{broker_snapshot, recv_posted, recv_summary, spawn_and_ready};

pub fn run() -> ! {
    println("[c2c-broker-oracle] START");
    let mut broker_tid = calibration::run();
    let mut role_gate = false;
    for (stage_idx, n) in SWEEP_LEVELS.into_iter().enumerate() {
        let mut latencies = [0u64; MAX_CLIENTS];
        let mut send_latencies = [0u64; MAX_CLIENTS];
        let mut reply_waits = [0u64; MAX_CLIENTS];
        let mut worker_latencies = [0u64; MAX_CLIENTS];
        let mut reply_pump_latencies = [0u64; MAX_CLIENTS];
        let mut client_wake_latencies = [0u64; MAX_CLIENTS];
        let (summaries, observed_tid, before, after) =
            run_stage(n, stage_idx as u64, ClientMode::EchoSync, 1, 0, false);
        broker_tid = broker_tid.max(observed_tid);
        let total = aggregate(&summaries[..n]);
        for (idx, summary) in summaries[..n].iter().enumerate() {
            latencies[idx] = summary.latency_ns;
            send_latencies[idx] = summary.send_latency_ns;
            reply_waits[idx] = summary.reply_wait_ns;
            worker_latencies[idx] = summary.worker_latency_ns;
            reply_pump_latencies[idx] = summary.reply_pump_latency_ns;
            client_wake_latencies[idx] = summary.client_wake_latency_ns;
        }
        let delta = snapshot_delta(before, after);
        let (p50, p99) = percentile_pair(&mut latencies, n);
        let (_, send_p99) = percentile_pair(&mut send_latencies, n);
        let (_, reply_wait_p99) = percentile_pair(&mut reply_waits, n);
        let (_, worker_p99) = percentile_pair(&mut worker_latencies, n);
        let (_, reply_pump_p99) = percentile_pair(&mut reply_pump_latencies, n);
        let (_, client_wake_p99) = percentile_pair(&mut client_wake_latencies, n);
        print_sweep_line(
            n,
            total,
            delta,
            p50,
            p99,
            send_p99,
            reply_wait_p99,
            worker_p99,
            reply_pump_p99,
            client_wake_p99,
        );
        if !role_gate && total.success > 0 && delta.network_progress > 0 {
            role_gate = true;
            print_role_gate(true, observed_tid);
        }
    }
    if !role_gate {
        print_role_gate(false, broker_tid);
    }
    run_soak();
    run_overflow();
    ostd::syscall::sys_exit(role_gate as usize ^ 1);
}

fn run_soak() {
    let per_client = SOAK_CALLS / MAX_CLIENTS as u16;
    let (summaries, _, before, after) =
        run_stage(MAX_CLIENTS, 100, ClientMode::EchoSync, per_client, 0, false);
    let total = aggregate(&summaries);
    print_soak_line(total, snapshot_delta(before, after), after);
}

fn run_overflow() {
    let before = broker_snapshot(9000).unwrap_or_default();
    let (hold_tid, broker_tid) = spawn_and_ready(ClientConfig {
        mode: ClientMode::HoldAsync,
        request_count: 1,
        base_sequence: 9001,
        hold_turns: service_net_broker::bench_oracle::MAX_HOLD_TURNS,
        ack_posts: true,
        wait_for_start: false,
        wait_for_drain: true,
    });
    recv_posted(hold_tid, 1);
    for _ in 0..64 {
        yield_now();
    }
    let mut tids = [0usize; 5];
    for (idx, slot) in tids[..4].iter_mut().enumerate() {
        let (tid, _) = spawn_and_ready(ClientConfig {
            mode: ClientMode::EchoAsync,
            request_count: 4,
            base_sequence: 9100 + (idx as u64 * 8),
            hold_turns: 0,
            ack_posts: true,
            wait_for_start: false,
            wait_for_drain: true,
        });
        recv_posted(tid, 4);
        *slot = tid;
    }
    let (overflow_tid, _) = spawn_and_ready(ClientConfig {
        mode: ClientMode::EchoAsync,
        request_count: 1,
        base_sequence: 9500,
        hold_turns: 0,
        ack_posts: true,
        wait_for_start: false,
        wait_for_drain: true,
    });
    recv_posted(overflow_tid, 1);
    let mut drain = [0u8; DRAIN_BYTES];
    encode_drain(&mut drain);
    let _ = sys_send(hold_tid, &drain);
    for &tid in &tids[..4] {
        let _ = sys_send(tid, &drain);
    }
    let _ = sys_send(overflow_tid, &drain);
    let mut summaries = [ClientSummary::default(); 6];
    summaries[0] = recv_summary(hold_tid);
    for (idx, &tid) in tids[..4].iter().enumerate() {
        summaries[idx + 1] = recv_summary(tid);
    }
    summaries[5] = recv_summary(overflow_tid);
    let after = broker_snapshot(9900).unwrap_or(before);
    let total = aggregate(&summaries);
    let overflow_ok = summaries[0].success == 1
        && summaries[1..5]
            .iter()
            .all(|s| s.success == 4 && s.busy == 0 && s.indeterminate == 0)
        && summaries[5].busy == 1
        && summaries[5].success == 0
        && after.peak_request_queue >= 16;
    print_overflow_line(
        if overflow_ok { "PASS" } else { "BLOCKED" },
        total,
        snapshot_delta(before, after),
        after.peak_request_queue,
    );
    let _ = broker_tid;
}

fn run_stage(
    n: usize,
    seed: u64,
    mode: ClientMode,
    request_count: u16,
    hold_turns: u16,
    staged: bool,
) -> (
    [ClientSummary; MAX_CLIENTS],
    usize,
    OracleSnapshot,
    OracleSnapshot,
) {
    let before = broker_snapshot(seed).unwrap_or_default();
    let mut tids = [0usize; MAX_CLIENTS];
    let mut summaries = [ClientSummary::default(); MAX_CLIENTS];
    let mut broker_tid = 0usize;
    for (idx, slot) in tids[..n].iter_mut().enumerate() {
        let (tid, observed_tid) = spawn_and_ready(ClientConfig {
            mode,
            request_count,
            base_sequence: seed * 100 + idx as u64,
            hold_turns,
            ack_posts: staged,
            wait_for_start: true,
            wait_for_drain: staged,
        });
        *slot = tid;
        broker_tid = observed_tid;
    }
    let mut start = [0u8; START_BYTES];
    encode_start(&mut start);
    for &tid in &tids[..n] {
        let _ = sys_send(tid, &start);
    }
    if staged {
        let mut drain = [0u8; DRAIN_BYTES];
        encode_drain(&mut drain);
        for &tid in &tids[..n] {
            let _ = sys_send(tid, &drain);
        }
    }
    for (idx, &tid) in tids[..n].iter().enumerate() {
        summaries[idx] = recv_summary(tid);
    }
    let after = broker_snapshot(seed + 1).unwrap_or(before);
    (summaries, broker_tid, before, after)
}
