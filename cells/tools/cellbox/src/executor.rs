//! Shell AST executor — runs parsed commands, handles pipes and redirects.
//!
//! Byte pipelines between built-in commands are implemented via an in-memory
//! capture stack (`shell_state`): each stage's output is captured into a
//! `Vec<u8>`, then passed as stdin to the next stage. `CaptureGuard` (RAII,
//! Law 8) pops the capture on every exit path, including early returns.
//!
//! Typed pipelines (`ps --records | where … | take N | to jsonl`) never touch
//! that stack: `typed::admit` statically admits the WHOLE pipeline (stages,
//! redirects, background, stateful built-ins) before any stage runs or any file
//! is opened, and `typed::run_rows` pulls rows through a bounded chain.
//!
//! While a terminal session owns the shell, spawned cells join typed and byte
//! pipelines as external stages: `ExtRun` (end of file) starts them inside one
//! pipeline Operation, joins neighbouring stages with kernel streams, pumps the
//! bytes that enter or leave the external segment, drains every child's stderr to
//! the shell's own stderr sink, keeps servicing the session event queue (Ctrl-C
//! cancels the whole pipeline) and reaps every stage on every exit path.
//! Child bytes relayed to the terminal (that pump, and the serial foreground drain)
//! are followed by `screen_reset`: a child that left the alternate screen, a hidden
//! cursor or bracketed paste on is followed by one `cellos-vt1` soft reset.
//!
//! All shell state lives in `shell_state` behind locks; this module holds no
//! mutable statics of its own.

extern crate alloc;

use crate::jobs::{JobState, Jobs};
use crate::parser::{Ast, Cmd, QuoteStyle, Redirect, Word};
use crate::screen_reset::{RelayScreen, SOFT_RESET};
use crate::shell_state::{self as state, CaptureGuard, LoopSignal};
use crate::typed::{
    self, Admission, BytePlan, ByteHead, DefaultSink, Head, Redir, RedirKind, StageIn, StreamStop,
    TypedPlan, INTERRUPTED_STATUS,
};
use crate::text_engine::args::{with_legacy_parts, ArgCursor, UtilityStatus};
use crate::text_engine::records::{extend_input, InputBufferError, MAX_INPUT_BYTES};
use crate::text_tools::{awk, sed};
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use text_engine::pipeline::InterruptLatch;
use ostd::prelude::*;
use ostd::syscall;

use api::cell_session::frontend::{FRONTEND_ATTACHED, LOST_FRONTEND};
use api::cell_session::Status;
use core::sync::atomic::{AtomicBool, Ordering};
use terminal_sdk::input::EventKind;
use terminal_sdk::session::{FrontendOutput, TerminalSession};
use terminal_sdk::pipe_child::{PipeChild, PipeSpec, Pipeline};
use terminal_sdk::stream::Endpoint;

/// The one terminal session of this shell, which it created and controls: for the
/// physical serial frontend (`create_serial`) or for a frontend holder that launched it
/// (`create_for_frontend`). Every terminal sink below goes through it.
static SERIAL_SESSION: ostd::sync::Mutex<Option<TerminalSession>> =
    ostd::sync::Mutex::new(None);

/// The frontend holder's output writers when the session was created for a frontend
/// (`FrontendHost`). The serial-controller relay then forwards the session's output and
/// diagnostic rings to these endpoints instead of the physical UART. `None` on serial.
static FRONTEND_OUTPUT: ostd::sync::Mutex<Option<FrontendOutput>> = ostd::sync::Mutex::new(None);

/// True when the session was created for a frontend holder, not the physical serial.
pub fn frontend_mode() -> bool {
    FRONTEND_OUTPUT.lock().is_some()
}

/// Set once at frontend-session start when the launch context carried the merged-stream
/// entry (`CELLBOX_STREAMS=merged`, see `context_session::streams_merged`): the holder
/// draws one terminal stream, so every diagnostic is written to the stdout ring and the
/// relay forwards both rings to the holder's stdout writer, in program order. Only
/// meaningful in frontend mode.
static STREAMS_MERGED: AtomicBool = AtomicBool::new(false);

/// True when diagnostics are merged into stdout: a frontend shell launched with the
/// merged-stream entry. Serial and legacy shells never merge.
fn streams_merged() -> bool {
    frontend_mode() && STREAMS_MERGED.load(Ordering::Acquire)
}

/// The terminal endpoint one write goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sink {
    Stdout,
    Stderr,
}

/// Diagnostics use the stderr endpoint unless the streams are merged; everything else is
/// stdout.
fn sink_for(diagnostic: bool, merged: bool) -> Sink {
    if diagnostic && !merged {
        Sink::Stderr
    } else {
        Sink::Stdout
    }
}

fn write_sink(session: &mut TerminalSession, sink: Sink, bytes: &[u8]) -> Result<usize, Status> {
    match sink {
        Sink::Stdout => session.write_stdout(bytes, 0),
        Sink::Stderr => session.write_stderr(bytes, 0),
    }
}

pub fn start_serial_session() -> Result<(), Status> {
    let session = TerminalSession::create_serial(24, 80)?;
    *SERIAL_SESSION.lock() = Some(session);
    Ok(())
}

/// Install the controller session this shell created for its launching frontend holder
/// (`TerminalSession::create_for_frontend`). From here on the shell runs the serial
/// controller path unchanged; only the relay's device is the holder's writers.
#[cfg(not(feature = "shell_test"))]
pub fn start_frontend_session(session: TerminalSession, output: FrontendOutput, merged: bool) {
    STREAMS_MERGED.store(merged, Ordering::Release);
    *FRONTEND_OUTPUT.lock() = Some(output);
    *SERIAL_SESSION.lock() = Some(session);
}

/// End the terminal session this shell controls (serial or frontend).
pub fn close_session() -> Result<(), Status> {
    let mut slot = SERIAL_SESSION.lock();
    let session = slot.as_mut().ok_or(Status::WrongSession)?;
    session.close()?;
    *slot = None;
    Ok(())
}

/// `Lost` reasons of a frontend session that are notices, not input loss: the holder
/// detached (the session waits Detached, no EOF) or a frontend re-attached.
fn frontend_notice(reason: u32) -> bool {
    reason == LOST_FRONTEND || reason == FRONTEND_ATTACHED
}

/// Write `bytes` to one holder writer, waiting politely while the holder has not drained.
fn forward_to_frontend(endpoint: Endpoint, bytes: &[u8]) -> Result<(), Status> {
    let mut backoff = crate::context_session::Backoff::new();
    write_terminal_bytes(bytes, |pending| endpoint.write(pending, 0), || Ok::<(), Status>(()), || backoff.pause())
}

/// One relayed chunk to the frontend device: the physical UART (fd 1/2) on serial, the
/// holder's writers in frontend mode (the stderr ring goes to the holder's stdout when
/// the streams are merged).
fn relay_device(diagnostic: bool, bytes: &[u8]) -> Result<(), Status> {
    let frontend = *FRONTEND_OUTPUT.lock();
    if let Some(output) = frontend {
        let endpoint = match sink_for(diagnostic, streams_merged()) {
            Sink::Stdout => output.stdout,
            Sink::Stderr => output.stderr,
        };
        return forward_to_frontend(endpoint, bytes);
    }
    let mut offset = 0;
    while offset < bytes.len() {
        let written = syscall::sys_write(if diagnostic { 2 } else { 1 }, &bytes[offset..])
            .map_err(|_| Status::Io)?;
        if written == 0 || written > bytes.len() - offset {
            return Err(Status::Io);
        }
        offset += written;
    }
    Ok(())
}

/// Frontend rendering is byte-preserving Write, never diagnostic Log. The two owner
/// streams remain separate even when the physical serial device is shared; a frontend
/// holder receives them on its own writers (merged into stdout on request).
fn render_serial(session: &mut TerminalSession) -> Result<(), Status> {
    relay_serial(session, None).map(|_| ())
}

/// `render_serial` that also hands every relayed byte to `screen` (the output of a
/// foreground child, see `screen_reset`). `Ok(true)` when a stream still had bytes after
/// its bounded turn.
fn relay_serial(session: &mut TerminalSession, mut screen: Option<&mut RelayScreen>) -> Result<bool, Status> {
    // Child/command output may echo secrets: the staging buffer is wiped on every exit path.
    let mut bytes = ostd::session::Scrub::<512>::new();
    let mut pending = false;
    for diagnostic in [false, true] {
        let mut drained = false;
        // Bound each turn so a busy stdout cannot starve stderr or completion.
        for _ in 0..8 {
            let read = if diagnostic {
                session.drain_stderr(&mut bytes)
            } else {
                session.drain_stdout(&mut bytes)
            };
            let count = match read {
                Ok(0) => return Err(Status::Malformed),
                Ok(count) => count,
                Err(Status::WouldBlock | Status::Eof) => {
                    drained = true;
                    break;
                }
                Err(status) => return Err(status),
            };
            if let Some(screen) = screen.as_deref_mut() {
                screen.feed(diagnostic, &bytes[..count]);
            }
            relay_device(diagnostic, &bytes[..count])?;
        }
        pending |= !drained;
    }
    Ok(pending)
}

/// The fixed first-trust prompt for an input-owner `TrustPrompt` notice (D79-13), built
/// only from the notice's fields. Input admitted them as graphic ASCII, so no control
/// byte from the descriptor can reach the terminal.
fn trust_prompt_text(kind: &EventKind) -> Option<String> {
    let EventKind::TrustPrompt { host, port, algorithm, fingerprint } = kind else { return None; };
    Some(alloc::format!(
        "\r\nThe authenticity of host '{host}' (port {port}) can't be established.\r\n{algorithm} key fingerprint is {fingerprint}.\r\nType 'yes' and Enter to trust it permanently; anything else refuses: "
    ))
}

/// Calls made for one owner-notice acknowledgement (yields first, then one-tick sleeps).
const ACK_ATTEMPTS: u32 = 64;

/// Acknowledge one owner notice: `ack` is retried with `pause` between calls while input
/// answers transiently (mid-transition, busy, uncertain transport), at most
/// [`ACK_ATTEMPTS`] calls. Any other refusal, or exhausted retries, is returned.
fn acknowledge_with(mut ack: impl FnMut() -> Result<(), Status>, mut pause: impl FnMut()) -> Result<(), Status> {
    let mut attempt = 1;
    loop {
        match ack() {
            Ok(()) => return Ok(()),
            Err(Status::WouldBlock | Status::TimedOut | Status::Indeterminate) if attempt < ACK_ATTEMPTS => {
                attempt += 1;
                pause();
            }
            Err(status) => return Err(status),
        }
    }
}

/// Input-owner notices while a foreground child runs. The input owner captures the answer
/// itself and routes `TrustPrompt`/`TrustPromptEnd` to this controller's own control ring,
/// never the child's. Each notice is acknowledged only after it is drawn: input arms its
/// capture on the prompt's acknowledgement and keeps the session's input from the child
/// until the end's acknowledgement. While the prompt is drawn or an acknowledgement is
/// being retried, the child's output is not relayed (it waits in its rings). The prompt
/// ends on `TrustPromptEnd` or on foreground restoration (the kernel flushes the
/// controller's ring there).
#[derive(Default)]
struct OwnerPrompt {
    drawn: bool,
    failed: bool,
    /// The handshake failed and the foreground child must be cancelled: retried each turn
    /// with this backoff until the cancellation is in or the foreground is restored.
    cancel: Option<crate::context_session::Backoff>,
}

/// What one `cancel_foreground` result means for a cancellation left pending by a failed
/// prompt handshake.
#[derive(Debug, PartialEq, Eq)]
enum CancelStep {
    /// Cancelled now or already: the operation is cancelled or over (gone or revoked).
    Done,
    /// The owner is busy or the outcome is uncertain: try again next turn.
    Retry,
    /// Refused for good: the child runs on until it exits, input keeps its input.
    Stop(Status),
}
fn cancel_step(result: Result<(), Status>) -> CancelStep {
    match result {
        Ok(()) | Err(Status::Cancelled | Status::Stale | Status::Revoked) => CancelStep::Done,
        Err(Status::WouldBlock | Status::TimedOut | Status::Indeterminate) => CancelStep::Retry,
        Err(status) => CancelStep::Stop(status),
    }
}

impl OwnerPrompt {
    /// Draw and acknowledge the pending notices. When the handshake cannot complete (a
    /// notice unreadable, undrawable or never acknowledged), input still holds this child's
    /// input, so a cancellation of the foreground child becomes pending (`cancel_turn`):
    /// its restoration ends the quarantine and the confirmation.
    fn poll(&mut self, session: &mut TerminalSession) {
        while !self.failed {
            let event = match session.next_owner_notice() {
                Ok(Some(event)) => event,
                Ok(None) => return,
                Err(status) => return self.give_up(status),
            };
            let drawn = match &event.kind {
                EventKind::TrustPromptEnd { .. } if self.drawn => relay_device(false, b"\r\n").map(|()| false),
                kind => match trust_prompt_text(kind) {
                    // Drawn from the first byte: a failed write may leave part of it on screen.
                    Some(text) => { self.drawn = true; relay_device(false, text.as_bytes()).map(|()| true) }
                    None => Ok(false),
                },
            };
            let mut backoff = crate::context_session::Backoff::new();
            match drawn.and_then(|drawn| acknowledge_with(|| session.acknowledge_owner_notice(&event), || backoff.pause()).map(|()| drawn)) {
                Ok(drawn) => self.drawn = drawn,
                Err(status) => return self.give_up(status),
            }
        }
    }
    /// Stop prompting for this child and make its cancellation pending. A prompt that is
    /// (even partly) on screen is ended with `\r\n` before any child output is relayed again.
    fn give_up(&mut self, status: Status) {
        self.failed = true;
        self.cancel = Some(crate::context_session::Backoff::new());
        if core::mem::take(&mut self.drawn) { let _ = relay_device(false, b"\r\n"); }
        let _ = syscall::sys_log(&alloc::format!("shell: trust prompt handshake failed: {:?}\n", status));
    }
    fn relay(&mut self, session: &mut TerminalSession, screen: &mut RelayScreen) -> Result<bool, Status> {
        if self.drawn { Ok(false) } else { relay_serial(session, Some(screen)) }
    }
    fn restored(&mut self) {
        self.cancel = None;
        if core::mem::take(&mut self.drawn) { let _ = relay_device(false, b"\r\n"); }
    }
    /// One turn of a pending cancellation (nothing when none is pending).
    fn cancel_turn(&mut self, cancel: impl FnOnce() -> Result<(), Status>) {
        let Some(backoff) = self.cancel.as_mut() else { return; };
        match cancel_step(cancel()) {
            CancelStep::Done => self.cancel = None,
            CancelStep::Retry => backoff.pause(),
            CancelStep::Stop(status) => {
                self.cancel = None;
                let _ = syscall::sys_log(&alloc::format!("shell: foreground cancel after a failed trust prompt refused: {:?}\n", status));
            }
        }
    }
}

/// Retain the unsent suffix across short writes and backpressure. The blocked
/// callback runs outside the session lock, before any sleep.
fn write_terminal_bytes<E: From<Status>>(
    bytes: &[u8],
    mut write: impl FnMut(&[u8]) -> Result<usize, Status>,
    mut blocked: impl FnMut() -> Result<(), E>,
    mut pause: impl FnMut(),
) -> Result<(), E> {
    let mut offset = 0;
    while offset < bytes.len() {
        match write(&bytes[offset..]) {
            Ok(count) if count > 0 && count <= bytes.len() - offset => offset += count,
            Ok(_) => return Err(Status::Malformed.into()),
            Err(Status::WouldBlock) => {
                blocked()?;
                pause();
            }
            Err(status) => return Err(status.into()),
        }
    }
    Ok(())
}

/// `write_terminal_bytes` against the endpoint `sink_for(diagnostic, merged)` selects.
/// The routing is decided once per write, so the unsent suffix always retries on the
/// endpoint that took the first byte.
fn route_terminal_write<E: From<Status>>(
    bytes: &[u8],
    diagnostic: bool,
    merged: bool,
    mut write: impl FnMut(Sink, &[u8]) -> Result<usize, Status>,
    blocked: impl FnMut() -> Result<(), E>,
    pause: impl FnMut(),
) -> Result<(), E> {
    let sink = sink_for(diagnostic, merged);
    write_terminal_bytes(bytes, |pending| write(sink, pending), blocked, pause)
}

fn terminal_write_while<E: From<Status>>(
    bytes: &[u8],
    diagnostic: bool,
    blocked: impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    route_terminal_write(
        bytes,
        diagnostic,
        streams_merged(),
        |sink, pending| {
            // Never retain SESSION while polling input or waiting for the host.
            let mut slot = SERIAL_SESSION.lock();
            let session = slot.as_mut().ok_or(Status::WrongSession)?;
            let write = write_sink(session, sink, pending);
            if matches!(write, Ok(_) | Err(Status::WouldBlock)) {
                render_serial(session)?;
            }
            write
        },
        blocked,
        || {},
    )
}

fn terminal_write(bytes: &[u8], diagnostic: bool) -> Result<(), Status> {
    terminal_write_while(bytes, diagnostic, || Ok(()))
}

/// Only an input event, never an endpoint status, denotes a user interrupt.
#[derive(Debug, PartialEq, Eq)]
enum TerminalWriteStop {
    Output(Status),
    Interrupted,
}

impl From<Status> for TerminalWriteStop {
    fn from(status: Status) -> Self {
        Self::Output(status)
    }
}

pub fn terminal_stdout(bytes: &[u8]) -> Result<(), Status> {
    terminal_write(bytes, false)
}

pub fn terminal_stderr(bytes: &[u8]) -> Result<(), Status> {
    terminal_write(bytes, true)
}

/// The `cellos-vt1` soft reset after a relayed child left the alternate screen, a hidden
/// cursor or bracketed paste on (`screen_reset`). Shell stdout, after the child bytes it
/// follows; a failed delivery is diagnostic only, the command already ran.
fn restore_screen() {
    if terminal_stdout(SOFT_RESET).is_err() {
        let _ = syscall::sys_log("shell: terminal soft reset delivery failed\n");
    }
}

/// `restore_screen` while the caller already holds `SERIAL_SESSION` (serial foreground):
/// session stdout, relayed to the device after every accepted or refused write.
fn restore_screen_locked(session: &mut TerminalSession) {
    let written: Result<(), Status> = write_terminal_bytes(
        SOFT_RESET,
        |pending| {
            let write = write_sink(session, Sink::Stdout, pending);
            if matches!(write, Ok(_) | Err(Status::WouldBlock)) {
                render_serial(session)?;
            }
            write
        },
        || Ok(()),
        || ostd::task::yield_now(),
    );
    if written.is_err() {
        let _ = syscall::sys_log("shell: terminal soft reset delivery failed\n");
    }
}

/// What the device has been told while an in-process TUI (`top`) wrote to it: the
/// tracked screen modes, and whether any write failed (the device then holds an unknown
/// prefix of it).
#[derive(Clone, Copy)]
struct TuiScreen {
    tracker: crate::screen_reset::ModeTracker,
    failed: bool,
}

/// Output of an in-process TUI. The shell is the only drainer of its own session rings,
/// so a TUI cannot write the stdout ring while it blocks for its terminal (D64-5 S1):
/// every chunk goes straight to the device the ring relay would reach (`relay_device`:
/// the UART fd 1 on serial, the holder's stdout writer in frontend mode), in order.
#[derive(Clone, Copy)]
pub struct TuiSink<'a> {
    screen: &'a core::cell::Cell<TuiScreen>,
}

impl vitui::backend::ByteSink for TuiSink<'_> {
    type Error = Status;
    const MAX_WRITE: usize = 512;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Status> {
        let mut screen = self.screen.get();
        let written = relay_device(false, bytes);
        match written {
            Ok(()) => screen.tracker.feed(bytes),
            Err(_) => screen.failed = true,
        }
        self.screen.set(screen);
        written.map(|()| bytes.len())
    }
}

