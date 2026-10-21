// SPDX-License-Identifier: MIT
//! Navigation stage timing. One monotonic origin per navigation; every stamp
//! prints `[ocel/t] +<ms> <stage>` so a serial log yields a load waterfall.
use core::sync::atomic::{AtomicU64, Ordering};

static ORIGIN_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 { ostd::syscall::sys_get_time_ms().unwrap_or(0) }

/// Reset the origin at the start of a navigation.
pub fn begin(url: &str) {
    ORIGIN_MS.store(now_ms(), Ordering::Relaxed);
    ostd::io::println(&alloc::format!("[ocel/t] +0ms navigate {}", url));
}

/// Milliseconds since `begin`.
pub fn elapsed_ms() -> u64 { now_ms().saturating_sub(ORIGIN_MS.load(Ordering::Relaxed)) }

/// Print one stage stamp.
pub fn stamp(stage: &str) {
    ostd::io::println(&alloc::format!("[ocel/t] +{}ms {}", elapsed_ms(), stage));
}

/// Print a stage stamp with the duration of the stage itself.
pub fn stamp_took(stage: &str, started_ms: u64) {
    let now = now_ms();
    ostd::io::println(&alloc::format!(
        "[ocel/t] +{}ms {} ({}ms)",
        now.saturating_sub(ORIGIN_MS.load(Ordering::Relaxed)), stage, now.saturating_sub(started_ms)
    ));
}

pub fn now() -> u64 { now_ms() }
