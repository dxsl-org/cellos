// SPDX-License-Identifier: MIT
//! `cellos-vt1` screen model of the GUI Terminal (spec §4 "Terminal profile").
//!
//! Cells carry a character (plus at most one combining mark) and the attributes the
//! renderer draws: foreground/background colour, bold and inverse. The model has
//! autowrap with a pending-wrap state, tab stops every 8 columns, scroll margins,
//! primary and alternate screens (`?1049` saves/restores the cursor and leaves the
//! primary content untouched), cursor visibility and the bracketed-paste flag.
//!
//! Width follows `unicode-width` `width()` (ambiguous = 1): combining marks join the
//! previous cell, wide characters take a lead cell plus a trailing cell, and the cursor
//! never rests on a trailing half. Every allocation is bounded: rows 1..=256, columns
//! 1..=512, at most 131,072 cells per screen, scrollback at most 1,024 lines and 1 MiB.
//!
//! Line feed also returns the carriage: CellOS sessions have no output line discipline
//! (no ONLCR), so programs write bare `\n` and expect the next line to start at column 0.

extern crate alloc;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use unicode_width::UnicodeWidthChar;

pub const MAX_COLS: usize = 512;
pub const MAX_ROWS: usize = 256;
pub const MAX_CELLS: usize = 131_072;
pub const MAX_SCROLLBACK_LINES: usize = 1024;
pub const MAX_SCROLLBACK_BYTES: usize = 1 << 20;
pub const TAB_WIDTH: usize = 8;

/// A colour as the byte stream selected it; the renderer resolves it to pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    /// 0..=15 are the 16 ANSI colours (SGR 30-37/90-97), 16..=255 the 256-colour cube
    /// and grey ramp.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Graphic rendition the screen keeps per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pen {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub inverse: bool,
}

impl Pen {
    pub const DEFAULT: Self = Self { fg: Color::Default, bg: Color::Default, bold: false, inverse: false };
}

/// How many columns a cell's character covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Narrow,
    /// First column of a two-column character.
    Wide,
    /// Second column of a two-column character; draws nothing itself.
    Trail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCell {
    pub ch: char,
    /// One combining mark joined to `ch`; further marks are dropped (bounded storage).
    pub mark: Option<char>,
    pub pen: Pen,
    pub width: Width,
}

impl TerminalCell {
    pub const BLANK: Self = Self { ch: ' ', mark: None, pen: Pen::DEFAULT, width: Width::Narrow };
}

/// Why a resize was refused; the previous screen is kept unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeError {
    /// Outside rows 1..=256, columns 1..=512 or 131,072 cells.
    OutOfBounds,
    /// The new screen could not be allocated.
    NoMemory,
}

#[derive(Debug, Clone, Copy)]
struct Saved {
    x: usize,
    y: usize,
    pen: Pen,
    wrap_pending: bool,
}

pub struct TerminalGrid {
    pub cols: usize,
    pub rows: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
    /// The last column was written with autowrap on: the next printable wraps first.
    wrap_pending: bool,
    /// The active screen.
    cells: Vec<TerminalCell>,
    /// The primary screen while the alternate screen is active; empty otherwise.
    primary: Vec<TerminalCell>,
    alternate: bool,
    pen: Pen,
    /// ESC 7 / CSI s slot per screen: `[primary, alternate]`. `?1049h` uses the primary's.
    saved: [Option<Saved>; 2],
    /// Scroll margins, 0-based inclusive.
    top: usize,
    bottom: usize,
    autowrap: bool,
    cursor_visible: bool,
    bracketed_paste: bool,
    scrollback: VecDeque<Box<[TerminalCell]>>,
    scrollback_bytes: usize,
}

fn checked_dims(cols: usize, rows: usize) -> Result<usize, ResizeError> {
    if !(1..=MAX_COLS).contains(&cols) || !(1..=MAX_ROWS).contains(&rows) || cols * rows > MAX_CELLS {
        return Err(ResizeError::OutOfBounds);
    }
    Ok(cols * rows)
}

fn blank_screen(cells: usize) -> Result<Vec<TerminalCell>, ResizeError> {
    let mut screen = Vec::new();
    screen.try_reserve_exact(cells).map_err(|_| ResizeError::NoMemory)?;
    screen.resize(cells, TerminalCell::BLANK);
    Ok(screen)
}