/// Run an in-process TUI on this shell's session: take the session guard once, relay
/// everything still queued in the rings, then hand `run` the session and a [`TuiSink`].
/// Afterwards, when the device may still be in a TUI screen mode (a restore was lost, or
/// any write failed), one `SOFT_RESET` follows. Nothing inside `run` may write through
/// the shell's terminal sinks (`shell_*`, `terminal_*`): the guard is not reentrant, so
/// diagnostics are written after this returns.
pub fn with_tui<R>(run: impl FnOnce(&mut TerminalSession, TuiSink<'_>) -> R) -> Result<R, Status> {
    let mut slot = SERIAL_SESSION.lock();
    let session = slot.as_mut().ok_or(Status::WrongSession)?;
    while relay_serial(session, None)? {}
    let screen = core::cell::Cell::new(TuiScreen { tracker: crate::screen_reset::ModeTracker::new(), failed: false });
    let result = run(session, TuiSink { screen: &screen });
    let screen = screen.get();
    if (screen.failed || screen.tracker.dirty()) && relay_device(false, SOFT_RESET).is_err() {
        let _ = syscall::sys_log("shell: terminal soft reset delivery failed\n");
    }
    Ok(result)
}

/// Diagnostic sink: SDK stderr in a serial session, the one-shot's stderr grant in a
/// `/bin/cellbox <command>` one-shot, the legacy console otherwise. Never captured, so a
/// diagnostic from a pipeline stage or redirect stays visible.
pub fn shell_stderr(text: &str) {
    if SERIAL_SESSION.lock().is_some() {
        if terminal_stderr(text.as_bytes()).is_err() {
            let _ = syscall::sys_log("shell: terminal diagnostic delivery failed\n");
        }
    } else if state::oneshot_active() {
        state::oneshot_stderr(text.as_bytes());
    } else {
        ostd::io::print(text);
    }
}

/// `shell_stderr(text)` followed by a newline.
pub fn shell_stderrln(text: &str) {
    shell_stderr(text);
    shell_stderr("\n");
}

/// True while a serial terminal session owns the console (legacy fd 0 is then never read).
pub fn serial_active() -> bool {
    SERIAL_SESSION.lock().is_some()
}

/// Interactive terminal output that must appear *now* and is never part of a command's data:
/// prompts and screen control. Serial session → SDK stdout immediately (the dispatcher only
/// flushes its capture after the command ends); legacy → the console, bypassing captures.
/// A one-shot has no terminal: nothing is written (CBX-29, no screen control to a
/// non-terminal).
pub fn shell_console(text: &str) {
    if serial_active() {
        if terminal_stdout(text.as_bytes()).is_err() {
            let _ = syscall::sys_log("shell: terminal output delivery failed\n");
        }
    } else if !state::oneshot_active() {
        ostd::io::print(text);
    }
}

/// True when `top` may draw its TUI: a controller session is installed, only the REPL's
/// own capture is active (never inside `$(...)`, a pipeline stage or a redirect), and the
/// session answers its capabilities.
pub fn interactive_terminal() -> bool {
    terminal_is_ours(serial_active(), state::capture_depth())
        && SERIAL_SESSION.lock().as_ref().is_some_and(|session| session.capabilities().is_ok())
}

/// The terminal belongs to the running command: a session exists and nothing but the
/// REPL's own line capture (depth 1) is active.
fn terminal_is_ours(session: bool, capture_depth: usize) -> bool {
    session && capture_depth <= 1
}

/// A string whose whole allocation is zeroed when it is dropped, so typed text never lingers on an
/// early return. `take` hands the contents to the caller, who then owns the wiping. Sized so the
/// 128-byte line limit can never reallocate (a realloc would free an unwiped copy).
struct Wiped(String);
impl Wiped {
    fn with_capacity(capacity: usize) -> Self { Self(String::with_capacity(capacity)) }
    fn replace(&mut self, text: &str) {
        ostd::session::zeroize_string(&mut self.0);
        self.0.push_str(text);
    }
    fn take(&mut self) -> String { core::mem::take(&mut self.0) }
}
impl Drop for Wiped {
    fn drop(&mut self) { ostd::session::zeroize_string(&mut self.0); }
}

/// Terminal events carry typed text: every exit path wipes the event it was holding.
struct ScrubbedEvent(terminal_sdk::input::Event);
impl Drop for ScrubbedEvent {
    fn drop(&mut self) { self.0.scrub(); }
}

/// Longest command line the editor accepts, in bytes.
pub const SHELL_LINE_MAX: usize = 4096;

/// The submit key of every line reader (command line, `read -s`, prompts, ssh password):
/// Enter, or Control+J. The serial decoder delivers a lone LF as Control+J (the `spec.md`
/// input grammar), and Control+J is accept-line, as in readline, so a client that ends lines
/// with LF alone submits them. Key-level TUI reading (`TerminalKeys`) keeps Control+J a key.
pub(crate) fn submits_line(code: u32, modifiers: u32) -> bool {
    use api::services::session::{key_code, modifiers as key_modifiers};
    (code == key_code::ENTER && modifiers == 0)
        || (code == u32::from(b'j') && modifiers == key_modifiers::CONTROL)
}

/// The key a line reader acts on: Enter for any submit key, the code itself for an unmodified
/// key, and nothing (`0`, which no reader handles) for any other modified key.
fn line_key(code: u32, modifiers: u32) -> u32 {
    if submits_line(code, modifiers) {
        api::services::session::key_code::ENTER
    } else if modifiers == 0 {
        code
    } else {
        0
    }
}

/// Appends typed text unless the line would exceed `SHELL_LINE_MAX`. Overflowing text marks the
/// line as overflowed instead of being kept in part, so a truncated command never runs.
fn push_line_text(line: &mut Wiped, overflow: &mut bool, text: &str) -> bool {
    if *overflow || line.0.len() + text.len() > SHELL_LINE_MAX {
        *overflow = true;
        return false;
    }
    line.0.push_str(text);
    true
}

/// Typed canonical input keeps pasted newlines inert until a separate Enter.
/// No public fd-0 read is reachable from the admitted serial path. A line longer than
/// `SHELL_LINE_MAX` is refused whole at Enter, never run truncated.
pub fn terminal_read_line(history: &mut Vec<String>) -> Result<Option<String>, Status> {
    let mut line = Wiped::with_capacity(SHELL_LINE_MAX);
    let mut draft = Wiped::with_capacity(SHELL_LINE_MAX);
    let mut overflow = false;
    let mut history_index = history.len();
    let mut paste = false;
    loop {
        let event = ScrubbedEvent({
            let mut slot = SERIAL_SESSION.lock();
            slot.as_mut().ok_or(Status::WrongSession)?.next_event(u64::MAX)?
        });
        match &event.0.kind {
            EventKind::Text(text) => {
                if push_line_text(&mut line, &mut overflow, text) {
                    terminal_stdout(text.as_bytes())?;
                }
            }
            EventKind::PasteBegin => paste = true,
            EventKind::PasteEnd => paste = false,
            EventKind::Key { code, modifiers, pressed: 1 | 2 } if !paste => {
                match line_key(*code, *modifiers) {
                    0x110001 if overflow => {
                        line.replace("");
                        overflow = false;
                        history_index = history.len();
                        shell_stderrln(&alloc::format!(
                            "\nshell: line longer than {} bytes; discarded",
                            SHELL_LINE_MAX
                        ));
                        terminal_stdout(b"Cellos > ")?;
                    }
                    0x110001 => {
                        terminal_stdout(b"\n")?;
                        if !line.0.trim().is_empty()
                            && history.last().map(String::as_str) != Some(line.0.as_str())
                        {
                            if history.len() == 128 {
                                history.remove(0);
                            }
                            history.push(line.0.clone());
                        }
                        return Ok(Some(line.take()));
                    }
                    0x110003 => {
                        if line.0.pop().is_some() {
                            terminal_stdout(b"\x08 \x08")?;
                        }
                    }
                    0x110010 | 0x110011 => {
                        if history_index == history.len() {
                            draft.replace(&line.0);
                        }
                        overflow = false;
                        if *code == 0x110010 {
                            history_index = history_index.saturating_sub(1);
                        } else {
                            history_index = (history_index + 1).min(history.len());
                        }
                        line.replace(history.get(history_index).map(String::as_str).unwrap_or(draft.0.as_str()));
                        terminal_stdout(b"\r\x1b[2KCellos > ")?;
                        terminal_stdout(line.0.as_bytes())?;
                    }
                    _ => {}
                }
            }
            EventKind::Interrupt => {
                paste = false;
                terminal_stdout(b"^C\n")?;
                return Ok(Some(String::new()));
            }
            EventKind::Eof => return Ok(None),
            // The frontend holder detached: the session waits Detached (no EOF) and the
            // typed line is intact. Logged (fixed text, never input) so a runner can see it.
            EventKind::Lost(LOST_FRONTEND) => {
                paste = false;
                let _ = syscall::sys_log("shell: frontend detached; session waits\n");
            }
            // A frontend re-attached (a Resize with its size came first): redraw the
            // prompt and the line being edited on the new frontend.
            EventKind::Lost(FRONTEND_ATTACHED) => {
                let _ = syscall::sys_log("shell: frontend re-attached; redraw\n");
                terminal_stdout(b"\r\x1b[2KCellos > ")?;
                terminal_stdout(line.0.as_bytes())?;
            }
            EventKind::Lost(reason) => {
                // Input emits Lost for routine conditions (unmapped key, undecodable
                // escape, event-queue overflow). Discard the possibly corrupt partial
                // line and keep the only console shell alive.
                line.replace("");
                draft.replace("");
                history_index = history.len();
                overflow = false;
                paste = false;
                shell_stderrln(&alloc::format!("\nshell: input lost (reason {}); line discarded", reason));
                terminal_stdout(b"Cellos > ")?;
            }
            _ => {}
        }
    }
}

enum SecretEnd {
    Enter,
    Interrupt,
    Eof,
}

fn read_secret_events(session: &mut TerminalSession, line: &mut Wiped) -> Result<SecretEnd, Status> {
    let mut paste = false;
    loop {
        let event = ScrubbedEvent(session.next_event(u64::MAX)?);
        match &event.0.kind {
            // No echo, no cursor edits, no history navigation: only text, Backspace and Enter.
            EventKind::Text(text) => {
                if line.0.len() + text.len() <= 128 {
                    line.0.push_str(text);
                }
            }
            EventKind::PasteBegin => paste = true,
            EventKind::PasteEnd => paste = false,
            EventKind::Key { code, modifiers, pressed: 1 | 2 } if !paste => match line_key(*code, *modifiers) {
                0x110001 => return Ok(SecretEnd::Enter),
                0x110003 => {
                    line.0.pop();
                }
                _ => {}
            },
            EventKind::Interrupt => return Ok(SecretEnd::Interrupt),
            EventKind::Eof => return Ok(SecretEnd::Eof),
            // A frontend detach/re-attach drops nothing; any other lost event may have
            // dropped part of the secret: never accept a corrupt value.
            EventKind::Lost(reason) if frontend_notice(*reason) => {}
            EventKind::Lost(_) => return Err(Status::Io),
            _ => {}
        }
    }
}

/// Silent canonical read for `read -s`: the typed text is never echoed, never edited through
/// history and never recorded. The owner's echo-off lease is acquired before the first key is
/// read; a frontend that cannot suppress echo (the physical UART) answers `Unsupported` and no
/// secret is collected. The lease is released and the text wiped on every exit path.
pub fn terminal_read_secret() -> Result<Option<String>, Status> {
    let mut line = Wiped::with_capacity(128);
    let end = {
        let mut slot = SERIAL_SESSION.lock();
        let session = slot.as_mut().ok_or(Status::WrongSession)?;
        let mut guard = session.set_mode(2, 0)?;
        let read = read_secret_events(guard.session(), &mut line);
        let released = guard.release();
        let end = read?;
        released?;
        end
    };
    match end {
        SecretEnd::Enter => {
            terminal_stdout(b"\n")?;
            Ok(Some(line.take()))
        }
        SecretEnd::Interrupt => {
            terminal_stdout(b"^C\n")?;
            Err(Status::Cancelled)
        }
        SecretEnd::Eof => Ok(None),
    }
}

/// One decoded key press of a single-key read. Typed text yields only its first character; a
/// pasted sequence never produces a key (a paste must not answer a prompt or quit a program).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalKey {
    Char(char),
    Enter,
    Backspace,
    /// Ctrl-C: the user cancels the running builtin.
    Interrupt,
    /// Input ended (the session's stdin was closed).
    Eof,
}

/// Single-key reader over the serial session. It never echoes anything. The paste state lives
/// here so it survives between non-blocking polls.
pub struct TerminalKeys {
    paste: bool,
}

impl TerminalKeys {
    pub const fn new() -> Self {
        Self { paste: false }
    }

    /// Pure decoder for one event; `None` means the event is not a key press (release, resize,
    /// lost, unmapped key, anything inside a paste).
    fn classify(&mut self, kind: &EventKind) -> Option<TerminalKey> {
        match kind {
            EventKind::Text(text) if !self.paste => text.chars().next().map(TerminalKey::Char),
            EventKind::PasteBegin => {
                self.paste = true;
                None
            }
            EventKind::PasteEnd => {
                self.paste = false;
                None
            }
            EventKind::Key { code, modifiers: 0, pressed: 1 | 2 } if !self.paste => match *code {
                0x110001 => Some(TerminalKey::Enter),
                0x110003 => Some(TerminalKey::Backspace),
                _ => None,
            },
            EventKind::Interrupt => {
                self.paste = false;
                Some(TerminalKey::Interrupt)
            }
            EventKind::Eof => Some(TerminalKey::Eof),
            _ => None,
        }
    }

    /// Next key press from `session` (the caller holds the session guard, see `with_tui`).
    /// `deadline_ns` is an absolute monotonic time: `0` polls without blocking and
    /// `u64::MAX` waits forever. `Ok(None)` means no key arrived before the deadline.
    pub fn read_from(&mut self, session: &mut TerminalSession, deadline_ns: u64) -> Result<Option<TerminalKey>, Status> {
        loop {
            let event = match session.next_event(deadline_ns) {
                Ok(event) => ScrubbedEvent(event),
                Err(Status::WouldBlock | Status::TimedOut) => return Ok(None),
                Err(status) => return Err(status),
            };
            if let Some(key) = self.classify(&event.0.kind) {
                return Ok(Some(key));
            }
        }
    }
}

/// Outcome of a confirmation-style line read.
pub enum PromptLine {
    Line(String),
    /// Ctrl-C: the caller must abandon its whole operation.
    Cancelled,
    Eof,
}

/// What one event did to a prompt line; the caller performs the echo.
enum PromptStep {
    Idle,
    /// Text was appended starting at this byte offset of the line.
    Appended(usize),
    Erased,
    Enter,
    Cancel,
    Eof,
    Lost,
}

/// Pure line editing for prompts: text, Backspace, Enter, Ctrl-C only. No history, no cursor
/// motion. Pasted text stays inert until a separate Enter, exactly as in `terminal_read_line`.
fn prompt_step(line: &mut Wiped, paste: &mut bool, kind: &EventKind) -> PromptStep {
    match kind {
        EventKind::Text(text) => {
            if line.0.len() + text.len() <= 128 {
                let start = line.0.len();
                line.0.push_str(text);
                if !text.is_empty() {
                    return PromptStep::Appended(start);
                }
            }
            PromptStep::Idle
        }
        EventKind::PasteBegin => {
            *paste = true;
            PromptStep::Idle
        }
        EventKind::PasteEnd => {
            *paste = false;
            PromptStep::Idle
        }
        EventKind::Key { code, modifiers, pressed: 1 | 2 } if !*paste => match line_key(*code, *modifiers) {
            0x110001 => PromptStep::Enter,
            0x110003 if line.0.pop().is_some() => PromptStep::Erased,
            _ => PromptStep::Idle,
        },
        EventKind::Interrupt => {
            *paste = false;
            PromptStep::Cancel
        }
        EventKind::Eof => PromptStep::Eof,
        // A frontend detach/re-attach drops nothing; any other lost event may have dropped
        // part of the answer: never accept a corrupt one.
        EventKind::Lost(reason) if frontend_notice(*reason) => PromptStep::Idle,
        EventKind::Lost(_) => PromptStep::Lost,
        _ => PromptStep::Idle,
    }
}

/// Canonical single-line answer for builtin prompts (`rm -i`). Echoes typed text, supports
/// Backspace, never records history, and Ctrl-C cancels. The caller prints the prompt text
/// itself with `shell_console`. The typed text is wiped on every exit path.
pub fn terminal_prompt_line() -> Result<PromptLine, Status> {
    let mut line = Wiped::with_capacity(128);
    let mut paste = false;
    loop {
        let event = ScrubbedEvent({
            let mut slot = SERIAL_SESSION.lock();
            slot.as_mut().ok_or(Status::WrongSession)?.next_event(u64::MAX)?
        });
        match prompt_step(&mut line, &mut paste, &event.0.kind) {
            PromptStep::Idle => {}
            PromptStep::Appended(start) => terminal_stdout(&line.0.as_bytes()[start..])?,
            PromptStep::Erased => terminal_stdout(b"\x08 \x08")?,
            PromptStep::Enter => {
                terminal_stdout(b"\n")?;
                return Ok(PromptLine::Line(line.take()));
            }
            PromptStep::Cancel => {
                terminal_stdout(b"^C\n")?;
                return Ok(PromptLine::Cancelled);
            }
            PromptStep::Eof => return Ok(PromptLine::Eof),
            PromptStep::Lost => return Err(Status::Io),
        }
    }
}

/// Route command output through the current sink.
///
/// Built-in data retains the existing capture stack (including pipelines and
/// redirects). The serial dispatcher sends its final capture to SDK stdout;
/// terminal diagnostics bypass captures through the separate SDK stderr stream.
pub fn shell_print(s: &str) {
    state::write_out(s);
}

/// `shell_print(s)` followed by a newline.
pub fn shell_println(s: &str) {
    shell_print(s);
    shell_print("\n");
}

/// Return the current pipe-fed stdin bytes, empty when no pipe is active.
///
/// Commands that accept either a file argument or stdin (e.g., `grep`, `wc`)
/// call this when no file path is given. Owned rather than borrowed — see
/// `shell_state::stdin_bytes`.
pub fn shell_stdin() -> Vec<u8> {
    state::stdin_bytes()
}

/// All recognized shell built-in names, used by tab completion.
#[cfg(not(feature = "shell_test"))] // reason: tab completion only exists in the interactive REPL
pub const BUILTINS: &[&str] = &[
    "alias", "awk", "bg", "blktest", "break", "cat", "cd", "clear", "continue", "date", "echo", "edit",
    "ssh", "completion",
    "env", "exec", "exit", "export", "fg", "find", "free", "grep", "head", "help", "history", "hv",
    "ifconfig", "ip", "jobs", "kill", "ls", "mkdir", "mv", "ps", "pwd", "read", "rg", "rm", "rmdir",
    "sed", "shutdown", "sleep", "snapshot", "sort", "source", "tail", "tee", "test", "top",
    "touch", "unalias", "uniq", "unset", "uname", "uptime", "vappend", "vcat", "vwrite", "wc",
    "cp",
];

// ── Shell-global state ────────────────────────────────────────────────────────
//
// Functions, variables, the exit flag and the loop signal all live in
// `shell_state` behind locks. These aliases keep the call sites in this module
// short and mark which of them the rest of the crate may use.

#[cfg(not(feature = "shell_test"))] // reason: only the REPL loop can honour an exit request
pub use crate::shell_state::take_exit_request;
pub use crate::shell_state::{define_function, request_exit};
use crate::shell_state::{
    get_function, get_var, set_loop_signal, set_var, take_loop_signal, unset_var,
};

/// A failed command substitution: the containing command does not run and ends with
/// this status.
#[derive(Debug, PartialEq, Eq)]
struct SubstFailed(i32);

impl SubstFailed {
    /// Record the status as `$?` and return it.
    fn abort(self) -> i32 {
        state::set_status(self.0);
        self.0
    }
}

/// Capture one supported command for `$(...)`.
///
/// A capture failure aborts the containing command so it cannot substitute
/// fabricated output.
fn run_capture(inner: &str) -> Result<String, SubstFailed> {
    let mut words = inner.split_whitespace();
    let cmd = match words.next() {
        Some(command) => command,
        None => return Ok(String::new()),
    };
    let args: alloc::vec::Vec<&str> = words.collect();
    let output = match cmd {
        "echo" => {
            let bytes = crate::commands::cmd_echo_to_vec(&args);
            String::from(core::str::from_utf8(&bytes).unwrap_or(""))
        }
        "vcat" | "cat" => {
            let Some(path) = args.first() else {
                return Ok(String::new());
            };
            match crate::cmd_fs::read_file_vfs_owned(path, 4096) {
                Ok(bytes) => String::from(core::str::from_utf8(&bytes).unwrap_or("")),
                Err(_) => {
                    shell_stderrln("shell: command substitution read failed");
                    String::new()
                }
            }
        }
        "pwd" => {
            let mut cwd = crate::cmd_cwd::get_shell_cwd().map_err(|_| SubstFailed(1))?;
            cwd.push('\n');
            cwd
        }
        "top" => {
            let argv: Vec<String> = inner.split_whitespace().map(String::from).collect();
            if typed::classify_command(&argv) != typed::CmdClass::Session {
                // Batch forms are not substitutable, like every other unlisted command.
                return Ok(String::new());
            }
            // A substitution is a capture, never the terminal: interactive `top` meets its
            // runtime gate under this capture and refuses (status 2, a `-b` hint) before
            // drawing anything (D64-9).
            let capture = CaptureGuard::new();
            let status = crate::commands::cmd_top(&argv[1..]);
            let bytes = capture.finish();
            if status != 0 {
                return Err(SubstFailed(status));
            }
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => String::new(),
    };
    Ok(output)
}

/// Expand variable and single-level command substitutions in one token.
///
/// A failed capture propagates so the containing command ends with its status.
fn expand_token(s: &str) -> Result<String, SubstFailed> {
    if !s.contains('$') {
        return Ok(String::from(s));
    }
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'(' {
                // Command substitution: $(...). Scan to matching ')'.
                // Single-level only — nested $( $() ) passes through as literal.
                let inner_start = i + 2;
                let mut depth = 1usize;
                let mut j = inner_start;
                while j < bytes.len() {
                    if bytes[j] == b'(' {
                        depth += 1;
                    } else if bytes[j] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if depth == 0 {
                    // bytes[inner_start..j] is a run of shell token chars, which the
                    // parser guarantees are ASCII — the checked decode cannot fail.
                    let inner = core::str::from_utf8(&bytes[inner_start..j]).unwrap_or("");
                    // Reject nested $(...) — pass $(  literally so the user can see the issue.
                    if !inner.contains("$(") {
                        let captured = run_capture(inner.trim())?;
                        result.push_str(captured.trim_end_matches('\n'));
                        i = j + 1;
                        continue;
                    }
                }
                // Unmatched paren or nested: emit '$(' literally and continue.
                result.push('$');
                i += 1;
                continue;
            }
            if next == b'{' {
                // `${NAME}` — the value; `${#NAME}` — its length in characters. Anything
                // else (unclosed, empty or not a name) stays literal.
                let close = bytes[i + 2..].iter().position(|&b| b == b'}');
                let inner = close.and_then(|close| core::str::from_utf8(&bytes[i + 2..i + 2 + close]).ok());
                if let (Some(close), Some(inner)) = (close, inner) {
                    let (length, name) = match inner.strip_prefix('#') {
                        Some(name) => (true, name),
                        None => (false, inner),
                    };
                    if is_var_name(name) {
                        let mut value = get_var(name).unwrap_or_default();
                        if length {
                            result.push_str(&alloc::format!("{}", value.chars().count()));
                        } else {
                            result.push_str(&value);
                        }
                        // The value may be a `read -s` secret: wipe the copy.
                        ostd::session::zeroize_string(&mut value);
                        i += close + 3;
                        continue;
                    }
                }
                result.push('$');
                i += 1;
                continue;
            }
            if next == b'?' {
                // $? — exit code of the last command.
                if let Some(v) = get_var("?") {
                    result.push_str(&v);
                }
                i += 2;
                continue;
            }
            if next == b'#' {
                // $# — positional argument count.
                if let Some(v) = get_var("#") {
                    result.push_str(&v);
                }
                i += 2;
                continue;
            }
            if next == b'@' {
                // $@ — all positional arguments joined with spaces.
                if let Some(v) = get_var("@") {
                    result.push_str(&v);
                }
                i += 2;
                continue;
            }
            if next.is_ascii_digit() && next != b'0' {
                // $1..$9 — single-digit positional parameter.
                // Guarded by `next.is_ascii_digit()` above.
                let key = core::str::from_utf8(&bytes[i + 1..i + 2]).unwrap_or("");
                if let Some(v) = get_var(key) {
                    result.push_str(&v);
                }
                i += 2;
                continue;
            }
            if next.is_ascii_alphabetic() || next == b'_' {
                let start = i + 1;
                let end = bytes[start..]
                    .iter()
                    .take_while(|&&b| b.is_ascii_alphanumeric() || b == b'_')
                    .count()
                    + start;
                // `take_while` above accepted only ASCII alphanumeric / '_'.
                let name = core::str::from_utf8(&bytes[start..end]).unwrap_or("");
                if let Some(v) = get_var(name) {
                    result.push_str(&v);
                }
                // Unset variables expand to empty string (POSIX default).
                i = end;
                continue;
            }
        }
        result.push(bytes[i] as char); // shell tokens are ASCII
        i += 1;
    }
    Ok(result)
}

