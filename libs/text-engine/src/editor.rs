//! Pure edit state for `cellbox edit` (contract: command-contracts §3).
//!
//! The document is its bytes. [`EditBuffer`] keeps them split into lines, each
//! with its own terminator (`LF`, `CRLF` or none for the last line), so a file
//! reopens byte for byte: mixed endings, a lone CR (ordinary content), tabs
//! and a missing or present final newline all survive. Every mutation is a
//! splice of the serialized bytes and is recorded as its exact inverse, so undo
//! restores bytes rather than an approximation.
//!
//! Positions are byte offsets that always sit on a UTF-8 scalar boundary and
//! never inside a terminator. Display width is the caller's: vertical motion
//! takes a `width` function (the terminal SDK's rules); tabs are 8-column
//! stops here.
//!
//! Limits are explicit and refused before mutation: 4 MiB, 262,144 lines, no
//! NUL, 256 KiB clipboard and paste, and undo bounded by 1,024 groups and
//! 1 MiB of retained payload across undo and redo.
//!
//! Ideas, not code, follow Kibi (MIT OR Apache-2.0); no Kibi source is copied.

pub mod highlight;
#[cfg(test)]
mod tests;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

/// Largest document, in stored bytes.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
/// Most logical lines.
pub const MAX_LINES: usize = 262_144;
/// Largest clipboard content and largest single paste.
pub const MAX_CLIPBOARD: usize = 256 * 1024;
/// Most retained undo + redo groups.
pub const MAX_UNDO_GROUPS: usize = 1024;
/// Most retained old + new payload bytes across undo + redo.
pub const MAX_UNDO_BYTES: usize = 1024 * 1024;
/// Longest search query.
pub const MAX_QUERY: usize = 1024;
/// Tab stop width in display columns.
pub const TAB_STOP: usize = 8;

/// Line terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// Only the last line.
    None,
    Lf,
    CrLf,
}

impl Ending {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub ending: Ending,
}

/// Why a document cannot be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    InvalidUtf8,
    Nul,
    TooLarge,
    TooManyLines,
}

/// Why an edit was refused; nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    Nul,
    TooLarge,
    TooManyLines,
    /// The change would not fit the undo budget on its own.
    UndoBudget,
    ClipboardFull,
}

/// A cursor position: line index and byte offset in its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub line: usize,
    pub byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    At(Pos),
    /// Found after wrapping to the start once.
    Wrapped(Pos),
    NotFound,
}

#[derive(Debug, Clone)]
struct Edit {
    offset: usize,
    removed: String,
    inserted: String,
    cursor_before: Pos,
    anchor_before: Option<Pos>,
    cursor_after: Pos,
}

#[derive(Debug, Clone)]
struct Group {
    id: u64,
    edit: Edit,
}

impl Group {
    fn payload(&self) -> usize {
        self.edit.removed.len() + self.edit.inserted.len()
    }
}

#[derive(Debug, Clone)]
pub struct EditBuffer {
    lines: Vec<Line>,
    len: usize,
    cursor: Pos,
    anchor: Option<Pos>,
    desired_col: Option<usize>,
    clipboard: String,
    query: String,
    undo: VecDeque<Group>,
    redo: Vec<Group>,
    retained: usize,
    next_id: u64,
    /// State id below the oldest retained undo group.
    floor: u64,
    saved: u64,
}

impl Default for EditBuffer {
    fn default() -> Self {
        Self::from_parsed(alloc::vec![Line { text: String::new(), ending: Ending::None }], 0)
    }
}

fn parse(s: &str) -> Vec<Line> {
    let bytes = s.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            let (end, ending) = if i > start && bytes[i - 1] == b'\r' { (i - 1, Ending::CrLf) } else { (i, Ending::Lf) };
            lines.push(Line { text: String::from(&s[start..end]), ending });
            start = i + 1;
        }
    }
    lines.push(Line { text: String::from(&s[start..]), ending: Ending::None });
    lines
}

fn serialized_len(line: &Line) -> usize {
    line.text.len() + line.ending.as_str().len()
}

