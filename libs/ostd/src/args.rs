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

const STRUCTURED_ARGV_PREFIX: &str = "\0argv1\0";
pub const MAX_SPAWN_ARGV_BYTES: usize = 512;

/// Publish structured arguments for the next spawned cell.
///
/// Each argument is preserved byte-for-byte, including spaces and empty
/// strings. Returns `false` without publishing when an argument contains NUL,
/// the encoded payload exceeds [`MAX_SPAWN_ARGV_BYTES`], or allocation fails.
/// The caller must invoke this immediately before spawning the target cell.
pub fn set_spawn_argv(argv: &[String]) -> bool {
    let mut required = STRUCTURED_ARGV_PREFIX.len();
    for arg in argv {
        if arg.as_bytes().contains(&0) {
            return false;
        }
        required = match required.checked_add(arg.len() + 1) {
            Some(required) if required <= MAX_SPAWN_ARGV_BYTES => required,
            _ => return false,
        };
    }
    let mut encoded = String::new();
    if encoded.try_reserve_exact(required).is_err() {
        return false;
    }
    encoded.push_str(STRUCTURED_ARGV_PREFIX);
    for arg in argv {
        encoded.push_str(arg);
        encoded.push('\0');
    }
    crate::syscall::sys_set_spawn_args(&encoded)
}

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
    let mut buf = [0u8; MAX_SPAWN_ARGV_BYTES];
    let n = crate::syscall::sys_spawn_args(&mut buf);
    if n == 0 {
        return Vec::new();
    }
    decode_spawn_args(&buf[..n])
}

fn decode_spawn_args(bytes: &[u8]) -> Vec<String> {
    if let Some(payload) = bytes.strip_prefix(STRUCTURED_ARGV_PREFIX.as_bytes()) {
        if payload.is_empty() {
            return Vec::new();
        }
        let Some(body) = payload.strip_suffix(&[0]) else {
            return Vec::new();
        };
        let mut argv = Vec::new();
        for raw in body.split(|byte| *byte == 0) {
            let Ok(arg) = core::str::from_utf8(raw) else {
                return Vec::new();
            };
            argv.push(String::from(arg));
        }
        return argv;
    }
    let text = match core::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    text.split_ascii_whitespace().map(String::from).collect()
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

#[cfg(test)]
mod tests {
    use super::{decode_spawn_args, STRUCTURED_ARGV_PREFIX};

    #[test]
    fn structured_argv_preserves_spaces_and_empty_items() {
        let mut raw = alloc::string::String::from(STRUCTURED_ARGV_PREFIX);
        raw.push_str("two words");
        raw.push('\0');
        raw.push('\0');
        assert_eq!(decode_spawn_args(raw.as_bytes()), ["two words", ""]);
    }

    #[test]
    fn legacy_argv_remains_whitespace_separated() {
        assert_eq!(decode_spawn_args(b"one two"), ["one", "two"]);
    }
}
