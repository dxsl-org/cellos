//! Pure `top` core: option parsing, sampling arithmetic, and row ordering.
//!
//! Sampling itself (`sys_get_procs2`) and rendering stay in the shell; this
//! module turns two [`ProcessInfoV2`] snapshots into sorted [`TopRow`]s.

#[cfg(test)]
mod tests;

use alloc::string::String;
use api::syscall::ProcessInfoV2;
use core::cmp::Ordering;

/// Scheduler tick rate the kernel timestamps samples with.
pub const TIMER_HZ: u64 = 10_000_000;

/// Upper bound on rows sampled and rendered per frame.
pub const MAX_ROWS: usize = 64;

pub const USAGE: &str =
    "Usage: top [-a] [-b] [-n COUNT] [-d SECS] [-o cpu|mem|heap|pid|state|name]";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Cpu,
    Mem,
    Heap,
    Pid,
    State,
    Name,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopOptions {
    pub batch: bool,
    pub count: Option<usize>,
    pub delay_ticks: u64,
    pub show_all: bool,
    pub sort: SortKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopRow {
    pub pid: u64,
    pub state: u32,
    pub name: String,
    pub cpu_tenths: u16,
    pub heap_bytes: u64,
    pub mem_bytes: u64,
}

/// Parse `top` flags.
///
/// # Errors
/// Returns [`USAGE`] for an unknown flag or a missing/unparsable flag value.
pub fn parse_options(args: &[&str]) -> Result<TopOptions, &'static str> {
    let mut options = TopOptions {
        batch: false,
        count: None,
        delay_ticks: TIMER_HZ,
        show_all: false,
        sort: SortKey::Cpu,
    };
    let mut index = 0usize;
    while let Some(arg) = args.get(index).copied() {
        match arg {
            "-a" => options.show_all = true,
            "-b" => options.batch = true,
            "-n" => {
                index += 1;
                options.count = Some(parse_usize(args.get(index).copied()).ok_or(USAGE)?);
            }
            "-d" => {
                index += 1;
                options.delay_ticks = parse_u64(args.get(index).copied())
                    .and_then(|secs| secs.checked_mul(TIMER_HZ))
                    .ok_or(USAGE)?;
            }
            "-o" => {
                index += 1;
                options.sort = parse_sort(args.get(index).copied()).ok_or(USAGE)?;
            }
            _ => return Err(USAGE),
        }
        index += 1;
    }
    Ok(options)
}

/// Turn two telemetry snapshots into sorted display rows.
///
/// CPU percent comes from the delta between the two samples, so the first
/// frame after startup (no matching `previous` row) reports 0.0%.
pub fn build_rows(
    previous: &[ProcessInfoV2],
    current: &[ProcessInfoV2],
    show_all: bool,
    sort: SortKey,
) -> alloc::vec::Vec<TopRow> {
    let mut rows = alloc::vec::Vec::with_capacity(current.len().min(MAX_ROWS));
    for row in current.iter() {
        if !show_all && row.state == 3 {
            continue;
        }
        let prev = previous.iter().find(|candidate| candidate.id == row.id);
        let wall_delta = prev
            .map(|entry| row.sample_ticks.saturating_sub(entry.sample_ticks))
            .unwrap_or(0);
        let cpu_delta = prev
            .map(|entry| row.cpu_run_ticks.saturating_sub(entry.cpu_run_ticks))
            .unwrap_or(0);
        rows.push(TopRow {
            pid: row.id,
            state: row.state,
            name: trim_name(&row.name),
            cpu_tenths: cpu_tenths(cpu_delta, wall_delta),
            heap_bytes: row.heap_bytes,
            mem_bytes: row.owned_bytes,
        });
    }
    rows.sort_unstable_by(|left, right| compare_rows(left, right, sort));
    rows
}

pub fn parse_sort(value: Option<&str>) -> Option<SortKey> {
    Some(match value? {
        "cpu" => SortKey::Cpu,
        "mem" => SortKey::Mem,
        "heap" => SortKey::Heap,
        "pid" => SortKey::Pid,
        "state" => SortKey::State,
        "name" => SortKey::Name,
        _ => return None,
    })
}

pub fn parse_usize(value: Option<&str>) -> Option<usize> {
    value?.parse().ok()
}

pub fn parse_u64(value: Option<&str>) -> Option<u64> {
    value?.parse().ok()
}

pub fn trim_name(name: &[u8; 32]) -> String {
    String::from(
        core::str::from_utf8(name)
            .unwrap_or("???")
            .trim_matches('\0'),
    )
}

pub fn state_label(state: u32) -> &'static str {
    match state {
        0 => "Ready",
        1 => "Running",
        2 => "Waiting",
        3 => "Dead",
        _ => "???",
    }
}

/// CPU share of the sample window in tenths of a percent, clamped to 100.0%.
pub fn cpu_tenths(cpu_delta: u64, wall_delta: u64) -> u16 {
    cpu_delta
        .saturating_mul(1000)
        .checked_div(wall_delta)
        // Clamp in u64 before narrowing: a sample window shorter than the CPU
        // delta (clock skew across a hotswap) would otherwise wrap the cast.
        .map_or(0, |tenths| core::cmp::min(tenths, 1000) as u16)
}

pub fn compare_rows(left: &TopRow, right: &TopRow, sort: SortKey) -> Ordering {
    match sort {
        SortKey::Cpu => right.cpu_tenths.cmp(&left.cpu_tenths),
        SortKey::Mem => right.mem_bytes.cmp(&left.mem_bytes),
        SortKey::Heap => right.heap_bytes.cmp(&left.heap_bytes),
        SortKey::Pid => left.pid.cmp(&right.pid),
        SortKey::State => state_rank(left.state).cmp(&state_rank(right.state)),
        SortKey::Name => left.name.cmp(&right.name),
    }
    .then_with(|| left.pid.cmp(&right.pid))
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes;
    let mut unit = 0usize;
    while value >= 1024 && unit + 1 < UNITS.len() {
        value /= 1024;
        unit += 1;
    }
    alloc::format!("{value}{}", UNITS[unit])
}

fn state_rank(state: u32) -> u8 {
    match state {
        1 => 0,
        0 => 1,
        2 => 2,
        3 => 3,
        _ => 4,
    }
}

pub fn sort_label(sort: SortKey) -> &'static str {
    match sort {
        SortKey::Cpu => "cpu",
        SortKey::Mem => "mem",
        SortKey::Heap => "heap",
        SortKey::Pid => "pid",
        SortKey::State => "state",
        SortKey::Name => "name",
    }
}
