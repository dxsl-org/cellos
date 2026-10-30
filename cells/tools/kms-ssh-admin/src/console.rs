//! The terminal side of the cell: a small [`Console`] trait over the foreground session,
//! and the two readers that sit on a typed/pasted event stream.
//!
//! The key block reader is its own reader (the ssh passphrase reader refuses paste): it
//! accepts typed text and bracketed or unbracketed paste, Backspace, Enter, and ends at the
//! END marker line. Escape (or Ctrl-C, end of input) cancels. If a paste is still running
//! when the reader stops, the rest of the paste is consumed and dropped: left in the input
//! queue it would reach the shell as typed commands.

use api::services::session::{key_code, modifiers};
use terminal_sdk::input::EventKind;

use crate::armor::END;
use crate::secret::Secret;

/// One step of the input stream.
pub enum Next {
    Event(EventKind),
    /// The session (or this child's foreground in it) is gone for good.
    Lost,
}

pub trait Events {
    /// The next event, waiting as long as it takes.
    fn next(&mut self) -> Next;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    /// The END marker line was read.
    Done,
    /// Escape, Ctrl-C or end of input.
    Cancelled,
    /// More than `armor::TEXT_MAX` bytes.
    TooLong,
    Lost,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Word {
    Given(alloc::string::String),
    Cancelled,
    Lost,
}

/// What the commands need from the terminal.
pub trait Console {
    fn out(&mut self, text: &str);
    fn err(&mut self, text: &str);
    /// Read one armored block under the echo-off lease into `buf` (nothing is echoed).
    fn read_key_block(&mut self, buf: &mut Secret) -> Block;
    /// Read one typed word (a paste is ignored), Enter submits, Escape cancels.
    fn read_word(&mut self) -> Word;
    /// A session operation failed in a way that means the session is gone.
    fn lost(&self) -> bool;
}

/// Longest word the confirmation prompt keeps; longer input is simply never `delete`.
const WORD_MAX: usize = 16;
/// How much of a leftover paste is read and dropped before the reader gives up on it.
const DRAIN_MAX: usize = 512 * 1024;

fn is_submit(code: u32, mods: u32) -> bool {
    (code == key_code::ENTER && mods == 0)
        || (code == u32::from(b'j') && mods == modifiers::CONTROL)
}

/// True when `kind` is a press or repeat of the unmodified key `code`.
fn plain_key(kind: &EventKind, wanted: u32) -> bool {
    matches!(kind, EventKind::Key { code, modifiers: 0, pressed: 1 | 2 } if *code == wanted)
}

/// Inspect the complete lines of `buf` from `*checked` on; true when one is the END line.
fn end_line_seen(buf: &Secret, checked: &mut usize) -> bool {
    let bytes = buf.as_slice();
    while let Some(at) = bytes[*checked..].iter().position(|&b| b == b'\n') {
        let line = &bytes[*checked..*checked + at];
        *checked += at + 1;
        if line.strip_suffix(b"\r").unwrap_or(line) == END {
            return true;
        }
    }
    false
}

/// Consume and drop the remainder of a running paste (bounded), then stop.
fn drain_paste(events: &mut dyn Events) {
    let mut seen = 0usize;
    while seen < DRAIN_MAX {
        match events.next() {
            Next::Lost => return,
            Next::Event(mut kind) => {
                match &kind {
                    EventKind::PasteEnd | EventKind::Eof | EventKind::Lost(_) => return,
                    EventKind::Text(text) => seen += text.len().max(1),
                    _ => seen += 1,
                }
                kind.scrub();
            }
        }
    }
}

/// Read the block into `buf` (capacity `armor::TEXT_MAX`). On every result but `Done` the
/// caller wipes `buf`.
pub fn read_block(events: &mut dyn Events, buf: &mut Secret) -> Block {
    let mut paste = false;
    let mut checked = 0usize;
    let outcome = loop {
        let mut kind = match events.next() {
            Next::Lost => return Block::Lost,
            Next::Event(kind) => kind,
        };
        let step = match &kind {
            EventKind::PasteBegin => {
                paste = true;
                None
            }
            EventKind::PasteEnd => {
                paste = false;
                // The last line of a paste has no terminator: give it one.
                if !buf.is_empty() && buf.as_slice().last() != Some(&b'\n') && !buf.push(b'\n') {
                    Some(Block::TooLong)
                } else if end_line_seen(buf, &mut checked) {
                    Some(Block::Done)
                } else {
                    None
                }
            }
            EventKind::Text(text) => {
                if !buf.push_slice(text.as_bytes()) {
                    Some(Block::TooLong)
                } else if text.contains('\n') && end_line_seen(buf, &mut checked) {
                    Some(Block::Done)
                } else {
                    None
                }
            }
            EventKind::Key { code, modifiers: mods, pressed: 1 | 2 } if is_submit(*code, *mods) => {
                if !buf.push(b'\n') {
                    Some(Block::TooLong)
                } else if end_line_seen(buf, &mut checked) {
                    Some(Block::Done)
                } else {
                    None
                }
            }
            kind if plain_key(kind, key_code::ESCAPE) => Some(Block::Cancelled),
            kind if plain_key(kind, key_code::BACKSPACE) => {
                if !paste {
                    buf.pop();
                    // A removed line break reopens its line for inspection.
                    if checked > buf.len() {
                        checked = buf.as_slice().iter().rposition(|&b| b == b'\n').map_or(0, |at| at + 1);
                    }
                }
                None
            }
            EventKind::Interrupt | EventKind::Eof => Some(Block::Cancelled),
            EventKind::Lost(_) => return Block::Lost,
            _ => None,
        };
        kind.scrub();
        if let Some(outcome) = step {
            break outcome;
        }
    };
    if paste {
        drain_paste(events);
    }
    outcome
}

/// Read one typed word: text and Backspace edit it, Enter submits it, a paste is ignored.
pub fn read_word(events: &mut dyn Events) -> Word {
    let mut word = alloc::string::String::new();
    let mut paste = false;
    loop {
        let mut kind = match events.next() {
            Next::Lost => return Word::Lost,
            Next::Event(kind) => kind,
        };
        let done = match &kind {
            EventKind::PasteBegin => {
                paste = true;
                None
            }
            EventKind::PasteEnd => {
                paste = false;
                None
            }
            EventKind::Text(text) if !paste => {
                if word.len() + text.len() <= WORD_MAX {
                    word.push_str(text);
                } else {
                    // Never the confirmation word, and never longer than the bound.
                    word.clear();
                    word.push_str("?");
                }
                None
            }
            EventKind::Key { code, modifiers: mods, pressed: 1 | 2 } if !paste && is_submit(*code, *mods) => {
                Some(Word::Given(core::mem::take(&mut word)))
            }
            kind if plain_key(kind, key_code::ESCAPE) => Some(Word::Cancelled),
            kind if !paste && plain_key(kind, key_code::BACKSPACE) => {
                word.pop();
                None
            }
            EventKind::Interrupt | EventKind::Eof => Some(Word::Cancelled),
            EventKind::Lost(_) => Some(Word::Lost),
            _ => None,
        };
        kind.scrub();
        if let Some(done) = done {
            if paste {
                drain_paste(events);
            }
            return done;
        }
    }
}

#[cfg(not(test))]
pub use tty::{refuse_without_terminal, Tty};

/// The real console: the foreground session of this child.
#[cfg(not(test))]
mod tty {
    use super::{read_block, read_word, Block, Console, Events, Next, Word};
    use crate::secret::Secret;
    use api::cell_session::Status;
    use ostd::child_context::{ChildContext, GrantSlot};
    use terminal_sdk::session::TerminalSession;
    use terminal_sdk::stream::{Endpoint, EndpointRef, OperationRef};