/// A shell variable name: an ASCII letter or `_`, then letters, digits or `_`.
fn is_var_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn expand_word(word: &Word) -> Result<String, SubstFailed> {
    let mut expanded = String::new();
    for segment in &word.segments {
        match segment.quote {
            QuoteStyle::Single => expanded.push_str(&segment.text),
            QuoteStyle::None | QuoteStyle::Double => {
                expanded.push_str(&expand_token(&segment.text)?);
            }
        }
    }
    Ok(expanded)
}

/// Parse and execute `line`, capturing all `shell_print` output into a `Vec<u8>`.
///
/// Used by the `shell_test` feature harness to assert on command output without
/// requiring a real serial console.  The `CaptureGuard` pops the capture even if
/// the command panics or returns early.
#[cfg(feature = "shell_test")]
pub fn capture_line(line: &str, jobs: &mut Jobs) -> Vec<u8> {
    let guard = CaptureGuard::new();
    let ast = crate::parser::parse(line);
    execute(&ast, jobs);
    guard.finish()
}

/// Execute an `Ast` and return the last command's exit code.
///
/// `stdin_data` is the bytes available on stdin for the first command in a pipeline.
pub fn execute(ast: &Ast, jobs: &mut Jobs) -> i32 {
    match ast {
        Ast::Empty => 0,
        Ast::Simple(cmd) => exec_simple(cmd, jobs),
        Ast::Pipeline(cmds) => exec_pipeline(cmds, jobs),
        Ast::Invalid(reason) => reject_line(reason),
        Ast::BackgroundPipeline(_) => reject_line(
            "background `&` is supported only for a single external command; the pipeline was not started",
        ),
        Ast::Background(cmd) => {
            // Refuse typed, stateful and session commands BEFORE any effect: `&` is
            // never silently ignored and never mutates the parent context.
            if let Ok(stage) = stage_input(cmd) {
                let stage = match explicit_stage(stage) {
                    Ok(stage) => stage,
                    Err(code) => return code,
                };
                if let Some(reason) = typed::background_refusal(&stage) {
                    return reject_line(&reason);
                }
            }
            // Cooperative background: the shell is a single-task executor with no
            // async spawn capability for built-ins. `cmd &` runs synchronously and
            // is marked Done before control returns. True async background would
            // require spawning the command as a separate Cell via SpawnCap — not
            // in scope for G1. `fg`/`bg` built-ins report this limitation.
            let name = cmd
                .argv
                .first()
                .map(|word| word.text.as_str())
                .unwrap_or("?");
            let jid = jobs.add(name);
            // Background job notification always goes to console, not the sink.
            shell_stderrln(&alloc::format!("[{}] running", jid));
            // Signal spawn_external to skip sys_wait so a long-running external
            // cell (httpd) does not park the shell forever. Built-ins ignore it.
            state::set_bg_spawn(true);
            exec_cmd(cmd, &[], jobs);
            state::set_bg_spawn(false);
            jobs.set_state(jid, JobState::Done);
            0
        }
        Ast::Sequence(sub) => {
            let mut last = 0;
            for s in sub {
                last = execute(s, jobs);
            }
            last
        }
        Ast::Case { expr, arms } => {
            let value = match expand_token(expr) {
                Ok(value) => value,
                Err(failed) => return failed.0,
            };
            for (pattern, body) in arms {
                if case_matches(pattern, &value) {
                    execute(body, jobs);
                    break;
                }
            }
            0
        }
        Ast::FuncDef { name, body } => {
            define_function(name, body);
            0
        }
        Ast::And(left, right) => {
            let code = execute(left, jobs);
            if code == 0 {
                execute(right, jobs)
            } else {
                code
            }
        }
        Ast::Or(left, right) => {
            let code = execute(left, jobs);
            if code != 0 {
                execute(right, jobs)
            } else {
                code
            }
        }
        Ast::While { cond, body } => {
            loop {
                if execute(cond, jobs) != 0 {
                    break;
                }
                execute(body, jobs);
                match take_loop_signal() {
                    LoopSignal::Break => break,
                    LoopSignal::Continue => continue,
                    LoopSignal::None => {}
                }
            }
            0
        }
        Ast::For { var, words, body } => {
            'for_loop: for word in words {
                set_var(var, word);
                execute(body, jobs);
                match take_loop_signal() {
                    LoopSignal::Break => break 'for_loop,
                    LoopSignal::Continue => continue 'for_loop,
                    LoopSignal::None => {}
                }
            }
            0
        }
        Ast::If {
            cond,
            then_b,
            else_b,
        } => {
            let code = execute(cond, jobs);
            if code == 0 {
                execute(then_b, jobs)
            } else if let Some(eb) = else_b {
                execute(eb, jobs)
            } else {
                0
            }
        }
    }
}

/// Refuse a command line before it has any effect: one stderr diagnostic, status 2.
fn reject_line(reason: &str) -> i32 {
    shell_stderr("shell: ");
    shell_stderrln(reason);
    state::set_status(2);
    2
}

/// Expand a command's words and redirect targets for whole-pipeline admission.
/// Expansion has no effect (substitutions are read-only), so it may run before
/// admission; a failure aborts the command with the substitution's status like
/// `exec_cmd` does.
fn stage_input(cmd: &Cmd) -> Result<StageIn, SubstFailed> {
    let argv = cmd
        .argv
        .iter()
        .map(expand_word)
        .collect::<Result<Vec<_>, _>>()?;
    let mut redirects = Vec::with_capacity(cmd.redirects.len());
    for redirect in &cmd.redirects {
        let (kind, word) = match redirect {
            Redirect::StdoutTo(word) => (RedirKind::Out, word),
            Redirect::StdoutAppend(word) => (RedirKind::Append, word),
            Redirect::StdinFrom(word) => (RedirKind::In, word),
            Redirect::StderrTo(word) => (RedirKind::Err, word),
        };
        redirects.push(Redir { kind, path: expand_word(word)? });
    }
    Ok(StageIn { argv, redirects })
}

/// The in-shell explicit form (S10-0.6) of an expanded argv, in process and without a
/// spawn: `Ok` with a Run name's `cellbox`/`/bin/cellbox` prefix dropped (any other argv
/// unchanged), or `Err` with the one-shot's answer (`text` on stdout or stderr, status).
fn explicit_form(mut argv: Vec<String>) -> Result<Vec<String>, (i32, bool, String)> {
    match crate::oneshot::prefixed(&argv) {
        crate::oneshot::Prefixed::No => Ok(argv),
        crate::oneshot::Prefixed::Strip => {
            argv.remove(0);
            Ok(argv)
        }
        crate::oneshot::Prefixed::Answer { status, stdout, text } => Err((status, stdout, text)),
    }
}

/// [`explicit_form`] for a command about to be admitted or dispatched: an answer is
/// written and recorded as `$?` before any effect, and returned as `Err(status)`.
fn explicit_argv(argv: Vec<String>) -> Result<Vec<String>, i32> {
    explicit_form(argv).map_err(|(status, stdout, text)| {
        if stdout {
            shell_print(&text);
        } else {
            shell_stderr(&text);
        }
        state::set_status(status);
        status
    })
}

/// [`explicit_argv`] on an expanded stage; its redirects are kept.
fn explicit_stage(stage: StageIn) -> Result<StageIn, i32> {
    let StageIn { argv, redirects } = stage;
    Ok(StageIn { argv: explicit_argv(argv)?, redirects })
}

/// One simple command. Only commands that can be typed stages or interactive
/// sessions go through whole-command admission, plus an external whose stdout is
/// redirected (a one-stage byte pipeline: its stdout is never the terminal session);
/// everything else is unchanged.
fn exec_simple(cmd: &Cmd, jobs: &mut Jobs) -> i32 {
    if let Some(first) = cmd.argv.first() {
        let args: Vec<&str> = cmd.argv[1..].iter().map(|word| word.text.as_str()).collect();
        let redirected_external = serial_active()
            && cmd
                .redirects
                .iter()
                .any(|redirect| matches!(redirect, Redirect::StdoutTo(_) | Redirect::StdoutAppend(_)))
            && (matches!(
                typed::classify_command(core::slice::from_ref(&first.text)),
                typed::CmdClass::External
            ) || (first.text == "ssh"
                // A publickey exec is an external byte stage: with its stdout redirected it is
                // a one-stage pipeline (every other ssh keeps the foreground launch).
                && typed::is_ssh_stage(&cmd.argv.iter().map(|word| word.text.clone()).collect::<Vec<String>>())));
        if typed::might_be_typed(&first.text, &args) || redirected_external {
            let stage = match stage_input(cmd) {
                Ok(stage) => stage,
                Err(failed) => return failed.abort(),
            };
            let stage = match explicit_stage(stage) {
                Ok(stage) => stage,
                Err(code) => return code,
            };
            match typed::admit(core::slice::from_ref(&stage), serial_active()) {
                Admission::Legacy => {}
                Admission::Reject(reason) => return reject_line(&reason),
                Admission::Typed(plan) => {
                    return exec_typed(&plan, core::slice::from_ref(cmd), core::slice::from_ref(&stage), jobs)
                }
                Admission::Bytes(plan) => {
                    return exec_bytes(&plan, core::slice::from_ref(cmd), core::slice::from_ref(&stage), jobs)
                }
            }
        }
    }
    exec_cmd(cmd, &[], jobs)
}

/// Execute a multi-stage pipeline.
///
/// The whole pipeline is expanded and admitted first; a refusal (unsupported
/// category combination, stateful built-in, session, >16 stages, bad redirect)
/// returns status 2 with no stage run and no file opened. Admitted byte-only
/// pipelines keep the capture composition; typed pipelines run as one job.
fn exec_pipeline(cmds: &[Cmd], jobs: &mut Jobs) -> i32 {
    if cmds.is_empty() {
        return 0;
    }
    let mut stages = Vec::with_capacity(cmds.len());
    for cmd in cmds {
        let stage = match stage_input(cmd) {
            Ok(stage) => stage,
            Err(failed) => return failed.abort(),
        };
        match explicit_stage(stage) {
            Ok(stage) => stages.push(stage),
            Err(code) => return code,
        }
    }
    match typed::admit(&stages, serial_active()) {
        Admission::Reject(reason) => reject_line(&reason),
        Admission::Legacy => {
            let mut statuses = Vec::new();
            run_byte_chain(cmds, Vec::new(), jobs, &mut statuses)
        }
        Admission::Typed(plan) => exec_typed(&plan, cmds, &stages, jobs),
        Admission::Bytes(plan) => exec_bytes(&plan, cmds, &stages, jobs),
    }
}

/// Run byte stages in order, piping each stage's output into the next stage's
/// stdin. Intermediate stages are captured into `Vec<u8>` buffers; the final
/// stage runs directly through the current sink so its exit code is preserved
/// and any outer capture (nested pipeline or `$(...)`) captures it correctly.
/// `statuses` receives one status per stage run. `cmds` must be non-empty.
fn run_byte_chain(cmds: &[Cmd], mut stdin_data: Vec<u8>, jobs: &mut Jobs, statuses: &mut Vec<i32>) -> i32 {
    let last_idx = cmds.len() - 1;
    for (i, cmd) in cmds.iter().enumerate() {
        if i == last_idx {
            // Last stage: run directly (no intermediate capture).
            // Wire pipe stdin so built-ins without a file path read from it.
            state::set_stdin(&stdin_data);
            let code = exec_cmd(cmd, &stdin_data, jobs);
            state::clear_stdin();
            statuses.push(code);
            return code;
        }
        let (captured, code) = capture_cmd(cmd, &stdin_data, jobs);
        statuses.push(code);
        stdin_data = captured;
    }
    0
}

/// Run a command and capture its output into a `Vec<u8>` together with its status.
///
/// Pushes a capture so that any built-in calling `shell_print` writes into it
/// instead of the serial console. The guard pops it on every exit path, which is
/// what makes an outer capture (a nested pipeline, or `$(...)`) resume correctly.
fn capture_cmd(cmd: &Cmd, stdin: &[u8], jobs: &mut Jobs) -> (Vec<u8>, i32) {
    let guard = CaptureGuard::new();
    let code = exec_cmd(cmd, stdin, jobs);
    (guard.finish(), code)
}

/// Record a failure against pipeline member `stage` (clamped) and return `status`.
fn fail_member(members: &mut [(String, i32)], stage: usize, status: i32) -> i32 {
    if let Some(last) = members.len().checked_sub(1) {
        members[stage.min(last)].1 = status;
    }
    status
}

/// How the local (non-external) typed segment delivered its output.
enum LocalOutcome {
    /// Written to the terminal while rows were pulled.
    Streamed(Result<typed::StreamEnd, typed::RunFailure>),
    /// Collected for a redirect, a byte consumer or an enclosing capture.
    Collected(Result<Vec<u8>, typed::RunFailure>),
}

/// Look at the session event queue without blocking: true when Ctrl-C was seen.
/// Everything else typed meanwhile is discarded (scrubbed). An input service
/// failure clears `enabled`, after which nothing is polled.
fn poll_session_interrupt(enabled: &mut bool) -> bool {
    if !*enabled {
        return false;
    }
    for _ in 0..16 {
        let event = {
            let mut slot = SERIAL_SESSION.lock();
            let Some(session) = slot.as_mut() else {
                *enabled = false;
                return false;
            };
            match session.next_event(0) {
                Ok(event) => ScrubbedEvent(event),
                Err(Status::WouldBlock | Status::TimedOut) => return false,
                Err(_) => {
                    *enabled = false;
                    return false;
                }
            }
        };
        if matches!(event.0.kind, EventKind::Interrupt) {
            return true;
        }
    }
    false
}

/// The interrupt latch of one job (D74-1): every poller of the session queue —
/// the streamed terminal sink, each external segment's pump and a working
/// producer (`rg`) — reads Ctrl-C through it, so an interrupt one of them
/// consumed is never lost to the others.
pub(crate) fn job_latch() -> Rc<InterruptLatch> {
    let mut enabled = true;
    InterruptLatch::new(Box::new(move || poll_session_interrupt(&mut enabled)))
}

/// A one-shot's stdout as the sink of a streamed typed segment (S10-0.5): each slice is
/// written with the finite one-shot deadline. A failed write (reader closed, revoked,
/// deadline) stops the producer; there is no terminal, so nothing interrupts.
struct OneShotSink;

impl typed::ChunkSink for OneShotSink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), typed::SinkStop> {
        state::oneshot_stdout(bytes)
            .map_err(|status| typed::SinkStop::Failed(alloc::format!("stdout failed: {:?}", status)))
    }

    fn interrupted(&mut self) -> bool {
        false
    }
}

/// The terminal as the sink of a streamed typed segment: bytes reach the session
/// stdout as they are produced (`terminal_write_while` retries backpressure with
/// the same backoff as every other writer), and a real Ctrl-C is polled (through
/// the job latch) between rows and whenever the terminal is not accepting bytes.
struct TerminalSink {
    latch: Rc<InterruptLatch>,
}

impl typed::ChunkSink for TerminalSink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), typed::SinkStop> {
        if self.latch.is_set() {
            return Err(typed::SinkStop::Interrupted);
        }
        terminal_write_while(bytes, false, || {
            if self.latch.poll_now() {
                Err(TerminalWriteStop::Interrupted)
            } else {
                Ok(())
            }
        })
        .map_err(|stop| match stop {
            TerminalWriteStop::Interrupted => typed::SinkStop::Interrupted,
            TerminalWriteStop::Output(status) => {
                typed::SinkStop::Failed(alloc::format!("terminal output failed: {:?}", status))
            }
        })
    }

    fn interrupted(&mut self) -> bool {
        self.latch.poll_now()
    }
}

/// Output already printed by earlier commands of this line sits in the REPL's own
/// capture until the line ends. Streamed rows bypass that capture, so it goes to
/// the terminal first to keep the output in command order.
pub fn flush_repl_capture() {
    let mut earlier = state::take_capture();
    if earlier.is_empty() {
        return;
    }
    if terminal_stdout(&earlier).is_err() {
        let _ = syscall::sys_log("shell: terminal output delivery failed\n");
    }
    ostd::session::zeroize_vec(&mut earlier);
}

/// Run an admitted typed pipeline as ONE job: one id, one cancellation root (the
/// shell task, or the pipeline Operation when external stages take part), one
/// whole-job status and per-stage member statuses.
fn exec_typed(plan: &TypedPlan, cmds: &[Cmd], stages: &[StageIn], jobs: &mut Jobs) -> i32 {
    let names: Vec<String> = stages.iter().map(|stage| stage.argv[0].clone()).collect();
    let job = jobs.add(&names.join(" | "));
    let mut members: Vec<(String, i32)> = names.into_iter().map(|name| (name, 0)).collect();
    let status = run_typed(plan, cmds, stages, jobs, &mut members);
    jobs.complete(job, status, members);
    state::set_status(status);
    status
}

/// Run an admitted byte pipeline that ends in external commands as ONE job.
fn exec_bytes(plan: &BytePlan, cmds: &[Cmd], stages: &[StageIn], jobs: &mut Jobs) -> i32 {
    let names: Vec<String> = stages.iter().map(|stage| stage.argv[0].clone()).collect();
    let job = jobs.add(&names.join(" | "));
    let mut members: Vec<(String, i32)> = names.into_iter().map(|name| (name, 0)).collect();
    let status = run_bytes(plan, cmds, stages, jobs, &mut members);
    jobs.complete(job, status, members);
    state::set_status(status);
    status
}

/// Bytes entering the external stages of a byte pipeline, then the stages
/// themselves. The externals are started first, so an unknown program or an
/// oversized launch fails before a built-in head runs or a file is read.
fn run_bytes(
    plan: &BytePlan,
    cmds: &[Cmd],
    stages: &[StageIn],
    jobs: &mut Jobs,
    members: &mut [(String, i32)],
) -> i32 {
    let first = plan.prefix;
    let mut run = match ExtRun::start(&stages[first..], first > 0, &job_latch()) {
        Ok(run) => run,
        Err(failed) => return fail_member(members, first + failed.stage, failed.status),
    };
    let feed = match &plan.head {
        ByteHead::None => Vec::new(),
        ByteHead::Cat(path) => match crate::native_rows::read_file_bytes_exact(path) {
            Ok(bytes) if bytes.len() <= typed::MAX_SINK_BYTES => bytes,
            Ok(_) => {
                shell_stderrln("shell: input exceeds the typed pipeline byte bound");
                return fail_member(members, 0, 1);
            }
            Err(code) => return fail_member(members, 0, code),
        },
        ByteHead::Capture => {
            // Built-in producers run over captured bytes; nothing reaches the terminal.
            let guard = CaptureGuard::new();
            let mut statuses = Vec::new();
            run_byte_chain(&cmds[..first], Vec::new(), jobs, &mut statuses);
            let bytes = guard.finish();
            for (index, status) in statuses.iter().enumerate() {
                if let Some(member) = members.get_mut(index) {
                    member.1 = *status;
                    if *status != 0 {
                        shell_stderrln(&alloc::format!("shell: {} exited with status {}", member.0, status));
                    }
                }
            }
            if bytes.len() > typed::MAX_SINK_BYTES {
                shell_stderrln("shell: input exceeds the typed pipeline byte bound");
                return fail_member(members, first.saturating_sub(1), 1);
            }
            bytes
        }
    };
    let driven = drive_external_sink(&mut run, feed, plan.output.as_ref());
    settle_external_segment(run, driven, members, first, plan.output.as_ref())
}

/// Bytes feeding the leading `from` of a byte-headed typed pipeline, plus the
/// index of that `from` stage.
fn byte_head(plan: &TypedPlan, cmds: &[Cmd], jobs: &mut Jobs) -> Result<(Vec<u8>, usize), i32> {
    match &plan.head {
        Head::Cat(path) => crate::native_rows::read_file_bytes_exact(path).map(|bytes| (bytes, 1)),
        Head::FromFile(path) => crate::native_rows::read_file_bytes_exact(path).map(|bytes| (bytes, 0)),
        Head::Capture => {
            let (bytes, code) = capture_cmd(&cmds[0], &[], jobs);
            if code != 0 {
                shell_stderrln(&alloc::format!("shell: stage 0 exited with status {}", code));
                Err(code)
            } else {
                Ok((bytes, 1))
            }
        }
        // A one-shot's deferred stdin (`/bin/cellbox rg … -`), read on this demand.
        Head::Stdin => Ok((state::stdin_bytes(), 0)),
        Head::Producer | Head::External { .. } => Err(2),
    }
}

