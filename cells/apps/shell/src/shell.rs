use ostd::prelude::*;
use crate::commands;

pub struct ViShell<'a> {
    prompt: &'a str,
}

impl<'a> ViShell<'a> {
    pub fn new() -> Self {
        Self { prompt: "ViOS > " }
    }

    pub fn run(&self) {
        loop {
            // Print prompt
            ostd::io::print(self.prompt);

            // Read input
            let mut buffer = [0u8; 128];
            let len = self.read_line(&mut buffer);

            if len > 0 {
                if let Ok(line) = core::str::from_utf8(&buffer[..len]) {
                     let _ = self.dispatch(line);
                }
            }
        }
    }

    fn read_line(&self, buffer: &mut [u8]) -> usize {
        let mut idx = 0;
        loop {
            if idx >= buffer.len() {
                break;
            }
            let mut c = [0u8; 1];
            // sys_read(0) is stdin
            if let Ok(n) = ostd::syscall::sys_read(0, &mut c) {
                if n > 0 {
                     let ch = c[0];
                     if ch == b'\r' || ch == b'\n' {
                         ostd::io::print("\n");
                         break;
                     }
                     // Backspace
                     if ch == 8 || ch == 127 {
                         if idx > 0 {
                             ostd::io::print("\x08 \x08");
                             idx -= 1;
                         }
                         continue;
                     }

                     // Echo
                     if let Ok(s) = core::str::from_utf8(&c) {
                         ostd::io::print(s);
                     }

                     buffer[idx] = ch;
                     idx += 1;
                } else {
                    // Non-blocking read? usually we yield.
                    // If sys_read is blocking in kernel, we are good.
                    // If not, we should yield.
                    ostd::syscall::sys_yield();
                }
            } else {
                ostd::syscall::sys_yield();
            }
        }
        idx
    }

    pub fn dispatch(&self, line: &str) -> ViResult<()> {
        let mut parts = line.trim().split_whitespace();
        let cmd = parts.next().ok_or(ViError::InvalidInput)?;

        match cmd {
            "ls" => commands::cmd_ls(parts),
            "cat" => commands::cmd_cat(parts),
            "help" => commands::cmd_help(),
            "clear" => commands::cmd_clear(),
            _ => {
                ostd::io::print("ViOS: command not found: ");
                ostd::io::println(cmd);
                Ok(())
            }
        }
    }
}