impl EditBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open exact stored bytes, refusing what the editor cannot keep intact.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, OpenError> {
        if bytes.len() > MAX_BYTES {
            return Err(OpenError::TooLarge);
        }
        if bytes.contains(&0) {
            return Err(OpenError::Nul);
        }
        let text = core::str::from_utf8(bytes).map_err(|_| OpenError::InvalidUtf8)?;
        if bytes.iter().filter(|&&b| b == b'\n').count() + 1 > MAX_LINES {
            return Err(OpenError::TooManyLines);
        }
        Ok(Self::from_parsed(parse(text), bytes.len()))
    }

    fn from_parsed(lines: Vec<Line>, len: usize) -> Self {
        Self {
            lines,
            len,
            cursor: Pos::default(),
            anchor: None,
            desired_col: None,
            clipboard: String::new(),
            query: String::new(),
            undo: VecDeque::new(),
            redo: Vec::new(),
            retained: 0,
            next_id: 1,
            floor: 0,
            saved: 0,
        }
    }

    /// The exact document bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len);
        for line in &self.lines {
            out.extend_from_slice(line.text.as_bytes());
            out.extend_from_slice(line.ending.as_str().as_bytes());
        }
        out
    }

    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
    pub fn lines(&self) -> &[Line] { &self.lines }
    pub fn line_count(&self) -> usize { self.lines.len() }
    pub fn cursor(&self) -> Pos { self.cursor }
    pub fn clipboard(&self) -> &str { &self.clipboard }
    pub fn last_query(&self) -> &str { &self.query }

    /// Ordered half-open selection, when an anchor is set and differs from the cursor.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    pub fn has_anchor(&self) -> bool { self.anchor.is_some() }

    /// Whether the bytes may differ from the last saved state. Conservative:
    /// an edit and its manual reversal stay dirty; undo back to the saved
    /// state is clean.
    pub fn is_dirty(&self) -> bool { self.state() != self.saved }

    /// The current bytes were committed to storage.
    pub fn mark_saved(&mut self) { self.saved = self.state(); }

    fn state(&self) -> u64 { self.undo.back().map_or(self.floor, |g| g.id) }

    // ── Offsets ──────────────────────────────────────────────────────────

    fn line_start(&self, line: usize) -> usize {
        self.lines[..line].iter().map(serialized_len).sum()
    }

    fn offset(&self, pos: Pos) -> usize { self.line_start(pos.line) + pos.byte }

    /// Line containing serialized offset `off` and that line's start.
    fn line_at(&self, off: usize) -> (usize, usize) {
        let mut start = 0;
        for (i, line) in self.lines.iter().enumerate() {
            let next = start + serialized_len(line);
            if off < next || i + 1 == self.lines.len() {
                return (i, start);
            }
            start = next;
        }
        unreachable!("a buffer always has a line")
    }

    /// Cursor position for `off`; an offset inside a terminator moves past it.
    fn pos_of(&self, off: usize) -> Pos {
        let (line, start) = self.line_at(off);
        let byte = off - start;
        if byte > self.lines[line].text.len() && line + 1 < self.lines.len() {
            return Pos { line: line + 1, byte: 0 };
        }
        Pos { line, byte: byte.min(self.lines[line].text.len()) }
    }

    /// Replace `len` serialized bytes at `off` with `ins`; returns the removed bytes.
    fn splice(&mut self, off: usize, len: usize, ins: &str) -> String {
        let (s, s_start) = self.line_at(off);
        let (mut e, _) = self.line_at(off + len);
        // Include the following line so a removed terminator rejoins correctly.
        if e + 1 < self.lines.len() {
            e += 1;
        }
        let e_is_last = e + 1 == self.lines.len();
        let mut region = String::new();
        for line in &self.lines[s..=e] {
            region.push_str(&line.text);
            region.push_str(line.ending.as_str());
        }
        let rel = off - s_start;
        let removed = String::from(&region[rel..rel + len]);
        region.replace_range(rel..rel + len, ins);
        let mut fresh = parse(&region);
        if !e_is_last {
            // The region ended with a terminator: its empty tail is not a line.
            fresh.pop();
        }
        self.lines.splice(s..=e, fresh);
        self.len = self.len - len + ins.len();
        removed
    }

    // ── Editing ──────────────────────────────────────────────────────────

    /// Validate, apply and record one edit group.
    fn edit(&mut self, off: usize, len: usize, ins: &str) -> Result<(), EditError> {
        if ins.as_bytes().contains(&0) {
            return Err(EditError::Nul);
        }
        if self.len - len + ins.len() > MAX_BYTES {
            return Err(EditError::TooLarge);
        }
        if len + ins.len() > MAX_UNDO_BYTES {
            return Err(EditError::UndoBudget);
        }
        // Each '\n' ends a line, so a removed range holds one per line boundary it spans.
        let removed_breaks = if len == 0 { 0 } else { self.line_at(off + len).0 - self.line_at(off).0 };
        let added = ins.bytes().filter(|&b| b == b'\n').count();
        if self.lines.len() - removed_breaks + added > MAX_LINES {
            return Err(EditError::TooManyLines);
        }
        let cursor_before = self.cursor;
        let anchor_before = self.anchor;
        let removed = self.splice(off, len, ins);
        self.cursor = self.pos_of(off + ins.len());
        self.anchor = None;
        self.desired_col = None;
        let group = Group {
            id: self.next_id,
            edit: Edit { offset: off, removed, inserted: String::from(ins), cursor_before, anchor_before, cursor_after: self.cursor },
        };
        self.next_id += 1;
        self.retained -= self.redo.drain(..).map(|g| g.payload()).sum::<usize>();
        self.retained += group.payload();
        self.undo.push_back(group);
        while self.undo.len() + self.redo.len() > MAX_UNDO_GROUPS || self.retained > MAX_UNDO_BYTES {
            let Some(oldest) = self.undo.pop_front() else { break };
            self.retained -= oldest.payload();
            self.floor = oldest.id;
        }
        Ok(())
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection().map(|(a, b)| {
            let start = self.offset(a);
            (start, self.offset(b) - start)
        })
    }

    /// Insert text at the cursor, replacing the selection.
    pub fn insert(&mut self, text: &str) -> Result<(), EditError> {
        let (off, len) = self.selection_range().unwrap_or((self.offset(self.cursor), 0));
        self.edit(off, len, text)
    }

    /// The terminator Enter inserts: the cursor line's own, else the nearest
    /// preceding one, else the first in the document, else LF.
    pub fn newline_ending(&self) -> Ending {
        let line = self.cursor.line;
        self.lines[..=line].iter().rev().map(|l| l.ending)
            .chain(self.lines[line..].iter().map(|l| l.ending))
            .find(|e| *e != Ending::None)
            .unwrap_or(Ending::Lf)
    }

    pub fn newline(&mut self) -> Result<(), EditError> {
        self.insert(self.newline_ending().as_str())
    }

    /// Delete the selection, or the scalar / terminator before the cursor.
    pub fn backspace(&mut self) -> Result<(), EditError> {
        if let Some((off, len)) = self.selection_range() {
            return self.edit(off, len, "");
        }
        let Pos { line, byte } = self.cursor;
        let off = self.offset(self.cursor);
        if byte > 0 {
            let prev = self.lines[line].text[..byte].chars().next_back().map_or(0, char::len_utf8);
            self.edit(off - prev, prev, "")
        } else if line > 0 {
            let unit = self.lines[line - 1].ending.as_str().len();
            self.edit(off - unit, unit, "")
        } else {
            Ok(())
        }
    }

    /// Delete the selection, or the scalar / terminator after the cursor.
    pub fn delete(&mut self) -> Result<(), EditError> {
        if let Some((off, len)) = self.selection_range() {
            return self.edit(off, len, "");
        }
        let Pos { line, byte } = self.cursor;
        let text = &self.lines[line].text;
        let unit = match text[byte..].chars().next() {
            Some(c) => c.len_utf8(),
            None => self.lines[line].ending.as_str().len(),
        };
        if unit == 0 {
            return Ok(());
        }
        self.edit(self.offset(self.cursor), unit, "")
    }

    fn selected_text(&self) -> Option<String> {
        let (off, len) = self.selection_range()?;
        Some(String::from_utf8(self.to_bytes()[off..off + len].to_vec()).expect("scalar-boundary range"))
    }

    /// Copy the selection; `Ok(false)` when there is none.
    pub fn copy(&mut self) -> Result<bool, EditError> {
        let Some(text) = self.selected_text() else { return Ok(false) };
        if text.len() > MAX_CLIPBOARD {
            return Err(EditError::ClipboardFull);
        }
        self.clipboard = text;
        Ok(true)
    }

    /// Copy, then delete the selection.
    pub fn cut(&mut self) -> Result<bool, EditError> {
        let Some(text) = self.selected_text() else { return Ok(false) };
        if text.len() > MAX_CLIPBOARD {
            return Err(EditError::ClipboardFull);
        }
        let (off, len) = self.selection_range().expect("selection");
        self.edit(off, len, "")?;
        self.clipboard = text;
        Ok(true)
    }

    /// Paste the private clipboard over the selection; no-op when empty.
    pub fn paste(&mut self) -> Result<(), EditError> {
        if self.clipboard.is_empty() {
            return Ok(());
        }
        let text = core::mem::take(&mut self.clipboard);
        let result = self.insert(&text);
        self.clipboard = text;
        result
    }

    /// One bracketed paste: literal text, one undo group, whole-event refusal.
    pub fn paste_text(&mut self, text: &str) -> Result<(), EditError> {
        if text.len() > MAX_CLIPBOARD {
            return Err(EditError::ClipboardFull);
        }
        self.insert(text)
    }

    pub fn undo(&mut self) -> bool {
        let Some(group) = self.undo.pop_back() else { return false };
        let e = &group.edit;
        self.splice(e.offset, e.inserted.len(), &e.removed);
        self.cursor = e.cursor_before;
        self.anchor = e.anchor_before;
        self.desired_col = None;
        self.redo.push(group);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(group) = self.redo.pop() else { return false };
        let e = &group.edit;
        self.splice(e.offset, e.removed.len(), &e.inserted);
        self.cursor = e.cursor_after;
        self.anchor = None;
        self.desired_col = None;
        self.undo.push_back(group);
        true
    }

    pub fn undo_depth(&self) -> (usize, usize) { (self.undo.len(), self.redo.len()) }
    pub fn retained_undo_bytes(&self) -> usize { self.retained }

    // ── Selection and motion ─────────────────────────────────────────────

    /// Set the anchor at the cursor, or clear it.
    pub fn toggle_anchor(&mut self) {
        self.anchor = match self.anchor {
            Some(_) => None,
            None => Some(self.cursor),
        };
    }

    pub fn clear_selection(&mut self) { self.anchor = None; }

    fn moved(&mut self, pos: Pos) {
        self.cursor = pos;
        self.desired_col = None;
    }

    pub fn left(&mut self) {
        let Pos { line, byte } = self.cursor;
        if byte > 0 {
            let prev = self.lines[line].text[..byte].chars().next_back().map_or(0, char::len_utf8);
            self.moved(Pos { line, byte: byte - prev });
        } else if line > 0 {
            self.moved(Pos { line: line - 1, byte: self.lines[line - 1].text.len() });
        }
    }

    pub fn right(&mut self) {
        let Pos { line, byte } = self.cursor;
        match self.lines[line].text[byte..].chars().next() {
            Some(c) => self.moved(Pos { line, byte: byte + c.len_utf8() }),
            None if line + 1 < self.lines.len() => self.moved(Pos { line: line + 1, byte: 0 }),
            None => {}
        }
    }

    pub fn home(&mut self) { self.moved(Pos { line: self.cursor.line, byte: 0 }); }

    pub fn end(&mut self) {
        let line = self.cursor.line;
        self.moved(Pos { line, byte: self.lines[line].text.len() });
    }

    /// Display column of `byte` in `line`: tabs to the next stop, other
    /// scalars by `width`.
    pub fn column(&self, line: usize, byte: usize, width: &dyn Fn(char) -> usize) -> usize {
        let mut col = 0;
        for c in self.lines[line].text[..byte].chars() {
            col = if c == '\t' { (col / TAB_STOP + 1) * TAB_STOP } else { col + width(c) };
        }
        col
    }

    /// Move `delta` lines keeping the desired display column, landing on the
    /// closest scalar boundary and never inside a wide cell.
    pub fn vertical(&mut self, delta: isize, width: &dyn Fn(char) -> usize) {
        let desired = self.desired_col.unwrap_or_else(|| self.column(self.cursor.line, self.cursor.byte, width));
        let target = (self.cursor.line as isize + delta).clamp(0, self.lines.len() as isize - 1) as usize;
        let mut col = 0;
        let mut byte = 0;
        for c in self.lines[target].text.chars() {
            let next = if c == '\t' { (col / TAB_STOP + 1) * TAB_STOP } else { col + width(c) };
            if next > desired {
                if next - desired < desired - col {
                    byte += c.len_utf8();
                }
                break;
            }
            col = next;
            byte += c.len_utf8();
        }
        self.cursor = Pos { line: target, byte };
        self.desired_col = Some(desired);
    }

    /// Page motion: one viewport height minus one line, at least one.
    pub fn page(&mut self, down: bool, viewport: usize, width: &dyn Fn(char) -> usize) {
        let step = viewport.saturating_sub(1).max(1) as isize;
        self.vertical(if down { step } else { -step }, width);
    }

    /// Go to 1-based logical line `n`; out of range changes nothing.
    pub fn go_to_line(&mut self, n: usize) -> bool {
        if n == 0 || n > self.lines.len() {
            return false;
        }
        self.moved(Pos { line: n - 1, byte: 0 });
        true
    }

    /// Literal, case-sensitive search after the cursor, wrapping once.
    pub fn find(&mut self, query: &str) -> Found {
        if query.is_empty() || query.len() > MAX_QUERY {
            return Found::NotFound;
        }
        self.query = String::from(query);
        self.find_next()
    }

    /// Repeat the last non-empty query.
    pub fn find_next(&mut self) -> Found {
        if self.query.is_empty() {
            return Found::NotFound;
        }
        let doc = String::from_utf8(self.to_bytes()).expect("buffer is UTF-8");
        let here = self.offset(self.cursor);
        let from = doc[here..].chars().next().map_or(here, |c| here + c.len_utf8());
        let found = match doc[from..].find(self.query.as_str()) {
            Some(i) => Found::At(self.pos_of(from + i)),
            None => match doc.find(self.query.as_str()) {
                Some(i) => Found::Wrapped(self.pos_of(i)),
                None => Found::NotFound,
            },
        };
        if let Found::At(pos) | Found::Wrapped(pos) = found {
            self.anchor = None;
            self.moved(pos);
        }
        found
    }
}
