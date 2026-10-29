// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Scoped TUI mode over a terminal session.
//!
//! Input routing (raw bytes, bracketed paste) is the session owner's mode lease,
//! taken through the SDK's `RawInput`; the owner restores it if the holder dies.
//! The input owner does not accept the AlternateScreen/CursorVisible lease bits,
//! so the screen state ([`TUI_SCREEN`]) is output: the SDK's `TerminalMode`
//! transition bytes, written through the backend and restored on exit/drop.
//!
//! A session can be as large as 512×256 cells, and a Ratatui terminal keeps two
//! full buffers of its viewport. [`TuiSession::enter_bounded`] caps the drawn area
//! at the top-left, so the buffers never exceed the cap whatever the session size.

use core::fmt;
use ratatui_core::backend::Backend;
use ratatui_core::layout::{Rect, Size};
use ratatui_core::terminal::{Terminal, TerminalOptions, Viewport};
use terminal_sdk::input::Event as SdkEvent;
use terminal_sdk::raw_input::RawInput;
use terminal_sdk::session::TerminalSession;
use terminal_sdk::Status;

use crate::backend::{ByteSink, CellosBackend, Error, TUI_SCREEN};
use crate::event::{Event, EventAdapter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError<E> {
    /// The session refused or failed a request.
    Session(Status),
    /// Terminal output failed.
    Output(Error<E>),
}

impl<E: fmt::Debug> fmt::Display for SessionError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(status) => write!(f, "terminal session error: {status:?}"),
            Self::Output(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl<E: fmt::Debug> core::error::Error for SessionError<E> {}

/// A running TUI: raw-input lease, TUI screen mode and a Ratatui terminal.
pub struct TuiSession<'s, W: ByteSink> {
    session: &'s mut TerminalSession,
    input: RawInput,
    events: EventAdapter,
    terminal: Terminal<CellosBackend<W>>,
    /// Largest drawn area ([`TuiSession::enter_bounded`]); `None` draws the whole session.
    max: Option<Size>,
    active: bool,
}

impl<'s, W: ByteSink> TuiSession<'s, W> {
    /// Takes the raw-input lease, enters [`TUI_SCREEN`] and clears it. `sink`
    /// is normally `session.stdout()`. The size is the session's current one.
    /// Nothing stays changed when entry fails.
    pub fn enter(
        session: &'s mut TerminalSession,
        sink: W,
    ) -> Result<Self, SessionError<W::Error>> {
        Self::open(session, sink, None)
    }

    /// [`TuiSession::enter`] with the drawn area clamped to `max` at the top-left
    /// (a fixed Ratatui viewport). The backend keeps the session's exact size, a
    /// Resize re-applies the clamp and repaints, and the screen outside the drawn
    /// area stays cleared. `frame.area()` is the drawn area; `terminal().size()`
    /// is the session size.
    pub fn enter_bounded(
        session: &'s mut TerminalSession,
        sink: W,
        max: Size,
    ) -> Result<Self, SessionError<W::Error>> {
        Self::open(session, sink, Some(max))
    }

    fn open(
        session: &'s mut TerminalSession,
        sink: W,
        max: Option<Size>,
    ) -> Result<Self, SessionError<W::Error>> {
        let capabilities = session.capabilities().map_err(SessionError::Session)?;
        let backend = CellosBackend::new(sink, capabilities.cols, capabilities.rows)
            .map_err(SessionError::Output)?;
        let mut input = RawInput::enter(session).map_err(SessionError::Session)?;
        let screen = (move || {
            let mut backend = backend;
            backend.set_screen_mode(TUI_SCREEN)?;
            let mut terminal = new_terminal(backend, max)?;
            terminal.clear()?;
            terminal.backend_mut().flush()?;
            Ok(terminal)
        })();
        match screen {
            Ok(terminal) => Ok(Self {
                session,
                input,
                events: EventAdapter::new(),
                terminal,
                max,
                active: true,
            }),
            Err(error) => {
                // The dropped backend already tried to restore the screen.
                let _ = input.leave(session);
                Err(SessionError::Output(error))
            }
        }
    }

    pub fn terminal(&mut self) -> &mut Terminal<CellosBackend<W>> {
        &mut self.terminal
    }

    /// Next input event; `deadline_ns` follows `RawInput::next_event` (`0` polls,
    /// `u64::MAX` waits). A Resize is applied to the backend (and to a bounded
    /// viewport) before it is returned, so the next draw clears and repaints at
    /// the new size.
    pub fn next_event(&mut self, deadline_ns: u64) -> Result<Event, SessionError<W::Error>> {
        loop {
            let SdkEvent { kind, .. } = self
                .input
                .next_event(self.session, deadline_ns)
                .map_err(SessionError::Session)?;
            if let Some(event) = self.events.map(kind) {
                if let Event::Resize { cols, rows } = event {
                    apply_resize(&mut self.terminal, cols, rows, self.max)
                        .map_err(SessionError::Output)?;
                }
                return Ok(event);
            }
        }
    }

    /// Restores the screen, then releases the raw-input lease; reports the
    /// first failure. Dropping does the same without reporting.
    pub fn exit(mut self) -> Result<(), SessionError<W::Error>> {
        self.restore()
    }

    fn restore(&mut self) -> Result<(), SessionError<W::Error>> {
        if !core::mem::replace(&mut self.active, false) {
            return Ok(());
        }
        self.events.clear();
        let screen = self
            .terminal
            .backend_mut()
            .restore_screen_mode()
            .map_err(SessionError::Output);
        let lease = self
            .input
            .leave(self.session)
            .map_err(SessionError::Session);
        screen.and(lease)
    }
}

impl<W: ByteSink> Drop for TuiSession<'_, W> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// The drawn area of a `size` session under the cap `max`: the top-left corner.
pub(crate) fn bounded_area(size: Size, max: Size) -> Rect {
    Rect::new(0, 0, size.width.min(max.width), size.height.min(max.height))
}

/// A full-screen terminal, or a fixed viewport clamped to `max`.
pub(crate) fn new_terminal<W: ByteSink>(
    backend: CellosBackend<W>,
    max: Option<Size>,
) -> Result<Terminal<CellosBackend<W>>, Error<W::Error>> {
    let Some(max) = max else {
        return Terminal::new(backend);
    };
    let viewport = Viewport::Fixed(bounded_area(backend.size()?, max));
    Terminal::with_options(backend, TerminalOptions { viewport })
}

/// Applies a session Resize: the backend takes the exact size; a bounded
/// viewport is re-clamped at once (buffers resized, screen cleared). A
/// full-screen terminal resizes itself at the next draw.
pub(crate) fn apply_resize<W: ByteSink>(
    terminal: &mut Terminal<CellosBackend<W>>,
    cols: u16,
    rows: u16,
    max: Option<Size>,
) -> Result<(), Error<W::Error>> {
    terminal.backend_mut().set_size(cols, rows)?;
    if let Some(max) = max {
        terminal.resize(bounded_area(Size::new(cols, rows), max))?;
    }
    Ok(())
}
