use ostd::executor::yield_now;
use ostd::prelude::*;

pub struct AsyncStdin;

impl AsyncStdin {
    /// Read a line from stdin into an owned Vec<u8>.
    ///
    /// Returns the bytes entered (excluding the newline). Passing ownership
    /// instead of `&mut [u8]` satisfies Law 2: no borrowed slice across `.await`.
    pub async fn read_line(
        &self,
        max_len: usize,
        history: &mut alloc::collections::VecDeque<alloc::string::String>,
    ) -> Vec<u8> {
        let mut buffer: Vec<u8> = Vec::with_capacity(max_len);
        let mut history_idx = history.len();
        let mut escape_state: u8 = 0; // 0=Normal 1=Esc 2=Bracket

        loop {
            if buffer.len() >= max_len {
                break;
            }
            let mut c = [0u8; 1];
            match ostd::syscall::sys_read(0, &mut c) {
                Ok(n) if n > 0 => {
                    let ch = c[0];

                    // ANSI escape sequence state machine
                    if escape_state == 0 {
                        if ch == 0x1B {
                            escape_state = 1;
                            continue;
                        }
                    } else if escape_state == 1 {
                        if ch == b'[' {
                            escape_state = 2;
                        } else {
                            escape_state = 0;
                        }
                        continue;
                    } else if escape_state == 2 {
                        escape_state = 0;
                        match ch {
                            b'A' => {
                                // Up arrow — load previous history entry
                                if history_idx > 0 {
                                    history_idx -= 1;
                                    Self::clear_line(&buffer);
                                    buffer.clear();
                                    if let Some(cmd) = history.get(history_idx) {
                                        ostd::io::print(cmd);
                                        buffer.extend_from_slice(cmd.as_bytes());
                                    }
                                }
                            }
                            b'B' => {
                                // Down arrow — load next history entry or blank
                                if history_idx < history.len() {
                                    history_idx += 1;
                                    Self::clear_line(&buffer);
                                    buffer.clear();
                                    if let Some(cmd) = history.get(history_idx) {
                                        ostd::io::print(cmd);
                                        buffer.extend_from_slice(cmd.as_bytes());
                                    }
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Normal character processing
                    if ch == b'\r' || ch == b'\n' {
                        ostd::io::print("\n");
                        break;
                    }
                    if ch == 8 || ch == 127 {
                        // Backspace
                        if !buffer.is_empty() {
                            ostd::io::print("\x08 \x08");
                            buffer.pop();
                        }
                        continue;
                    }
                    // Echo and append
                    if let Ok(s) = core::str::from_utf8(&c) {
                        ostd::io::print(s);
                    }
                    buffer.push(ch);
                }
                _ => {
                    yield_now().await;
                }
            }
        }
        buffer
    }

    fn clear_line(current: &[u8]) {
        for _ in 0..current.len() {
            ostd::io::print("\x08 \x08");
        }
    }
}
