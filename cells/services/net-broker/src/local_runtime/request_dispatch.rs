use super::lock_broker_state;
use ostd::syscall::sys_get_time;
use service_net_broker::bench_oracle::{
    self, encode_hold_command, encode_snapshot_payload, OracleCommand, STATIC_FOOTPRINT_BYTES,
};
use service_net_broker::local_ingress::{ReplyStatus, MAX_REPLY_BODY};
use service_net_broker::local_queue::{QueuedReply, WorkerRequest, WORKER_HEARTBEAT_TICKS};

pub(super) fn process_request(request: &WorkerRequest) -> QueuedReply {
    match bench_oracle::parse_command(&request.payload[..request.payload_len]) {
        Ok(OracleCommand::Echo(body)) => {
            let mut payload = [0u8; MAX_REPLY_BODY];
            match bench_oracle::encode_timed_echo_reply(body, sys_get_time(), &mut payload) {
                Ok(len) => reply_with_payload(request, ReplyStatus::Success, &payload[..len]),
                Err(_) => reply_with_payload(request, ReplyStatus::Indeterminate, &[]),
            }
        }
        Ok(OracleCommand::Snapshot) => {
            let mut payload = [0u8; MAX_REPLY_BODY];
            let mut snapshot = [0u8; bench_oracle::SNAPSHOT_BYTES];
            let counters = lock_broker_state(true).counters;
            encode_snapshot_payload(&counters, STATIC_FOOTPRINT_BYTES as u64, &mut snapshot);
            payload[..snapshot.len()].copy_from_slice(&snapshot);
            reply_with_payload(request, ReplyStatus::Success, &payload[..snapshot.len()])
        }
        Ok(OracleCommand::Hold { work_turns }) => {
            run_bounded_hold(work_turns);
            let mut payload = [0u8; 3];
            let len = encode_hold_command(work_turns, &mut payload).unwrap_or(0);
            reply_with_payload(request, ReplyStatus::Success, &payload[..len])
        }
        Err(_) => reply_with_payload(request, ReplyStatus::Indeterminate, &[]),
    }
}

fn run_bounded_hold(work_turns: u16) {
    for _ in 0..work_turns {
        ostd::syscall::sys_heartbeat(WORKER_HEARTBEAT_TICKS);
        ostd::task::yield_now();
    }
}

fn reply_with_payload(request: &WorkerRequest, status: ReplyStatus, payload: &[u8]) -> QueuedReply {
    QueuedReply::new(
        request.key.caller_tid,
        request.key.request_id,
        request.client_sequence,
        status,
        payload,
        request.order,
    )
}
