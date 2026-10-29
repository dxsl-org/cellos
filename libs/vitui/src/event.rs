// SPDX-License-Identifier: Apache-2.0 OR MIT
//! TUI events from the SDK's typed input and control frames.

use alloc::string::String;
use core::fmt;
use terminal_sdk::codec::{KeyAction, KeyState, KeySymbol, Modifiers};
use terminal_sdk::input::EventKind;
use terminal_sdk::raw_input::LOST_UNDECODABLE;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub symbol: KeySymbol,
    pub modifiers: Modifiers,
    pub state: KeyState,
}

/// Typed text, keys and pastes can be secrets: `Debug` never shows them, and
/// consumers SHOULD call [`Event::scrub`] once the text is consumed.
#[derive(Clone, PartialEq, Eq)]
pub enum Event {
    Key(Key),
    Text(String),
    /// One complete bracketed paste; its newlines are inert text.
    Paste(String),
    Resize {
        cols: u16,
        rows: u16,
    },
    Interrupt,
    Eof,
    /// Input was lost; the reason is the SDK's `Lost` code.
    Lost(u32),
}

impl Event {
    /// Wipe any text this event carries.
    pub fn scrub(&mut self) {
        if let Self::Text(text) | Self::Paste(text) = self {
            scrub(text);
        }
    }
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(_) => f.write_str("Key(<redacted>)"),
            Self::Text(text) => write!(f, "Text(<redacted {} bytes>)", text.len()),
            Self::Paste(text) => write!(f, "Paste(<redacted {} bytes>)", text.len()),
            Self::Resize { cols, rows } => f
                .debug_struct("Resize")
                .field("cols", cols)
                .field("rows", rows)
                .finish(),
            Self::Interrupt => f.write_str("Interrupt"),
            Self::Eof => f.write_str("Eof"),
            Self::Lost(reason) => f.debug_tuple("Lost").field(reason).finish(),
        }
    }
}

/// Folds SDK event kinds (from `RawInput::next_event`, `TerminalSession::next_event`
/// or `next_control`) into [`Event`]s: `PasteBegin`, its `Text` and `PasteEnd`
/// become one [`Event::Paste`].
#[derive(Default)]
pub struct EventAdapter {
    paste: Option<String>,
}

impl EventAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// The event completed by `kind`, or `None` while a paste is being collected.
    pub fn map(&mut self, kind: EventKind) -> Option<Event> {
        Some(match kind {
            EventKind::Key {
                code,
                modifiers,
                pressed,
            } => match KeyAction::from_key(code, modifiers, pressed) {
                Ok(KeyAction::Key {
                    symbol,
                    modifiers,
                    state,
                }) => Event::Key(Key {
                    symbol,
                    modifiers,
                    state,
                }),
                _ => Event::Lost(LOST_UNDECODABLE),
            },
            EventKind::Text(mut text) => match &mut self.paste {
                Some(paste) => {
                    paste.push_str(&text);
                    scrub(&mut text);
                    return None;
                }
                None => Event::Text(text),
            },
            EventKind::PasteBegin => {
                // A second begin restarts: the unterminated paste is dropped.
                if let Some(mut stale) = self.paste.replace(String::new()) {
                    scrub(&mut stale);
                    return Some(Event::Lost(LOST_UNDECODABLE));
                }
                return None;
            }
            EventKind::PasteEnd => match self.paste.take() {
                Some(paste) => Event::Paste(paste),
                None => Event::Lost(LOST_UNDECODABLE),
            },
            EventKind::Resize { rows, cols } => Event::Resize { cols, rows },
            EventKind::Interrupt => Event::Interrupt,
            EventKind::Eof => Event::Eof,
            EventKind::Lost(reason) => Event::Lost(reason),
            // Input-owner notices address the session controller only; the SDK never
            // delivers them as reader events, so an application has nothing to map.
            EventKind::TrustPrompt { .. } | EventKind::TrustPromptEnd { .. } => return None,
        })
    }

    /// Wipe a paste still being collected.
    pub fn clear(&mut self) {
        if let Some(mut paste) = self.paste.take() {
            scrub(&mut paste);
        }
    }
}

impl Drop for EventAdapter {
    fn drop(&mut self) {
        self.clear();
    }
}

fn scrub(text: &mut String) {
    let mut kind = EventKind::Text(core::mem::take(text));
    kind.scrub();
}
