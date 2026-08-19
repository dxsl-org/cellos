use super::c2c_broker_oracle_wire::{
    decode_config, encode_posted, encode_ready, encode_summary, is_drain, is_start, ClientConfig,
    ClientMode, ClientSummary, CONFIG_BYTES, DRAIN_BYTES, POSTED_BYTES, READY_BYTES, START_BYTES,
    SUMMARY_BYTES,
};
use crate::framework::timer::ticks_to_ns;
use ostd::syscall::{sys_get_time, sys_recv, sys_send, SyscallResult};
use service_net_broker::bench_oracle::{
    decode_reply_frame, encode_echo_request, encode_hold_request, MAX_HOLD_TURNS,
};

mod support;

use support::{
    count_reply, finish, payload_matches, update_summary, wait_broker, wait_config, wait_drain,
    wait_start,
};

const IPC_BUF_SIZE: usize = api::ipc::IPC_BUF_SIZE;
const ECHO_BODY: &[u8] = b"c2c-oracle";
const ASYNC_BATCH_CAP: usize = 4;

pub fn run_client() -> ! {
    let (parent_tid, config) = wait_config();
    let Some(broker_tid) = wait_broker() else {
        finish(
            parent_tid,
            ClientSummary {
                attempted: config.request_count,
                indeterminate: config.request_count,
                ..ClientSummary::default()
            },
            1,
        );
    };
    let mut ready = [0u8; READY_BYTES];
    encode_ready(broker_tid, &mut ready);
    let _ = sys_send(parent_tid, &ready);
    if config.wait_for_start {
        wait_start(parent_tid);
    }
    let summary = match config.mode {
        ClientMode::EchoSync => run_sync(broker_tid, config),
        ClientMode::EchoAsync | ClientMode::HoldAsync => run_async(parent_tid, broker_tid, config),
    };
    finish(parent_tid, summary, summary.correlation as usize);
}

fn run_sync(broker_tid: usize, config: ClientConfig) -> ClientSummary {
    let mut summary = ClientSummary {
        attempted: config.request_count,
        ..ClientSummary::default()
    };
    let mut tx = [0u8; IPC_BUF_SIZE];
    let mut rx = [0u8; IPC_BUF_SIZE];
    for offset in 0..config.request_count {
        let sequence = config.base_sequence + offset as u64;
        let len = match encode_echo_request(sequence, ECHO_BODY, &mut tx) {
            Ok(len) => len,
            Err(_) => {
                summary.indeterminate += 1;
                continue;
            }
        };
        let start = sys_get_time();
        if !matches!(sys_send(broker_tid, &tx[..len]), SyscallResult::Ok(_)) {
            summary.indeterminate += 1;
            continue;
        }
        let latency = match sys_recv(broker_tid, &mut rx) {
            SyscallResult::Ok(_) => Some(ticks_to_ns(sys_get_time().saturating_sub(start))),
            SyscallResult::Err(_) => None,
        };
        update_summary(&mut summary, sequence, &rx, latency, config);
    }
    summary
}

fn run_async(parent_tid: usize, broker_tid: usize, config: ClientConfig) -> ClientSummary {
    let mut summary = ClientSummary {
        attempted: config.request_count,
        ..ClientSummary::default()
    };
    let mut tx = [0u8; IPC_BUF_SIZE];
    let mut rx = [0u8; IPC_BUF_SIZE];
    let mut posted = [0u8; POSTED_BYTES];
    let mut seen = [false; ASYNC_BATCH_CAP];
    let mut replies_expected = 0u16;
    for offset in 0..config.request_count {
        let sequence = config.base_sequence + offset as u64;
        let request = match config.mode {
            ClientMode::HoldAsync => {
                encode_hold_request(sequence, config.hold_turns.min(MAX_HOLD_TURNS), &mut tx)
            }
            _ => encode_echo_request(sequence, ECHO_BODY, &mut tx),
        };
        if let Ok(len) = request {
            if matches!(sys_send(broker_tid, &tx[..len]), SyscallResult::Ok(_)) {
                replies_expected += 1;
            } else {
                summary.indeterminate += 1;
            }
        } else {
            summary.indeterminate += 1;
        }
        if config.ack_posts {
            encode_posted(offset + 1, &mut posted);
            let _ = sys_send(parent_tid, &posted);
        }
    }
    if config.wait_for_drain {
        wait_drain(parent_tid);
    }
    for _ in 0..replies_expected {
        if !matches!(sys_recv(broker_tid, &mut rx), SyscallResult::Ok(_)) {
            summary.indeterminate += 1;
            continue;
        }
        match decode_reply_frame(&rx) {
            Ok(reply) => {
                let idx = reply.client_sequence.saturating_sub(config.base_sequence) as usize;
                if idx >= ASYNC_BATCH_CAP || idx >= config.request_count as usize || seen[idx] {
                    summary.correlation += 1;
                    continue;
                }
                seen[idx] = true;
                count_reply(
                    &mut summary,
                    reply.status,
                    reply.client_sequence == config.base_sequence + idx as u64,
                    payload_matches(config, reply.payload),
                );
            }
            Err(_) => summary.correlation += 1,
        }
    }
    summary
}
