// SPDX-License-Identifier: MIT
//! GUI Terminal session: a `terminal_sdk::FrontendHost` attached to the real
//! `/bin/cellbox sh`, with its output bytes folded into the monospace grid.
//!
//! The shell is the controller of its own session (Phase 05 S5-3 annex option C); this
//! Cell only holds the frontend capability the input service issued. Nothing here
//! interprets commands or fabricates responses: keyboard input goes to the session as
//! normalized events through the frontend attachment, and everything on screen is what
//! the shell relayed (its own output and its children's) to this Cell's output rings.
//!
//! Window close inside the live Cell releases the frontend (the shell keeps a Detached
//! session); [`TerminalSession::reattach`] resumes the same shell on a new frontend.

extern crate alloc;
use alloc::format;
use ostd::input::KeyEvent;
use terminal_sdk::{ChildPoll, FrontendHost, FrontendState, Status};

use crate::grid::TerminalGrid;
use crate::keymap::{idle_action, map_key, HostInput, IdleAction};
use crate::sink::ByteSink;

/// Program the Terminal runs. Argv mirrors init's advisory `["sh"]`.
pub const SHELL_PATH: &str = "/bin/cellbox";
const SHELL_ARGV: [&str; 1] = ["sh"];
// No cwd grant is ever sent: a cwd grant would seal the shell, and VFS refuses every
// path-addressed request of a sealed cell (all external commands and file paths would be
// Denied). The shell defaults its own cwd to `/`.
// Merged-streams tty mode: with this entry in its own launch context the shell writes its
// diagnostics to the stdout ring and relays everything to this Cell's stdout writer, so
// one writer fixes the order of everything on screen. The key/value literals are the
// shell's contract (`cells/tools/cellbox`). The list is the sorted, `ChildContext`-valid
// environment `FrontendHost::launch` expects.
const SHELL_ENV: [(&str, &str); 1] = [("CELLBOX_STREAMS", "merged")];

/// Session limits the input service enforces (`validate_dimensions`).
pub const MAX_COLS: usize = 512;
pub const MAX_ROWS: usize = 256;
pub const MIN_COLS: usize = 40;
pub const MIN_ROWS: usize = 12;
/// Read frames drained per stream per pump; bounds one frame's work.
const DRAIN_FRAMES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// Nothing started yet.
    Idle,
    Running,
    /// The shell exited; Enter restarts.
    Exited,
    /// A session/launch/stream error; Enter retries.
    Failed(Status),
}

pub struct TerminalSession {
    host: Option<FrontendHost>,
    pub grid: TerminalGrid,
    sink: ByteSink,
    pub state: RunState,
    pub should_close: bool,
}

