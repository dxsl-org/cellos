//! `edit [PATH]`: the compact foreground editor (Phase 08, command-contracts §3).
//!
//! Open binds the file's D03 observation; saves publish only against it (see
//! [`editor`]). The editor is a session command: never in a pipeline, a
//! redirect, the background or a substitution. Exit status: 0 clean exit or
//! discard, 1 open/terminal failure, 2 usage, 125 terminal session lost.

mod editor;
mod render;
mod store;
#[cfg(test)]
mod tests;

use alloc::format;
use alloc::string::String;

use api::cell_session::frontend::{FRONTEND_ATTACHED, LOST_FRONTEND};
use api::cell_session::Status;
use api::ipc::{Observation, SaveExpect};
use ostd::ViError;
use terminal_sdk::session::TerminalSession;
use text_engine::editor::{EditBuffer, OpenError, MAX_BYTES};
use vitui::event::Event;
use vitui::ratatui::core::layout::Size;
use vitui::session::{SessionError, TuiSession};
use vitui::theme::Theme;

use crate::executor::{TerminalKey, TerminalKeys, TuiSink};
use editor::{Editor, Step, Store, SESSION_LOST};
use store::VfsStore;

pub const USAGE: &str = "usage: edit [PATH]";
const USAGE_STATUS: i32 = 2;
/// Largest drawn area, as for `top` (two Ratatui buffers of 4,800 cells).
const MAX_VIEW: Size = Size::new(120, 40);
const LEASE_TRIES: usize = 8;

/// Whether `argv` (after `edit`) is a valid invocation: at most one operand, no
/// options. Help alone is valid too.
pub fn valid_args(args: &[String]) -> bool {
    match args {
        [] => true,
        [only] if only == "-h" || only == "--help" => true,
        [only] => !only.is_empty() && !only.starts_with('-'),
        _ => false,
    }
}

pub fn cmd_edit(args: &[String]) -> i32 {
    if !valid_args(args) {
        crate::executor::shell_stderrln(USAGE);
        return USAGE_STATUS;
    }
    if matches!(args.first().map(String::as_str), Some("-h" | "--help")) {
        crate::executor::shell_println(USAGE);
        return 0;
    }
    if !crate::executor::interactive_terminal() {
        crate::executor::shell_stderrln("edit: no interactive terminal here");
        return 1;
    }
    let cwd = crate::cmd_cwd::get_shell_cwd().unwrap_or_else(|_| String::from("/"));
    let mut store = VfsStore::new();
    let (buf, path, binding, note) = match args.first() {
        None => (EditBuffer::new(), None, None, None),
        Some(arg) => {
            let path = crate::cmd_fs::resolve_path_from(&cwd, arg);
            match open(&mut store, &path) {
                Ok((buf, binding, note)) => (buf, Some(path), binding, note),
                Err(message) => {
                    crate::executor::shell_stderrln(&format!("edit: {path}: {message}"));
                    return 1;
                }
            }
        }
    };
    let mut editor = Editor::new(buf, path, binding, cwd);
    editor.message = note;
    crate::executor::flush_repl_capture();
    let status = crate::executor::with_tui(|session, sink| interact(session, sink, &mut editor, &mut store))
        .unwrap_or_else(|status| Err(format!("terminal unavailable: {status:?}")));
    match status {
        Ok(status) => status,
        Err(message) => {
            crate::executor::shell_stderrln(&format!("edit: {message}"));
            if editor.buf.is_dirty() {
                crate::executor::shell_stderrln("edit: unsaved changes were not written");
            }
            1
        }
    }
}

fn open_error(error: OpenError) -> String {
    String::from(match error {
        OpenError::InvalidUtf8 => "not UTF-8 text; refused without changes",
        OpenError::Nul => "contains NUL bytes; refused without changes",
        OpenError::TooLarge => "larger than 4 MiB; refused without changes",
        OpenError::TooManyLines => "more than 262,144 lines; refused without changes",
    })
}

fn read_bytes(path: &str) -> Result<alloc::vec::Vec<u8>, String> {
    ostd::clients::VfsClient::new().read_file_bounded(path, MAX_BYTES)
        .map_err(|error| format!("cannot read ({error:?})"))
}

