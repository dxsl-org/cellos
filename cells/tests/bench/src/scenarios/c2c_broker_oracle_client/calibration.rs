extern crate alloc;

use super::{
    decode_reply_frame, decode_timed_echo_reply, encode_echo_request, ClientConfig, ClientSummary,
    ECHO_BODY, IPC_BUF_SIZE,
};
use crate::framework::timer::ticks_to_ns;
use crate::scenarios::c2c_broker_oracle_report::{
    BROKER_CALIBRATION_SAMPLES, BROKER_CALIBRATION_WARMUP,
};
use alloc::vec::Vec;
use ostd::syscall::{sys_get_time, sys_send, SyscallResult};
use service_net_broker::local_ingress::ReplyStatus;

#[derive(Clone, Copy)]
struct TimedSample {
    total: u64,
    send: u64,
    reply_wait: u64,
    worker: u64,
    reply_pump: u64,
    client_wake: u64,
}

enum CallOutcome {
    Sample(TimedSample),
    Busy,
    Indeterminate,
    Correlation,
    InvalidTiming,
}

struct Samples {
    total: Vec<u64>,
    send: Vec<u64>,
    reply_wait: Vec<u64>,
    worker: Vec<u64>,
    reply_pump: Vec<u64>,
    client_wake: Vec<u64>,
}

impl Samples {
    fn new(capacity: usize) -> Self {
        Self {
            total: Vec::with_capacity(capacity),
            send: Vec::with_capacity(capacity),
            reply_wait: Vec::with_capacity(capacity),
            worker: Vec::with_capacity(capacity),
            reply_pump: Vec::with_capacity(capacity),
            client_wake: Vec::with_capacity(capacity),
        }
    }

    fn push(&mut self, sample: TimedSample) {
        self.total.push(sample.total);
        self.send.push(sample.send);
        self.reply_wait.push(sample.reply_wait);
        self.worker.push(sample.worker);
        self.reply_pump.push(sample.reply_pump);
        self.client_wake.push(sample.client_wake);
    }
}

pub fn run(broker_tid: usize, config: ClientConfig) -> ClientSummary {
    let sample_count = config.request_count.min(BROKER_CALIBRATION_SAMPLES);
    let mut summary = ClientSummary {
        attempted: sample_count,
        ..ClientSummary::default()
    };
    if sample_count == 0 {
        return summary;
    }
    let mut samples = Samples::new(sample_count as usize);
    let mut tx = [0u8; IPC_BUF_SIZE];
    let mut rx = [0u8; IPC_BUF_SIZE];
    let total_calls = BROKER_CALIBRATION_WARMUP.saturating_add(sample_count);
    for offset in 0..total_calls {
        let sequence = config.base_sequence + offset as u64;
        let outcome = measure_call(broker_tid, sequence, &mut tx, &mut rx);
        if offset < BROKER_CALIBRATION_WARMUP {
            if !matches!(outcome, CallOutcome::Sample(_)) {
                summary.warmup_failures = summary.warmup_failures.saturating_add(1);
            }
            continue;
        }
        match outcome {
            CallOutcome::Sample(sample) => {
                summary.success = summary.success.saturating_add(1);
                samples.push(sample);
            }
            CallOutcome::Busy => summary.busy = summary.busy.saturating_add(1),
            CallOutcome::Indeterminate => {
                summary.indeterminate = summary.indeterminate.saturating_add(1)
            }
            CallOutcome::Correlation => summary.correlation = summary.correlation.saturating_add(1),
            CallOutcome::InvalidTiming => {
                summary.success = summary.success.saturating_add(1);
                summary.timing_invalid = summary.timing_invalid.saturating_add(1);
            }
        }
    }
    if samples.total.len() == sample_count as usize {
        let (p50, p99) = percentiles(&mut samples.total);
        summary.latency_p50_ns = p50;
        summary.latency_ns = p99;
        summary.send_latency_ns = percentiles(&mut samples.send).1;
        summary.reply_wait_ns = percentiles(&mut samples.reply_wait).1;
        summary.worker_latency_ns = percentiles(&mut samples.worker).1;
        summary.reply_pump_latency_ns = percentiles(&mut samples.reply_pump).1;
        summary.client_wake_latency_ns = percentiles(&mut samples.client_wake).1;
    }
    summary
}

fn measure_call(
    broker_tid: usize,
    sequence: u64,
    tx: &mut [u8; IPC_BUF_SIZE],
    rx: &mut [u8; IPC_BUF_SIZE],
) -> CallOutcome {
    let Ok(len) = encode_echo_request(sequence, ECHO_BODY, tx) else {
        return CallOutcome::Indeterminate;
    };
    let start = sys_get_time();
    if !matches!(sys_send(broker_tid, &tx[..len]), SyscallResult::Ok(_)) {
        return CallOutcome::Indeterminate;
    }
    let sent = sys_get_time();
    if !super::super::c2c_broker_oracle::recv_from_broker(broker_tid, rx) {
        return CallOutcome::Indeterminate;
    }
    let received = sys_get_time();
    let Ok(reply) = decode_reply_frame(rx) else {
        return CallOutcome::Correlation;
    };
    if reply.client_sequence != sequence {
        return CallOutcome::Correlation;
    }
    match reply.status {
        ReplyStatus::Busy => return CallOutcome::Busy,
        ReplyStatus::Indeterminate | ReplyStatus::NotSupported => {
            return CallOutcome::Indeterminate
        }
        ReplyStatus::Success => {}
    }
    let Ok(timestamps) = decode_timed_echo_reply(reply.payload, ECHO_BODY) else {
        return CallOutcome::Correlation;
    };
    let worker = timestamps.worker_done_ticks;
    let reply_sent = timestamps.reply_send_ticks;
    if sent < start || worker < sent || reply_sent < worker || received < reply_sent {
        return CallOutcome::InvalidTiming;
    }
    CallOutcome::Sample(TimedSample {
        total: ticks_to_ns(received - start),
        send: ticks_to_ns(sent - start),
        reply_wait: ticks_to_ns(received - sent),
        worker: ticks_to_ns(worker - sent),
        reply_pump: ticks_to_ns(reply_sent - worker),
        client_wake: ticks_to_ns(received - reply_sent),
    })
}

fn percentiles(samples: &mut [u64]) -> (u64, u64) {
    samples.sort_unstable();
    let last = samples.len().saturating_sub(1);
    let p50 = (samples.len() / 2).min(last);
    let p99 = ((samples.len() * 99) / 100).min(last);
    (samples[p50], samples[p99])
}