impl TerminalSession {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            host: None,
            grid: TerminalGrid::new(cols, rows),
            sink: ByteSink::new(),
            state: RunState::Idle,
            should_close: false,
        }
    }

    /// Register a fresh frontend and launch the shell bound to it (first start and
    /// restart). The shell attaches the frontend when it has created its session;
    /// [`Self::pump`] notices that without blocking the window.
    pub fn start(&mut self) {
        self.stop_host();
        self.reset_screen();
        self.should_close = false;
        // The frontend registers the grid's own dimensions (`crate::grid_dims`).
        let (rows, cols) = (self.grid.rows as u16, self.grid.cols as u16);
        match FrontendHost::open(rows, cols) {
            Ok(mut host) => match host.launch(SHELL_PATH, &SHELL_ARGV, &SHELL_ENV) {
                Ok(()) => {
                    self.host = Some(host);
                    self.state = RunState::Running;
                }
                Err(status) => {
                    // Dropping the host releases the half-started frontend and operation.
                    drop(host);
                    self.fail("cannot start /bin/cellbox", status);
                }
            },
            Err(status) => self.fail("cannot open terminal frontend", status),
        }
    }

    /// True while a shell runs behind this frontend (attached or not yet).
    pub fn running(&self) -> bool {
        self.state == RunState::Running && self.host.is_some()
    }

    /// The window is closing inside this live Cell: release the frontend. The shell keeps
    /// its session (Detached) and its output rings stay drained into the grid.
    pub fn detach(&mut self) -> Result<(), Status> {
        let host = self.host.as_mut().ok_or(Status::NotFound)?;
        if host.state() != FrontendState::Attached {
            host.poll_frontend()?;
        }
        host.release()
    }

    /// A new window of `cols`×`rows` for this Cell: resume the same shell on a fresh
    /// frontend. The shell redraws its prompt line; the grid keeps the transcript.
    pub fn reattach(&mut self, cols: usize, rows: usize) -> Result<(), Status> {
        if (cols, rows) != (self.grid.cols, self.grid.rows) && self.grid.resize(cols, rows).is_err() {
            return Err(Status::Malformed);
        }
        let host = self.host.as_mut().ok_or(Status::NotFound)?;
        host.reattach(rows as u16, cols as u16)
    }

    /// Handle one compositor key event; returns whether the grid changed.
    pub fn on_key(&mut self, ke: &KeyEvent) -> bool {
        if self.state == RunState::Running {
            return match map_key(ke) {
                Some(input) => self.submit(input),
                None => false,
            };
        }
        match idle_action(ke) {
            IdleAction::Restart => {
                self.start();
                true
            }
            IdleAction::Close => {
                self.should_close = true;
                false
            }
            IdleAction::None => false,
        }
    }

    fn submit(&mut self, input: HostInput) -> bool {
        let Some(host) = self.host.as_mut() else { return false };
        let result = match input {
            HostInput::Text(ch) => {
                let mut buf = [0u8; 4];
                host.submit_text(ch.encode_utf8(&mut buf))
            }
            HostInput::Key { code, modifiers, pressed } => host.submit_key(code, modifiers, pressed),
        };
        match result {
            Ok(()) => false,
            // A full input queue or a combination the owner refuses drops this key only.
            Err(Status::QuotaExceeded | Status::WouldBlock | Status::UnsupportedInput) => false,
            Err(status) => {
                self.fail("input failed", status);
                true
            }
        }
    }

    /// Window resize: the grid first, then the session's viewport.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols == self.grid.cols && rows == self.grid.rows {
            return;
        }
        // A refused grid keeps the previous size; the session is then not resized either.
        if let Err(error) = self.grid.resize(cols, rows) {
            self.notice(&format!("[terminal: resize to {}x{} refused: {:?}]", cols, rows, error));
            return;
        }
        if let Some(host) = self.host.as_mut() {
            if let Err(status) = host.resize(rows as u16, cols as u16) {
                self.fail("resize failed", status);
            }
        }
    }

    /// Drain pending output into the grid, notice the shell attaching the frontend and
    /// notice its exit. Returns whether the grid changed (true = re-render).
    pub fn pump(&mut self) -> bool {
        let Some(host) = self.host.as_mut() else { return false };
        match drain(host, &mut self.sink, &mut self.grid) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(status) => {
                self.fail("output failed", status);
                return true;
            }
        }
        // Until the shell has created its session for this frontend, keys are dropped
        // (`WouldBlock`); one QueryFrontend per idle pump picks the attachment up.
        if matches!(host.state(), FrontendState::Pending | FrontendState::Bound) {
            if let Err(status) = host.poll_frontend() {
                self.fail("frontend state failed", status);
                return true;
            }
        }
        match host.poll_child() {
            Ok(ChildPoll::Running) => false,
            Ok(ChildPoll::Exited) => {
                // Output written just before the exit is still in the pipes.
                let tail = drain(host, &mut self.sink, &mut self.grid);
                self.stop_host();
                self.reset_screen();
                self.state = RunState::Exited;
                self.notice("[shell exited - Enter restarts, Esc closes]");
                if let Err(status) = tail {
                    self.notice(&format!("[terminal: final output drain failed: {:?}]", status));
                }
                true
            }
            Err(status) => {
                self.fail("child state failed", status);
                true
            }
        }
    }

    /// Release the session (cancels a live shell). Idempotent.
    pub fn shutdown(&mut self) {
        self.stop_host();
    }

    fn stop_host(&mut self) {
        if let Some(mut host) = self.host.take() {
            let _ = host.close();
        }
    }

    fn fail(&mut self, what: &str, status: Status) {
        self.stop_host();
        self.reset_screen();
        self.state = RunState::Failed(status);
        self.notice(&format!("[terminal: {}: {:?} - Enter retries, Esc closes]", what, status));
    }

    /// The session's program is gone (or a new one starts): whatever screen modes it left
    /// (alternate screen, hidden cursor, margins, rendition, half-parsed sequence) end too.
    fn reset_screen(&mut self) {
        self.sink.reset();
        self.grid.reset_modes();
    }

    /// One status line on its own row, outside the byte stream.
    fn notice(&mut self, text: &str) {
        if self.grid.cursor_x != 0 {
            self.grid.write_str("\n");
        }
        self.grid.write_str(text);
        self.grid.write_str("\n");
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.stop_host();
    }
}

/// One bounded read of the stdout (`false`) or stderr (`true`) stream.
trait Streams {
    fn read(&mut self, stderr: bool, out: &mut [u8]) -> Result<usize, Status>;
}