/// Read `path` bound to one observation: the observation is taken before and
/// after the read and must not change, so the bytes are exactly that state.
fn open(store: &mut VfsStore, path: &str) -> Result<(EditBuffer, Option<SaveExpect>, Option<String>), String> {
    match store.observe(path) {
        Ok(Observation::Absent) => Ok((EditBuffer::new(), Some(SaveExpect::Absent), Some(String::from("new file")))),
        Ok(observation @ Observation::Present { regular: true, size, .. }) => {
            if size > MAX_BYTES as u64 {
                return Err(open_error(OpenError::TooLarge));
            }
            let bytes = read_bytes(path)?;
            if store.observe(path).ok() != Some(observation) {
                return Err(String::from("changed while being read; try again"));
            }
            let buf = EditBuffer::from_bytes(&bytes).map_err(open_error)?;
            Ok((buf, observation.expect(), None))
        }
        Ok(Observation::Present { .. }) => Err(String::from("not a regular file")),
        Err(ViError::NotSupported) => {
            // D78-1: editable, but this mount cannot publish conditionally.
            let note = Some(String::from("this mount has no conditional save: use Save As (^O) under /srv"));
            match ostd::clients::VfsClient::new().stat(path) {
                Ok((_, true)) => Err(String::from("is a directory")),
                Ok((size, false)) if size > MAX_BYTES as u64 => Err(open_error(OpenError::TooLarge)),
                Ok((_, false)) => Ok((EditBuffer::from_bytes(&read_bytes(path)?).map_err(open_error)?, None, note)),
                Err(_) => Ok((EditBuffer::new(), None, note)),
            }
        }
        Err(error) => Err(format!("cannot open ({error:?})")),
    }
}

fn interact(session: &mut TerminalSession, sink: TuiSink<'_>, editor: &mut Editor, store: &mut impl Store) -> Result<i32, String> {
    let theme = match session.capabilities() {
        Ok(capabilities) => Theme::new(crate::top::tui::color_mode(capabilities.frontend)),
        Err(status) => return Err(format!("terminal unavailable: {status:?}")),
    };
    let mut keys = TerminalKeys::new();
    let mut tries = 0;
    let mut tui = loop {
        // Type-ahead before the screen exists is discarded; end of input ends the run.
        loop {
            match keys.read_from(session, 0) {
                Ok(None) => break,
                Ok(Some(TerminalKey::Eof)) => return Ok(SESSION_LOST),
                Ok(Some(_)) => {}
                Err(status) => return Err(format!("terminal input failed: {status:?}")),
            }
        }
        match TuiSession::enter_bounded(&mut *session, sink, MAX_VIEW) {
            Ok(tui) => break tui,
            Err(SessionError::Session(Status::WouldBlock)) if tries + 1 < LEASE_TRIES => tries += 1,
            Err(error) => return Err(format!("{error}")),
        }
    };
    let result = run_loop(&mut tui, editor, store, &theme);
    match tui.exit() {
        Err(error) if result.is_ok() => Err(format!("terminal restore failed: {error}")),
        _ => result,
    }
}

fn draw<W: vitui::backend::ByteSink>(tui: &mut TuiSession<'_, W>, editor: &mut Editor, theme: &Theme) -> Result<(), String> {
    tui.terminal()
        .draw(|frame| {
            let area = frame.area();
            if let Some(position) = render::draw(editor, theme, area, frame.buffer_mut()) {
                frame.set_cursor_position(position);
            }
        })
        .map(drop)
        .map_err(|_| String::from("terminal output failed"))
}

fn run_loop<W: vitui::backend::ByteSink>(
    tui: &mut TuiSession<'_, W>,
    editor: &mut Editor,
    store: &mut impl Store,
    theme: &Theme,
) -> Result<i32, String> {
    draw(tui, editor, theme)?;
    loop {
        let deadline = ostd::session::monotonic_ns().map_err(|status| format!("clock unavailable: {status:?}"))?
            .saturating_add(1_000_000_000);
        match tui.next_event(deadline) {
            Ok(mut event) => {
                let step = match &event {
                    Event::Resize { .. } => Step::Continue,
                    Event::Lost(reason) if *reason == LOST_FRONTEND => Step::Exit(SESSION_LOST),
                    Event::Lost(reason) if *reason == FRONTEND_ATTACHED => Step::Continue,
                    Event::Lost(_) => {
                        editor.message = Some(String::from("some input was lost; nothing was applied from it"));
                        Step::Continue
                    }
                    _ => editor.handle(&event, store),
                };
                event.scrub();
                if let Step::Exit(status) = step {
                    return Ok(status);
                }
                draw(tui, editor, theme)?;
            }
            Err(SessionError::Session(Status::TimedOut)) => {}
            Err(SessionError::Output(_)) => return Err(String::from("terminal output failed")),
            Err(SessionError::Session(_)) => return Ok(SESSION_LOST),
        }
    }
}
