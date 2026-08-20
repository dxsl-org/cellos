#[cfg(feature = "restart-oracle")]
use super::broker_snapshot;
#[cfg(feature = "restart-oracle")]
use alloc::format;
#[cfg(feature = "restart-oracle")]
use ostd::io::println;
#[cfg(feature = "restart-oracle")]
use ostd::syscall::{sys_lookup_service, sys_send, SyscallResult};
#[cfg(feature = "restart-oracle")]
use ostd::task::yield_now;
#[cfg(feature = "restart-oracle")]
use service_net_broker::bench_oracle::{
    decode_reply_frame, decode_timed_echo_reply, encode_echo_request, encode_restart_request,
    OracleSnapshot, STATIC_FOOTPRINT_BYTES,
};
#[cfg(feature = "restart-oracle")]
use service_net_broker::local_ingress::ReplyStatus;

#[cfg(feature = "restart-oracle")]
const IPC_BUF_SIZE: usize = api::ipc::IPC_BUF_SIZE;
#[cfg(feature = "restart-oracle")]
const RESTART_SEQUENCE: u64 = 0xC2C0_0300;
#[cfg(feature = "restart-oracle")]
const RESTART_BODY: &[u8] = b"c2c-restart";
#[cfg(feature = "restart-oracle")]
const RELOOKUP_TURNS: usize = 2_000;

#[cfg(not(feature = "restart-oracle"))]
pub fn run() -> bool {
    true
}

#[cfg(feature = "restart-oracle")]
pub fn run() -> bool {
    let Some(old_tid) = sys_lookup_service(api::syscall::service::NET_BROKER) else {
        print_result(0, 0, false, false, false, false, None);
        return false;
    };
    let mut tx = [0u8; IPC_BUF_SIZE];
    let restart_len = encode_restart_request(RESTART_SEQUENCE, &mut tx).unwrap_or(0);
    if restart_len == 0 || !matches!(sys_send(old_tid, &tx[..restart_len]), SyscallResult::Ok(_)) {
        print_result(old_tid, 0, false, false, false, false, None);
        return false;
    }

    let stale_len = encode_echo_request(RESTART_SEQUENCE, RESTART_BODY, &mut tx).unwrap_or(0);
    let stale_indeterminate =
        stale_len != 0 && !matches!(sys_send(old_tid, &tx[..stale_len]), SyscallResult::Ok(_));
    let gap_observed = sys_lookup_service(api::syscall::service::NET_BROKER) != Some(old_tid);
    let Some(new_tid) = wait_for_replacement(old_tid) else {
        print_result(
            old_tid,
            0,
            gap_observed,
            stale_indeterminate,
            false,
            false,
            None,
        );
        return false;
    };

    let snapshot = broker_snapshot(RESTART_SEQUENCE + 1);
    let state_reset = snapshot.is_some_and(snapshot_is_fresh);
    let retry_ok = retry_echo(new_tid, &mut tx);
    print_result(
        old_tid,
        new_tid,
        gap_observed,
        stale_indeterminate,
        state_reset,
        retry_ok,
        snapshot,
    );
    new_tid != old_tid && stale_indeterminate && state_reset && retry_ok
}

#[cfg(feature = "restart-oracle")]
fn wait_for_replacement(old_tid: usize) -> Option<usize> {
    for _ in 0..RELOOKUP_TURNS {
        if let Some(tid) = sys_lookup_service(api::syscall::service::NET_BROKER) {
            if tid != old_tid {
                return Some(tid);
            }
        }
        yield_now();
    }
    None
}

#[cfg(feature = "restart-oracle")]
fn retry_echo(new_tid: usize, tx: &mut [u8; IPC_BUF_SIZE]) -> bool {
    let Ok(len) = encode_echo_request(RESTART_SEQUENCE + 2, RESTART_BODY, tx) else {
        return false;
    };
    if !matches!(sys_send(new_tid, &tx[..len]), SyscallResult::Ok(_)) {
        return false;
    }
    let mut rx = [0u8; IPC_BUF_SIZE];
    if !super::super::c2c_broker_oracle::recv_from_broker(new_tid, &mut rx) {
        return false;
    }
    decode_reply_frame(&rx).is_ok_and(|reply| {
        reply.status == ReplyStatus::Success
            && reply.client_sequence == RESTART_SEQUENCE + 2
            && decode_timed_echo_reply(reply.payload, RESTART_BODY).is_ok()
    })
}

#[cfg(feature = "restart-oracle")]
fn snapshot_is_fresh(snapshot: OracleSnapshot) -> bool {
    snapshot.accepted == 1
        && snapshot.completed == 0
        && snapshot.busy == 0
        && snapshot.terminal == 0
        && snapshot.indeterminate == 0
        && snapshot.duplicate == 0
        && snapshot.stale_reply == 0
        && snapshot.try_send_busy == 0
        && snapshot.heartbeat_miss == 0
        && snapshot.watchdog_expired == 0
        && snapshot.peak_request_queue == 1
        && snapshot.peak_reply_queue == 0
        && snapshot.peak_in_flight == 1
        && snapshot.static_footprint_bytes == STATIC_FOOTPRINT_BYTES as u64
}

#[cfg(feature = "restart-oracle")]
fn print_result(
    old_tid: usize,
    new_tid: usize,
    gap: bool,
    stale_indeterminate: bool,
    state_reset: bool,
    retry_ok: bool,
    snapshot: Option<OracleSnapshot>,
) {
    let snapshot = snapshot.unwrap_or_default();
    let pass = new_tid != 0 && new_tid != old_tid && stale_indeterminate && state_reset && retry_ok;
    println(&format!(
        "[c2c-broker-oracle] restart status={} old_tid={} registry_gap={} stale_send={} new_tid={} state_reset={} retry={} accepted={} completed={} stale={} duplicate={} heartbeat_miss={} watchdog_expired={}",
        if pass { "PASS" } else { "BLOCKED" },
        old_tid,
        if gap { "OBSERVED" } else { "MISSED" },
        if stale_indeterminate { "INDETERMINATE" } else { "FAILED" },
        new_tid,
        if state_reset { "PASS" } else { "BLOCKED" },
        if retry_ok { "PASS" } else { "BLOCKED" },
        snapshot.accepted,
        snapshot.completed,
        snapshot.stale_reply,
        snapshot.duplicate,
        snapshot.heartbeat_miss,
        snapshot.watchdog_expired,
    ));
}
