use super::{
    decode_config, decode_reply_frame, encode_summary, is_drain, is_start, ClientConfig,
    ClientMode, ClientSummary, CONFIG_BYTES, DRAIN_BYTES, ECHO_BODY, START_BYTES, SUMMARY_BYTES,
};
use ostd::syscall::{sys_exit, sys_lookup_service, sys_recv, sys_send, SyscallResult};
use ostd::task::yield_now;
use service_net_broker::local_ingress::ReplyStatus;

pub fn update_summary(
    summary: &mut ClientSummary,
    sequence: u64,
    rx: &[u8],
    latency_ns: Option<u64>,
    config: ClientConfig,
) {
    match decode_reply_frame(rx) {
        Ok(reply) => {
            summary.latency_ns = latency_ns.unwrap_or(0);
            count_reply(
                summary,
                reply.status,
                reply.client_sequence == sequence,
                payload_matches(config, reply.payload),
            );
        }
        Err(_) => summary.correlation += 1,
    }
}

pub fn payload_matches(config: ClientConfig, payload: &[u8]) -> bool {
    match config.mode {
        ClientMode::HoldAsync => {
            payload.len() == 3
                && payload[0] == service_net_broker::bench_oracle::OP_HOLD
                && payload[1..3] == config.hold_turns.to_le_bytes()
        }
        _ => service_net_broker::bench_oracle::decode_timed_echo_reply(payload, ECHO_BODY).is_ok(),
    }
}

pub fn wait_config() -> (usize, ClientConfig) {
    let mut buf = [0u8; CONFIG_BYTES];
    loop {
        if let SyscallResult::Ok(sender) = sys_recv(0, &mut buf) {
            if let Some(config) = decode_config(&buf) {
                return (sender, config);
            }
        }
        yield_now();
    }
}

pub fn wait_broker() -> Option<usize> {
    for _ in 0..200 {
        if let Some(broker_tid) = sys_lookup_service(api::syscall::service::NET_BROKER) {
            return Some(broker_tid);
        }
        yield_now();
    }
    None
}

pub fn wait_start(parent_tid: usize) {
    let mut buf = [0u8; START_BYTES];
    wait_signal(parent_tid, &mut buf, is_start);
}

pub fn wait_drain(parent_tid: usize) {
    let mut buf = [0u8; DRAIN_BYTES];
    wait_signal(parent_tid, &mut buf, is_drain);
}

pub fn finish(parent_tid: usize, summary: ClientSummary, code: usize) -> ! {
    let mut out = [0u8; SUMMARY_BYTES];
    encode_summary(summary, &mut out);
    let _ = sys_send(parent_tid, &out);
    sys_exit(code)
}

pub(super) fn count_reply(
    summary: &mut ClientSummary,
    status: ReplyStatus,
    sequence_ok: bool,
    payload_ok: bool,
) {
    if !sequence_ok {
        summary.correlation += 1;
        return;
    }
    match status {
        ReplyStatus::Success if payload_ok => summary.success += 1,
        ReplyStatus::Success => summary.correlation += 1,
        ReplyStatus::Busy => summary.busy += 1,
        ReplyStatus::Indeterminate => summary.indeterminate += 1,
    }
}

fn wait_signal(parent_tid: usize, buf: &mut [u8], accept: fn(&[u8]) -> bool) {
    while !matches!(sys_recv(parent_tid, buf), SyscallResult::Ok(_)) || !accept(buf) {
        yield_now();
    }
}
