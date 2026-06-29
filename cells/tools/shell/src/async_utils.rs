use ostd::executor::yield_now;
use ostd::input::{InputEvent, KeyState, KeySym};
use ostd::prelude::*;
use ostd::syscall::{sys_lookup_service, sys_recv_timeout, SyscallResult};
use api::input::{INPUT_EVENT_OPCODE, decode_event};
use api::syscall::service;

pub struct AsyncStdin;

impl AsyncStdin {
    /// Read a line from stdin into an owned Vec<u8>.
    ///
    /// Sources in priority order:
    ///   1. VirtIO keyboard + UART via input service (EV_ASCII relay), received
    ///      via `sys_recv_timeout` (1-tick block) so shell enters `TaskState::Recv`
    ///      and input service's `sys_try_send` can deliver events.
    ///   2. UART/serial via sys_read(fd=0) — fallback when input service absent
    ///      or when the 1-tick IPC window times out with no event.
    ///
    /// Returns the bytes entered (excluding the newline). Ownership satisfies
    /// Law 2: no borrowed slice across `.await`.
    pub async fn read_line(
        &self,
        max_len: usize,
        history: &mut alloc::collections::VecDeque<alloc::string::String>,
    ) -> Vec<u8> {
        let mut buffer: Vec<u8> = Vec::with_capacity(max_len);
        let mut history_idx = history.len();
        let mut escape_state: u8 = 0; // ANSI state machine — UART path only

        'read: loop {
            if buffer.len() >= max_len {
                break;
            }

            // ── Input service path ──────────────────────────────────────────
            // sys_recv_timeout puts shell into TaskState::Recv for 1 tick
            // (~10ms), allowing input service's sys_try_send to deliver events.
            // poll_events (sys_try_recv) never places shell in Recv — input
            // service's sys_try_send always drops when shell is not in Recv
            // state (G18 fix side-effect resolved here).
            if let Some(input_tid) = sys_lookup_service(service::INPUT) {
                let mut frame = [0u8; 65];
                match sys_recv_timeout(0, &mut frame, 100) {
                    SyscallResult::Ok(sender) if sender == input_tid => {
                        if frame[0] == INPUT_EVENT_OPCODE {
                            if let Some(ev) = decode_event(&frame[1..]) {
                                let InputEvent::Key(k) = ev else { continue 'read; };
                                if !matches!(k.state, KeyState::Pressed | KeyState::Repeated) {
                                    continue 'read;
                                }
                                match k.keysym {
                                    KeySym::Return => {
                                        ostd::io::print("\n");
                                        break 'read;
                                    }
                                    KeySym::Backspace => {
                                        if !buffer.is_empty() {
                                            ostd::io::print("\x08 \x08");
                                            buffer.pop();
                                        }
                                    }
                                    KeySym::Tab => {
                                        Self::handle_tab(&mut buffer);
                                    }
                                    KeySym::Up => {
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
                                    KeySym::Down => {
                                        if history_idx < history.len() {
                                            history_idx += 1;
                                            Self::clear_line(&buffer);
                                            buffer.clear();
                                            if history_idx < history.len() {
                                                if let Some(cmd) = history.get(history_idx) {
                                                    ostd::io::print(cmd);
                                                    buffer.extend_from_slice(cmd.as_bytes());
                                                }
                                            }
                                        }
                                    }
                                    _ => {
                                        if let Some(ch) = k.char() {
                                            let cp = ch as u32;
                                            if cp >= 0x20 && cp <= 0x7E && buffer.len() < max_len {
                                                let byte = ch as u8;
                                                if let Ok(s) = core::str::from_utf8(core::slice::from_ref(&byte)) {
                                                    ostd::io::print(s);
                                                }
                                                buffer.push(byte);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // IPC message received (even if unparseable) — re-check
                        // input service for the next char before falling to UART.
                        continue 'read;
                    }
                    SyscallResult::Ok(0) => {
                        // timeout — no IPC event; fall through to UART.
                    }
                    SyscallResult::Ok(_other) => {
                        // Unexpected IPC sender — discard and fall through to UART.
                    }
                    SyscallResult::Err(_) => {}
                }
            }

            // ── UART / serial fallback ────────────────────────────────────
            // Handles the early-boot case (input service not yet registered)
            // and characters that arrived in the UART ring buffer directly.
            // When input service is online, the kernel routes UART bytes to
            // input service via EV_ASCII IPC (not the ring buffer), so this
            // path is mostly idle when the IPC path above is active.
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
                        escape_state = if ch == b'[' { 2 } else { 0 };
                        continue;
                    } else if escape_state == 2 {
                        escape_state = 0;
                        match ch {
                            b'A' => {
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

                    if ch == 0x09 {
                        Self::handle_tab(&mut buffer);
                        continue;
                    }
                    if ch == b'\r' || ch == b'\n' {
                        ostd::io::print("\n");
                        break;
                    }
                    if ch == 8 || ch == 127 {
                        if !buffer.is_empty() {
                            ostd::io::print("\x08 \x08");
                            buffer.pop();
                        }
                        continue;
                    }
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

    /// TAB completion: complete the last token against built-in command names.
    fn handle_tab(buffer: &mut Vec<u8>) {
        let line = core::str::from_utf8(buffer).unwrap_or("");
        let token = line.split_whitespace().last().unwrap_or("");
        let token_bytes = token.len();

        let matches: alloc::vec::Vec<&str> = crate::executor::BUILTINS
            .iter()
            .filter(|b| b.starts_with(token))
            .copied()
            .collect();

        match matches.len() {
            0 => {}
            1 => {
                for _ in 0..token_bytes {
                    ostd::io::print("\x08 \x08");
                    buffer.pop();
                }
                let completed = matches[0];
                ostd::io::print(completed);
                ostd::io::print(" ");
                buffer.extend_from_slice(completed.as_bytes());
                buffer.push(b' ');
            }
            _ => {
                ostd::io::print("\n");
                for (i, m) in matches.iter().enumerate() {
                    if i > 0 {
                        ostd::io::print("  ");
                    }
                    ostd::io::print(m);
                }
                ostd::io::print("\n");
                if let Ok(s) = core::str::from_utf8(buffer) {
                    ostd::io::print(s);
                }
            }
        }
    }

    fn clear_line(current: &[u8]) {
        for _ in 0..current.len() {
            ostd::io::print("\x08 \x08");
        }
    }
}
