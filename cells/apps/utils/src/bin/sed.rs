#![no_std]
#![no_main]
extern crate ostd;
extern crate alloc;

use alloc::{string::String, vec::Vec};
use ostd::syscall;

/// sed — minimal stream editor: supports `s/pattern/replacement/` substitution.
///
/// In v1.0 the pattern is treated as a literal string (no regex).
/// Full regex support is deferred to Phase 17b when regex-lite is integrated.
///
/// Since args cannot be passed yet (Phase 17a pipes), the expression is
/// read from the first stdin line prefixed with `s/`, e.g.:
///   echo 's/foo/bar/' | sed
///
/// TODO (Phase 17a): receive expression via arg IPC.
#[no_mangle]
pub fn main() {
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        match syscall::sys_read(0, &mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    let text = core::str::from_utf8(&data).unwrap_or("");

    // Extract first line as the sed expression; remaining lines are input.
    let mut lines = text.lines();
    let expr = match lines.next() { Some(e) => e, None => { syscall::sys_exit(0); } };
    let rest: Vec<&str> = lines.collect();

    // Parse s/pat/rep/ (literal, single substitution per line).
    if let Some(body) = expr.strip_prefix("s/") {
        let mut parts = body.splitn(3, '/');
        let pat = parts.next().unwrap_or("");
        let rep = parts.next().unwrap_or("");
        for line in rest {
            let replaced = if pat.is_empty() {
                String::from(line)
            } else {
                // Replace first occurrence only (POSIX default).
                match line.find(pat) {
                    Some(pos) => {
                        let mut out = String::from(&line[..pos]);
                        out.push_str(rep);
                        out.push_str(&line[pos + pat.len()..]);
                        out
                    }
                    None => String::from(line),
                }
            };
            ostd::io::println(&replaced);
        }
    } else {
        // Unknown expression — pass through unchanged.
        for line in rest { ostd::io::println(line); }
    }
    syscall::sys_exit(0);
}
