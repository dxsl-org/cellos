// SPDX-License-Identifier: MPL-2.0

//! Command-line argument helpers for Cell applications.
//!
//! The shell (or any spawner) calls [`sys_set_spawn_args`][crate::syscall::sys_set_spawn_args]
//! before spawning a cell; the kernel moves the bytes into a per-task private
//! slot so back-to-back spawns cannot race.  [`args()`] reads and parses that
//! slot.  The slot is consumed on first read — subsequent calls return an empty
//! `Vec`.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Return the command-line arguments passed to this cell by its spawner.
///
/// Arguments are space-separated UTF-8 tokens set by the spawner via
/// [`sys_set_spawn_args`][crate::syscall::sys_set_spawn_args].  Returns an empty
/// `Vec` when no args were set or after the stash has already been consumed.
///
/// The returned `Vec` contains only the arguments — no `argv[0]` program-name
/// entry is prepended.
///
/// # Example
/// ```no_run
/// let args = ostd::args();
/// match args.as_slice() {
///     [] => ostd::io::println("no args"),
///     [path, rest @ ..] => { /* process path … */ }
/// }
/// ```
pub fn args() -> Vec<String> {
    let mut buf = [0u8; 512];
    let n = crate::syscall::sys_spawn_args(&mut buf);
    if n == 0 {
        return Vec::new();
    }
    let s = match core::str::from_utf8(&buf[..n]) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    s.split_ascii_whitespace().map(String::from).collect()
}

/// Print `usage` and exit if the cell was spawned with `-h` or `--help`.
///
/// Reads the spawn-args stash once.  Call at the start of [`AppEvent::Init`] or
/// via [`CellRuntime::help`] to get automatic `--help` handling with zero
/// boilerplate in the event handler.
pub fn check_help(usage: &str) {
    let argv = args();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        crate::io::println(usage);
        crate::syscall::sys_exit(0);
    }
}