fn run_typed(
    plan: &TypedPlan,
    cmds: &[Cmd],
    stages: &[StageIn],
    jobs: &mut Jobs,
    members: &mut [(String, i32)],
) -> i32 {
    use text_engine::pipeline::{RowSource, Stage};
    // One interrupt latch for the whole job: the terminal sink, every external
    // segment and a working producer read Ctrl-C through it.
    let latch = job_latch();
    // External stages are started before any stage computes, so an unknown program
    // fails before the typed segment (or a capturing head) has run. Dropping an
    // `ExtRun` on any early return cancels and reaps what was started.
    let mut tail = if plan.tail_externals > 0 {
        match ExtRun::start(&stages[plan.typed_cmds..], true, &latch) {
            Ok(run) => Some(run),
            Err(failed) => return fail_member(members, plan.typed_cmds + failed.stage, failed.status),
        }
    } else {
        None
    };
    let mut head_run: Option<Rc<core::cell::RefCell<ExtRun>>> = None;
    // `rg -` consumes the head's bytes itself (no `from`).
    let search_stdin = plan.search.as_ref().filter(|args| args.stdin);
    // Head: a native row producer, an external segment, or bytes converted by the
    // leading `from` (or searched by `rg -`). Construction validates arguments only;
    // producers do no work until pulled.
    let (source, first_stage, rest): (Box<dyn RowSource>, usize, &[Stage]) = match &plan.head {
        Head::Producer => match crate::native_rows::row_producer(&plan.producer_argv, &latch) {
            Some(Ok(source)) => (source, 0, &plan.stages[..]),
            Some(Err(code)) => return fail_member(members, 0, code),
            None => {
                shell_stderrln("shell: record producer is not available");
                return fail_member(members, 0, 2);
            }
        },
        Head::External { count } => {
            let format = match (search_stdin, plan.stages.first()) {
                (Some(_), _) => None,
                (None, Some(Stage::From(format))) => Some(format.clone()),
                (None, _) => {
                    shell_stderrln("shell: external output needs `from json|jsonl`");
                    return fail_member(members, *count, 2);
                }
            };
            let run = match ExtRun::start(&stages[..*count], false, &latch) {
                Ok(run) => run,
                Err(failed) => return fail_member(members, failed.stage, failed.status),
            };
            let shared = Rc::new(core::cell::RefCell::new(run));
            head_run = Some(shared.clone());
            match (search_stdin, format) {
                (Some(args), _) => {
                    let input = crate::rg::StdinInput::Stream(Box::new(ChildBytes(shared)));
                    match crate::rg::stdin_producer(args, input, &latch) {
                        Ok(source) => (source, *count, &plan.stages[..]),
                        Err(code) => return fail_member(members, *count, code),
                    }
                }
                (None, format) => {
                    let format = format.unwrap_or(text_engine::pipeline::Format::JsonlTyped);
                    let rows: Box<dyn RowSource> = Box::new(typed::StreamRows::new(ChildBytes(shared), format));
                    (rows, *count, &plan.stages[1..])
                }
            }
        }
        Head::Cat(_) | Head::Capture | Head::FromFile(_) | Head::Stdin => {
            let (bytes, from_index) = match byte_head(plan, cmds, jobs) {
                Ok(head) => head,
                Err(code) => return fail_member(members, 0, code),
            };
            if bytes.len() > typed::MAX_SINK_BYTES {
                shell_stderrln("shell: input exceeds the typed pipeline byte bound");
                return fail_member(members, from_index, 1);
            }
            if let Some(args) = search_stdin {
                match crate::rg::stdin_producer(args, crate::rg::StdinInput::Bytes(bytes), &latch) {
                    Ok(source) => (source, from_index, &plan.stages[..]),
                    Err(code) => return fail_member(members, from_index, code),
                }
            } else {
                let Some(Stage::From(format)) = plan.stages.first() else {
                    shell_stderrln("shell: byte input needs `from json|jsonl`");
                    return fail_member(members, from_index, 2);
                };
                // Malformed input fails here, before any redirect is opened or written.
                match typed::rows_from_bytes(format, &bytes) {
                    Ok(source) => (source, from_index, &plan.stages[1..]),
                    Err(error) => {
                        shell_stderrln(&alloc::format!("shell: {}: {}", members[from_index].0, error));
                        return fail_member(members, from_index, 1);
                    }
                }
            }
        }
    };
    // The producer's completion status, published when the chain releases it.
    let (source, completion) = typed::observe_completion(source);
    let search = plan.search.as_ref().map(|_| SearchMember { index: plan.search_index, completion });

    // Rows feeding external commands: serialized into the first stage's stdin as it
    // consumes them (bounded memory, consumer-driven), never collected.
    if let Some(mut run) = tail.take() {
        let feed_sink = if plan.search_text && plan.text_display { DefaultSink::SearchText } else { DefaultSink::TypedJsonl };
        let feed = match typed::RowFeed::new(source, plan.category, first_stage, rest, feed_sink) {
            Ok(feed) => feed,
            Err(failure) => {
                let name = members.get(failure.stage).map_or("pipeline", |member| member.0.as_str());
                shell_stderrln(&alloc::format!("shell: {}: {}", name, failure.message));
                return fail_member(members, failure.stage, failure.status);
            }
        };
        run.set_rows(feed);
        let driven = drive_external_sink(&mut run, Vec::new(), plan.tail_output.as_ref());
        if let Some(failure) = run.take_row_failure() {
            // A row stage failed after some rows were already delivered: stop the
            // external stages (the failed feed is not a clean end), then report it.
            let statuses = run.conclude();
            for (offset, status) in statuses.iter().enumerate() {
                if let Some(member) = members.get_mut(plan.typed_cmds + offset) {
                    member.1 = *status;
                }
            }
            drop(run);
            if let Some(search) = &search {
                search.record(members, &latch, false);
            }
            let name = members.get(failure.stage).map_or("pipeline", |member| member.0.as_str());
            shell_stderrln(&alloc::format!("shell: {}: {}", name, failure.message));
            return fail_member(members, failure.stage, failure.status);
        }
        // The feed (and the producer) is released with the run, before its status is read.
        let status = settle_external_segment(run, driven, members, plan.typed_cmds, plan.tail_output.as_ref());
        if let Some(search) = &search {
            search.record(members, &latch, status != INTERRUPTED_STATUS);
        }
        return status;
    }

    let terminal = plan.output.is_none() && plan.typed_cmds == cmds.len();
    let oneshot = state::oneshot_active();
    let default_sink = typed::default_sink(terminal && !oneshot, plan);
    // The live terminal is a streaming sink: each encoded row is written as it is
    // pulled and Ctrl-C is polled between rows, so an endless producer is neither
    // buffered nor uninterruptible. A one-shot's stdout streams the same way (S10-0.5):
    // a closed reader stops the producer at once. Only a redirect target, a byte
    // consumer or an enclosing capture (`$(...)`) need the whole, bounded buffer.
    let outcome = if terminal && oneshot && state::capture_depth() == 0 {
        LocalOutcome::Streamed(typed::stream_rows(source, plan.category, first_stage, rest, default_sink, &mut OneShotSink))
    } else if terminal && serial_active() && state::capture_depth() <= 1 {
        flush_repl_capture();
        let mut sink = TerminalSink { latch: latch.clone() };
        LocalOutcome::Streamed(typed::stream_rows(source, plan.category, first_stage, rest, default_sink, &mut sink))
    } else {
        // A Collected run polls Ctrl-C before every pull (S10-3b), so an endless
        // producer feeding a byte consumer (`| cat`) stops with 130 and writes nothing.
        let bound = if plan.search.is_some() { typed::SEARCH_COLLECT_BOUND } else { typed::COLLECT_BOUND };
        LocalOutcome::Collected(typed::run_rows_within(source, plan.category, first_stage, rest, default_sink, bound, &mut || {
            latch.poll_now()
        }))
    };

    // An external head is reaped whatever the typed segment did: a satisfied `take`
    // or an error already closed its stream, so the stages are cancelled here.
    if let Some(shared) = head_run {
        let mut run = shared.borrow_mut();
        let statuses = run.conclude();
        let cancelled = run.was_cancelled();
        let not_started = run.not_started();
        drop(run);
        for (index, status) in statuses.iter().enumerate() {
            if let Some(member) = members.get_mut(index) {
                member.1 = *status;
                // A stage that never started ran nothing and reports no exit.
                if *status != 0 && !cancelled && index >= not_started && !latch.is_set() {
                    shell_stderrln(&alloc::format!("shell: {} exited with status {}", member.0, status));
                }
            }
        }
    }
    // The search member's status; reported as an earlier failure unless it is the
    // pipeline status itself (D76-8, D76-15).
    let search_status = search.as_ref().map(|search| search.record(members, &latch, !plan.search_status));
    // Ctrl-C ends the job with 130 whoever saw it; a Collected run writes nothing.
    if latch.is_set() {
        shell_console("^C\n");
        return INTERRUPTED_STATUS;
    }

    let bytes = match outcome {
        LocalOutcome::Streamed(Ok(end)) => {
            if end == typed::StreamEnd::Interrupted {
                shell_console("^C\n");
                return end.status();
            }
            return match search_status {
                Some(status) if plan.search_status => status,
                _ => end.status(),
            };
        }
        LocalOutcome::Collected(Ok(bytes)) => bytes,
        LocalOutcome::Streamed(Err(failure)) | LocalOutcome::Collected(Err(failure)) => {
            let name = members.get(failure.stage).map_or("pipeline", |member| member.0.as_str());
            shell_stderrln(&alloc::format!("shell: {}: {}", name, failure.message));
            return fail_member(members, failure.stage, failure.status);
        }
    };
    let success = match search_status {
        Some(status) if plan.search_status => status,
        _ => 0,
    };

    // `to` (or `rg`'s default sink) fed byte-consuming built-ins: they run over the
    // captured bytes.
    if plan.typed_cmds < cmds.len() {
        let mut statuses = Vec::new();
        let code = run_byte_chain(&cmds[plan.typed_cmds..], bytes, jobs, &mut statuses);
        for (offset, status) in statuses.iter().enumerate() {
            let index = plan.typed_cmds + offset;
            if let Some(member) = members.get_mut(index) {
                member.1 = *status;
                if *status != 0 && offset + 1 < statuses.len() {
                    shell_stderrln(&alloc::format!("shell: {} exited with status {}", member.0, status));
                }
            }
        }
        return code;
    }
    if let Some((path, append)) = &plan.output {
        if !crate::cmd_fs::vfs_write_chunked(path, &bytes, *append) {
            shell_stderr("shell: cannot write '");
            shell_stderr(path);
            shell_stderrln("'");
            return fail_member(members, usize::MAX, 1);
        }
        return success;
    }
    // Terminal display of rows/serializer output; redirects above stay byte-exact.
    shell_print(&String::from_utf8_lossy(&bytes));
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        shell_print("\n");
    }
    success
}

/// The `rg` stage of a typed job: its pipeline index and completion slot.
struct SearchMember {
    index: usize,
    completion: typed::CompletionSlot,
}

impl SearchMember {
    /// Store the search's status in its member (130 after Ctrl-C) and return it.
    /// With `report`, a nonzero status that is not the pipeline status is listed
    /// like any other failed non-final stage (`NAME exited with status N`).
    fn record(&self, members: &mut [(String, i32)], latch: &InterruptLatch, report: bool) -> i32 {
        let status = match self.completion.get() {
            Some(status) if !latch.is_set() => status,
            _ if latch.is_set() => INTERRUPTED_STATUS,
            _ => 0,
        };
        if let Some(member) = members.get_mut(self.index) {
            member.1 = status;
            if report && status != 0 && !latch.is_set() {
                shell_stderrln(&alloc::format!("shell: {} exited with status {}", member.0, status));
            }
        }
        status
    }
}

/// Execute one simple command.
///
/// Handles redirection, built-in dispatch, and external binary spawn.
fn exec_cmd(cmd: &Cmd, _stdin: &[u8], jobs: &mut Jobs) -> i32 {
    if cmd.is_empty() {
        return 0;
    }

    // Failed command substitution aborts before dispatch with a nonzero status.
    let expanded = match cmd
        .argv
        .iter()
        .map(expand_word)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(expanded) => expanded,
        Err(failed) => return failed.abort(),
    };
    // `cellbox CMD` / `/bin/cellbox CMD` (S10-0.6): the same table as the one-shot.
    let expanded = match explicit_argv(expanded) {
        Ok(expanded) => expanded,
        Err(code) => return code,
    };
    let prog: &str = &expanded[0];
    let args: Vec<String> = expanded[1..].to_vec();

    // Detect `KEY=VALUE` assignment (key is non-empty alphanumeric+underscore).
    if args.is_empty() {
        if let Some(eq) = prog.find('=') {
            let key = &prog[..eq];
            if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                set_var(key, &prog[eq + 1..]);
                return 0;
            }
        }
    }

    // echo with stdout redirect: fast path using cmd_echo_to_vec (no OutputSink needed).
    if prog == "echo" {
        if let Some(Redirect::StdoutTo(path)) = cmd
            .redirects
            .iter()
            .find(|r| matches!(r, Redirect::StdoutTo(_)))
        {
            let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            let bytes = crate::commands::cmd_echo_to_vec(&arg_refs);
            let path = match expand_word(path) {
                Ok(path) => path,
                Err(failed) => return failed.abort(),
            };
            let code = if crate::cmd_fs::write_file(&path, &bytes) {
                0
            } else {
                shell_stderr("echo: cannot write '");
                shell_stderr(&path);
                shell_stderrln("'");
                1
            };
            state::set_status(code);
            return code;
        }
        if let Some(Redirect::StdoutAppend(path)) = cmd
            .redirects
            .iter()
            .find(|r| matches!(r, Redirect::StdoutAppend(_)))
        {
            let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            let bytes = crate::commands::cmd_echo_to_vec(&arg_refs);
            let path = match expand_word(path) {
                Ok(path) => path,
                Err(failed) => return failed.abort(),
            };
            let code = if crate::cmd_fs::append_file(&path, &bytes) {
                0
            } else {
                shell_stderr("echo: cannot append '");
                shell_stderr(&path);
                shell_stderrln("'");
                1
            };
            state::set_status(code);
            return code;
        }
    }

    // StdinFrom redirect: preload the file into a buffer and expose it via
    // shell_stdin() so built-ins (grep, wc, …) can read from it.
    let stdin_file_buf: Vec<u8>;
    let effective_stdin: &[u8] = if let Some(Redirect::StdinFrom(path)) = cmd
        .redirects
        .iter()
        .find(|r| matches!(r, Redirect::StdinFrom(_)))
    {
        let path = match expand_word(path) {
            Ok(path) => path,
            Err(failed) => return failed.abort(),
        };
        stdin_file_buf = match crate::cmd_fs::read_file_vfs_owned(&path, 4096) {
            Ok(bytes) => bytes,
            Err(_) => {
                shell_stderr("shell: cannot open '");
                shell_stderr(&path);
                shell_stderrln("'");
                return 1;
            }
        };
        &stdin_file_buf
    } else {
        _stdin
    };

    // Detect stdout/stderr redirect for non-echo commands.
    //
    // ViCell has one output channel (serial console). `2>file` is therefore
    // semantically equivalent to `>file` — both capture `shell_print` output.
    // When both `>` and `2>` are present, `>` takes precedence; `2>` is a
    // documented no-op in that case (single-channel limitation).
    let stdout_redir = cmd
        .redirects
        .iter()
        .find_map(|redirect| match redirect {
            Redirect::StdoutTo(path) => Some((path, false)),
            Redirect::StdoutAppend(path) => Some((path, true)),
            _ => None,
        })
        .or_else(|| {
            cmd.redirects.iter().find_map(|redirect| match redirect {
                // Fallback: StderrTo reuses the stdout-capture path (one-channel shell).
                Redirect::StderrTo(path) => Some((path, false)),
                _ => None,
            })
        });
    let stdout_redir = match stdout_redir {
        Some((path, append)) => match expand_word(path) {
            Ok(path) => Some((path, append)),
            Err(failed) => return failed.abort(),
        },
        None => None,
    };

    // Wire the pipe-fed stdin so pipe-aware built-ins can read it.
    state::set_stdin(effective_stdin);

    let code = if let Some((path, append)) = stdout_redir {
        // Capture this command's output into a buffer, then write to VFS.
        let captured: Vec<u8>;
        let status;
        {
            let guard = CaptureGuard::new();
            status = dispatch_builtin(prog, &args, jobs);
            captured = guard.finish();
        } // capture popped here, before the VFS write
        let write_ok = crate::cmd_fs::vfs_write_chunked(&path, &captured, append);
        if status == 0 && !write_ok {
            1
        } else {
            status
        }
    } else {
        dispatch_builtin(prog, &args, jobs)
    };

    // Clear pipe stdin; leave the capture stack alone (exec_cmd does not own it).
    state::clear_stdin();

    state::set_status(code);
    code
}

/// Match a case pattern against a value.
///
/// `*` is a catch-all; everything else is exact string equality.
fn case_matches(pattern: &str, value: &str) -> bool {
    pattern == "*" || pattern == value
}

/// Convert a small positional-arg index (1-9) to an owned `String` key.
///
/// Avoids `i32_to_str` which writes to a single shared static buffer —
/// calling it twice invalidates the first result while the second is alive.
fn usize_key(n: usize) -> String {
    let digit = b'0' + (n as u8).min(9);
    // SAFETY: `digit` is always a valid ASCII byte.
    String::from(digit as char) // `digit` is b'0'..=b'9', so this is ASCII
}

/// Convert a small non-negative integer to a &str backed by a fixed buffer.
///
/// Returns "0" for 0, the decimal string for 1-127, and "1" for anything else.
/// Used only for the bounded positional-argument count, never command statuses.
fn i32_to_str(n: i32) -> &'static str {
    // Use a 'static lookup table for the most common exit codes (0–9).
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "6",
        7 => "7",
        8 => "8",
        9 => "9",
        127 => "127",
        _ => "1",
    }
}

/// One catalog command's implementation: a legacy `ViResult` helper (Ok → 0, Err → 1)
/// or a command that returns its own status.
#[derive(Clone, Copy)]
enum Catalog {
    Legacy(for<'a> fn(crate::text_engine::args::LegacyArgs<'a>) -> ViResult<()>),
    Status(fn(&[String]) -> i32),
}

/// The closed catalog table (S10-0.4): every `oneshot::Class::Run` name except `rg`,
/// which is always a typed producer and runs through admission. Shell-state, session,
/// launch and unknown names have no entry, so nothing reached through this table can
/// launch a Cell.
fn catalog_handler(prog: &str) -> Option<Catalog> {
    use Catalog::{Legacy, Status};
    Some(match prog {
        "ls" => Legacy(crate::commands::cmd_ls),
        "cat" => Legacy(crate::commands::cmd_cat),
        "wc" => Legacy(crate::cmd_fs::cmd_wc),
        "head" => Legacy(crate::cmd_fs::cmd_head),
        "tail" => Legacy(crate::cmd_fs::cmd_tail),
        "find" => Legacy(crate::cmd_fs::cmd_find),
        "uniq" => Legacy(crate::cmd_fs::cmd_uniq),
        "sort" => Legacy(crate::cmd_fs::cmd_sort),
        "tee" => Legacy(crate::cmd_fs::cmd_tee),
        "mkdir" => Legacy(crate::cmd_fs::cmd_mkdir),
        "rmdir" => Legacy(crate::cmd_fs::cmd_rmdir),
        "rm" => Legacy(crate::cmd_fs::cmd_rm),
        "touch" => Legacy(crate::cmd_fs::cmd_touch),
        "mv" => Legacy(crate::cmd_fs::cmd_mv),
        "cp" => Legacy(crate::cmd_fs::cmd_cp),
        "vcat" => Legacy(crate::cmd_fs::cmd_vcat),
        "vwrite" => Legacy(crate::cmd_fs::cmd_vwrite),
        "vappend" => Legacy(crate::cmd_fs::cmd_vappend),
        "pwd" => Legacy(crate::cmd_sys::cmd_pwd),
        "uname" => Legacy(crate::cmd_sys::cmd_uname),
        "free" => Legacy(crate::cmd_sys::cmd_free),
        "env" => Legacy(crate::cmd_sys::cmd_env),
        "uptime" => Legacy(crate::cmd_sys::cmd_uptime),
        "sleep" => Status(crate::cmd_sys::cmd_sleep),
        "blktest" => Legacy(crate::cmd_sys::cmd_blkio_test),
        "echo" => Legacy(crate::commands::cmd_echo),
        "ifconfig" | "ip" => Legacy(crate::cmd_sys::cmd_ifconfig),
        "date" => Legacy(crate::cmd_sys::cmd_date),
        "top" => Status(crate::commands::cmd_top),
        "edit" => Status(crate::edit::cmd_edit),
        "ps" => Status(crate::commands::cmd_ps),
        "grep" => Status(cmd_grep_args),
        "sed" => Status(cmd_sed_args),
        "awk" => Status(cmd_awk_args),
        "kill" => Status(|_| crate::commands::cmd_kill()),
        "snapshot" => Status(|_| crate::commands::cmd_snapshot()),
        "shutdown" => Status(|_| crate::cmd_sys::cmd_shutdown()),
        "help" => Status(|_| crate::commands::cmd_help().map(|_| 0).unwrap_or(1)),
        "test" => Status(|args| {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            cmd_test(&refs).map(|_| 0).unwrap_or(1)
        }),
        "[" => Status(|args| {
            let refs: Vec<&str> = args.iter().map(String::as_str).filter(|arg| *arg != "]").collect();
            cmd_test(&refs).map(|_| 0).unwrap_or(1)
        }),
        _ => return None,
    })
}

/// Run `prog` through the closed catalog table: `None` for any name outside it. Never
/// reaches `spawn_external`.
fn dispatch_catalog(prog: &str, args: &[String]) -> Option<i32> {
    catalog_handler(prog).map(|handler| match handler {
        Catalog::Legacy(run) => match with_legacy_parts(args, run) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Catalog::Status(run) => run(args),
    })
}

/// Run one `/bin/cellbox <command>` (S10-0.4/0.5): `argv[0]` is a `oneshot::Class::Run`
/// name and every word is literal (no expansion, globbing, redirection or `|`). Typed
/// producers (`ps|ls|top --records`, `rg`) stream typed JSONL to the one-shot stdout
/// (`--text`: the plain renderer); everything else runs through the closed catalog
/// table. Nothing here launches a Cell.
#[cfg(all(not(test), not(feature = "shell_test")))]
pub fn run_catalog_command(argv: &[String]) -> i32 {
    let stage = StageIn { argv: argv.to_vec(), redirects: Vec::new() };
    let status = match typed::admit_oneshot(&stage) {
        Admission::Typed(plan) => {
            let mut jobs = Jobs::new();
            let cmd = literal_cmd(argv);
            exec_typed(&plan, core::slice::from_ref(&cmd), core::slice::from_ref(&stage), &mut jobs)
        }
        Admission::Reject(reason) => reject_line(&reason),
        Admission::Legacy => dispatch_catalog(&argv[0], &argv[1..]).unwrap_or(crate::oneshot::UNKNOWN_STATUS),
        // Admission without external stages never plans a byte pipeline.
        Admission::Bytes(_) => crate::oneshot::REFUSED_STATUS,
    };
    state::set_status(status);
    status
}

/// A command whose words are all literal (single-quoted): expansion returns them
/// unchanged.
#[cfg(any(test, not(feature = "shell_test")))]
fn literal_cmd(argv: &[String]) -> Cmd {
    let words = argv
        .iter()
        .map(|text| Word {
            text: text.clone(),
            quote: QuoteStyle::Single,
            segments: alloc::vec![crate::parser::WordSegment { text: text.clone(), quote: QuoteStyle::Single }],
        })
        .collect();
    Cmd { argv: words, redirects: Vec::new(), malformed: None }
}

