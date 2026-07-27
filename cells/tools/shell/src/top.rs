//! Shell adapter for `top`: samples `GetProcs2` and drives the render loop.
//!
//! Option parsing, delta arithmetic and row ordering are pure and live in
//! `text_engine::top`; only the syscall sampling, the delay/quit loop and the
//! terminal writes stay here.

extern crate alloc;

mod render;

use alloc::vec::Vec;
use api::syscall::ProcessInfoV2;
use ostd::{prelude::*, syscall};
use text_engine::top::{build_rows, parse_options, TopOptions, MAX_ROWS};

pub fn cmd_top(args: core::str::SplitWhitespace<'_>) -> ViResult<()> {
    let words: Vec<&str> = args.collect();
    let options = match parse_options(&words) {
        Ok(options) => options,
        Err(usage) => {
            crate::executor::shell_println(usage);
            return Ok(());
        }
    };
    run_top(options)
}

fn run_top(options: TopOptions) -> ViResult<()> {
    let mut remaining = options
        .count
        .unwrap_or(if options.batch { 1 } else { usize::MAX });
    // The first sample only seeds the deltas — CPU% needs two snapshots.
    let mut previous = sample_processes()?;
    while remaining > 0 {
        if !wait_for_next_sample(options.delay_ticks, options.batch) {
            break;
        }
        let current = sample_processes()?;
        let rows = build_rows(&previous, &current, options.show_all, options.sort);
        render::render_frame(&rows, options.sort, options.batch, remaining);
        previous = current;
        remaining = remaining.saturating_sub(1);
    }
    if !options.batch {
        ostd::io::print("\x1b[2J\x1b[1;1H");
    }
    Ok(())
}

fn sample_processes() -> ViResult<Vec<ProcessInfoV2>> {
    let mut buffer = [ProcessInfoV2::default(); MAX_ROWS];
    syscall::sys_get_procs2(&mut buffer)
        .map(|count| buffer[..count.min(MAX_ROWS)].to_vec())
        .map_err(|_| ViError::PermissionDenied)
}

/// Block until the sample deadline; returns `false` when interactive mode is
/// quit with `q`/`Q` (batch mode never reads stdin, so it always returns true).
fn wait_for_next_sample(delay_ticks: u64, batch: bool) -> bool {
    let deadline = syscall::sys_get_time().saturating_add(delay_ticks);
    loop {
        if !batch {
            let mut byte = [0u8; 1];
            if let Ok(read) = ostd::syscall::sys_read(0, &mut byte) {
                if read > 0 && matches!(byte[0], b'q' | b'Q') {
                    return false;
                }
            }
        }
        if syscall::sys_get_time() >= deadline {
            return true;
        }
        ostd::task::yield_now();
    }
}
