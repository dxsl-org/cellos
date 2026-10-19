// SPDX-License-Identifier: MIT
//! Terminal session execution and exclusive reader lease management.

extern crate alloc;
use alloc::format;
use alloc::string::String;

use crate::grid::TerminalGrid;

pub struct TerminalSession {
    pub session_id: u32,
    pub title: String,
    pub grid: TerminalGrid,
    pub has_reader_lease: bool,
    pub input_line: String,
    pub should_close: bool,
}

impl TerminalSession {
    pub fn new(session_id: u32, cols: usize, rows: usize) -> Self {
        let mut grid = TerminalGrid::new(cols, rows);
        grid.write_str("=== CellOS GUI Terminal (x86_64) ===\nType 'help' for commands. Single-reader stream lease active.\n\nCellos > ");

        Self {
            session_id,
            title: format!("Terminal #{}", session_id),
            grid,
            has_reader_lease: true,
            input_line: String::new(),
            should_close: false,
        }
    }

    pub fn send_interrupt(&mut self) {
        self.grid.write_str("^C\nCellos > ");
        self.input_line.clear();
    }

    pub fn feed_char(&mut self, ch: char) {
        if !self.has_reader_lease {
            return;
        }

        match ch {
            '\r' | '\n' => {
                self.grid.write_char('\n');
                let cmd = core::mem::take(&mut self.input_line);
                let trimmed = cmd.trim();
                if !trimmed.is_empty() {
                    self.execute_command(trimmed);
                }
                if !self.should_close {
                    self.grid.write_str("Cellos > ");
                }
            }
            '\x08' => {
                // Backspace
                if !self.input_line.is_empty() {
                    self.input_line.pop();
                    self.grid.write_char('\x08');
                }
            }
            '\x03' => {
                // Ctrl+C
                self.send_interrupt();
            }
            _ => {
                if !ch.is_control() {
                    self.input_line.push(ch);
                    self.grid.write_char(ch);
                }
            }
        }
    }

    pub fn execute_command(&mut self, cmd: &str) {
        let parts: alloc::vec::Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }

        match parts[0] {
            "help" => {
                self.grid.write_str("Available commands:\n");
                self.grid.write_str("  help    - Show this message\n");
                self.grid.write_str("  clear   - Clear terminal screen\n");
                self.grid.write_str("  uname   - Print kernel name and version\n");
                self.grid.write_str("  pwd     - Print current working directory\n");
                self.grid.write_str("  echo    - Print arguments\n");
                self.grid.write_str("  exit    - Close terminal session\n");
            }
            "clear" => {
                self.grid.clear();
            }
            "uname" => {
                self.grid.write_str("CellOS 0.2.1 x86_64 Intel VT-x C2C\n");
            }
            "pwd" => {
                self.grid.write_str("/data\n");
            }
            "echo" => {
                if parts.len() > 1 {
                    for (i, p) in parts[1..].iter().enumerate() {
                        if i > 0 {
                            self.grid.write_char(' ');
                        }
                        self.grid.write_str(p);
                    }
                }
                self.grid.write_char('\n');
            }
            "exit" => {
                self.grid.write_str("[terminal] Session terminated.\n");
                self.should_close = true;
            }
            other => {
                let msg = format!("shell: command not found: {}\n", other);
                self.grid.write_str(&msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_session_command_execution() {
        let mut session = TerminalSession::new(1, 80, 24);
        assert!(session.has_reader_lease);
        assert!(!session.should_close);

        // Execute echo
        for ch in "echo hello\n".chars() {
            session.feed_char(ch);
        }
        assert!(session.input_line.is_empty());

        // Interrupt Ctrl+C
        session.feed_char('a');
        session.feed_char('b');
        assert_eq!(session.input_line, "ab");
        session.send_interrupt();
        assert!(session.input_line.is_empty());

        // Exit
        for ch in "exit\n".chars() {
            session.feed_char(ch);
        }
        assert!(session.should_close);
    }
}