/// Dispatch to the matching shell built-in: the catalog table first, then the
/// shell-only state, session and launch built-ins.
///
/// Returns the exit code (0 = success, non-zero = error).
/// Falls through to `spawn_external` if no built-in matches.
fn dispatch_builtin(prog: &str, args: &[String], jobs: &mut Jobs) -> i32 {
    // SDK exec must use prepared context and owner-backed foreground publication.
    if prog == "exec" && SERIAL_SESSION.lock().is_some() {
        return match args.split_first() {
            Some((path, argv)) => spawn_external(path, argv),
            None => {
                shell_stderrln("Usage: exec <path> [args...]");
                2
            }
        };
    }
    if let Some(code) = dispatch_catalog(prog, args) {
        return code;
    }
    let legacy_result = match prog {
        "cd" => Some(with_legacy_parts(args, crate::cmd_cwd::cmd_cd)),
        "exec" => Some(with_legacy_parts(args, crate::commands::cmd_exec)),
        "history" => Some(with_legacy_parts(args, crate::history::cmd_history)),
        _ => None,
    };
    if let Some(result) = legacy_result {
        return match result {
            Ok(()) => 0,
            Err(_) => 1,
        };
    }

    match prog {
        "ssh" => crate::ssh::cmd_ssh(args),
        "completion" => cmd_completion(args),
        "hv" => cmd_hv(args),
        "clear" => crate::commands::cmd_clear().map(|_| 0).unwrap_or(1),
        "jobs" => {
            print_jobs(jobs);
            0
        }
        "fg" | "bg" => {
            shell_println(
                "fg/bg: no job control — background jobs run synchronously in this shell",
            );
            2
        }
        "source" | "." => cmd_source(args, jobs).map(|_| 0).unwrap_or(1),
        "break" => {
            set_loop_signal(LoopSignal::Break);
            0
        }
        "continue" => {
            set_loop_signal(LoopSignal::Continue);
            0
        }
        "read" => {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            cmd_read(&refs).map(|_| 0).unwrap_or(1)
        }
        "exit" => {
            let code = args
                .first()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            request_exit(code);
            0
        }
        "unset" => {
            for name in args {
                unset_var(name);
            }
            0
        }
        _ => {
            if let Some(body) = get_function(prog) {
                let nargs = args.len().min(9);
                let mut saved: Vec<(String, Option<String>)> = Vec::with_capacity(nargs + 2);
                for i in 1..=nargs {
                    let key = usize_key(i);
                    saved.push((key.clone(), get_var(&key)));
                }
                saved.push((String::from("#"), get_var("#")));
                saved.push((String::from("@"), get_var("@")));
                for i in 1..=nargs {
                    set_var(&usize_key(i), &args[i - 1]);
                }
                set_var("#", i32_to_str(nargs as i32));
                set_var("@", &args.join(" "));
                let mut buf = [0u8; 480];
                let bb = body.as_bytes();
                let blen = bb.len().min(479);
                buf[..blen].copy_from_slice(&bb[..blen]);
                let result = if let Ok(s) = core::str::from_utf8(&buf[..blen]) {
                    let ast = crate::parser::parse(s);
                    execute(&ast, jobs)
                } else {
                    1
                };
                for (k, v) in &saved {
                    match v {
                        Some(old) => set_var(k, old),
                        None => unset_var(k),
                    }
                }
                result
            } else {
                spawn_external(prog, args)
            }
        }
    }
}

/// Print all jobs: id, state, name, and for finished pipelines the whole-job
/// status and member statuses.
fn print_jobs(jobs: &Jobs) {
    for job in jobs.list() {
        let mut line = alloc::format!(
            "[{}] {}  {}",
            job.id,
            match job.state {
                JobState::Running => "Running",
                JobState::Done => "Done   ",
            },
            job.name
        );
        if let Some(status) = job.status {
            line.push_str(&alloc::format!("  status={}", status));
        }
        if job.members.len() > 1 {
            line.push_str("  members:");
            for (name, status) in job.members {
                line.push_str(&alloc::format!(" {}={}", name, status));
            }
        }
        line.push('\n');
        shell_print(&line);
    }
}

/// `test` / `[` — evaluate a condition and return 0 (true) or 1 (false).
///
/// Supported forms:
/// - `-f path`   : path exists and is a regular file
/// - `-z str`    : string is empty
/// - `-n str`    : string is non-empty
/// - `a = b`     : string equality
/// - `a != b`    : string inequality
fn cmd_test(args: &[&str]) -> ViResult<()> {
    let ok = Ok(());
    let fail = Err(ViError::NotFound); // any non-Ok maps to exit code 1
    match args {
        ["-f", path] => {
            if matches!(crate::cmd_fs::stat_file_vfs(path), Some((_, false))) {
                ok
            } else {
                fail
            }
        }
        ["-d", path] => {
            if matches!(crate::cmd_fs::stat_file_vfs(path), Some((_, true))) {
                ok
            } else {
                fail
            }
        }
        ["-e", path] => {
            if crate::cmd_fs::stat_file_vfs(path).is_some() {
                ok
            } else {
                fail
            }
        }
        [s1, "-z"] | ["-z", s1] => {
            if s1.is_empty() {
                ok
            } else {
                fail
            }
        }
        [s1, "-n"] | ["-n", s1] => {
            if !s1.is_empty() {
                ok
            } else {
                fail
            }
        }
        _ => {
            // String comparison: `a = b` or `a != b`.
            // args may be ["a", "=", "b"] or ["a", "!=", "b"].
            if args.len() == 3 {
                let (a, op, b) = (args[0], args[1], args[2]);
                match op {
                    "=" | "==" => {
                        if a == b {
                            ok
                        } else {
                            fail
                        }
                    }
                    "!=" => {
                        if a != b {
                            ok
                        } else {
                            fail
                        }
                    }
                    _ => fail,
                }
            } else {
                fail
            }
        }
    }
}

/// `read [-s] [VAR]` uses the active terminal lease; legacy callers retain fd 0. `-s` reads
/// silently (echo-off lease, no history, text wiped) and is refused where echo cannot be suppressed.
fn cmd_read(args: &[&str]) -> ViResult<()> {
    let (silent, rest) = match args.first() {
        Some(&"-s") => (true, &args[1..]),
        _ => (false, args),
    };
    let var = rest.first().copied().unwrap_or("REPLY");
    let serial = SERIAL_SESSION.lock().is_some();
    if silent && !serial {
        shell_stderrln("read: -s requires a terminal session that can suppress echo");
        return Err(ViError::NotSupported);
    }
    let mut line = if serial {
        let read = if silent {
            terminal_read_secret()
        } else {
            // `read` never records history; the scratch history only exists to satisfy the editor.
            let mut scratch = Vec::new();
            let read = terminal_read_line(&mut scratch);
            for entry in scratch.iter_mut() {
                ostd::session::zeroize_string(entry);
            }
            read
        };
        match read {
            Ok(Some(line)) => line,
            Err(Status::Unsupported) if silent => {
                shell_stderrln("read: -s is unsupported on this console (echo cannot be suppressed)");
                return Err(ViError::NotSupported);
            }
            Ok(None) | Err(_) => return Err(ViError::IO),
        }
    } else {
        let mut line = String::new();
        ostd::io::stdin().read_line(&mut line)?;
        line
    };
    let value = line.trim_end_matches(&['\r', '\n'][..]);
    set_var(var, value);
    ostd::session::zeroize_string(&mut line);
    Ok(())
}

/// `source <path>` — read a shell script from VFS and execute each line.
///
/// Lines starting with `#` and blank lines are skipped. The script runs in the
/// current shell's Jobs context, so spawns from the script are tracked normally.
/// Maximum script size is 4096 bytes (same limit as VFS OP_READ reply).
fn cmd_source(args: &[String], jobs: &mut Jobs) -> ViResult<()> {
    let path = match args.first() {
        Some(p) => p.as_str(),
        None => {
            shell_stderrln("Usage: source <path>");
            return Ok(());
        }
    };
    let bytes = crate::cmd_fs::read_file_vfs_owned(path, 4096).inspect_err(|_| {
        shell_stderr("source: cannot open '");
        shell_stderr(path);
        shell_stderrln("'");
    })?;
    let content = core::str::from_utf8(&bytes).unwrap_or("");
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let ast = crate::parser::parse(line);
        execute(&ast, jobs);
    }
    Ok(())
}

/// Attempt to spawn an external binary from `/bin/<prog>`.
///
/// Arguments are published via `sys_set_spawn_args` (a reserved state-stash
/// slot) for the spawned cell to read on startup — `sys_spawn_from_path` does
/// not yet carry argv on the new cell's stack. We always set the slot (empty
/// when there are no args) so the cell never reads a previous command's args.
fn spawn_external(prog: &str, args: &[String]) -> i32 {
    if SERIAL_SESSION.lock().is_some() {
        return spawn_serial_foreground(prog, args);
    }
    if !ostd::set_spawn_argv(args) {
        shell_println("shell: external argv exceeds 512-byte transport limit");
        return 1;
    }

    let path = if prog.starts_with('/') {
        alloc::string::String::from(prog)
    } else {
        let mut p = alloc::string::String::from("/bin/");
        p.push_str(prog);
        p
    };
    // Try the kernel's own path route first: it is the only route that reaches a
    // cell whose reviewed launch edge carries authority (the VFS-loaded route
    // refuses a non-empty child ceiling), and for VIFS1-resident cells it is also
    // the first one that can succeed. A *rejected* attempt discards the staged
    // command line by design, and the fallback below may need a second attempt, so
    // the argv is published again in between — `args` is still owned here, which is
    // the only place that can do it.
    if !ostd::set_spawn_argv(args) {
        shell_println("shell: external argv exceeds 512-byte transport limit");
        return 1;
    }
    let mut spawned = syscall::sys_spawn_from_path_raw(&path);
    if matches!(spawned, syscall::SyscallResult::Err(_)) {
        let _ = ostd::set_spawn_argv(args);
        spawned = syscall::sys_spawn_from_path(&path);
    }
    match spawned {
        syscall::SyscallResult::Ok(tid) => {
            // Backgrounded (`cmd &`): do NOT sys_wait. A long-running external
            // cell (httpd) would otherwise park the shell forever, so no later
            // command runs. The child keeps the shell's focus grant; that is
            // acceptable for a server that never reads the keyboard. Foreground
            // spawns fall through to the focus-handoff + sys_wait below.
            if state::bg_spawn() {
                return 0;
            }
            // Drain any pending input-service IPC events before ClearFocus.
            // Shell reads UART via sys_read(0), so Enter arrives via BOTH the ring
            // buffer (consumed in read_line) AND input-service EV_ASCII IPC. The IPC
            // path blocks input service in sys_send(shell, Enter_event) while shell
            // is not in sys_recv. drain_pending_input_events() uses sys_try_recv with
            // mask=input_tid (the correct G18 wildcard) to drain the queued message,
            // unblocking input service so release_focus does not deadlock.
            ostd::input::drain_pending_input_events();
            // Release keyboard focus before blocking in sys_wait. While the shell
            // is in sys_wait it is NOT in sys_recv, so the input service would
            // block indefinitely trying to dispatch key events to this cell and
            // eventually be killed by its own heartbeat watchdog (G18 deadlock).
            ostd::input::release_focus();
            // Foreground: block until the child exits so it owns the console
            // (stdin/UART). Without this the shell loops back to read the next
            // line and races interactive children (e.g. `hypha`) for keystrokes.
            // Fast commands return immediately (kernel Wait short-circuits when
            // the child is already Terminated). Background (`&`) already runs
            // synchronously in G1, so this does not regress it.
            let code = match syscall::sys_wait(tid) {
                syscall::SyscallResult::Ok(code) => code as i32,
                syscall::SyscallResult::Err(_) => 0,
            };
            // Re-acquire focus for the next interactive prompt.
            for _ in 0..10 {
                if ostd::input::request_focus() {
                    break;
                }
            }
            code
        }
        syscall::SyscallResult::Err(_) => {
            if prog != "vm-runner" && !prog.starts_with('/') {
                let app_toml = alloc::format!("/apps/{}/{}.toml", prog, prog);
                let app_lua = alloc::format!("/apps/{}/{}.lua", prog, prog);
                let has_app = match syscall::sys_open_cap(&app_toml) {
                    Ok(cap) => {
                        syscall::sys_close_cap(cap);
                        true
                    }
                    Err(_) => match syscall::sys_open_cap(&app_lua) {
                        Ok(cap) => {
                            syscall::sys_close_cap(cap);
                            true
                        }
                        Err(_) => false,
                    },
                };

                if has_app {
                    let mut runner_args = alloc::vec![alloc::string::String::from(prog)];
                    runner_args.extend_from_slice(args);
                    return spawn_external("vm-runner", &runner_args);
                }

                // Check for direct native user app binary at /apps/<prog>/<prog>
                let direct_app = alloc::format!("/apps/{}/{}", prog, prog);
                if let Ok(cap) = syscall::sys_open_cap(&direct_app) {
                    syscall::sys_close_cap(cap);
                    return spawn_external(&direct_app, args);
                }
            }
            shell_stderr("shell: command not found: ");
            shell_stderrln(prog);
            127
        }
    }
}

/// Start the Tier-3 hypervisor cell on demand (`hv`).
///
/// Cellos brings up Tier 1 and 2 and lands on this prompt; a guest is a workload
/// the operator asks for, not part of boot. The cell's own `boot_arm` creates the
/// VM and streams the guest image, so there is nothing to configure here beyond
/// starting it. It is spawned detached and the shell keeps keyboard focus, which
/// is what leaves the prompt usable while the guest boots; servers that want the
/// VM preloaded build init with `hv-autostart` instead of typing this.
fn cmd_hv(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("start") => {}
        Some("help") | Some("-h") | Some("--help") => {
            shell_println("hv: start the Tier-3 hypervisor cell (and its guest) on demand");
            shell_println("    hv          start it (background; the prompt stays live)");
            shell_println("    hv help     this text");
            return 0;
        }
        Some(other) => {
            shell_print("hv: unknown subcommand: ");
            shell_println(other);
            return 1;
        }
    }

    // Publish an empty command line: the spawn slot is shared state and a stale
    // one would be handed to the new cell.
    let _ = ostd::set_spawn_argv(&[]);
    // The raw path route first, exactly like `spawn_external`: it is the one that
    // reaches a cell whose reviewed launch edge carries authority, and the
    // VFS-loaded route refuses a non-empty child ceiling.
    let mut spawned = syscall::sys_spawn_from_path_raw("/bin/hypervisor");
    if matches!(spawned, syscall::SyscallResult::Err(_)) {
        spawned = syscall::sys_spawn_from_path("/bin/hypervisor");
    }
    match spawned {
        syscall::SyscallResult::Ok(tid) => {
            shell_println(&alloc::format!("hv: hypervisor cell started, tid={tid}"));
            0
        }
        syscall::SyscallResult::Err(_) => {
            shell_println(
                "hv: spawn refused — /bin/hypervisor missing from this image, or its launch edge is not reviewed",
            );
            1
        }
    }
}

fn cmd_grep_args(args: &[String]) -> i32 {
    crate::text_tools::grep::run(args)
}

fn cmd_sed_args(args: &[String]) -> i32 {
    let mut cursor = ArgCursor::new(args);
    let mut suppress = false;
    let expr = loop {
        match cursor.next() {
            Some("-n") => suppress = true,
            Some(arg) => break String::from(arg),
            None => return UtilityStatus::Error.exit_code(),
        }
    };
    let path = cursor.next_owned();
    if cursor.next().is_some() {
        shell_println("sed: only one input file is supported");
        return UtilityStatus::Error.exit_code();
    }
    let text = match read_utility_text("sed", path.as_deref()) {
        Ok(text) => text,
        Err(code) => return code,
    };
    let output = match sed::execute(&expr, suppress, &text) {
        Ok(output) => output,
        Err(err) => {
            shell_print("sed: ");
            shell_println(err.message());
            return UtilityStatus::Error.exit_code();
        }
    };
    for line in output {
        shell_println(&line);
    }
    UtilityStatus::Selected.exit_code()
}

fn cmd_awk_args(args: &[String]) -> i32 {
    let mut cursor = ArgCursor::new(args);
    let mut separator = None;
    let program = loop {
        match cursor.next() {
            Some("-F") => separator = cursor.next_owned(),
            Some(arg) if arg.starts_with("-F") && arg.len() > 2 => {
                separator = Some(String::from(&arg[2..]));
            }
            Some(arg) => break String::from(arg),
            None => return UtilityStatus::Error.exit_code(),
        }
    };
    if !awk::looks_like_program(&program) {
        let joined = args.join(" ");
        return match crate::cmd_fs::cmd_awk(joined.split_whitespace()) {
            Ok(()) => UtilityStatus::Selected.exit_code(),
            Err(_) => UtilityStatus::Error.exit_code(),
        };
    }
    let path = cursor.next_owned();
    if cursor.next().is_some() {
        shell_println("awk: only one input file is supported");
        return UtilityStatus::Error.exit_code();
    }
    let text = match read_utility_text("awk", path.as_deref()) {
        Ok(text) => text,
        Err(code) => return code,
    };
    let separator = match awk::Separator::from_flag(separator.as_deref()) {
        Ok(separator) => separator,
        Err(err) => {
            shell_print("awk: ");
            shell_println(err.message());
            return UtilityStatus::Error.exit_code();
        }
    };
    let output = match awk::execute(&program, separator, &text) {
        Ok(output) => output,
        Err(err) => {
            shell_print("awk: ");
            shell_println(err.message());
            return UtilityStatus::Error.exit_code();
        }
    };
    for line in output {
        shell_println(&line);
    }
    UtilityStatus::Selected.exit_code()
}

fn read_utility_text(name: &str, path: Option<&str>) -> Result<String, i32> {
    let bytes = if let Some(path) = path {
        match read_path_bytes(path) {
            Ok(bytes) => bytes,
            Err(UtilityReadError::Io) => {
                shell_stderr(name);
                shell_stderr(": cannot open '");
                shell_stderr(path);
                shell_stderrln("'");
                return Err(UtilityStatus::Error.exit_code());
            }
            Err(UtilityReadError::InputTooLarge) => {
                shell_print(name);
                shell_println(": input exceeds 65536-byte limit");
                return Err(UtilityStatus::Error.exit_code());
            }
            Err(UtilityReadError::AllocationFailed) => {
                shell_print(name);
                shell_println(": input allocation failed");
                return Err(UtilityStatus::Error.exit_code());
            }
        }
    } else {
        let stdin = shell_stdin();
        let mut bytes = Vec::new();
        if let Err(err) = extend_input(&mut bytes, &stdin) {
            shell_print(name);
            shell_println(match err {
                InputBufferError::TooLarge => ": input exceeds 65536-byte limit",
                InputBufferError::AllocationFailed => ": input allocation failed",
            });
            return Err(UtilityStatus::Error.exit_code());
        }
        bytes
    };
    core::str::from_utf8(&bytes).map(String::from).map_err(|_| {
        shell_print(name);
        shell_println(": input is not valid UTF-8");
        UtilityStatus::Error.exit_code()
    })
}

enum UtilityReadError {
    Io,
    InputTooLarge,
    AllocationFailed,
}

