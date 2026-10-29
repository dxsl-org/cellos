//! The interactive `top` session loop (Phase 06 S6-4).
//!
//! Order of effects: the seed capture runs before anything reaches the terminal, so a
//! telemetry failure renders exactly as in batch mode (status 1, no screen bytes). Then
//! the REPL's earlier output is flushed, type-ahead is drained (`q`/Ctrl-C/EOF typed
//! before the first frame are honoured), and the TUI is entered through
//! `executor::with_tui` on a viewport capped at [`MAX_VIEW`]. The only wait is
//! `TuiSession::next_event(deadline)`. Every exit path restores the screen and releases
//! the raw lease before a diagnostic is written.

use super::view::{Step, ViewState, INTERRUPTED};
use super::TelemetryReader;
use crate::executor::{TerminalKey, TerminalKeys, TuiSink};
use alloc::format;
use alloc::string::String;
use api::cell_session::frontend::{FRONTEND_ATTACHED, FRONTEND_KIND_GUI, LOST_FRONTEND};
use api::cell_session::Status;
use terminal_sdk::session::TerminalSession;
use text_engine::top::sample::{Frame, Sampler};
use text_engine::top::text::TopError;
use text_engine::top::{TopOptions, View, USAGE_STATUS};
use vitui::event::Event;
use vitui::ratatui::core::layout::Size;
use vitui::session::{SessionError, TuiSession};
use vitui::theme::{ColorMode, Theme};

/// The largest drawn area: two Ratatui buffers of 4,800 cells (D64-7 Z1).
pub const MAX_VIEW: Size = Size::new(120, 40);
/// Shortest interactive delay (D64-12): `-d 0` would redraw continuously.
pub const MIN_DELAY_NS: u64 = 100_000_000;
/// Drain-then-enter attempts while typed input keeps the raw lease busy (D64-17).
const LEASE_TRIES: usize = 8;

/// How an interactive run ended: its exit status and at most one diagnostic, written
/// by the caller after the terminal is restored.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub status: i32,
    pub diagnostic: Option<String>,
}

impl Outcome {
    fn status(status: i32) -> Self {
        Self { status, diagnostic: None }
    }

    fn failed(status: i32, diagnostic: String) -> Self {
        Self { status, diagnostic: Some(diagnostic) }
    }

    fn telemetry(error: TopError) -> Self {
        Self::failed(error.status(), error.message("top"))
    }
}

/// Successive frames of one view.
pub trait Source {
    fn capture(&mut self, view: View) -> Result<Frame, TopError>;
}

/// `TelemetrySnapshot` captures and one sampler for the whole run.
struct Live {
    reader: TelemetryReader,
    sampler: Sampler,
}

impl Source for Live {
    fn capture(&mut self, view: View) -> Result<Frame, TopError> {
        self.reader.set_view(view);
        self.reader.capture(&mut self.sampler, view)
    }
}

/// Run interactive `top`. Refuses (status 2, one diagnostic naming `-b`, no capture and
/// no screen bytes) below the delay floor or without an interactive terminal.
pub fn run(options: &TopOptions) -> Outcome {
    if options.delay_ns < MIN_DELAY_NS {
        return Outcome::failed(USAGE_STATUS, String::from("top: interactive delay is at least 0.1 s; use top -b for faster frames"));
    }
    if !crate::executor::interactive_terminal() {
        return Outcome::failed(USAGE_STATUS, String::from("top: no interactive terminal here; use top -b for text frames"));
    }
    let mut source = Live { reader: TelemetryReader::new(options.view), sampler: Sampler::new() };
    run_with(options, &mut source, |source, view| {
        crate::executor::flush_repl_capture();
        crate::executor::with_tui(|session, sink| interact(session, sink, source, view, options))
            .unwrap_or_else(|status| Outcome::failed(1, format!("top: terminal unavailable: {status:?}")))
    })
}

/// The seed capture, then `terminal` with the view; a telemetry failure at the seed
/// never reaches `terminal`.
pub fn run_with<S: Source>(
    options: &TopOptions,
    source: &mut S,
    terminal: impl FnOnce(&mut S, &mut ViewState) -> Outcome,
) -> Outcome {
    let mut view = ViewState::new(options);
    // The seed capture only sets the baseline: a rate needs two snapshots.
    if let Err(error) = source.capture(view.view()) {
        return Outcome::telemetry(error);
    }
    terminal(source, &mut view)
}

/// Truecolor on the GUI Terminal; a terminal behind a UART (or anything else) gets the
/// 16 ANSI colours (D64-8).
pub fn color_mode(frontend: u8) -> ColorMode {
    if frontend == FRONTEND_KIND_GUI {
        ColorMode::TrueColor
    } else {
        ColorMode::Ansi16
    }
}

/// Consume typed input queued before the TUI: `q`/EOF end the run (0), Ctrl-C ends it
/// (130); every other key is discarded (and wiped).
fn drain_type_ahead(keys: &mut TerminalKeys, session: &mut TerminalSession) -> Result<Option<i32>, Status> {
    loop {
        match keys.read_from(session, 0)? {
            None => return Ok(None),
            Some(TerminalKey::Char('q' | 'Q') | TerminalKey::Eof) => return Ok(Some(0)),
            Some(TerminalKey::Interrupt) => return Ok(Some(INTERRUPTED)),
            Some(_) => {}
        }
    }
}

fn now() -> Result<u64, Status> {
    ostd::session::monotonic_ns()
}

