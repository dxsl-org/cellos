//! control_loop_jitter — periodic deadline adherence under load.
//!
//! A RealTime cell wakes every `PERIOD_TICKS` (via `recv_timeout`, which blocks
//! until timeout since nothing sends to it), measures the actual elapsed period,
//! and records the per-cycle error (|actual − period|) plus a deadline-miss when
//! the cycle overruns by more than `SLACK_TICKS`. Mirrors a fixed-rate control
//! loop (PID / software PWM) and proves "control-loop meets deadline" (G1 #3).
//!
//! Period is 50 ms (5 scheduler ticks) so jitter is meaningful above the 10 ms
//! tick quantum. Runs under background load cells spawned by the orchestrator.
//!
//! ⚠️ Runtime numbers require the bench cell embedded at `/bin/bench` (phase-05).

extern crate alloc;
use crate::framework::rt_report::RtReport;
use crate::framework::timer::timer_freq_hz;
use alloc::vec::Vec;
use ostd::syscall::{sys_exit, sys_get_time, sys_recv, sys_recv_timeout, sys_send, SyscallResult};
/// Receive timeout uses 10 ms scheduler ticks; five ticks request a 50 ms period.
const PERIOD_SCHEDULER_TICKS: u64 = 5;
/// Target period duration in milliseconds (5 scheduler ticks @ 10 ms/tick = 50 ms).
const PERIOD_MS: u64 = 50;
/// Allowed measured overrun before a deadline miss: 5 ms slack.
const SLACK_MS: u64 = 5;
/// Number of measured periods.
const CL_ITERS: u32 = 200;

pub const RESULT_LEN: usize = 57;

fn put_u64(buf: &mut [u8; RESULT_LEN], offset: usize, value: u64) {
    buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u64(buf: &[u8; 64], offset: usize) -> u64 {
    u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap_or([0; 8]))
}

pub fn decode_result(buf: &[u8; 64]) -> api::ViResult<RtReport> {
    if buf[0] != 0 || buf[RESULT_LEN..].iter().any(|&byte| byte != 0xa5) {
        return Err(api::ViError::InvalidInput);
    }
    let min = get_u64(buf, 1);
    let p50 = get_u64(buf, 9);
    let p99 = get_u64(buf, 17);
    let p99_9 = get_u64(buf, 25);
    let max = get_u64(buf, 33);
    let jitter = get_u64(buf, 41);
    if min > p50 || p50 > p99 || p99 > p99_9 || p99_9 > max || jitter != max.saturating_sub(min) {
        return Err(api::ViError::InvalidInput);
    }
    Ok(RtReport {
        name: "control_loop",
        n: CL_ITERS,
        min,
        p50,
        p99,
        p99_9,
        max,
        jitter,
        deadline_miss: u32::try_from(get_u64(buf, 49)).map_err(|_| api::ViError::InvalidInput)?,
    })
}

/// RealTime probe role: run the periodic loop and return a private report wire
/// to the orchestrator, which publishes it only after cleanup succeeds.
pub fn run_control_loop() -> ! {
    let mut buf = [0u8; 8];
    // Block for the orchestrator's start ping so we can reply "done" to it later.
    let orch = loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(s) if s > 0 => break s,
            _ => ostd::task::yield_now(),
        }
    };

    let mut errors: Vec<u64> = Vec::with_capacity(CL_ITERS as usize);
    let mut miss = 0u32;
    let mut valid = true;
    let freq = timer_freq_hz();
    let period_time_ticks = (PERIOD_MS * freq) / 1000;
    let slack_time_ticks = (SLACK_MS * freq) / 1000;
    let mut prev = sys_get_time();
    for _ in 0..CL_ITERS {
        match sys_recv_timeout(0, &mut buf, PERIOD_SCHEDULER_TICKS) {
            SyscallResult::Ok(0) => {}
            _ => {
                valid = false;
                break;
            }
        }
        let now = sys_get_time();
        let actual = now.saturating_sub(prev);
        let err = actual.abs_diff(period_time_ticks);
        errors.push(err);
        if actual > period_time_ticks + slack_time_ticks {
            miss += 1;
        }
        prev = now;
    }

    if valid {
        let r = RtReport::build("control_loop", &mut errors, miss);
        let mut result = [0u8; RESULT_LEN];
        put_u64(&mut result, 1, r.min);
        put_u64(&mut result, 9, r.p50);
        put_u64(&mut result, 17, r.p99);
        put_u64(&mut result, 25, r.p99_9);
        put_u64(&mut result, 33, r.max);
        put_u64(&mut result, 41, r.jitter);
        put_u64(&mut result, 49, u64::from(r.deadline_miss));
        let _ = sys_send(orch, &result);
    } else {
        let _ = sys_send(orch, &[1u8]);
    }
    sys_exit(0);
}
