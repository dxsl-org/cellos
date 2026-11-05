//! The built-in remote shell: a bounded line editor and canned commands.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Longest command line kept; further printable bytes are dropped (not echoed)
/// so a newline-less stream cannot grow the cell heap.
pub const MAX_LINE: usize = 256;
pub const PROMPT: &[u8] = b"Cellos > ";
pub const BANNER: &[u8] = b"\r\n\x1b[1;36m=== Cellos Remote SSH Server v0.2.1 (x86_64) ===\x1b[0m\r\nType 'help' for built-in commands.\r\n\r\nCellos > ";

/// Feed terminal input into `line`; return everything to echo back (characters,
/// command output and prompts) as one stream, so one inbound packet yields one
/// outbound burst rather than a packet per byte.
pub fn feed(line: &mut Vec<u8>, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in data {
        match b {
            b'\r' | b'\n' => {
                out.extend_from_slice(b"\r\n");
                let cmd = core::str::from_utf8(line).unwrap_or("");
                out.extend_from_slice(execute(cmd.trim()).as_bytes());
                out.extend_from_slice(PROMPT);
                line.clear();
            }
            0x7F | 0x08 if line.pop().is_some() => out.extend_from_slice(b"\x08 \x08"),
            0x20..=0x7E if line.len() < MAX_LINE => {
                line.push(b);
                out.push(b);
            }
            _ => {}
        }
    }
    out
}

pub fn execute(cmd: &str) -> String {
    let Some(name) = cmd.split_whitespace().next() else {
        return String::new();
    };
    match name {
        "help" => String::from(
            "Cellos Shell v0.2.1 (SSH Remote Session) — built-in commands:\r\n  System:  uname  ps  uptime  free  clear  help  exit\r\n  Files:   ls /bin\r\n",
        ),
        "uname" => String::from("Cellos cellos-kernel 0.2.1 x86_64\r\n"),
        "uptime" => {
            let ticks = ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0);
            format!("uptime: {} seconds ({} ticks)\r\n", ticks / 100, ticks)
        }
        "clear" => String::from("\x1b[2J\x1b[H"),
        "ps" => {
            let mut rows = [api::syscall::ProcessInfo::default(); 32];
            let mut out =
                String::from("  PID  STATE      NAME\r\n  ---  ---------  ----------------\r\n");
            if let Ok(count) = ostd::syscall::sys_get_procs(&mut rows) {
                for row in rows.iter().take(count) {
                    let state = match row.state {
                        1 => "Ready",
                        2 => "Waiting",
                        3 => "Exited",
                        4 => "Running",
                        _ => "Unknown",
                    };
                    let name = core::str::from_utf8(&row.name)
                        .unwrap_or("unknown")
                        .trim_matches('\0');
                    out.push_str(&format!("  {:<4} {:<10} {}\r\n", row.id, state, name));
                }
            }
            out
        }
        "ls" => String::from(
            "/bin:\r\n  ahci       cat        compositor config     e1000      echo\r\n  fb-console igb        input      kill       ls         net\r\n  nvme       platform   ps         serial     shell      sshd\r\n  xhci\r\n",
        ),
        "free" => String::from("Heap: 4096 KB total, single-address-space isolation\r\n"),
        "exit" => String::from("Connection closed by foreign host.\r\n"),
        other => format!("shell: command not found: {}\r\n", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_is_capped_and_overflow_is_not_echoed() {
        let mut line = Vec::new();
        let echo = feed(&mut line, &[b'a'; MAX_LINE + 50]);
        assert_eq!(line.len(), MAX_LINE);
        assert_eq!(echo.len(), MAX_LINE);
    }
}