    const FOREVER: u64 = u64::MAX;

    /// Say why on this child's stderr stream, when it has one (a pipeline stage does).
    pub fn refuse_without_terminal(context: &ChildContext) {
        let Some(grant) = context.grants().find(|g| g.slot == GrantSlot::Stderr && g.kind == 1) else {
            return;
        };
        let (Ok(operation), Ok(object)) = (OperationRef::from_raw(context.operation()), EndpointRef::from_raw(grant.object))
        else {
            return;
        };
        let endpoint = Endpoint::new(object, operation);
        let mut data = crate::report::NO_TERMINAL.as_bytes();
        while !data.is_empty() {
            match endpoint.write(data, FOREVER) {
                Ok(n) if n > 0 && n <= data.len() => data = &data[n..],
                _ => return,
            }
        }
    }

    /// Longest wait for one event; the loop re-arms it, so a cancel is never starved.
    const SLICE_MS: u64 = 1_000;

    pub struct Tty {
        session: TerminalSession,
        lost: bool,
    }

    impl Tty {
        pub fn new(session: TerminalSession) -> Self {
            Self { session, lost: false }
        }

        fn write(&mut self, stderr: bool, mut data: &[u8]) {
            while !data.is_empty() {
                let written = if stderr {
                    self.session.write_stderr(data, FOREVER)
                } else {
                    self.session.write_stdout(data, FOREVER)
                };
                match written {
                    Ok(n) if n > 0 && n <= data.len() => data = &data[n..],
                    Err(status) => return note(&mut self.lost, status),
                    Ok(_) => return,
                }
            }
        }
    }

    /// These answers mean the session (or this child's foreground in it) is gone for good.
    fn note(lost: &mut bool, status: Status) {
        if matches!(
            status,
            Status::Stale | Status::Revoked | Status::BrokenPipe | Status::WrongSession | Status::WrongEpoch
        ) {
            *lost = true;
        }
    }

    struct Source<'a> {
        session: &'a mut TerminalSession,
        lost: &'a mut bool,
    }

    impl Events for Source<'_> {
        fn next(&mut self) -> Next {
            loop {
                // Without the monotonic clock the wait is a one-tick poll.
                let until = ostd::session::monotonic_ns()
                    .map_or(0, |now| now.saturating_add(SLICE_MS.saturating_mul(1_000_000)));
                match self.session.next_event(until) {
                    Ok(event) => return Next::Event(event.kind),
                    Err(Status::TimedOut | Status::WouldBlock) => {
                        if until == 0 {
                            let _ = ostd::syscall::sys_set_timer(1);
                        }
                    }
                    Err(status) => {
                        note(self.lost, status);
                        return Next::Lost;
                    }
                }
            }
        }
    }

    impl Console for Tty {
        fn out(&mut self, text: &str) {
            self.write(false, text.as_bytes());
        }

        fn err(&mut self, text: &str) {
            self.write(true, text.as_bytes());
        }

        fn read_key_block(&mut self, buf: &mut Secret) -> Block {
            let mut guard = match self.session.set_mode(2, 0) {
                Ok(guard) => guard,
                Err(status) => {
                    note(&mut self.lost, status);
                    return Block::Lost;
                }
            };
            let result = read_block(&mut Source { session: guard.session(), lost: &mut self.lost }, buf);
            match guard.release() {
                Ok(()) => result,
                Err(status) => {
                    note(&mut self.lost, status);
                    Block::Lost
                }
            }
        }

        fn read_word(&mut self) -> Word {
            read_word(&mut Source { session: &mut self.session, lost: &mut self.lost })
        }

        fn lost(&self) -> bool {
            self.lost
        }
    }
}