/// Inside `with_tui`: drain, enter, run the event loop, exit. Writes nothing through the
/// shell's sinks (the session guard is held).
fn interact(
    session: &mut TerminalSession,
    sink: TuiSink<'_>,
    source: &mut impl Source,
    view: &mut ViewState,
    options: &TopOptions,
) -> Outcome {
    let theme = match session.capabilities() {
        Ok(capabilities) => Theme::new(color_mode(capabilities.frontend)),
        Err(status) => return Outcome::failed(1, format!("top: terminal unavailable: {status:?}")),
    };
    let mut keys = TerminalKeys::new();
    let mut tries = 0;
    let mut tui = loop {
        match drain_type_ahead(&mut keys, session) {
            Ok(None) => {}
            Ok(Some(status)) => return Outcome::status(status),
            Err(status) => return Outcome::failed(1, format!("top: terminal input failed: {status:?}")),
        }
        match TuiSession::enter_bounded(&mut *session, sink, MAX_VIEW) {
            Ok(tui) => break tui,
            Err(SessionError::Session(Status::WouldBlock)) if tries + 1 < LEASE_TRIES => tries += 1,
            Err(SessionError::Session(Status::WouldBlock)) => {
                return Outcome::failed(1, String::from("top: terminal input busy"))
            }
            Err(error) => return Outcome::failed(1, format!("top: {error}")),
        }
    };
    let outcome = run_loop(&mut tui, source, view, options, &theme);
    match tui.exit() {
        Err(error) if outcome.diagnostic.is_none() && outcome.status == 0 => {
            Outcome::failed(1, format!("top: terminal restore failed: {error}"))
        }
        _ => outcome,
    }
}

/// Capture now, prepare and draw; a capture error ends the run (the caller exits the
/// TUI before the diagnostic is written, so a stale frame is never left as current).
fn refresh<W: vitui::backend::ByteSink>(
    tui: &mut TuiSession<'_, W>,
    source: &mut impl Source,
    view: &mut ViewState,
    theme: &Theme,
) -> Result<Frame, Outcome> {
    let mut frame = source.capture(view.view()).map_err(Outcome::telemetry)?;
    view.prepare(&mut frame);
    draw(tui, view, &frame, theme)?;
    Ok(frame)
}

fn draw<W: vitui::backend::ByteSink>(
    tui: &mut TuiSession<'_, W>,
    view: &mut ViewState,
    frame: &Frame,
    theme: &Theme,
) -> Result<(), Outcome> {
    let terminal = tui.terminal();
    let size = terminal.size().map_err(|_| output_failed())?;
    let clamped = size.width > MAX_VIEW.width || size.height > MAX_VIEW.height;
    terminal
        .draw(|screen| {
            let area = screen.area();
            view.draw(frame, theme, area, clamped, screen.buffer_mut());
        })
        .map(drop)
        .map_err(|_| output_failed())
}

fn output_failed() -> Outcome {
    Outcome::failed(1, String::from("top: terminal output failed"))
}

fn run_loop<W: vitui::backend::ByteSink>(
    tui: &mut TuiSession<'_, W>,
    source: &mut impl Source,
    view: &mut ViewState,
    options: &TopOptions,
    theme: &Theme,
) -> Outcome {
    let clock_failed = |status: Status| Outcome::failed(1, format!("top: monotonic clock unavailable: {status:?}"));
    let mut remaining = options.count;
    let mut frame = match refresh(tui, source, view, theme) {
        Ok(frame) => frame,
        Err(outcome) => return outcome,
    };
    remaining = remaining.map(|count| count.saturating_sub(1));
    let mut next = match now() {
        Ok(now) => now.saturating_add(options.delay_ns),
        Err(status) => return clock_failed(status),
    };
    loop {
        let step = match tui.next_event(next) {
            Ok(mut event) => {
                let step = match &event {
                    Event::Resize { .. } => Step::Redraw,
                    // D64-13: the holder detached; the shell continues Detached.
                    Event::Lost(reason) if *reason == LOST_FRONTEND => {
                        return Outcome::failed(1, String::from("top: terminal frontend detached"))
                    }
                    Event::Lost(reason) if *reason == FRONTEND_ATTACHED => Step::Redraw,
                    _ => view.on_event(&event, &frame),
                };
                event.scrub();
                step
            }
            Err(SessionError::Session(Status::TimedOut)) => {
                if remaining == Some(0) {
                    return Outcome::status(0);
                }
                let frame_now = match refresh(tui, source, view, theme) {
                    Ok(frame) => frame,
                    Err(outcome) => return outcome,
                };
                frame = frame_now;
                remaining = remaining.map(|count| count.saturating_sub(1));
                next = match now() {
                    Ok(now) => next.saturating_add(options.delay_ns).max(now),
                    Err(status) => return clock_failed(status),
                };
                Step::Idle
            }
            Err(SessionError::Output(_)) => return output_failed(),
            Err(SessionError::Session(status)) => {
                return Outcome::failed(1, format!("top: terminal input failed: {status:?}"))
            }
        };
        match step {
            Step::Idle => {}
            Step::Redraw => {
                view.resort(&mut frame);
                if let Err(outcome) = draw(tui, view, &frame, theme) {
                    return outcome;
                }
            }
            Step::Recapture => {
                frame = match refresh(tui, source, view, theme) {
                    Ok(frame) => frame,
                    Err(outcome) => return outcome,
                };
                remaining = remaining.map(|count| count.saturating_sub(1));
                next = match now() {
                    Ok(now) => now.saturating_add(options.delay_ns),
                    Err(status) => return clock_failed(status),
                };
            }
            Step::Quit(status) => return Outcome::status(status),
        }
    }
}