impl Streams for FrontendHost {
    fn read(&mut self, stderr: bool, out: &mut [u8]) -> Result<usize, Status> {
        if stderr { self.drain_stderr(out) } else { self.drain_stdout(out) }
    }
}

/// Read both streams (bounded) into the grid. `Ok(true)` if any byte arrived.
///
/// stdout is drained first, then stderr. The Terminal starts the shell in merged mode
/// (`SHELL_ENV`), so the shell writes every diagnostic to its stdout ring and relays its
/// own and its children's output to this Cell's stdout writer; the stderr ring stays
/// empty and the drain order between the two endpoints is irrelevant. Only a child's own
/// stdout/stderr interleaving is unordered: the shell relays its two rings in turns.
fn drain<S: Streams>(host: &mut S, sink: &mut ByteSink, grid: &mut TerminalGrid) -> Result<bool, Status> {
    let mut buf = [0u8; 512];
    let mut any = false;
    for stderr in [false, true] {
        for _ in 0..DRAIN_FRAMES {
            let n = host.read(stderr, &mut buf)?;
            if n == 0 {
                break;
            }
            sink.feed(grid, &buf[..n]);
            any = true;
        }
    }
    Ok(any)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use ostd::input::{KeyState, KeySym, Modifiers};
    use std::string::String;

    fn key(keysym: KeySym, state: KeyState) -> KeyEvent {
        KeyEvent { timestamp_ticks: 0, scancode: 0, keysym, character: 0, modifiers: Modifiers(0), state, _pad: [0; 2] }
    }

    fn row(grid: &TerminalGrid, r: usize) -> String {
        let text: String = (0..grid.cols).map(|c| grid.cell_at(c, r).unwrap().ch).collect();
        text.trim_end().into()
    }

    #[test]
    fn a_new_session_has_no_fabricated_screen_content() {
        let session = TerminalSession::new(80, 24);
        assert_eq!(session.state, RunState::Idle);
        assert!((0..24).all(|r| row(&session.grid, r).is_empty()));
    }

    #[test]
    fn faults_become_a_visible_notice_and_enter_retries_only_when_not_running() {
        let mut session = TerminalSession::new(80, 24);
        session.fail("input failed", Status::Revoked);
        assert_eq!(session.state, RunState::Failed(Status::Revoked));
        assert!(row(&session.grid, 0).contains("Revoked"));
        // Keys other than Enter/Esc are ignored while no shell runs.
        assert!(!session.on_key(&key(KeySym::Up, KeyState::Pressed)));
        assert!(!session.on_key(&key(KeySym::Return, KeyState::Released)));
        assert_eq!(session.state, RunState::Failed(Status::Revoked));
        assert!(!session.should_close);
        // Escape closes the application.
        assert!(!session.on_key(&key(KeySym::Escape, KeyState::Pressed)));
        assert!(session.should_close);
    }

    #[test]
    fn notices_start_on_their_own_row() {
        let mut session = TerminalSession::new(80, 24);
        session.grid.write_str("partial");
        session.notice("[x]");
        assert_eq!(row(&session.grid, 0), "partial");
        assert_eq!(row(&session.grid, 1), "[x]");
        assert_eq!(session.grid.cursor_y, 2);
    }

    #[test]
    fn a_failed_session_leaves_the_alternate_screen_before_its_notice() {
        let mut session = TerminalSession::new(80, 24);
        session.grid.write_str("Cellos > tui");
        session.sink.feed(&mut session.grid, b"\x1b[?1049h\x1b[?25l\x1b[31mframe\x1b[");
        session.fail("output failed", Status::Revoked);
        assert!(!session.grid.alternate_active() && session.grid.cursor_visible());
        assert!(session.sink.is_ground());
        assert_eq!(row(&session.grid, 0), "Cellos > tui");
        assert!(row(&session.grid, 1).contains("Revoked"));
    }

    #[test]
    fn resize_with_the_alternate_screen_up_keeps_the_primary_screen() {
        let mut session = TerminalSession::new(80, 24);
        session.sink.feed(&mut session.grid, b"Cellos > top\r\n\x1b[?1049h\x1b[HTUI");
        session.resize(100, 30);
        assert_eq!(row(&session.grid, 0), "TUI");
        session.sink.feed(&mut session.grid, b"\x1b[?1049l");
        assert_eq!((session.grid.cols, session.grid.rows), (100, 30));
        assert_eq!(row(&session.grid, 0), "Cellos > top");
        assert_eq!((session.grid.cursor_x, session.grid.cursor_y), (0, 1));
    }

    #[test]
    fn resize_without_a_session_only_resizes_the_grid() {
        let mut session = TerminalSession::new(80, 24);
        session.resize(100, 30);
        assert_eq!((session.grid.cols, session.grid.rows), (100, 30));
        session.resize(100, 30);
        assert_eq!(session.state, RunState::Idle);
    }

    #[test]
    fn window_close_without_a_shell_has_nothing_to_release_or_resume() {
        let mut session = TerminalSession::new(80, 24);
        assert!(!session.running());
        assert_eq!(session.detach(), Err(Status::NotFound));
        assert_eq!(session.reattach(80, 24), Err(Status::NotFound));
        // A refused re-attach never fabricates a running shell.
        assert_eq!(session.state, RunState::Idle);
    }

    #[test]
    fn pump_and_input_without_a_host_do_nothing() {
        let mut session = TerminalSession::new(80, 24);
        assert!(!session.pump());
        assert!(!session.submit(HostInput::Text('a')));
    }

    #[test]
    fn the_shell_launch_environment_is_exactly_the_merged_streams_entry_and_is_a_valid_context() {
        use api::cell_session::ObjectRefV1;
        use ostd::child_context::ChildContext;
        assert_eq!(SHELL_ENV, [("CELLBOX_STREAMS", "merged")]);
        // The exact list the launch passes survives the context encoder's validation
        // (sorted unique keys, no NUL/`=`) and reads back unchanged.
        let context = ChildContext::new(ObjectRefV1 { id: 1, generation: 1 }, &SHELL_ARGV, b"", &SHELL_ENV, &[])
            .expect("launch environment must be a valid child context");
        assert_eq!(context.environment().collect::<alloc::vec::Vec<_>>(), [("CELLBOX_STREAMS", "merged")]);
    }
}