/// Copy rows `shift..` of `old` (`old_cols` wide) into a blank `new_cols x new_rows` screen.
fn relayout(
    old: &[TerminalCell],
    old_cols: usize,
    new_cols: usize,
    new_rows: usize,
    shift: usize,
) -> Result<Vec<TerminalCell>, ResizeError> {
    let mut screen = blank_screen(new_cols * new_rows)?;
    let old_rows = old.len() / old_cols;
    let keep = new_cols.min(old_cols);
    for (dst, src) in (shift..old_rows).take(new_rows).enumerate() {
        let row = &mut screen[dst * new_cols..dst * new_cols + keep];
        row.copy_from_slice(&old[src * old_cols..src * old_cols + keep]);
        // A wide character whose trailing half was cut off is gone.
        if keep < old_cols && row[keep - 1].width == Width::Wide {
            row[keep - 1] = TerminalCell { ch: ' ', mark: None, width: Width::Narrow, ..row[keep - 1] };
        }
    }
    Ok(screen)
}

/// Append one screen line to the scrollback (trailing default blanks trimmed), dropping
/// the oldest lines first so both the line and the byte cap hold.
fn keep_line(scrollback: &mut VecDeque<Box<[TerminalCell]>>, total: &mut usize, row: &[TerminalCell]) {
    let used = row.iter().rposition(|cell| *cell != TerminalCell::BLANK).map_or(0, |last| last + 1);
    let bytes = used * core::mem::size_of::<TerminalCell>();
    while !scrollback.is_empty() && (scrollback.len() >= MAX_SCROLLBACK_LINES || *total + bytes > MAX_SCROLLBACK_BYTES) {
        if let Some(old) = scrollback.pop_front() {
            *total -= old.len() * core::mem::size_of::<TerminalCell>();
        }
    }
    let mut line = Vec::new();
    if line.try_reserve_exact(used).is_err() || scrollback.try_reserve(1).is_err() {
        // Out of memory: the line is not kept; the screen itself is unaffected.
        return;
    }
    line.extend_from_slice(&row[..used]);
    scrollback.push_back(line.into_boxed_slice());
    *total += bytes;
}

