//! `edit` key handling, prompts and conditional saves, with no terminal or VFS
//! access of its own: storage is a [`Store`], screen size comes from the caller.
//!
//! Save semantics (command-contracts §3.3): a save publishes the exact buffer
//! bytes only if the destination still matches the observation it is bound to
//! (taken when the file was opened, or when Save As asked about overwriting).
//! A confirmation is never re-bound to a fresh observation and a failed save is
//! never retried. Anything short of a committed publication leaves the buffer,
//! its history and its dirty state exactly as they were.

use alloc::format;
use alloc::string::String;

use api::ipc::{Observation, SaveExpect};
use ostd::ViError;
use terminal_sdk::codec::{KeyState, KeySymbol};
use text_engine::editor::{EditBuffer, EditError, Found, MAX_QUERY};
use text_engine::editor::highlight::Lang;
use vitui::event::{Event, Key};

/// Longest Save As path typed at the prompt.
pub const MAX_PATH: usize = 4096;
/// Exit status when the terminal session ends under the editor.
pub const SESSION_LOST: i32 = 125;

/// What a conditional save did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveResult {
    /// Published and synced; the new binding for the destination.
    Committed(SaveExpect),
    /// The destination no longer matched; nothing changed.
    Conflict,
    /// Proven not published, with the reason.
    NotPublished(&'static str),
    /// Publication may or may not have happened.
    Indeterminate,
    /// Refused before anything was staged or published.
    Failed(ViError),
}

/// Conditional-save storage.
pub trait Store {
    fn observe(&mut self, path: &str) -> Result<Observation, ViError>;
    fn save(&mut self, path: &str, bytes: &[u8], expect: SaveExpect) -> SaveResult;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    SaveAs,
    Find,
    GoTo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Edit,
    Prompt(PromptKind, String),
    /// Ctrl-Q on a dirty buffer: Save / Discard / Cancel.
    DirtyExit,
    /// Save As onto an existing file, bound to what was observed.
    Overwrite { path: String, expect: SaveExpect },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Continue,
    Exit(i32),
}

pub struct Editor {
    pub buf: EditBuffer,
    path: Option<String>,
    /// What a save of `path` must still find there; `None` when this mount
    /// cannot publish conditionally (or there is no path yet).
    binding: Option<SaveExpect>,
    cwd: String,
    pub mode: Mode,
    pub message: Option<String>,
    pub help: bool,
    /// Exit once the pending dirty-exit save commits.
    exit_after_save: bool,
    /// Text rows on screen, for page motion.
    pub rows: usize,
    pub top: usize,
    pub left: usize,
}

/// Display cells of one scalar as the renderer draws it: C0 and DEL as `^X`,
/// C1 as `<XX>`, tab handled by the buffer, everything else by Ratatui's width.
pub fn cell_width(c: char) -> usize {
    match c {
        '\0'..='\u{1f}' | '\u{7f}' => 2,
        '\u{80}'..='\u{9f}' => 4,
        _ => {
            let mut utf8 = [0u8; 4];
            vitui::ratatui::core::text::Span::raw(&*c.encode_utf8(&mut utf8)).width()
        }
    }
}

fn describe(error: EditError) -> &'static str {
    match error {
        EditError::Nul => "refused: NUL bytes are not editable text",
        EditError::TooLarge => "refused: the file would exceed 4 MiB",
        EditError::TooManyLines => "refused: the file would exceed 262,144 lines",
        EditError::UndoBudget => "refused: a change over 1 MiB cannot be undone",
        EditError::ClipboardFull => "refused: over the 256 KiB clipboard limit",
    }
}

impl Editor {
    pub fn new(buf: EditBuffer, path: Option<String>, binding: Option<SaveExpect>, cwd: String) -> Self {
        Self {
            buf, path, binding, cwd,
            mode: Mode::Edit, message: None, help: false, exit_after_save: false,
            rows: 1, top: 0, left: 0,
        }
    }

    pub fn path(&self) -> Option<&str> { self.path.as_deref() }
    pub fn lang(&self) -> Option<Lang> { self.path.as_deref().and_then(Lang::for_path) }

