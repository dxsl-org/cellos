use super::{
    decode_posted, decode_ready, decode_summary, encode_config, ClientConfig, ClientSummary,
    CONFIG_BYTES, POSTED_BYTES, READY_BYTES, SUMMARY_BYTES,
};
use api::task::TaskPriority;
use ostd::syscall::{
    sys_force_exit, sys_lookup_service, sys_recv, sys_send, sys_set_spawn_args, sys_spawn_pinned,
    SyscallResult,
};
use ostd::task::yield_now;
use service_net_broker::bench_oracle::{
    decode_reply_frame, decode_snapshot_payload, encode_snapshot_request, OracleSnapshot,
};
use service_net_broker::local_ingress::ReplyStatus;

const IPC_BUF_SIZE: usize = api::ipc::IPC_BUF_SIZE;
const PROBE_PATH: &str = "/bin/bench-probe";

pub fn spawn_and_ready(config: ClientConfig) -> (usize, usize) {
    sys_set_spawn_args("c2c-client");
    let tid = match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) => tid,
        _ => panic!("[c2c-broker-oracle] c2c-client spawn failed"),
    };
    let mut cfg = [0u8; CONFIG_BYTES];
    encode_config(config, &mut cfg);
    let _ = sys_send(tid, &cfg);
    (tid, recv_ready(tid))
}

pub fn recv_posted(tid: usize, count: u16) {
    let mut buf = [0u8; POSTED_BYTES];
    let mut seen = 0u16;
    while seen < count {
        if matches!(sys_recv(tid, &mut buf), SyscallResult::Ok(_))
            && decode_posted(&buf) == Some(seen + 1)
        {
            seen += 1;
        }
        yield_now();
    }
}

pub fn recv_summary(tid: usize) -> ClientSummary {
    let mut buf = [0u8; SUMMARY_BYTES];
    loop {
        if matches!(sys_recv(tid, &mut buf), SyscallResult::Ok(_)) {
            if let Some(summary) = decode_summary(&buf) {
                let _ = sys_force_exit(tid);
                yield_now();
                return summary;
            }
        }
        yield_now();
    }
}

pub fn broker_snapshot(sequence: u64) -> Option<OracleSnapshot> {
    let broker_tid = sys_lookup_service(api::syscall::service::NET_BROKER)?;
    let mut tx = [0u8; IPC_BUF_SIZE];
    let mut rx = [0u8; IPC_BUF_SIZE];
    let len = encode_snapshot_request(sequence, &mut tx).ok()?;
    matches!(sys_send(broker_tid, &tx[..len]), SyscallResult::Ok(_)).then_some(())?;
    super::super::c2c_broker_oracle::recv_from_broker(broker_tid, &mut rx).then_some(())?;
    let reply = decode_reply_frame(&rx).ok()?;
    (reply.status == ReplyStatus::Success).then_some(())?;
    decode_snapshot_payload(reply.payload).ok()
}

fn recv_ready(tid: usize) -> usize {
    let mut buf = [0u8; READY_BYTES];
    loop {
        if matches!(sys_recv(tid, &mut buf), SyscallResult::Ok(_)) {
            if let Some(broker_tid) = decode_ready(&buf) {
                return broker_tid;
            }
        }
        yield_now();
    }
}