impl TerminalGrid {
    /// A blank screen. Dimensions are clamped into the profile bounds.
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.clamp(1, MAX_COLS);
        let rows = rows.clamp(1, MAX_ROWS).min(MAX_CELLS / cols);
        Self {
            cols,
            rows,
            cursor_x: 0,
            cursor_y: 0,
            wrap_pending: false,
            cells: alloc::vec![TerminalCell::BLANK; cols * rows],
            primary: Vec::new(),
            alternate: false,
            pen: Pen::DEFAULT,
            saved: [None; 2],
            top: 0,
            bottom: rows - 1,
            autowrap: true,
            cursor_visible: true,
            bracketed_paste: false,
            scrollback: VecDeque::new(),
            scrollback_bytes: 0,
        }
    }

    pub fn cell_at(&self, col: usize, row: usize) -> Option<&TerminalCell> {
        if col < self.cols && row < self.rows {
            Some(&self.cells[row * self.cols + col])
        } else {
            None
        }
    }

    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    pub fn alternate_active(&self) -> bool {
        self.alternate
    }

    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    pub fn autowrap(&self) -> bool {
        self.autowrap
    }

    pub fn pen(&self) -> Pen {
        self.pen
    }

    /// Scroll margins as 0-based inclusive rows.
    pub fn margins(&self) -> (usize, usize) {
        (self.top, self.bottom)
    }

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    pub fn scrollback_bytes(&self) -> usize {
        self.scrollback_bytes
    }

    pub fn scrollback_line(&self, index: usize) -> Option<&[TerminalCell]> {
        self.scrollback.get(index).map(|line| &line[..])
    }

    /// Terminal-originated text (status notices): `\n` starts a new line, `\r` returns,
    /// everything else prints.
    pub fn write_str(&mut self, s: &str) {
        for ch in s.chars() {
            match ch {
                '\n' => self.line_feed(),
                '\r' => self.carriage_return(),
                _ => self.print(ch),
            }
        }
    }

    fn row_mut(&mut self, y: usize) -> &mut [TerminalCell] {
        let cols = self.cols;
        &mut self.cells[y * cols..(y + 1) * cols]
    }

    fn erase_cell(&self) -> TerminalCell {
        TerminalCell { pen: Pen { bg: self.pen.bg, ..Pen::DEFAULT }, ..TerminalCell::BLANK }
    }

    /// The cursor never rests on the trailing half of a wide character.
    fn settle(&mut self) {
        if self.cursor_x > 0 && self.cells[self.cursor_y * self.cols + self.cursor_x].width == Width::Trail {
            self.cursor_x -= 1;
        }
    }

    /// Writing over or erasing either half of a wide character removes the whole character.
    fn break_wide(&mut self, y: usize, x: usize) {
        let cols = self.cols;
        let row = self.row_mut(y);
        let lead = match row[x].width {
            Width::Narrow => return,
            Width::Wide => x,
            Width::Trail => x.saturating_sub(1),
        };
        for cell in row[lead..(lead + 2).min(cols)].iter_mut() {
            *cell = TerminalCell { ch: ' ', mark: None, width: Width::Narrow, ..*cell };
        }
    }

    /// Fill columns `from..to` of row `y` with erased cells.
    fn erase(&mut self, y: usize, from: usize, to: usize) {
        if from >= to {
            return;
        }
        self.break_wide(y, from);
        self.break_wide(y, to - 1);
        let blank = self.erase_cell();
        self.row_mut(y)[from..to].fill(blank);
    }

    pub fn print(&mut self, ch: char) {
        let width = match ch.width() {
            Some(width) => width,
            // Control characters never reach the screen.
            None => return,
        };
        if width == 0 {
            self.combine(ch);
            return;
        }
        if width == 2 && self.cols < 2 {
            return;
        }
        if self.wrap_pending {
            self.carriage_return();
            self.index();
        }
        if width == 2 && self.cursor_x + 1 >= self.cols {
            if self.autowrap {
                self.carriage_return();
                self.index();
            } else {
                self.cursor_x = self.cols - 2;
            }
        }
        let (x, y) = (self.cursor_x, self.cursor_y);
        self.break_wide(y, x);
        if width == 2 {
            self.break_wide(y, x + 1);
        }
        let pen = self.pen;
        let row = self.row_mut(y);
        if width == 2 {
            row[x] = TerminalCell { ch, mark: None, pen, width: Width::Wide };
            row[x + 1] = TerminalCell { ch: ' ', mark: None, pen, width: Width::Trail };
        } else {
            row[x] = TerminalCell { ch, mark: None, pen, width: Width::Narrow };
        }
        let next = x + width;
        if next >= self.cols {
            self.cursor_x = self.cols - 1;
            self.wrap_pending = self.autowrap;
        } else {
            self.cursor_x = next;
        }
        self.settle();
    }

    /// A zero-width character joins the cell written last; with no such cell on the
    /// line it is dropped.
    fn combine(&mut self, mark: char) {
        let x = if self.wrap_pending {
            self.cursor_x
        } else if self.cursor_x > 0 {
            self.cursor_x - 1
        } else {
            return;
        };
        let y = self.cursor_y;
        let row = self.row_mut(y);
        let x = if row[x].width == Width::Trail { x - 1 } else { x };
        if row[x].mark.is_none() {
            row[x].mark = Some(mark);
        }
    }

    pub fn carriage_return(&mut self) {
        self.cursor_x = 0;
        self.wrap_pending = false;
    }

    /// LF: next line at column 0 (see the module note on ONLCR).
    pub fn line_feed(&mut self) {
        self.carriage_return();
        self.index();
    }

    /// Cursor down one line; at the bottom margin the region scrolls up.
    fn index(&mut self) {
        self.wrap_pending = false;
        if self.cursor_y == self.bottom {
            self.scroll_up(1);
        } else if self.cursor_y + 1 < self.rows {
            self.cursor_y += 1;
        }
        self.settle();
    }

    pub fn backspace(&mut self) {
        let snapped_wide = self.wrap_pending
            && self.cells[self.cursor_y * self.cols + self.cursor_x].width == Width::Wide
            && self.cursor_x + 2 == self.cols;
        self.wrap_pending = false;
        if !snapped_wide && self.cursor_x > 0 {
            self.cursor_x -= 1;
        }
        self.settle();
    }

    pub fn tab(&mut self) {
        self.wrap_pending = false;
        let next = (self.cursor_x / TAB_WIDTH + 1) * TAB_WIDTH;
        self.cursor_x = next.min(self.cols - 1);
        self.settle();
    }

    fn moved(&mut self) {
        self.wrap_pending = false;
        self.settle();
    }

    pub fn cursor_up(&mut self, n: usize) {
        let limit = if self.cursor_y >= self.top { self.top } else { 0 };
        self.cursor_y = self.cursor_y.saturating_sub(n).max(limit);
        self.moved();
    }

    pub fn cursor_down(&mut self, n: usize) {
        let limit = if self.cursor_y <= self.bottom { self.bottom } else { self.rows - 1 };
        self.cursor_y = self.cursor_y.saturating_add(n).min(limit);
        self.moved();
    }

    pub fn cursor_forward(&mut self, n: usize) {
        self.cursor_x = self.cursor_x.saturating_add(n).min(self.cols - 1);
        self.moved();
    }

    pub fn cursor_back(&mut self, n: usize) {
        self.cursor_x = self.cursor_x.saturating_sub(n);
        self.moved();
    }

    pub fn next_line(&mut self, n: usize) {
        self.cursor_down(n);
        self.carriage_return();
    }

    pub fn previous_line(&mut self, n: usize) {
        self.cursor_up(n);
        self.carriage_return();
    }

    /// CHA: 1-based column, clamped.
    pub fn set_column(&mut self, col: usize) {
        self.cursor_x = col.max(1).min(self.cols) - 1;
        self.moved();
    }

    /// CUP/HVP: 1-based row and column, clamped to the screen.
    pub fn set_position(&mut self, row: usize, col: usize) {
        self.cursor_y = row.max(1).min(self.rows) - 1;
        self.cursor_x = col.max(1).min(self.cols) - 1;
        self.moved();
    }

    /// ED 0 (cursor to end), 1 (start to cursor), 2 (all); other modes are ignored.
    pub fn erase_display(&mut self, mode: u16) {
        let (x, y) = (self.cursor_x, self.cursor_y);
        match mode {
            0 => {
                self.erase(y, x, self.cols);
                for row in y + 1..self.rows {
                    self.erase(row, 0, self.cols);
                }
            }
            1 => {
                for row in 0..y {
                    self.erase(row, 0, self.cols);
                }
                self.erase(y, 0, x + 1);
            }
            2 => {
                for row in 0..self.rows {
                    self.erase(row, 0, self.cols);
                }
            }
            _ => return,
        }
        self.wrap_pending = false;
    }

    /// EL 0 (cursor to end), 1 (start to cursor), 2 (whole line); other modes are ignored.
    pub fn erase_line(&mut self, mode: u16) {
        let (x, y) = (self.cursor_x, self.cursor_y);
        match mode {
            0 => self.erase(y, x, self.cols),
            1 => self.erase(y, 0, x + 1),
            2 => self.erase(y, 0, self.cols),
            _ => return,
        }
        self.wrap_pending = false;
    }

    /// DECSTBM with 1-based rows (0 = default). Invalid regions are ignored; a valid one
    /// homes the cursor.
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let top = top.max(1);
        let bottom = if bottom == 0 { self.rows } else { bottom.min(self.rows) };
        if top >= bottom {
            return;
        }
        self.top = top - 1;
        self.bottom = bottom - 1;
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.moved();
    }

    /// SU / LF at the bottom margin: the region moves up `n` lines. Lines leaving the
    /// top of the full primary screen go to scrollback.
    pub fn scroll_up(&mut self, n: usize) {
        let height = self.bottom - self.top + 1;
        let n = n.min(height);
        if n == 0 {
            return;
        }
        let cols = self.cols;
        if self.top == 0 && !self.alternate {
            for line in 0..n {
                let start = (self.top + line) * cols;
                keep_line(&mut self.scrollback, &mut self.scrollback_bytes, &self.cells[start..start + cols]);
            }
        }
        let (start, end) = (self.top * cols, (self.bottom + 1) * cols);
        self.cells.copy_within(start + n * cols..end, start);
        let blank = self.erase_cell();
        self.cells[end - n * cols..end].fill(blank);
        self.settle();
    }

    /// SD: the region moves down `n` lines; blank lines appear at the top margin.
    pub fn scroll_down(&mut self, n: usize) {
        let height = self.bottom - self.top + 1;
        let n = n.min(height);
        if n == 0 {
            return;
        }
        let cols = self.cols;
        let (start, end) = (self.top * cols, (self.bottom + 1) * cols);
        self.cells.copy_within(start..end - n * cols, start + n * cols);
        let blank = self.erase_cell();
        self.cells[start..start + n * cols].fill(blank);
        self.settle();
    }

    fn screen_slot(&self) -> usize {
        usize::from(self.alternate)
    }

    /// ESC 7 / CSI s: position, rendition and pending wrap of the active screen.
    pub fn save_cursor(&mut self) {
        let slot = self.screen_slot();
        self.saved[slot] = Some(Saved { x: self.cursor_x, y: self.cursor_y, pen: self.pen, wrap_pending: self.wrap_pending });
    }

    /// ESC 8 / CSI u. Without a saved state the cursor homes with the default rendition.
    pub fn restore_cursor(&mut self) {
        let saved = self.saved[self.screen_slot()].unwrap_or(Saved { x: 0, y: 0, pen: Pen::DEFAULT, wrap_pending: false });
        self.cursor_x = saved.x.min(self.cols - 1);
        self.cursor_y = saved.y.min(self.rows - 1);
        self.pen = saved.pen;
        self.wrap_pending = saved.wrap_pending && self.autowrap && self.cursor_x == self.cols - 1;
        self.settle();
    }

    /// DECSET/DECRST: ?7 autowrap, ?25 cursor visibility, ?1049 alternate screen,
    /// ?2004 bracketed paste. Other modes are ignored.
    pub fn set_private_mode(&mut self, mode: u16, on: bool) {
        match mode {
            7 => {
                self.autowrap = on;
                if !on {
                    self.wrap_pending = false;
                }
            }
            25 => self.cursor_visible = on,
            1049 => {
                if on {
                    self.enter_alternate();
                } else {
                    self.leave_alternate();
                }
            }
            2004 => self.bracketed_paste = on,
            _ => {}
        }
    }

    /// `?1049h`: save the cursor, switch to a cleared alternate screen. If the alternate
    /// screen cannot be allocated the primary stays active.
    fn enter_alternate(&mut self) {
        if self.alternate {
            return;
        }
        self.save_cursor();
        let Ok(screen) = blank_screen(self.cols * self.rows) else { return };
        self.primary = core::mem::replace(&mut self.cells, screen);
        self.alternate = true;
        self.saved[1] = None;
        self.settle();
    }

    /// `?1049l`: back to the untouched primary screen and its saved cursor. A no-op on
    /// the primary screen, so a soft reset after an ordinary program changes nothing.
    fn leave_alternate(&mut self) {
        if !self.alternate {
            return;
        }
        self.cells = core::mem::take(&mut self.primary);
        self.alternate = false;
        self.restore_cursor();
    }

    /// Back to the profile defaults (a new shell starts, or the shell ended): primary
    /// screen, visible cursor, autowrap, full margins, default rendition, no paste mode.
    pub fn reset_modes(&mut self) {
        self.leave_alternate();
        self.cursor_visible = true;
        self.autowrap = true;
        self.bracketed_paste = false;
        self.pen = Pen::DEFAULT;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.wrap_pending = false;
    }

    /// SGR: 0 reset, 1 bold, 22 normal, 7/27 inverse, 30-37/90-97 and 40-47/100-107
    /// colours, 38/48 `5;n` and `2;r;g;b`, 39/49 defaults. Dim, italic, underline, blink,
    /// hidden and strike (and their resets) are accepted without effect; unknown codes
    /// are ignored. A malformed 38/48 ends the sequence.
    pub fn select_graphic_rendition(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.pen = Pen::DEFAULT;
            return;
        }
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.pen = Pen::DEFAULT,
                1 => self.pen.bold = true,
                22 => self.pen.bold = false,
                7 => self.pen.inverse = true,
                27 => self.pen.inverse = false,
                code @ 30..=37 => self.pen.fg = Color::Indexed((code - 30) as u8),
                39 => self.pen.fg = Color::Default,
                code @ 40..=47 => self.pen.bg = Color::Indexed((code - 40) as u8),
                49 => self.pen.bg = Color::Default,
                code @ 90..=97 => self.pen.fg = Color::Indexed((code - 90 + 8) as u8),
                code @ 100..=107 => self.pen.bg = Color::Indexed((code - 100 + 8) as u8),
                code @ (38 | 48) => {
                    let Some((color, used)) = extended_color(&params[i + 1..]) else { return };
                    if code == 38 {
                        self.pen.fg = color;
                    } else {
                        self.pen.bg = color;
                    }
                    i += used;
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Resize both screens, keeping their content from the top-left. When the cursor
    /// line would fall off the bottom, the oldest lines leave the top instead (primary
    /// lines into scrollback). Out-of-bounds sizes and failed allocations change nothing.
    pub fn resize(&mut self, cols: usize, rows: usize) -> Result<(), ResizeError> {
        checked_dims(cols, rows)?;
        if cols == self.cols && rows == self.rows {
            return Ok(());
        }
        let shift = (self.cursor_y + 1).saturating_sub(rows);
        let cells = relayout(&self.cells, self.cols, cols, rows, shift)?;
        let (primary, primary_shift) = if self.alternate {
            let y = self.saved[0].map_or(0, |saved| saved.y);
            let primary_shift = (y + 1).saturating_sub(rows);
            (relayout(&self.primary, self.cols, cols, rows, primary_shift)?, primary_shift)
        } else {
            (Vec::new(), shift)
        };
        // Committed from here on: no further failure.
        let old_primary = if self.alternate { &self.primary } else { &self.cells };
        for line in old_primary.chunks_exact(self.cols).take(primary_shift) {
            keep_line(&mut self.scrollback, &mut self.scrollback_bytes, line);
        }
        self.cells = cells;
        self.primary = primary;
        self.cols = cols;
        self.rows = rows;
        self.cursor_y -= shift;
        self.cursor_x = self.cursor_x.min(cols - 1);
        if let Some(saved) = self.saved[0].as_mut() {
            saved.y = saved.y.saturating_sub(primary_shift);
        }
        self.top = 0;
        self.bottom = rows - 1;
        self.wrap_pending = false;
        self.settle();
        Ok(())
    }
}