    fn note(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    fn resolve(&self, path: &str) -> String {
        crate::cmd_fs::resolve_path_from(&self.cwd, path)
    }

    pub fn handle(&mut self, event: &Event, store: &mut impl Store) -> Step {
        if let Event::Eof = event {
            return Step::Exit(SESSION_LOST);
        }
        if let Event::Key(Key { state: KeyState::Release, .. }) = event {
            return Step::Continue;
        }
        match core::mem::replace(&mut self.mode, Mode::Edit) {
            Mode::Edit => {
                self.message = None;
                self.edit(event, store)
            }
            Mode::Prompt(kind, input) => self.prompt(kind, input, event, store),
            Mode::DirtyExit => self.dirty_exit(event, store),
            Mode::Overwrite { path, expect } => self.overwrite(path, expect, event, store),
        }
    }

    fn edit(&mut self, event: &Event, store: &mut impl Store) -> Step {
        let result = match event {
            Event::Text(text) => self.buf.insert(text),
            Event::Paste(text) => self.buf.paste_text(text),
            // Ctrl-C inside the editor is Copy (contract §3.2).
            Event::Interrupt => self.buf.copy().map(drop),
            Event::Key(key) if key.modifiers.control && !key.modifiers.alt => {
                return self.control(key.symbol, store);
            }
            Event::Key(key) if key.modifiers.alt => Ok(()),
            Event::Key(key) => self.plain(key.symbol),
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.note(describe(error));
        }
        Step::Continue
    }

    fn plain(&mut self, symbol: KeySymbol) -> Result<(), EditError> {
        match symbol {
            KeySymbol::Enter => return self.buf.newline(),
            KeySymbol::Tab => return self.buf.insert("\t"),
            KeySymbol::Backspace => return self.buf.backspace(),
            KeySymbol::Delete => return self.buf.delete(),
            KeySymbol::Left => self.buf.left(),
            KeySymbol::Right => self.buf.right(),
            KeySymbol::Up => self.buf.vertical(-1, &cell_width),
            KeySymbol::Down => self.buf.vertical(1, &cell_width),
            KeySymbol::Home => self.buf.home(),
            KeySymbol::End => self.buf.end(),
            KeySymbol::PageUp => self.buf.page(false, self.rows, &cell_width),
            KeySymbol::PageDown => self.buf.page(true, self.rows, &cell_width),
            KeySymbol::Escape if self.help => self.help = false,
            KeySymbol::Escape => self.buf.clear_selection(),
            // A printable key without text (GUI key-only delivery).
            KeySymbol::Character(c) if !c.is_control() => {
                let mut utf8 = [0u8; 4];
                return self.buf.insert(c.encode_utf8(&mut utf8));
            }
            _ => {}
        }
        Ok(())
    }

    fn control(&mut self, symbol: KeySymbol, store: &mut impl Store) -> Step {
        let KeySymbol::Character(c) = symbol else { return Step::Continue };
        let result = match c.to_ascii_lowercase() {
            's' => return self.save(store),
            'o' => { self.mode = Mode::Prompt(PromptKind::SaveAs, String::new()); Ok(()) }
            'q' if self.buf.is_dirty() => { self.mode = Mode::DirtyExit; Ok(()) }
            'q' => return Step::Exit(0),
            'z' => { if !self.buf.undo() { self.note("nothing to undo"); } Ok(()) }
            'y' => { if !self.buf.redo() { self.note("nothing to redo"); } Ok(()) }
            ' ' => { self.buf.toggle_anchor(); Ok(()) }
            'c' => self.buf.copy().map(drop),
            'x' => self.buf.cut().map(drop),
            'v' => self.buf.paste(),
            'f' => { self.mode = Mode::Prompt(PromptKind::Find, String::new()); Ok(()) }
            'n' => { let found = self.buf.find_next(); self.found(found); Ok(()) }
            'g' => { self.mode = Mode::Prompt(PromptKind::GoTo, String::new()); Ok(()) }
            'h' => { self.help = !self.help; Ok(()) }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.note(describe(error));
        }
        Step::Continue
    }

    fn found(&mut self, found: Found) {
        match found {
            Found::At(_) => {}
            Found::Wrapped(_) => self.note("search wrapped to the top"),
            Found::NotFound => self.note("not found"),
        }
    }

    fn prompt(&mut self, kind: PromptKind, mut input: String, event: &Event, store: &mut impl Store) -> Step {
        let limit = match kind {
            PromptKind::SaveAs => MAX_PATH,
            PromptKind::Find => MAX_QUERY,
            PromptKind::GoTo => 10,
        };
        match event {
            Event::Text(text) if input.len() + text.len() <= limit && !text.chars().any(char::is_control) => {
                input.push_str(text);
            }
            Event::Text(_) => self.note("prompt input too long"),
            Event::Key(key) => match key.symbol {
                KeySymbol::Backspace => { input.pop(); }
                KeySymbol::Escape => {
                    self.exit_after_save = false;
                    return Step::Continue;
                }
                KeySymbol::Enter => return self.submit(kind, input, store),
                _ => {}
            },
            _ => {}
        }
        self.mode = Mode::Prompt(kind, input);
        Step::Continue
    }

    fn submit(&mut self, kind: PromptKind, input: String, store: &mut impl Store) -> Step {
        match kind {
            PromptKind::Find if input.is_empty() => {}
            PromptKind::Find => {
                let found = self.buf.find(&input);
                self.found(found);
            }
            PromptKind::GoTo => {
                let ok = input.parse::<usize>().is_ok_and(|n| self.buf.go_to_line(n));
                if !ok {
                    self.note(format!("line must be 1..{}", self.buf.line_count()));
                }
            }
            PromptKind::SaveAs if input.is_empty() => self.exit_after_save = false,
            PromptKind::SaveAs => return self.save_as(self.resolve(&input), store),
        }
        Step::Continue
    }

    fn dirty_exit(&mut self, event: &Event, store: &mut impl Store) -> Step {
        let answer = match event {
            Event::Text(text) => text.chars().next().map(|c| c.to_ascii_lowercase()),
            Event::Key(key) if key.symbol == KeySymbol::Escape => Some('c'),
            _ => None,
        };
        match answer {
            Some('s') => {
                self.exit_after_save = true;
                self.save(store)
            }
            Some('d') => Step::Exit(0),
            Some('c') => Step::Continue,
            // No default: anything else, Enter included, keeps asking.
            _ => {
                self.mode = Mode::DirtyExit;
                Step::Continue
            }
        }
    }

    fn overwrite(&mut self, path: String, expect: SaveExpect, event: &Event, store: &mut impl Store) -> Step {
        let answer = match event {
            Event::Text(text) => text.chars().next().map(|c| c.to_ascii_lowercase()),
            Event::Key(key) if matches!(key.symbol, KeySymbol::Escape | KeySymbol::Enter) => Some('n'),
            _ => None,
        };
        match answer {
            Some('y') => self.save_to(path, expect, store),
            Some('n') => {
                self.exit_after_save = false;
                self.note("not saved");
                Step::Continue
            }
            _ => {
                self.mode = Mode::Overwrite { path, expect };
                Step::Continue
            }
        }
    }

    fn save(&mut self, store: &mut impl Store) -> Step {
        match (self.path.clone(), self.binding) {
            (None, _) => {
                self.mode = Mode::Prompt(PromptKind::SaveAs, String::new());
                Step::Continue
            }
            (Some(path), Some(expect)) => self.save_to(path, expect, store),
            (Some(path), None) => {
                self.exit_after_save = false;
                self.note(format!("save refused: {path} is not on a mount with conditional save (use Save As under /srv)"));
                Step::Continue
            }
        }
    }

    fn save_as(&mut self, path: String, store: &mut impl Store) -> Step {
        match store.observe(&path) {
            Ok(Observation::Absent) => self.save_to(path, SaveExpect::Absent, store),
            Ok(observation) => match observation.expect() {
                Some(expect) => {
                    self.note(format!("{path} exists: overwrite? (y/n)"));
                    self.mode = Mode::Overwrite { path, expect };
                    Step::Continue
                }
                None => {
                    self.exit_after_save = false;
                    self.note(format!("save refused: {path} is not a regular file"));
                    Step::Continue
                }
            },
            Err(error) => {
                self.exit_after_save = false;
                self.note(refusal(&path, error));
                Step::Continue
            }
        }
    }

    fn save_to(&mut self, path: String, expect: SaveExpect, store: &mut impl Store) -> Step {
        let bytes = self.buf.to_bytes();
        let result = store.save(&path, &bytes, expect);
        let exit = core::mem::replace(&mut self.exit_after_save, false);
        match result {
            SaveResult::Committed(binding) => {
                self.buf.mark_saved();
                self.note(format!("saved {} bytes to {path} (synced; not a power-loss guarantee)", bytes.len()));
                self.path = Some(path);
                self.binding = Some(binding);
                if exit {
                    return Step::Exit(0);
                }
            }
            SaveResult::Conflict => self.note(format!("save refused: {path} changed since it was read; nothing written, edits kept")),
            SaveResult::NotPublished(reason) => self.note(format!("save failed ({reason}); {path} unchanged, edits kept")),
            SaveResult::Indeterminate => self.note(format!("save outcome unknown for {path}; edits kept and still unsaved")),
            SaveResult::Failed(error) => self.note(refusal(&path, error)),
        }
        Step::Continue
    }
}

fn refusal(path: &str, error: ViError) -> String {
    match error {
        ViError::NotSupported => format!("save refused: {path} is not on a mount with conditional save (use /srv)"),
        ViError::PermissionDenied => format!("save refused: permission denied for {path}"),
        ViError::OutOfMemory => format!("save refused: quota exceeded for {path}"),
        other => format!("save failed before publishing ({other:?}); {path} unchanged"),
    }
}