#[cfg(test)]
mod drain_tests {
    extern crate std;
    use super::*;
    use std::collections::VecDeque;
    use std::string::String;

    struct Scripted {
        out: VecDeque<&'static [u8]>,
        err: VecDeque<&'static [u8]>,
    }

    impl Streams for Scripted {
        fn read(&mut self, stderr: bool, out: &mut [u8]) -> Result<usize, Status> {
            let queue = if stderr { &mut self.err } else { &mut self.out };
            match queue.pop_front() {
                Some(chunk) => {
                    out[..chunk.len()].copy_from_slice(chunk);
                    Ok(chunk.len())
                }
                None => Ok(0),
            }
        }
    }

    fn row(grid: &TerminalGrid, r: usize) -> String {
        let text: String = (0..grid.cols).map(|c| grid.cell_at(c, r).unwrap().ch).collect();
        text.trim_end().into()
    }

    #[test]
    fn both_streams_are_drained_stdout_first_into_the_grid() {
        let mut host = Scripted {
            out: VecDeque::from([b"Cellos > ".as_slice()]),
            err: VecDeque::from([b"ls: /x: no such directory\n".as_slice()]),
        };
        let (mut sink, mut grid) = (ByteSink::new(), TerminalGrid::new(80, 24));
        assert_eq!(drain(&mut host, &mut sink, &mut grid), Ok(true));
        assert!(host.out.is_empty() && host.err.is_empty());
        // Merged mode keeps shell output on stdout; a child's stderr still follows it.
        assert_eq!(row(&grid, 0), "Cellos > ls: /x: no such directory");
    }

    #[test]
    fn merged_shell_output_keeps_program_order_on_the_single_stdout_stream() {
        let mut host = Scripted {
            out: VecDeque::from([b"Cellos > ls /x\r\n".as_slice(), b"ls: /x: no such directory\n".as_slice(), b"Cellos > ".as_slice()]),
            err: VecDeque::new(),
        };
        let (mut sink, mut grid) = (ByteSink::new(), TerminalGrid::new(80, 24));
        assert_eq!(drain(&mut host, &mut sink, &mut grid), Ok(true));
        assert_eq!(row(&grid, 0), "Cellos > ls /x");
        assert_eq!(row(&grid, 1), "ls: /x: no such directory");
        assert_eq!(row(&grid, 2), "Cellos >");
    }

    #[test]
    fn drain_error_is_propagated_and_empty_streams_report_nothing() {
        let mut idle = Scripted { out: VecDeque::new(), err: VecDeque::new() };
        let (mut sink, mut grid) = (ByteSink::new(), TerminalGrid::new(80, 24));
        assert_eq!(drain(&mut idle, &mut sink, &mut grid), Ok(false));
        struct Dead;
        impl Streams for Dead {
            fn read(&mut self, _: bool, _: &mut [u8]) -> Result<usize, Status> { Err(Status::Revoked) }
        }
        assert_eq!(drain(&mut Dead, &mut sink, &mut grid), Err(Status::Revoked));
    }
}