/// `5;n` or `2;r;g;b` after SGR 38/48: the colour and how many parameters it used.
fn extended_color(rest: &[u16]) -> Option<(Color, usize)> {
    let byte = |value: u16| u8::try_from(value).ok();
    match rest {
        [5, index, ..] => Some((Color::Indexed(byte(*index)?), 2)),
        [2, r, g, b, ..] => Some((Color::Rgb(byte(*r)?, byte(*g)?, byte(*b)?), 4)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::String;

    fn row(grid: &TerminalGrid, r: usize) -> String {
        let text: String = (0..grid.cols)
            .filter(|&c| grid.cell_at(c, r).unwrap().width != Width::Trail)
            .flat_map(|c| {
                let cell = grid.cell_at(c, r).unwrap();
                core::iter::once(cell.ch).chain(cell.mark)
            })
            .collect();
        text.trim_end().into()
    }

    #[test]
    fn notices_wrap_and_scroll_into_bounded_scrollback() {
        let mut grid = TerminalGrid::new(10, 3);
        grid.write_str("Hello\nWorld\nLine3\nLine4");
        assert_eq!(grid.scrollback_len(), 1);
        assert_eq!(grid.cursor_y, 2);
        assert_eq!(row(&grid, 0), "World");
        assert_eq!(row(&grid, 1), "Line3");
        assert_eq!(row(&grid, 2), "Line4");
        let first: String = grid.scrollback_line(0).unwrap().iter().map(|c| c.ch).collect();
        assert_eq!(first, "Hello");
    }

    #[test]
    fn scrollback_is_capped_by_lines_and_bytes() {
        let mut grid = TerminalGrid::new(512, 2);
        let line: String = core::iter::repeat('x').take(512).collect();
        for _ in 0..1500 {
            grid.write_str(&line);
            grid.line_feed();
        }
        assert!(grid.scrollback_len() <= MAX_SCROLLBACK_LINES);
        assert!(grid.scrollback_bytes() <= MAX_SCROLLBACK_BYTES);
        // Short lines: the line cap is the one that binds.
        let mut grid = TerminalGrid::new(80, 2);
        for _ in 0..1500 {
            grid.write_str("ab\n");
        }
        assert_eq!(grid.scrollback_len(), MAX_SCROLLBACK_LINES);
    }

    #[test]
    fn resize_keeps_content_and_refuses_out_of_bounds_sizes() {
        let mut grid = TerminalGrid::new(10, 3);
        grid.write_str("12345");
        assert_eq!(grid.resize(20, 5), Ok(()));
        assert_eq!((grid.cols, grid.rows), (20, 5));
        assert_eq!(row(&grid, 0), "12345");
        for (cols, rows) in [(0, 5), (513, 5), (20, 0), (20, 257), (512, 257), (600, 200)] {
            assert_eq!(grid.resize(cols, rows), Err(ResizeError::OutOfBounds), "{cols}x{rows}");
            assert_eq!((grid.cols, grid.rows), (20, 5));
        }
        assert_eq!(grid.resize(512, 256), Ok(()));
        assert_eq!(grid.cols * grid.rows, MAX_CELLS);
        assert_eq!(row(&grid, 0), "12345");
    }

    #[test]
    fn shrinking_below_the_cursor_moves_old_lines_to_scrollback() {
        let mut grid = TerminalGrid::new(10, 5);
        grid.write_str("a\nb\nc\nd\ne");
        assert_eq!(grid.resize(10, 2), Ok(()));
        assert_eq!((row(&grid, 0).as_str(), row(&grid, 1).as_str()), ("d", "e"));
        assert_eq!(grid.cursor_y, 1);
        assert_eq!(grid.scrollback_len(), 3);
    }

    #[test]
    fn narrowing_drops_a_wide_character_cut_in_half() {
        let mut grid = TerminalGrid::new(10, 2);
        grid.write_str("abcd世");
        assert_eq!(grid.resize(5, 2), Ok(()));
        assert_eq!(row(&grid, 0), "abcd");
        assert_eq!(grid.cell_at(4, 0).unwrap().width, Width::Narrow);
    }

    #[test]
    fn resize_with_the_alternate_screen_up_preserves_the_primary_screen() {
        let mut grid = TerminalGrid::new(20, 4);
        grid.write_str("prompt> top");
        grid.set_private_mode(1049, true);
        // The alternate screen starts blank with the cursor where it was.
        assert_eq!((grid.cursor_x, grid.cursor_y), (11, 0));
        grid.set_position(1, 1);
        grid.write_str("TUI");
        assert_eq!(grid.resize(30, 6), Ok(()));
        assert_eq!(row(&grid, 0), "TUI");
        grid.set_private_mode(1049, false);
        assert_eq!(row(&grid, 0), "prompt> top");
        assert_eq!((grid.cursor_x, grid.cursor_y), (11, 0));
        assert_eq!((grid.cols, grid.rows), (30, 6));
    }

    #[test]
    fn reset_modes_returns_to_the_profile_defaults() {
        let mut grid = TerminalGrid::new(20, 4);
        grid.write_str("primary");
        grid.set_private_mode(1049, true);
        grid.set_private_mode(25, false);
        grid.set_private_mode(2004, true);
        grid.set_private_mode(7, false);
        grid.set_scroll_region(2, 3);
        grid.select_graphic_rendition(&[1, 31]);
        grid.reset_modes();
        assert!(!grid.alternate_active() && grid.cursor_visible() && grid.autowrap() && !grid.bracketed_paste());
        assert_eq!(grid.margins(), (0, 3));
        assert_eq!(grid.pen(), Pen::DEFAULT);
        assert_eq!(row(&grid, 0), "primary");
    }
}