fn read_path_bytes(path: &str) -> Result<Vec<u8>, UtilityReadError> {
    if let Some((size, is_dir)) = crate::cmd_fs::stat_file_vfs(path) {
        if is_dir {
            return Err(UtilityReadError::Io);
        }
        if size > MAX_INPUT_BYTES {
            return Err(UtilityReadError::InputTooLarge);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| UtilityReadError::AllocationFailed)?;
        bytes.resize(size, 0);
        if size == 0 {
            return Ok(bytes);
        }
        if crate::cmd_fs::read_file_vfs_known_size(path, size, &mut bytes) == Ok(size) {
            return Ok(bytes);
        }
    }

    if !crate::cmd_fs::bootfs_fallback_allowed(path) {
        return Err(UtilityReadError::Io);
    }
    let fd = syscall::sys_open(path).map_err(|_| UtilityReadError::Io)?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        match syscall::sys_read(fd, &mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(err) = extend_input(&mut bytes, &chunk[..n]) {
                    syscall::sys_close(fd);
                    return Err(match err {
                        InputBufferError::TooLarge => UtilityReadError::InputTooLarge,
                        InputBufferError::AllocationFailed => UtilityReadError::AllocationFailed,
                    });
                }
            }
            Err(_) => {
                syscall::sys_close(fd);
                return Err(UtilityReadError::Io);
            }
        }
    }
    syscall::sys_close(fd);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{expand_word, set_var};
    use crate::parser::{parse, Ast};

    fn expanded_arg(line: &str) -> alloc::string::String {
        match parse(line) {
            Ast::Simple(cmd) => expand_word(&cmd.argv[1]).expect("expansion succeeds"),
            _ => panic!("expected Simple"),
        }
    }

    #[test]
    fn mixed_quote_expansion_is_segment_local() {
        set_var("HOME", "/home/test");
        assert_eq!(expanded_arg("echo pre'$HOME'"), "pre$HOME");
        assert_eq!(expanded_arg("echo '$HOME'suffix"), "$HOMEsuffix");
        assert_eq!(expanded_arg("echo $HOME\"/bin\""), "/home/test/bin");
        assert_eq!(expanded_arg("echo \"\""), "");
    }

    #[test]
    fn braced_parameters_expand_to_the_value_or_its_length() {
        set_var("P04_BRACED", "s3cr\u{e9}t-v");
        assert_eq!(expanded_arg("echo ${P04_BRACED}"), "s3cr\u{e9}t-v");
        assert_eq!(expanded_arg("echo ${#P04_BRACED}"), "8");
        assert_eq!(expanded_arg("echo len=${#P04_BRACED}."), "len=8.");
        assert_eq!(expanded_arg("echo \"[${#P04_BRACED}]\""), "[8]");
        assert_eq!(expanded_arg("echo ${#P04_UNSET_NAME}"), "0");
        assert_eq!(expanded_arg("echo ${P04_UNSET_NAME}x"), "x");
        // Not a name, empty or unclosed: kept literally.
        assert_eq!(expanded_arg("echo ${1x}"), "${1x}");
        assert_eq!(expanded_arg("echo ${}"), "${}");
        assert_eq!(expanded_arg("echo ${#}"), "${#}");
        assert_eq!(expanded_arg("echo ${P04_BRACED"), "${P04_BRACED");
        assert_eq!(expanded_arg("echo '${P04_BRACED}'"), "${P04_BRACED}");
    }

    #[test]
    fn builtin_output_stays_on_the_capture_stack_and_top_has_no_terminal_without_a_session() {
        use super::{interactive_terminal, serial_active, shell_print, shell_println, terminal_is_ours, CaptureGuard};
        use crate::shell_state::capture_depth;
        // The only test that touches the process-global capture stack.
        assert!(!serial_active());
        assert_eq!(capture_depth(), 0);
        assert!(!interactive_terminal(), "no session: top refuses its TUI");
        let outer = CaptureGuard::new();
        shell_print("a");
        shell_println("b");
        assert!(!interactive_terminal());
        {
            let inner = CaptureGuard::new();
            assert_eq!(capture_depth(), 2);
            shell_print("inner");
            assert!(!interactive_terminal(), "a nested capture never gets the TUI");
            assert_eq!(inner.finish(), b"inner");
        }
        {
            // A dropped (unfinished) capture discards its bytes and pops itself.
            let _abandoned = CaptureGuard::new();
            shell_print("lost");
        }
        assert_eq!(capture_depth(), 1);
        shell_print("c");
        assert_eq!(outer.finish(), b"ab\nc");
        assert_eq!(capture_depth(), 0);
        // With a session, only the REPL's own line capture (depth ≤ 1) leaves the terminal
        // to the running command; `$(top)` (depth 2) does not.
        assert!(terminal_is_ours(true, 0) && terminal_is_ours(true, 1));
        assert!(!terminal_is_ours(true, 2) && !terminal_is_ours(false, 1));
    }

    #[test]
    fn single_key_classification_ignores_pastes_releases_and_unmapped_keys() {
        use super::{TerminalKey, TerminalKeys};
        use terminal_sdk::input::EventKind;
        let mut keys = TerminalKeys::new();
        let text = |s: &str| EventKind::Text(alloc::string::String::from(s));
        let key = |code, modifiers, pressed| EventKind::Key { code, modifiers, pressed };
        assert_eq!(keys.classify(&text("q")), Some(TerminalKey::Char('q')));
        assert_eq!(keys.classify(&text("Qx")), Some(TerminalKey::Char('Q')));
        assert_eq!(keys.classify(&text("")), None);
        assert_eq!(keys.classify(&key(0x110001, 0, 1)), Some(TerminalKey::Enter));
        assert_eq!(keys.classify(&key(0x110001, 0, 2)), Some(TerminalKey::Enter));
        assert_eq!(keys.classify(&key(0x110003, 0, 1)), Some(TerminalKey::Backspace));
        assert_eq!(keys.classify(&key(0x110001, 0, 0)), None);
        assert_eq!(keys.classify(&key(0x110001, 4, 1)), None);
        assert_eq!(keys.classify(&key(0x110010, 0, 1)), None);
        assert_eq!(keys.classify(&EventKind::Resize { rows: 24, cols: 80 }), None);
        assert_eq!(keys.classify(&EventKind::Lost(1)), None);
        assert_eq!(keys.classify(&EventKind::Interrupt), Some(TerminalKey::Interrupt));
        assert_eq!(keys.classify(&EventKind::Eof), Some(TerminalKey::Eof));
        // Nothing typed inside a paste is a key press, across separate polls too.
        assert_eq!(keys.classify(&EventKind::PasteBegin), None);
        assert_eq!(keys.classify(&text("q")), None);
        assert_eq!(keys.classify(&key(0x110001, 0, 1)), None);
        // Ctrl-C always cancels, even inside a paste, and ends the paste state.
        assert_eq!(keys.classify(&EventKind::Interrupt), Some(TerminalKey::Interrupt));
        assert_eq!(keys.classify(&text("q")), Some(TerminalKey::Char('q')));
        assert_eq!(keys.classify(&EventKind::PasteBegin), None);
        assert_eq!(keys.classify(&EventKind::PasteEnd), None);
        assert_eq!(keys.classify(&text("y")), Some(TerminalKey::Char('y')));
    }

    #[test]
    fn command_line_overflow_is_refused_whole_never_truncated() {
        use super::{push_line_text, Wiped, SHELL_LINE_MAX};
        let mut line = Wiped::with_capacity(SHELL_LINE_MAX);
        let mut overflow = false;
        // A line over 128 bytes (the old limit) is kept exactly.
        let long = "x".repeat(300);
        assert!(push_line_text(&mut line, &mut overflow, &long));
        assert_eq!(line.0, long);
        // Exactly at the limit is accepted.
        assert!(push_line_text(&mut line, &mut overflow, &"y".repeat(SHELL_LINE_MAX - 300)));
        assert_eq!(line.0.len(), SHELL_LINE_MAX);
        assert!(!overflow);
        // One byte more marks the line overflowed; later short text is not appended either,
        // so no partial tail can make a different command.
        assert!(!push_line_text(&mut line, &mut overflow, "z"));
        assert!(overflow);
        line.0.pop();
        assert!(!push_line_text(&mut line, &mut overflow, "z"));
        assert_eq!(line.0.len(), SHELL_LINE_MAX - 1);
    }

    #[test]
    fn prompt_line_editing_is_echo_driven_bounded_and_cancellable() {
        use super::{prompt_step, PromptStep, Wiped};
        use terminal_sdk::input::EventKind;
        let mut line = Wiped::with_capacity(128);
        let mut paste = false;
        let text = |s: &str| EventKind::Text(alloc::string::String::from(s));
        let key = |code, pressed| EventKind::Key { code, modifiers: 0, pressed };
        assert!(matches!(prompt_step(&mut line, &mut paste, &text("y")), PromptStep::Appended(0)));
        assert!(matches!(prompt_step(&mut line, &mut paste, &text("es")), PromptStep::Appended(1)));
        assert_eq!(line.0, "yes");
        assert!(matches!(prompt_step(&mut line, &mut paste, &key(0x110003, 1)), PromptStep::Erased));
        assert_eq!(line.0, "ye");
        // Backspace on an empty line echoes nothing.
        let mut empty = Wiped::with_capacity(128);
        assert!(matches!(prompt_step(&mut empty, &mut paste, &key(0x110003, 1)), PromptStep::Idle));
        // The 128-byte bound drops overflowing text instead of growing the allocation.
        let mut full = Wiped::with_capacity(128);
        full.replace(&"n".repeat(127));
        assert!(matches!(prompt_step(&mut full, &mut paste, &text("zz")), PromptStep::Idle));
        assert_eq!(full.0.len(), 127);
        // Paste: text is inert, Enter inside the paste does not answer; a later Enter does.
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::PasteBegin), PromptStep::Idle));
        assert!(matches!(prompt_step(&mut line, &mut paste, &key(0x110001, 1)), PromptStep::Idle));
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::PasteEnd), PromptStep::Idle));
        assert!(matches!(prompt_step(&mut line, &mut paste, &key(0x110001, 1)), PromptStep::Enter));
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::Interrupt), PromptStep::Cancel));
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::Eof), PromptStep::Eof));
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::Lost(3)), PromptStep::Lost));
        assert!(matches!(prompt_step(&mut line, &mut paste, &key(0x110010, 1)), PromptStep::Idle));
    }

    #[test]
    fn enter_and_control_j_submit_a_line_other_modified_keys_do_not() {
        use super::{line_key, submits_line};
        use api::services::session::{key_code, modifiers};
        let j = u32::from(b'j');
        assert!(submits_line(key_code::ENTER, 0));
        assert!(submits_line(j, modifiers::CONTROL));
        assert!(!submits_line(j, 0));
        assert!(!submits_line(u32::from(b'm'), modifiers::CONTROL));
        assert!(!submits_line(u32::from(b'c'), modifiers::CONTROL));
        assert!(!submits_line(j, modifiers::CONTROL | modifiers::SHIFT));
        assert!(!submits_line(j, modifiers::CONTROL | modifiers::ALT));
        assert!(!submits_line(key_code::ENTER, modifiers::ALT));
        assert!(!submits_line(key_code::ENTER, modifiers::CONTROL));
        assert_eq!(line_key(j, modifiers::CONTROL), key_code::ENTER);
        assert_eq!(line_key(key_code::BACKSPACE, 0), key_code::BACKSPACE);
        assert_eq!(line_key(key_code::BACKSPACE, modifiers::ALT), 0);
        assert_eq!(line_key(j, 0), j);
    }

    #[test]
    fn prompt_answers_on_control_j_but_not_inside_a_paste_or_on_release() {
        use super::{prompt_step, PromptStep, Wiped};
        use api::services::session::modifiers;
        use terminal_sdk::input::EventKind;
        let mut line = Wiped::with_capacity(128);
        let mut paste = false;
        let ctrl = |c: u8, pressed| EventKind::Key { code: u32::from(c), modifiers: modifiers::CONTROL, pressed };
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::Text("y".into())), PromptStep::Appended(0)));
        assert!(matches!(prompt_step(&mut line, &mut paste, &ctrl(b'j', 0)), PromptStep::Idle));
        assert!(matches!(prompt_step(&mut line, &mut paste, &ctrl(b'k', 1)), PromptStep::Idle));
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::PasteBegin), PromptStep::Idle));
        assert!(matches!(prompt_step(&mut line, &mut paste, &ctrl(b'j', 1)), PromptStep::Idle));
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::PasteEnd), PromptStep::Idle));
        assert!(matches!(prompt_step(&mut line, &mut paste, &ctrl(b'j', 1)), PromptStep::Enter));
        assert_eq!(line.0, "y");
    }

    /// Serial bytes through the real session decoder and the SDK event codec into the prompt
    /// line reader: LF and CR each submit once, CRLF submits the line and then an empty line
    /// (old-shell parity, no coalescing), and an LF inside a bracketed paste stays inert.
    #[test]
    fn serial_lf_cr_and_crlf_submit_through_the_real_decoder() {
        use super::{prompt_step, PromptStep, Wiped};
        use api::services::session::{SerialDecoder, EVENT_HEADER_LEN, EVENT_MAX};
        use terminal_sdk::input::{Event, EventKind};
        fn events(bytes: &[u8]) -> alloc::vec::Vec<EventKind> {
            let mut decoder = SerialDecoder::new();
            let mut out = alloc::vec::Vec::new();
            for (i, byte) in bytes.iter().enumerate() {
                let mut frames = [0u8; EVENT_MAX];
                let n = decoder.feed(*byte, i as u64, &mut frames).unwrap();
                let mut offset = 0;
                while offset < n {
                    let len = u16::from_le_bytes([frames[offset + 18], frames[offset + 19]]) as usize;
                    let mut frame = frames[offset..offset + EVENT_HEADER_LEN + len].to_vec();
                    frame[..8].copy_from_slice(&1u64.to_le_bytes()); // sequence, assigned by the owner
                    frame[8..16].copy_from_slice(&1u64.to_le_bytes()); // epoch
                    out.push(Event::decode(&frame).unwrap().kind);
                    offset += EVENT_HEADER_LEN + len;
                }
            }
            out
        }
        // Each submission: the line text at the moment the reader answered Enter.
        fn submissions(bytes: &[u8]) -> alloc::vec::Vec<alloc::string::String> {
            let mut line = Wiped::with_capacity(128);
            let mut paste = false;
            let mut done = alloc::vec::Vec::new();
            for kind in events(bytes) {
                if let PromptStep::Enter = prompt_step(&mut line, &mut paste, &kind) {
                    done.push(line.take());
                }
            }
            done
        }
        assert_eq!(submissions(b"echo x\n"), ["echo x"]);
        assert_eq!(submissions(b"echo x\r"), ["echo x"]);
        assert_eq!(submissions(b"a\r\n"), ["a", ""]);
        assert_eq!(submissions(b"\x1b[200~a\nb\x1b[201~"), alloc::vec::Vec::<alloc::string::String>::new());
        assert_eq!(submissions(b"\x1b[200~a\nb\x1b[201~\n"), ["a\nb"]);
    }

    #[test]
    fn external_commands_resolve_to_bin_or_keep_an_absolute_path() {
        assert_eq!(super::external_path("cat"), "/bin/cat");
        assert_eq!(super::external_path("/apps/x/x"), "/apps/x/x");
    }

    #[test]
    fn terminal_forwarding_retries_only_the_unsent_binary_suffix() {
        use super::{write_terminal_bytes, Status};
        use core::cell::Cell;
        let bytes = b"\0\xffA\r\n\x80Z";
        let mut accepted = alloc::vec::Vec::new();
        let mut replies = alloc::collections::VecDeque::from([
            Ok(1), Err(Status::WouldBlock), Ok(2), Err(Status::WouldBlock),
            Err(Status::WouldBlock), Ok(1), Ok(3),
        ]);
        let polls = Cell::new(0);
        let pauses = Cell::new(0);
        let result = write_terminal_bytes::<Status>(
            bytes,
            |pending| {
                assert_eq!(pending, &bytes[accepted.len()..]);
                let reply = replies.pop_front().expect("one write per scripted turn");
                if let Ok(count) = reply {
                    accepted.extend_from_slice(&pending[..count]);
                }
                reply
            },
            || {
                polls.set(polls.get() + 1);
                Ok(())
            },
            || {
                assert_eq!(polls.get(), pauses.get() + 1, "poll input before sleeping");
                pauses.set(pauses.get() + 1);
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(accepted, bytes);
        assert!(replies.is_empty());
        assert_eq!(polls.get(), 3);
        assert_eq!(pauses.get(), 3);
    }

    #[test]
    fn terminal_forwarding_observes_interrupt_while_the_sink_stays_full() {
        use super::{write_terminal_bytes, Status, TerminalWriteStop};
        use core::cell::Cell;
        let bytes = b"\0\xffunsent";
        let mut accepted = alloc::vec::Vec::new();
        let attempts = Cell::new(0);
        let polls = Cell::new(0);
        let pauses = Cell::new(0);
        let result = write_terminal_bytes::<TerminalWriteStop>(
            bytes,
            |pending| {
                attempts.set(attempts.get() + 1);
                assert_eq!(pending, &bytes[accepted.len()..]);
                if accepted.is_empty() {
                    accepted.extend_from_slice(&pending[..2]);
                    Ok(2)
                } else {
                    Err(Status::WouldBlock)
                }
            },
            || {
                polls.set(polls.get() + 1);
                if polls.get() == 3 {
                    Err(TerminalWriteStop::Interrupted)
                } else {
                    Ok(())
                }
            },
            || pauses.set(pauses.get() + 1),
        );
        assert_eq!(result, Err(TerminalWriteStop::Interrupted));
        assert_eq!(accepted, &bytes[..2]);
        assert_eq!(attempts.get(), 4, "never retry after observing Ctrl-C");
        assert_eq!(pauses.get(), 2, "never sleep after observing Ctrl-C");
    }

    #[test]
    fn terminal_forwarding_does_not_relabel_endpoint_errors_as_ctrl_c() {
        use super::{write_terminal_bytes, Status, TerminalWriteStop};
        for status in [Status::Cancelled, Status::BrokenPipe, Status::Revoked, Status::Io] {
            let result = write_terminal_bytes::<TerminalWriteStop>(
                b"payload",
                |_| Err(status),
                || panic!("an endpoint error is not an input event"),
                || panic!("a failed endpoint must not be retried"),
            );
            assert_eq!(result, Err(TerminalWriteStop::Output(status)));
        }
        for count in [0, 8] {
            let result = write_terminal_bytes::<TerminalWriteStop>(
                b"payload",
                |_| Ok(count),
                || panic!("malformed progress is not an input event"),
                || panic!("malformed progress must not be retried"),
            );
            assert_eq!(result, Err(TerminalWriteStop::Output(Status::Malformed)));
        }
    }

    /// Two bounded host rings standing in for the stdout and stderr endpoints: each
    /// accepts at most `chunk` bytes per call and refuses (`WouldBlock`) every `block_every`th call.
    struct FakeRings {
        stdout: alloc::vec::Vec<u8>,
        stderr: alloc::vec::Vec<u8>,
        chunk: usize,
        block_every: usize,
        calls: usize,
    }

    impl FakeRings {
        fn new(chunk: usize, block_every: usize) -> Self {
            Self { stdout: alloc::vec::Vec::new(), stderr: alloc::vec::Vec::new(), chunk, block_every, calls: 0 }
        }

        fn write(&mut self, sink: super::Sink, pending: &[u8]) -> Result<usize, super::Status> {
            self.calls += 1;
            if self.block_every != 0 && self.calls % self.block_every == 0 {
                return Err(super::Status::WouldBlock);
            }
            let count = pending.len().min(self.chunk);
            match sink {
                super::Sink::Stdout => self.stdout.extend_from_slice(&pending[..count]),
                super::Sink::Stderr => self.stderr.extend_from_slice(&pending[..count]),
            }
            Ok(count)
        }
    }

    /// The shell's own writes for one failing command, in program order: the typed
    /// command echo and Enter newline (stdout), the diagnostic (stderr), the next prompt.
    const SCRIPT: [(&[u8], bool); 4] = [
        (b"/bin/ls /gui-missing" as &[u8], false),
        (b"\r\n" as &[u8], false),
        (b"ls: /gui-missing: no such directory\n\xff" as &[u8], true),
        (b"Cellos > " as &[u8], false),
    ];

    fn run_script(merged: bool, rings: &mut FakeRings) {
        for (bytes, diagnostic) in SCRIPT {
            super::route_terminal_write::<super::Status>(
                bytes,
                diagnostic,
                merged,
                |sink, pending| rings.write(sink, pending),
                || Ok(()),
                || {},
            )
            .expect("script write");
        }
    }

    #[test]
    fn merged_streams_deliver_newline_diagnostic_and_prompt_to_stdout_in_order() {
        // Short writes and backpressure must not reorder or split across endpoints.
        for (chunk, block_every) in [(usize::MAX, 0), (3, 0), (1, 2), (4, 3)] {
            let mut rings = FakeRings::new(chunk, block_every);
            run_script(true, &mut rings);
            let mut expected = alloc::vec::Vec::new();
            for (bytes, _) in SCRIPT {
                expected.extend_from_slice(bytes);
            }
            assert_eq!(rings.stdout, expected, "chunk {chunk} block_every {block_every}");
            assert!(rings.stderr.is_empty());
        }
    }

    #[test]
    fn unmerged_streams_keep_diagnostics_on_the_stderr_endpoint() {
        for (chunk, block_every) in [(usize::MAX, 0), (1, 2)] {
            let mut rings = FakeRings::new(chunk, block_every);
            run_script(false, &mut rings);
            assert_eq!(rings.stdout, b"/bin/ls /gui-missing\r\nCellos > ");
            assert_eq!(rings.stderr, b"ls: /gui-missing: no such directory\n\xff");
        }
    }

    #[test]
    fn merged_forwarding_still_abandons_the_unsent_suffix_on_ctrl_c() {
        use super::{route_terminal_write, Status, TerminalWriteStop};
        use core::cell::Cell;
        let mut rings = FakeRings::new(2, 2);
        let blocked = Cell::new(0);
        let pauses = Cell::new(0);
        let result = route_terminal_write::<TerminalWriteStop>(
            b"\0\xffdiagnostic",
            true,
            true,
            |sink, pending| rings.write(sink, pending),
            || {
                blocked.set(blocked.get() + 1);
                if blocked.get() == 2 {
                    Err(TerminalWriteStop::Interrupted)
                } else {
                    Ok(())
                }
            },
            || pauses.set(pauses.get() + 1),
        );
        assert_eq!(result, Err(TerminalWriteStop::Interrupted));
        // Retried on backpressure (one pause for the first refusal), never on the other endpoint.
        assert_eq!(pauses.get(), 1);
        assert_eq!(rings.stdout, b"\0\xffdi");
        assert!(rings.stderr.is_empty());
        // Endpoint errors keep their status when routed to stdout.
        let failed = route_terminal_write::<TerminalWriteStop>(
            b"x", true, true, |_, _| Err(Status::BrokenPipe), || Ok(()), || {},
        );
        assert_eq!(failed, Err(TerminalWriteStop::Output(Status::BrokenPipe)));
    }

    #[test]
    fn only_diagnostics_of_an_unmerged_shell_select_stderr() {
        use super::{sink_for, Sink};
        assert_eq!(sink_for(false, false), Sink::Stdout);
        assert_eq!(sink_for(false, true), Sink::Stdout);
        assert_eq!(sink_for(true, false), Sink::Stderr);
        assert_eq!(sink_for(true, true), Sink::Stdout);
    }

    #[test]
    fn the_merge_request_has_no_effect_outside_frontend_mode() {
        use super::{frontend_mode, streams_merged, STREAMS_MERGED};
        use core::sync::atomic::Ordering;
        assert!(!frontend_mode());
        STREAMS_MERGED.store(true, Ordering::Release);
        let merged = streams_merged();
        STREAMS_MERGED.store(false, Ordering::Release);
        assert!(!merged, "a serial or legacy shell never merges");
    }

    #[test]
    fn frontend_detach_and_attach_notices_never_discard_an_answer() {
        use super::{frontend_notice, prompt_step, EventKind, PromptStep, Wiped};
        use api::cell_session::frontend::{FRONTEND_ATTACHED, LOST_FRONTEND};
        assert!(frontend_notice(LOST_FRONTEND) && frontend_notice(FRONTEND_ATTACHED));
        for routine in [0, 1, 2, 3, 6] {
            assert!(!frontend_notice(routine));
        }
        let mut line = Wiped::with_capacity(128);
        let mut paste = false;
        line.0.push_str("y");
        for reason in [LOST_FRONTEND, FRONTEND_ATTACHED] {
            assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::Lost(reason)), PromptStep::Idle));
        }
        assert_eq!(line.0, "y");
        assert!(matches!(prompt_step(&mut line, &mut paste, &EventKind::Lost(3)), PromptStep::Lost));
    }

    #[test]
    fn owner_trust_prompt_is_built_only_from_the_owner_notice() {
        use super::{trust_prompt_text, EventKind};
        use api::cell_session::trust::TrustPromptV1;
        use api::services::session::{event_kind, EventHeaderV1};
        // The exact frame input writes to the controller's control ring.
        let prompt = TrustPromptV1 { port: 2222, host: "gateway.lan", algorithm: "ssh-ed25519", fingerprint: "SHA256:q0FkTQ5Ty1Wl7ixbhN5bNqH8eqbt4o2" };
        let mut frame = alloc::vec![0u8; 24 + TrustPromptV1::MAX_LEN];
        let len = prompt.encode(&mut frame[24..]).unwrap();
        frame.truncate(24 + len);
        EventHeaderV1 { sequence: 9, epoch: 4, kind: event_kind::TRUST_PROMPT, payload_len: len as u16, flags: 0 }.encode(&mut frame).unwrap();
        let event = terminal_sdk::input::Event::decode(&frame).unwrap();
        assert_eq!(
            trust_prompt_text(&event.kind).unwrap().as_bytes(),
            &b"\r\nThe authenticity of host 'gateway.lan' (port 2222) can't be established.\r\nssh-ed25519 key fingerprint is SHA256:q0FkTQ5Ty1Wl7ixbhN5bNqH8eqbt4o2.\r\nType 'yes' and Enter to trust it permanently; anything else refuses: "[..]
        );
        // Every other event draws nothing.
        for kind in [EventKind::TrustPromptEnd { accepted: true }, EventKind::Lost(4), EventKind::Eof] {
            assert!(trust_prompt_text(&kind).is_none());
        }
    }

    #[test]
    fn owner_notice_acknowledgement_retries_transient_refusals_only_within_its_bound() {
        use super::{acknowledge_with, ACK_ATTEMPTS};
        use api::cell_session::Status;
        // Drive the acknowledgement over a scripted sequence of owner answers.
        fn run(script: &[Result<(), Status>]) -> (Result<(), Status>, usize, usize) {
            let (mut calls, mut pauses) = (0usize, 0usize);
            let result = acknowledge_with(
                || { let answer = script.get(calls).copied().unwrap_or(Err(Status::WouldBlock)); calls += 1; answer },
                || pauses += 1,
            );
            (result, calls, pauses)
        }
        assert_eq!(run(&[Ok(())]), (Ok(()), 1, 0));
        // A busy, mid-transition or uncertain owner is retried with a pause between calls.
        assert_eq!(run(&[Err(Status::WouldBlock), Err(Status::TimedOut), Err(Status::Indeterminate), Ok(())]), (Ok(()), 4, 3));
        // A definite refusal (session lost, foreground over, wrong epoch) gives up at once.
        for status in [Status::Stale, Status::Denied, Status::WrongEpoch, Status::Revoked, Status::Malformed] {
            assert_eq!(run(&[Err(Status::WouldBlock), Err(status)]), (Err(status), 2, 1));
        }
        // Retries are bounded: the caller then cancels the foreground child.
        let (result, calls, pauses) = run(&[]);
        assert_eq!((result, calls, pauses), (Err(Status::WouldBlock), ACK_ATTEMPTS as usize, ACK_ATTEMPTS as usize - 1));
    }

    #[test]
    fn a_failed_prompt_handshake_keeps_cancelling_until_it_is_in() {
        use super::{cancel_step, CancelStep, OwnerPrompt};
        use api::cell_session::Status;
        // Cancelled now, already, or the operation is over: nothing left to do.
        for result in [Ok(()), Err(Status::Cancelled), Err(Status::Stale), Err(Status::Revoked)] {
            assert_eq!(cancel_step(result), CancelStep::Done, "{result:?}");
        }
        // Busy or uncertain: retried next turn, never dropped.
        for status in [Status::WouldBlock, Status::TimedOut, Status::Indeterminate] {
            assert_eq!(cancel_step(Err(status)), CancelStep::Retry);
        }
        // Refused for good: surfaced, not silently ignored.
        for status in [Status::Denied, Status::WrongOwner, Status::Malformed] {
            assert_eq!(cancel_step(Err(status)), CancelStep::Stop(status));
        }
        // No cancellation is issued while the handshake is healthy; a pending one stays
        // pending until a turn reports it in, then stops being issued.
        let mut prompt = OwnerPrompt::default();
        let mut calls = 0;
        prompt.cancel_turn(|| { calls += 1; Ok(()) });
        assert_eq!(calls, 0);
        prompt.cancel = Some(crate::context_session::Backoff::new());
        prompt.cancel_turn(|| { calls += 1; Err(Status::Cancelled) });
        assert!(prompt.cancel.is_none());
        prompt.cancel_turn(|| { calls += 1; Ok(()) });
        assert_eq!(calls, 1);
    }

    fn strings(words: &[&str]) -> alloc::vec::Vec<alloc::string::String> {
        words.iter().map(|word| alloc::string::String::from(*word)).collect()
    }

    #[test]
    fn one_shot_words_are_literal_and_never_expanded() {
        set_var("HOME", "/home/test");
        let argv = strings(&["echo", "", "a b", "|", ";", "$HOME", "*", ">", "$(echo x)", "&&"]);
        let cmd = super::literal_cmd(&argv);
        let expanded: alloc::vec::Vec<_> = cmd.argv.iter().map(|word| expand_word(word).expect("literal")).collect();
        assert_eq!(expanded, argv);
        assert!(cmd.redirects.is_empty() && cmd.malformed.is_none());
        let stage = crate::typed::StageIn { argv: argv.clone(), redirects: alloc::vec::Vec::new() };
        assert!(matches!(crate::typed::admit_oneshot(&stage), crate::typed::Admission::Legacy));
    }

    #[test]
    fn every_run_name_reaches_the_closed_table_or_admission_and_nothing_else_does() {
        use crate::oneshot::{classify, Class, Launch, TABLE};
        use crate::typed::{admit_oneshot, Admission, StageIn};
        for (name, class) in TABLE {
            let handled = super::catalog_handler(name).is_some();
            match class {
                // `rg` is always a typed producer: admission plans it or refuses it.
                Class::Run if *name == "rg" => assert!(!handled),
                Class::Run => assert!(handled, "{name}"),
                _ => assert!(!handled, "{name} must not be dispatchable from a one-shot"),
            }
        }
        // A Run argv either admits as typed / is refused, or is a closed-table name: the
        // one-shot never falls through to an unknown-name path, let alone a spawn.
        for argv in [
            &["ls", "/"][..], &["ls", "--records"], &["ps", "--records", "--text"], &["top"], &["top", "-b"],
            &["top", "--records"], &["edit", "f"], &["rg", "x", "/tmp"], &["rg", "-a", "x", "-"], &["rg"],
            &["cat"], &["cat", "/p04"], &["echo", "a b", ""], &["[", "1", "]"],
        ] {
            let argv = strings(argv);
            assert_eq!(classify(false, &argv), Launch::Run, "{argv:?}");
            match admit_oneshot(&StageIn { argv: argv.clone(), redirects: alloc::vec::Vec::new() }) {
                Admission::Typed(_) | Admission::Reject(_) => {}
                Admission::Legacy => assert!(super::catalog_handler(&argv[0]).is_some(), "{argv:?}"),
                Admission::Bytes(_) => panic!("{argv:?}: a one-shot never plans external stages"),
            }
        }
        for name in ["nope", "/bin/ls", "/bin/echo", "/bin/cellbox", "cellbox", "hotswap", "curl", "cd", "exec", "hv", "ssh", "source", "."] {
            assert!(super::catalog_handler(name).is_none(), "{name}");
        }
    }

    #[test]
    fn one_shot_records_default_to_typed_jsonl_and_text_selects_the_table() {
        use crate::typed::{admit_oneshot, default_sink, Admission, DefaultSink, Head, StageIn, TypedPlan};
        let plan = |words: &[&str]| -> TypedPlan {
            match admit_oneshot(&StageIn { argv: strings(words), redirects: alloc::vec::Vec::new() }) {
                Admission::Typed(plan) => plan,
                other => panic!("{words:?}: {other:?}"),
            }
        };
        let ps = plan(&["ps", "--records"]);
        assert_eq!(ps.head, Head::Producer);
        // A one-shot never displays: typed JSONL unless `--text`.
        assert_eq!(default_sink(false, &ps), DefaultSink::TypedJsonl);
        assert_eq!(default_sink(true, &ps), DefaultSink::Table);
        let text = plan(&["ps", "--records", "--text"]);
        assert_eq!(text.producer_argv, strings(&["ps", "--records"]));
        assert_eq!(default_sink(false, &text), DefaultSink::Table);
        let rg = plan(&["rg", "x", "/tmp"]);
        assert!(rg.search_status && rg.search_text);
        assert_eq!(default_sink(false, &rg), DefaultSink::TypedJsonl);
        assert_eq!(default_sink(false, &plan(&["rg", "--text", "x", "/tmp"])), DefaultSink::SearchText);
        // `rg … -` searches the one-shot stdin; the shell still needs piped bytes or `< FILE`.
        let stdin = plan(&["rg", "-a", "x", "-"]);
        assert_eq!((stdin.head.clone(), stdin.search_index, stdin.typed_cmds), (Head::Stdin, 0, 1));
        assert!(matches!(
            crate::typed::admit(&[StageIn { argv: strings(&["rg", "-a", "x", "-"]), redirects: alloc::vec::Vec::new() }], true),
            Admission::Reject(_)
        ));

        // The bytes of both defaults carry no terminal control.
        use text_engine::pipeline::{Category, RowSource, Value, VecSource};
        struct Collect(alloc::vec::Vec<u8>);
        impl crate::typed::ChunkSink for Collect {
            fn write(&mut self, bytes: &[u8]) -> Result<(), crate::typed::SinkStop> {
                self.0.extend_from_slice(bytes);
                Ok(())
            }
            fn interrupted(&mut self) -> bool {
                false
            }
        }
        let rows = || -> alloc::boxed::Box<dyn RowSource> {
            alloc::boxed::Box::new(VecSource::new(alloc::vec![
                Value::Record(alloc::vec![(alloc::string::String::from("tid"), Value::UInt(7))]),
                Value::Record(alloc::vec![(alloc::string::String::from("tid"), Value::UInt(42))]),
            ]))
        };
        for (sink, jsonl) in [(default_sink(false, &ps), true), (default_sink(false, &text), false)] {
            let mut out = Collect(alloc::vec::Vec::new());
            let end = crate::typed::stream_rows(rows(), Category::Rows, 0, &[], sink, &mut out).expect("streamed");
            assert_eq!(end.status(), 0);
            assert!(!out.0.contains(&0x1b), "{sink:?} wrote an ESC byte");
            let text = core::str::from_utf8(&out.0).expect("utf-8");
            let lines: alloc::vec::Vec<&str> = text.lines().collect();
            if jsonl {
                assert_eq!(lines.len(), 2, "{text}");
                assert!(lines.iter().all(|line| line.starts_with('{') && line.ends_with('}')), "{text}");
            } else {
                assert!(lines.len() >= 3 && lines.iter().all(|line| !line.starts_with('{')), "{text}");
            }
        }
    }

    #[test]
    fn the_in_shell_explicit_form_is_the_short_form_or_the_one_shot_answer() {
        use super::{explicit_form, stage_input};
        let stage = |line: &str| match parse(line) {
            Ast::Simple(cmd) => stage_input(&cmd).expect("expands"),
            _ => panic!("expected Simple"),
        };
        let explicit = |line: &str| explicit_form(stage(line).argv);
        assert_eq!(explicit("cellbox ls /"), Ok(stage("ls /").argv));
        assert_eq!(explicit("/bin/cellbox ls /p04"), Ok(stage("ls /p04").argv));
        assert_eq!(explicit("cellbox echo hi"), Ok(stage("echo hi").argv));
        assert_eq!(explicit("ls /"), Ok(stage("ls /").argv));
        let refused = |line: &str| match explicit(line) {
            Err((status, false, text)) => (status, text),
            other => panic!("{line}: {other:?}"),
        };
        assert_eq!(refused("cellbox cd /"), (2, crate::oneshot::refusal_message(crate::oneshot::Class::State, "cd")));
        assert_eq!(refused("/bin/cellbox cd /").0, 2);
        assert_eq!(refused("cellbox").0, 2);
        assert_eq!(refused("cellbox sh"), (2, alloc::string::String::from(crate::oneshot::SHELL_DOES_NOT_NEST)));
        assert_eq!(refused("cellbox /bin/ls /").0, 127);
        assert_eq!(refused("cellbox exec /bin/echo x").0, 2);
        // Typed admission sees through the prefix, before and after expansion.
        assert!(crate::typed::might_be_typed("cellbox", &["ps", "--records"]));
        assert!(crate::typed::might_be_typed("/bin/cellbox", &["rg", "x"]));
        assert!(!crate::typed::might_be_typed("cellbox", &["ls", "/"]));
        assert!(!crate::typed::might_be_typed("cellbox", &[]));
        let head = crate::typed::StageIn { argv: explicit("cellbox ps --records").expect("strip"), redirects: alloc::vec::Vec::new() };
        match crate::typed::admit(&[head, stage("select tid")], false) {
            crate::typed::Admission::Typed(plan) => assert_eq!(plan.producer_argv, stage("ps --records").argv),
            other => panic!("{other:?}"),
        }
    }
}

/// One governed preparation/publication and one reap. Pump output while the
/// input owner proves completion so a bounded child writer cannot deadlock Wait.
pub(crate) fn spawn_serial_foreground(prog: &str, args: &[String]) -> i32 {
    run_foreground(prog, args, false).0
}

/// [`spawn_serial_foreground`] for the ssh Cell: the child also gets the write-only
/// completion endpoint (slot 5), and its newest whole `cellbox.ssh.completion` frame
/// is returned with the status.
pub(crate) fn spawn_ssh_foreground(prog: &str, args: &[String]) -> (i32, Option<SshCompletion>) {
    run_foreground(prog, args, true)
}

fn run_foreground(prog: &str, args: &[String], completion: bool) -> (i32, Option<SshCompletion>) {
    if state::bg_spawn() {
        shell_stderrln("shell: SDK background launch requires explicit nonterminal streams");
        return (2, None);
    }
    let path = external_path(prog);
    let cwd = match crate::cmd_cwd::get_shell_cwd() {
        Ok(cwd) => cwd,
        Err(_) => {
            shell_stderrln("shell: cannot obtain current directory");
            return (1, None);
        }
    };
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut slot = SERIAL_SESSION.lock();
    let session = match slot.as_mut() {
        Some(session) => session,
        None => return (125, None),
    };
    let spawned = if completion {
        session.spawn_foreground_with_completion(&path, &argv, cwd.as_bytes(), &[], None)
    } else {
        session.spawn_foreground(&path, &argv, cwd.as_bytes(), &[], None)
    };
    let mut child = match spawned {
        Ok(child) => child,
        Err(status) => {
            drop(slot);
            shell_stderrln(&alloc::format!("shell: foreground preparation failed: {:?}", status));
            return (if status == Status::NotFound { 127 } else { 126 }, None);
        }
    };
    let tid = child.child().root_tid as usize;
    // Screen modes the child's relayed output switches (`screen_reset`), from its start.
    let mut screen = RelayScreen::new();
    let activation_failed = if let Some(status) = child.activation_error() {
        // Publication already happened: retain the real handle, cancel it and
        // complete owner-proven restoration/reaping instead of retrying spawn.
        let _ = session.cancel_foreground(&child, 2);
        let diagnostic = alloc::format!(
            "shell: foreground activation failed: {:?}\n", status
        );
        let mut offset = 0;
        let sink = sink_for(true, streams_merged());
        while offset < diagnostic.len() {
            match write_sink(session, sink, &diagnostic.as_bytes()[offset..]) {
                Ok(count) if count > 0 && count <= diagnostic.len() - offset => {
                    offset += count;
                }
                Err(Status::WouldBlock) => {}
                _ => {
                    let _ = syscall::sys_log("shell: activation diagnostic delivery failed\n");
                    break;
                }
            }
            if relay_serial(session, Some(&mut screen)).is_err() {
                break;
            }
        }
        true
    } else {
        false
    };
    // Input-owner trust prompt (D79-13): drawn here, the session's only output path.
    let mut prompt = OwnerPrompt::default();
    loop {
        // A failed prompt handshake (`poll` gives up once) leaves input withholding this child's
        // input: the child is cancelled, retried every turn until the cancellation is in or
        // restoration is observed below, which ends the quarantine and the confirmation.
        prompt.poll(session);
        prompt.cancel_turn(|| session.cancel_foreground(&child, 2));
        if let Err(status) = prompt.relay(session, &mut screen) {
            let _ = session.cancel_foreground(&child, 2);
            let _ = syscall::sys_wait(tid);
            let _ = syscall::sys_log(&alloc::format!(
                "shell: foreground output failed: {:?}\n", status
            ));
            syscall::sys_exit(125);
        }
        match session.finish_foreground(&mut child) {
            Ok(()) => { prompt.restored(); break; }
            Err(Status::WouldBlock | Status::TimedOut) => ostd::task::yield_now(),
            Err(status) => {
                let _ = session.cancel_foreground(&child, 2);
                let _ = syscall::sys_wait(tid);
                let _ = syscall::sys_log(&alloc::format!(
                    "shell: foreground owner restoration failed: {:?}\n", status
                ));
                // Never resume a prompt without authenticated restoration.
                syscall::sys_exit(125);
            }
        }
    }
    // The kernel's terminal record of the child (its preparer's QueryChild), not a TID
    // wait: the child may already be fully reaped once restoration is proven, and a
    // wait on a reaped TID cannot report its status. A child the input owner stopped
    // (Ctrl-C cancels its operation) reports 130.
    let code = if activation_failed {
        125
    } else {
        match child.wait_status() {
            Ok(record) => typed::reaped_status(Some(record.exit_code), record.reason, false),
            Err(_) => 125,
        }
    };
    // The child is reaped: relay what it left (bounded) so the tracker sees all of it,
    // then the soft reset iff it left a tracked mode on.
    for _ in 0..8 {
        match relay_serial(session, Some(&mut screen)) {
            Ok(true) => {}
            Ok(false) => break,
            Err(_) => {
                syscall::sys_exit(125);
            }
        }
    }
    if screen.needs_reset() {
        restore_screen_locked(session);
    }
    // The ssh Cell's lifecycle record: whatever it wrote before it ended (read before the
    // operation is closed; a torn trailing frame is ignored).
    let record = completion.then(|| {
        let mut frames = [0u8; 4096];
        let n = child.read_completion(&mut frames).unwrap_or(0);
        api::services::ssh_completion::last_frame(&frames[..n])
    }).flatten();
    // Reaped and restored: release the child operation so the session's 32-child
    // quota is reusable. Failure is diagnostic only; the command already ran.
    if let Err(status) = session.close_foreground(&child) {
        let _ = syscall::sys_log(&alloc::format!("shell: foreground operation close failed: {:?}\n", status));
    }
    (code, record)
}

type SshCompletion = api::services::ssh_completion::CompletionV1;

/// The newest `cellbox ssh` completion record of this shell (`completion` builtin).
static SSH_COMPLETION: ostd::sync::Mutex<Option<SshCompletion>> = ostd::sync::Mutex::new(None);

pub(crate) fn set_ssh_completion(record: SshCompletion) {
    *SSH_COMPLETION.lock() = Some(record);
}

/// `completion`: print the newest `cellbox.ssh.completion` record as one line.
pub(crate) fn cmd_completion(args: &[String]) -> i32 {
    if !args.is_empty() {
        shell_stderrln("usage: completion");
        return 2;
    }
    let Some(record) = *SSH_COMPLETION.lock() else {
        shell_stderrln("completion: no ssh completion recorded");
        return 1;
    };
    shell_println(&record.render());
    0
}

// ─── External byte stages (terminal session only) ────────────────────────────

/// Program path of an external command: an absolute path is used as given,
/// anything else resolves to `/bin/NAME`. Every external launch of a serial
/// session (foreground or pipeline stage) resolves through here.
fn external_path(prog: &str) -> String {
    if prog.starts_with('/') {
        String::from(prog)
    } else {
        alloc::format!("/bin/{}", prog)
    }
}

/// Cancellation reason for an interrupt, a consumer close and a failed pump.
const EXT_CANCEL_STOP: u32 = 2;

/// A stage that could not be started, with the status the pipeline reports.
struct StartFailure {
    stage: usize,
    status: i32,
}

fn setup_failure(status: Status) -> StartFailure {
    shell_stderrln(&alloc::format!("shell: pipeline preparation failed: {:?}", status));
    StartFailure { stage: 0, status: 126 }
}

struct ExtStage {
    name: String,
    child: Option<PipeChild>,
    /// An `ssh` publickey stage: it carries a completion stream whose record the shell keeps.
    ssh: bool,
}

/// One run of consecutive external stages inside one pipeline Operation.
///
/// Neighbouring stages share a kernel stream (4096-byte ring) the shell never
/// touches; the shell holds only the ends that cross the segment boundary:
/// the writer feeding stage 0 (`input`), the reader of the last stage's stdout
/// (`output`) and the reader of one stderr ring all children share (`errors`).
/// The Operations are laid out by `Pipeline` (see `libs/terminal/src/pipe_child.rs`):
/// a stage that exits early makes the kernel stop only the stages upstream of it,
/// never a stage downstream or the shell's own endpoints.
/// Every endpoint and child is released on every exit path (`Drop` cancels and
/// reaps), and `conclude` is the orderly way to do it.
struct ExtRun {
    pipeline: Option<Pipeline>,
    stages: Vec<ExtStage>,
    statuses: Vec<i32>,
    input: Option<Endpoint>,
    feed: Vec<u8>,
    feed_off: usize,
    /// Typed rows serialized on demand into `feed` (the streaming form of `to ... | external`).
    rows: Option<typed::RowFeed>,
    /// The row stage that failed while streaming `rows`, for attribution.
    row_failure: Option<typed::RunFailure>,
    output: Option<Endpoint>,
    errors: Option<Endpoint>,
    /// Shell copy of the shared stderr writer; closed once every stage has its own,
    /// so end of the stderr ring means every stage is gone.
    error_writer: Option<Endpoint>,
    /// Shell-held endpoint copies that exist only while stages are being started.
    held: Vec<Endpoint>,
    output_eof: bool,
    interrupted: bool,
    cancelled: bool,
    reaped: bool,
    /// Stages `0..not_started` were never started: the kernel tore the pipeline down
    /// because their consumer had already exited (spawn `Cancelled`).
    not_started: usize,
    /// The job's interrupt latch (shared with the terminal sink and the producer).
    latch: Rc<InterruptLatch>,
    steps: u32,
    reads: u32,
    idle: u32,
    /// Screen modes of the child bytes this run relayed to the terminal (`screen_reset`).
    screen: RelayScreen,
}

impl ExtRun {
    /// Start `commands` (at least one). Stages are committed last to first, so an
    /// unknown program is discovered before any upstream producer has run. `with_input`
    /// gives stage 0 a shell-fed stdin; without it stage 0 reads end of file. On
    /// failure everything started is cancelled and reaped, one diagnostic is printed
    /// and the failing stage with its status (127 not found, 126 otherwise) is returned.
    /// A `Cancelled` spawn of a stage that has a consumer in the segment is not a
    /// failure: the consumer already exited and the kernel closed the pipeline, so that
    /// stage and the ones before it are left unstarted (`not_started`, status 141).
    fn start(commands: &[StageIn], with_input: bool, latch: &Rc<InterruptLatch>) -> Result<ExtRun, StartFailure> {
        let operation = SERIAL_SESSION.lock().as_ref().map(|session| session.operation());
        let Some(operation) = operation else {
            shell_stderrln("shell: external commands need a terminal session");
            return Err(StartFailure { stage: 0, status: 125 });
        };
        let cwd = match crate::cmd_cwd::get_shell_cwd() {
            Ok(cwd) => cwd,
            Err(_) => {
                shell_stderrln("shell: cannot obtain current directory");
                return Err(StartFailure { stage: 0, status: 1 });
            }
        };
        let pipeline = Pipeline::create(operation).map_err(setup_failure)?;
        let mut run = ExtRun {
            pipeline: Some(pipeline),
            stages: commands
                .iter()
                .map(|command| ExtStage { name: command.argv[0].clone(), child: None, ssh: typed::is_ssh_stage(&command.argv) })
                .collect(),
            statuses: Vec::new(),
            input: None,
            feed: Vec::new(),
            feed_off: 0,
            rows: None,
            row_failure: None,
            output: None,
            errors: None,
            error_writer: None,
            held: Vec::new(),
            output_eof: false,
            interrupted: false,
            cancelled: false,
            reaped: false,
            not_started: 0,
            latch: latch.clone(),
            steps: 0,
            reads: 0,
            idle: 0,
            screen: RelayScreen::new(),
        };
        run.launch(commands, &cwd, with_input)?;
        Ok(run)
    }

    fn stream(&self) -> Result<(Endpoint, Endpoint), Status> {
        self.pipeline.as_ref().ok_or(Status::Revoked)?.stream()
    }

    /// Close a shell-held endpoint copy and forget it.
    fn release(&mut self, endpoint: Option<Endpoint>) {
        if let Some(endpoint) = endpoint {
            self.held.retain(|held| held.reference() != endpoint.reference());
            let _ = endpoint.close();
        }
    }

    fn launch(&mut self, commands: &[StageIn], cwd: &str, with_input: bool) -> Result<(), StartFailure> {
        match self.pipeline.as_mut() {
            Some(pipeline) => pipeline.layout(commands.len()).map_err(setup_failure)?,
            None => return Err(setup_failure(Status::Revoked)),
        }
        let (error_reader, error_writer) = self.stream().map_err(setup_failure)?;
        self.errors = Some(error_reader);
        self.error_writer = Some(error_writer);
        let (output_reader, output_writer) = self.stream().map_err(setup_failure)?;
        self.output = Some(output_reader);
        self.held.push(output_writer);
        let mut first_stdin = None;
        if with_input {
            // The feed stream lives in stage 0's own Operation: when stage 0 stops
            // reading, the kernel stops only that stage.
            let (reader, writer) = match self.pipeline.as_ref() {
                Some(pipeline) => pipeline.stage_stream(0).map_err(setup_failure)?,
                None => return Err(setup_failure(Status::Revoked)),
            };
            self.input = Some(writer);
            self.held.push(reader);
            first_stdin = Some(reader);
        }

        // Stages are committed last to first. A stage's stdin reader moves to the
        // stage at commit, so a consumer that exits at once leaves its stream without
        // a reader while the shell still holds the writer: the kernel then cancels the
        // stream's Operation (upstream stages only, see `Pipeline`), and the spawn of
        // the producer fails with `Cancelled`: that is the finished consumer, not an error.
        let mut stdout_for_stage = output_writer;
        for index in (0..commands.len()).rev() {
            let (stdin_for_stage, upstream_writer) = if index == 0 {
                (first_stdin.take(), None)
            } else {
                let link = match self.pipeline.as_ref() {
                    Some(pipeline) => pipeline.link(index - 1),
                    None => Err(Status::Revoked),
                };
                let (reader, writer) = match link {
                    Ok(pair) => pair,
                    Err(status) if typed::spawn_cancel_means_consumer_finished(status, index, commands.len()) => {
                        self.not_started = index + 1;
                        break;
                    }
                    Err(status) => return Err(setup_failure(status)),
                };
                self.held.push(reader);
                self.held.push(writer);
                (Some(reader), Some(writer))
            };
            let words = &commands[index].argv;
            let ssh = typed::is_ssh_stage(words);
            // An ssh stage is the client Cell: `/bin/cellbox-ssh` with `ssh` as argv[0] (its
            // entry keys on it) and a completion stream, so that it leaves the record the
            // contract requires for every ssh run.
            let (path, args): (String, Vec<&str>) = if ssh {
                (String::from(crate::ssh::SSH_PATH), crate::ssh::stage_argv(words))
            } else {
                (external_path(&words[0]), words[1..].iter().map(String::as_str).collect())
            };
            let spec = PipeSpec {
                path: &path,
                argv: &args,
                cwd: cwd.as_bytes(),
                environment: &[],
                cwd_source: None,
                completion: ssh,
            };
            let spawned = match self.pipeline.as_ref() {
                Some(pipeline) => pipeline.spawn(index, &spec, stdin_for_stage, stdout_for_stage, error_writer),
                None => Err(Status::Revoked),
            };
            // The child holds its own delegated copies now (or nothing was published).
            self.release(Some(stdout_for_stage));
            self.release(stdin_for_stage);
            match spawned {
                Ok(child) => self.stages[index].child = Some(child),
                Err(status) if typed::spawn_cancel_means_consumer_finished(status, index, commands.len()) => {
                    // The consumer finished before this producer was committed: pipe
                    // semantics, not a spawn failure. This stage and every one before it
                    // stay unstarted; the last stage decides the pipeline status.
                    self.not_started = index + 1;
                    break;
                }
                Err(status) => {
                    if status == Status::NotFound {
                        shell_stderr("shell: command not found: ");
                        shell_stderrln(&words[0]);
                    } else {
                        shell_stderrln(&alloc::format!("shell: cannot start {}: {:?}", words[0], status));
                    }
                    let code = if status == Status::NotFound { 127 } else { 126 };
                    return Err(StartFailure { stage: index, status: code });
                }
            }
            if let Some(writer) = upstream_writer {
                stdout_for_stage = writer;
            }
        }
        // Every stage owns a copy of the stderr writer: end of that ring now means
        // every stage has exited or closed it.
        let writer = self.error_writer.take();
        if let Some(writer) = writer {
            let _ = writer.close();
        }
        Ok(())
    }

    /// Hand the bytes for stage 0's stdin to the pump. No-op without shell-fed input;
    /// an empty feed gives stage 0 end of file at once.
    fn set_feed(&mut self, bytes: Vec<u8>) {
        if self.input.is_none() {
            return;
        }
        self.feed = bytes;
        self.feed_off = 0;
        if self.feed.is_empty() && self.rows.is_none() {
            self.close_input();
        }
    }

    /// Feed stage 0 from typed rows serialized on demand instead of from a buffer.
    fn set_rows(&mut self, rows: typed::RowFeed) {
        self.rows = Some(rows);
    }

    /// The row stage that failed while rows were streamed to stage 0, if any.
    fn take_row_failure(&mut self) -> Option<typed::RunFailure> {
        self.row_failure.take()
    }

    fn close_rows(&mut self) {
        if let Some(mut rows) = self.rows.take() {
            rows.close();
        }
    }

    /// Serialize the next rows into the (fully written) feed buffer. `Ok(true)` when
    /// the buffer holds new bytes; `Ok(false)` when the rows are exhausted. Checks
    /// for Ctrl-C first: producing rows can take a while (`top` waits a frame).
    fn refill(&mut self) -> Result<bool, StreamStop> {
        if self.rows.is_none() {
            return Ok(false);
        }
        self.poll_interrupt();
        if self.interrupted {
            self.close_rows();
            return Err(StreamStop::Interrupted);
        }
        ostd::session::zeroize_vec(&mut self.feed);
        self.feed_off = 0;
        let Some(rows) = self.rows.as_mut() else {
            return Ok(false);
        };
        match rows.fill(&mut self.feed, 2 * typed::STREAM_CHUNK) {
            Ok(more) => {
                if !more {
                    self.close_rows();
                }
                Ok(!self.feed.is_empty())
            }
            Err(failure) => {
                let message = failure.message.clone();
                self.row_failure = Some(failure);
                self.close_rows();
                Err(StreamStop::Failed(message))
            }
        }
    }

    fn close_input(&mut self) {
        if let Some(input) = self.input.take() {
            let _ = input.close();
        }
        ostd::session::zeroize_vec(&mut self.feed);
        self.feed_off = 0;
    }

    fn close_output(&mut self) {
        if let Some(output) = self.output.take() {
            let _ = output.close();
        }
    }

    fn close_errors(&mut self) {
        if let Some(errors) = self.errors.take() {
            let _ = errors.close();
        }
    }

    /// Move pending feed bytes into stage 0's stdin (serializing more rows when the
    /// buffer is empty); close the stream when everything was delivered so stage 0
    /// sees end of file. A stage that stopped reading (`BrokenPipe`, or its
    /// Operation cancelled/retired by the kernel) ends the feed and stops the row
    /// producer: its status and the kernel's pipeline teardown tell the story.
    fn feed_step(&mut self) -> Result<bool, StreamStop> {
        let Some(input) = self.input else {
            return Ok(false);
        };
        let mut progress = false;
        for _ in 0..8 {
            if self.feed_off >= self.feed.len() {
                if !self.refill()? {
                    break;
                }
                progress = true;
            }
            match input.write(&self.feed[self.feed_off..], 0) {
                Ok(count) if count > 0 && count <= self.feed.len() - self.feed_off => {
                    self.feed_off += count;
                    progress = true;
                }
                Ok(_) => return Err(StreamStop::Failed(String::from("malformed stream write"))),
                Err(Status::WouldBlock | Status::TimedOut) => break,
                Err(Status::BrokenPipe | Status::Cancelled | Status::Revoked | Status::Stale) => {
                    self.close_rows();
                    ostd::session::zeroize_vec(&mut self.feed);
                    self.feed_off = 0;
                    progress = true;
                    break;
                }
                Err(status) => {
                    return Err(StreamStop::Failed(alloc::format!("input stream failed: {:?}", status)))
                }
            }
        }
        if self.feed_off >= self.feed.len() && self.rows.is_none() {
            self.close_input();
            progress = true;
        }
        Ok(progress)
    }

    /// Forward one child chunk without hiding Ctrl-C behind a full host ring.
    /// The retry loop keeps the exact unsent suffix; cancellation abandons only
    /// that suffix and lets the normal conclude/reap path stop this pipeline.
    /// Every chunk handed to the terminal is followed by the screen-mode tracker.
    fn forward_terminal(&mut self, bytes: &[u8], diagnostic: bool) -> Result<(), StreamStop> {
        if self.interrupted {
            return Err(StreamStop::Interrupted);
        }
        self.screen.feed(diagnostic, bytes);
        terminal_write_while(bytes, diagnostic, || {
            self.poll_interrupt();
            if self.interrupted {
                Err(TerminalWriteStop::Interrupted)
            } else {
                Ok(())
            }
        }).map_err(|stop| match stop {
            TerminalWriteStop::Interrupted => StreamStop::Interrupted,
            TerminalWriteStop::Output(status) => {
                StreamStop::Failed(alloc::format!("terminal output failed: {:?}", status))
            }
        })
    }

    /// Child diagnostics go to the shell's stderr sink, never into data. End of the
    /// ring (or a revoked one) retires the reader.
    fn drain_errors(&mut self) -> bool {
        let Some(reader) = self.errors else {
            return false;
        };
        let mut chunk = ostd::session::Scrub::<512>::new();
        let mut progress = false;
        for _ in 0..8 {
            match reader.read(&mut chunk, 0) {
                Ok(count) => {
                    progress = true;
                    match self.forward_terminal(&chunk[..count], true) {
                        Ok(()) => {}
                        Err(StreamStop::Interrupted) => break,
                        Err(StreamStop::Failed(_)) => {
                            let _ = syscall::sys_log("shell: terminal diagnostic delivery failed\n");
                        }
                    }
                }
                Err(Status::WouldBlock | Status::TimedOut) => break,
                Err(_) => {
                    self.close_errors();
                    progress = true;
                    break;
                }
            }
        }
        progress
    }

    /// Look at the session event queue (through the job latch) without blocking.
    /// Ctrl-C marks the run interrupted; everything else typed meanwhile is
    /// discarded (scrubbed). An input service failure stops polling: the pipeline
    /// then runs to completion.
    fn poll_interrupt(&mut self) {
        if self.latch.poll_now() {
            self.interrupted = true;
        }
    }

    /// One pump turn: feed, diagnostics, interrupt check. `Ok(progress)`.
    fn step(&mut self) -> Result<bool, StreamStop> {
        self.steps = self.steps.wrapping_add(1);
        let mut progress = self.feed_step()?;
        progress |= self.drain_errors();
        if !progress || self.steps % 16 == 0 {
            self.poll_interrupt();
        }
        if self.interrupted {
            return Err(StreamStop::Interrupted);
        }
        Ok(progress)
    }

    /// Yield while idle; after a while sleep a tick instead of spinning.
    fn pause(&mut self, progress: bool) {
        if progress {
            self.idle = 0;
            return;
        }
        self.idle = self.idle.saturating_add(1);
        if self.idle > 64 {
            let _ = syscall::sys_set_timer(1);
        } else {
            ostd::task::yield_now();
        }
    }

    /// Next bytes of the last stage's stdout (at most 512), pumping the rest of the
    /// segment while it is empty. `Ok(None)` is end of output (every writer closed).
    fn read_output(&mut self, out: &mut [u8]) -> Result<Option<usize>, StreamStop> {
        loop {
            let Some(reader) = self.output else {
                return Ok(None);
            };
            match reader.read(out, 0) {
                Ok(count) => {
                    self.idle = 0;
                    // A busy producer must not starve feeding, diagnostics or Ctrl-C.
                    self.reads = self.reads.wrapping_add(1);
                    if self.reads % 8 == 0 {
                        self.step()?;
                    }
                    return Ok(Some(count));
                }
                Err(Status::Eof) => {
                    self.output_eof = true;
                    self.close_output();
                    return Ok(None);
                }
                // The kernel tore the pipeline Operation down before every stage was
                // started: nothing more can be read, the consumer has finished.
                Err(Status::Cancelled) if self.not_started > 0 => {
                    self.output_eof = true;
                    self.close_output();
                    return Ok(None);
                }
                Err(Status::WouldBlock | Status::TimedOut) => {}
                Err(status) => {
                    return Err(StreamStop::Failed(alloc::format!("output stream failed: {:?}", status)))
                }
            }
            let progress = self.step()?;
            self.pause(progress);
        }
    }

    /// The consumer needs no more output: upstream writers see `BrokenPipe`, and
    /// `conclude` cancels and reaps what is left.
    fn close_consumer(&mut self) {
        self.close_output();
    }

    /// After end of output: stop feeding and wait for every stage to close its stderr.
    fn settle(&mut self) -> Result<(), StreamStop> {
        self.close_input();
        while self.errors.is_some() {
            let progress = self.step()?;
            self.pause(progress);
        }
        Ok(())
    }

    /// Release the run and return one status per stage. A run whose output ended
    /// normally is awaited; anything else (consumer closed early, interrupt,
    /// failure) is cancelled first so the reap cannot block on a live stage.
    fn conclude(&mut self) -> Vec<i32> {
        if !self.reaped {
            let clean = self.output_eof && !self.interrupted && !self.cancelled;
            if clean && self.settle().is_ok() {
                self.reap(false);
            } else {
                self.reap(true);
            }
        }
        self.statuses.clone()
    }

    /// Close every shell-held endpoint, reap every stage (terminal record of its
    /// root task via `QueryChild`) and release the pipeline Operation. Idempotent.
    fn reap(&mut self, cancel: bool) {
        if self.reaped {
            return;
        }
        self.reaped = true;
        if cancel {
            self.cancelled = true;
            if let Some(pipeline) = &self.pipeline {
                let _ = pipeline.cancel(EXT_CANCEL_STOP);
            }
            // Diagnostics a stage wrote before it was stopped are still delivered.
            self.drain_errors();
        }
        self.close_input();
        self.close_output();
        self.close_errors();
        if let Some(writer) = self.error_writer.take() {
            let _ = writer.close();
        }
        for endpoint in core::mem::take(&mut self.held) {
            let _ = endpoint.close();
        }
        for (index, stage) in self.stages.iter_mut().enumerate() {
            let status = match stage.child.take() {
                Some(child) => {
                    let mut waited = child.wait_status();
                    if waited == Err(Status::TimedOut) && !self.cancelled {
                        // Never block forever on a stage that closed its streams but
                        // keeps running: stop the pipeline, then ask once more.
                        self.cancelled = true;
                        if let Some(pipeline) = &self.pipeline {
                            let _ = pipeline.cancel(EXT_CANCEL_STOP);
                        }
                        waited = child.wait_status();
                    }
                    let code = match waited {
                        Ok(record) => typed::reaped_status(Some(record.exit_code), record.reason, self.cancelled),
                        Err(status) => {
                            let _ = syscall::sys_log(&alloc::format!(
                                "shell: {}: exit status unavailable: {:?}\n", stage.name, status
                            ));
                            typed::reaped_status(None, 0, self.cancelled)
                        }
                    };
                    if stage.ssh {
                        // The ssh Cell's lifecycle record: whatever it wrote before it ended
                        // (read before the pipeline operation is closed; a torn trailing
                        // frame is ignored). The newest ssh stage's record is the shell's.
                        let mut frames = [0u8; 4096];
                        let n = child.read_completion(&mut frames).unwrap_or(0);
                        let record = api::services::ssh_completion::last_frame(&frames[..n]);
                        set_ssh_completion(crate::ssh::settle(record, code, api::services::ssh_completion::Mode::Exec));
                    }
                    code
                }
                None if index < self.not_started => typed::UPSTREAM_NOT_STARTED_STATUS,
                None => 0,
            };
            self.statuses.push(status);
        }
        if let Some(pipeline) = self.pipeline.take() {
            if let Err(status) = pipeline.close() {
                let _ = syscall::sys_log(&alloc::format!(
                    "shell: pipeline operation close failed: {:?}\n", status
                ));
            }
        }
        // Every stage is gone and its last bytes were relayed: the soft reset, once,
        // iff the relayed output left a tracked screen mode on.
        if core::mem::take(&mut self.screen).needs_reset() {
            restore_screen();
        }
    }

    fn interrupted(&self) -> bool {
        self.interrupted
    }

    /// Leading stages (`0..n`) that were never started because their consumer had
    /// already finished.
    fn not_started(&self) -> usize {
        self.not_started
    }

    /// True when the shell (not the stages) ended the run: Ctrl-C, a closed
    /// consumer or a pump failure. Cancelled stages are not failures to report.
    fn was_cancelled(&self) -> bool {
        self.cancelled
    }
}

impl Drop for ExtRun {
    fn drop(&mut self) {
        self.reap(true);
    }
}

/// Byte view of a shared external head for `typed::StreamRows`.
struct ChildBytes(alloc::rc::Rc<core::cell::RefCell<ExtRun>>);

impl typed::ByteSource for ChildBytes {
    fn read_chunk(&mut self, out: &mut [u8]) -> Result<Option<usize>, StreamStop> {
        self.0.borrow_mut().read_output(out)
    }

    fn close(&mut self, drained: bool) {
        if !drained {
            self.0.borrow_mut().close_consumer();
        }
    }
}

/// Feed `bytes` to the segment's first stage and deliver the last stage's stdout.
/// With a redirect the bytes are collected (bounded) and returned for the caller to
/// write after success; otherwise they reach the terminal as they arrive (byte
/// exact), or the current capture when one besides the REPL's own is active.
fn drive_external_sink(
    run: &mut ExtRun,
    bytes: Vec<u8>,
    redirect: Option<&(String, bool)>,
) -> Result<Vec<u8>, StreamStop> {
    run.set_feed(bytes);
    let redirected = redirect.is_some();
    let live = !redirected && serial_active() && state::capture_depth() <= 1;
    let mut collected = Vec::new();
    let mut total = 0usize;
    let mut chunk = ostd::session::Scrub::<512>::new();
    while let Some(count) = run.read_output(&mut chunk)? {
        if live {
            run.forward_terminal(&chunk[..count], false)?;
            continue;
        }
        total += count;
        if total > typed::MAX_SINK_BYTES {
            return Err(StreamStop::Failed(alloc::format!(
                "output exceeds the {} byte bound",
                typed::MAX_SINK_BYTES
            )));
        }
        if redirected {
            collected.extend_from_slice(&chunk[..count]);
        } else {
            state::write_out_bytes(&chunk[..count]);
        }
    }
    Ok(collected)
}

/// Reap the segment, record member statuses (stage `base + i`) and fold the pump
/// outcome into the pipeline status: 130 after Ctrl-C, 1 after a pump failure,
/// otherwise the last stage's status. Earlier nonzero statuses go to stderr; a
/// redirect target is written only when the last stage succeeded.
fn settle_external_segment(
    mut run: ExtRun,
    driven: Result<Vec<u8>, StreamStop>,
    members: &mut [(String, i32)],
    base: usize,
    redirect: Option<&(String, bool)>,
) -> i32 {
    let statuses = run.conclude();
    for (offset, status) in statuses.iter().enumerate() {
        if let Some(member) = members.get_mut(base + offset) {
            member.1 = *status;
        }
    }
    let cancelled = run.was_cancelled();
    let interrupted = run.interrupted();
    let not_started = run.not_started();
    drop(run);
    let collected = match driven {
        Ok(bytes) if !interrupted => bytes,
        Ok(_) | Err(StreamStop::Interrupted) => {
            shell_console("^C\n");
            return 130;
        }
        Err(StreamStop::Failed(message)) => {
            shell_stderrln(&alloc::format!("shell: {}", message));
            return fail_member(members, usize::MAX, 1);
        }
    };
    let names: Vec<&str> = members
        .iter()
        .skip(base)
        .take(statuses.len())
        .map(|member| member.0.as_str())
        .collect();
    // Stages that never started ran nothing: no exit to report for them.
    let started = not_started.min(statuses.len()).min(names.len());
    for line in typed::earlier_failures(&names[started..], &statuses[started..], cancelled) {
        shell_stderrln(&alloc::format!("shell: {}", line));
    }
    let last = statuses.last().copied().unwrap_or(0);
    if last == 0 {
        if let Some((path, append)) = redirect {
            if !crate::cmd_fs::vfs_write_chunked(path, &collected, *append) {
                shell_stderr("shell: cannot write '");
                shell_stderr(path);
                shell_stderrln("'");
                return fail_member(members, usize::MAX, 1);
            }
        }
    }
    last
}
