//! Minimal grid model of the vitui output profile (Phase 06 S6-4 §7.2), fed with a host's
//! stdout bytes so the `top` checks read the reconstructed screen by structure: border
//! glyphs, the highlight symbol, `id:gen` tokens, colours and mode flags, never wording.
//!
//! Modelled: printable UTF-8 (one cell per character); CR; LF (implies CR, as in the GUI
//! Terminal); BS; HT; CSI CUP/HVP, CUU/CUD/CUF/CUB, CNL/CPL, CHA, VPA; ED 0/1/2/3 and
//! EL 0/1/2 (erased cells take the current background); SGR 0, 1, 2, 3, 4, 7 and their
//! resets, 16-colour fg/bg, `38;5`/`48;5`, `38;2`/`48;2`; DECSET/DECRST 1049 (alternate
//! screen, cursor saved, cleared on entry), 25 and 2004; DECSC/DECRC; the last-column
//! pending wrap (the cursor stays on the last column until the next printable character
//! wraps or a cursor move clears it). OSC/DCS/APC/PM/SOS strings are skipped; scroll
//! regions, insert/delete and wide characters are not modelled (the TUI profile emits none).

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

pub const BOLD: u8 = 1;
pub const DIM: u8 = 2;
pub const ITALIC: u8 = 4;
pub const UNDERLINE: u8 = 8;
pub const INVERSE: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pen {
    pub fg: Color,
    pub bg: Color,
    pub attrs: u8,
}

impl Pen {
    pub const DEFAULT: Pen = Pen { fg: Color::Default, bg: Color::Default, attrs: 0 };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub pen: Pen,
}

impl Cell {
    pub const BLANK: Cell = Cell { ch: ' ', pen: Pen::DEFAULT };

    /// Neither a glyph nor a background: what an untouched cell looks like.
    pub fn is_default(&self) -> bool {
        self.ch == ' ' && self.pen.bg == Color::Default
    }
}

pub const TOP_LEFT: char = '\u{256d}';
pub const TOP_RIGHT: char = '\u{256e}';
pub const BOTTOM_LEFT: char = '\u{2570}';
pub const BOTTOM_RIGHT: char = '\u{256f}';
pub const VERTICAL: char = '\u{2502}';
pub const HORIZONTAL: char = '\u{2500}';
/// vitui `HIGHLIGHT_SYMBOL`: the selected table row starts with it.
pub const HIGHLIGHT: char = '\u{258c}';

#[derive(Clone, Copy, PartialEq, Eq)]
enum Parse {
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    /// OSC/DCS/APC/PM/SOS body, skipped up to BEL or ST.
    Text,
    TextEscape,
}

const CSI_MAX: usize = 64;

pub struct Screen {
    cols: usize,
    rows: usize,
    primary: Vec<Cell>,
    /// Allocated only while the alternate screen is active.
    alternate: Vec<Cell>,
    alt: bool,
    x: usize,
    y: usize,
    wrap: bool,
    saved: (usize, usize),
    pen: Pen,
    cursor_hidden: bool,
    paste: bool,
    parse: Parse,
    csi: Vec<u8>,
    csi_bad: bool,
    utf8: u32,
    need: u8,
    /// `ESC[?1049h` / `ESC[?1049l` sequences seen, erase-display operations (ED 0/1/2/3:
    /// a Ratatui clear, full-screen or row-wise for a fixed viewport), bytes fed.
    pub enters: usize,
    pub leaves: usize,
    pub clears: usize,
    pub bytes: u64,
}

impl Screen {
    pub fn new(cols: usize, rows: usize) -> Screen {
        let (cols, rows) = (cols.max(1), rows.max(1));
        Screen {
            cols,
            rows,
            primary: vec![Cell::BLANK; cols * rows],
            alternate: Vec::new(),
            alt: false,
            x: 0,
            y: 0,
            wrap: false,
            saved: (0, 0),
            pen: Pen::DEFAULT,
            cursor_hidden: false,
            paste: false,
            parse: Parse::Ground,
            csi: Vec::with_capacity(CSI_MAX),
            csi_bad: false,
            utf8: 0,
            need: 0,
            enters: 0,
            leaves: 0,
            clears: 0,
            bytes: 0,
        }
    }

    /// Follow the session size (the frontend's resize); overlapping content is kept.
    pub fn fit(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        let copy = |old: &[Cell], (oc, or): (usize, usize)| {
            let mut grid = vec![Cell::BLANK; cols * rows];
            for y in 0..rows.min(or) {
                for x in 0..cols.min(oc) {
                    grid[y * cols + x] = old[y * oc + x];
                }
            }
            grid
        };
        let old = (self.cols, self.rows);
        self.primary = copy(&self.primary, old);
        if self.alt {
            self.alternate = copy(&self.alternate, old);
        }
        self.cols = cols;
        self.rows = rows;
        self.x = self.x.min(cols - 1);
        self.y = self.y.min(rows - 1);
        self.saved = (self.saved.0.min(cols - 1), self.saved.1.min(rows - 1));
        self.wrap = false;
    }

    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    pub fn alternate(&self) -> bool {
        self.alt
    }

    pub fn cursor_hidden(&self) -> bool {
        self.cursor_hidden
    }

    pub fn bracketed_paste(&self) -> bool {
        self.paste
    }

    /// The active screen's cell, blank outside the grid.
    pub fn cell(&self, x: usize, y: usize) -> Cell {
        if x >= self.cols || y >= self.rows {
            return Cell::BLANK;
        }
        self.grid()[y * self.cols + x]
    }

    /// Characters of row `y`, columns `x0..x1` of the active screen.
    pub fn text(&self, y: usize, x0: usize, x1: usize) -> String {
        (x0..x1.min(self.cols)).map(|x| self.cell(x, y).ch).collect()
    }

    fn grid(&self) -> &[Cell] {
        if self.alt { &self.alternate } else { &self.primary }
    }

    fn grid_mut(&mut self) -> &mut [Cell] {
        if self.alt { &mut self.alternate } else { &mut self.primary }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.bytes += bytes.len() as u64;
        for &byte in bytes {
            self.byte(byte);
        }
    }

    fn byte(&mut self, b: u8) {
        match self.parse {
            Parse::Ground => self.ground(b),
            Parse::Escape => self.escape(b),
            Parse::EscapeIntermediate => {
                if !(0x20..=0x2f).contains(&b) {
                    self.parse = Parse::Ground;
                }
            }
            Parse::Csi => self.csi_byte(b),
            Parse::Text => match b {
                0x07 => self.parse = Parse::Ground,
                0x1b => self.parse = Parse::TextEscape,
                _ => {}
            },
            Parse::TextEscape => {
                if b == b'\\' {
                    self.parse = Parse::Ground;
                } else {
                    self.parse = Parse::Escape;
                    self.escape(b);
                }
            }
        }
    }

    fn ground(&mut self, b: u8) {
        if self.need > 0 {
            if (0x80..=0xbf).contains(&b) {
                self.utf8 = (self.utf8 << 6) | u32::from(b & 0x3f);
                self.need -= 1;
                if self.need == 0 {
                    let ch = char::from_u32(self.utf8).unwrap_or('\u{fffd}');
                    if !('\u{80}'..='\u{9f}').contains(&ch) {
                        self.print(ch);
                    }
                }
                return;
            }
            self.need = 0;
            self.print('\u{fffd}');
        }
        match b {
            0x1b => self.parse = Parse::Escape,
            0x08 => {
                self.wrap = false;
                self.x = self.x.saturating_sub(1);
            }
            0x09 => {
                self.wrap = false;
                self.x = ((self.x / 8 + 1) * 8).min(self.cols - 1);
            }
            0x0a..=0x0c => {
                self.wrap = false;
                self.x = 0;
                self.linefeed();
            }
            0x0d => {
                self.wrap = false;
                self.x = 0;
            }
            0x00..=0x1f | 0x7f => {}
            0x20..=0x7e => self.print(b as char),
            0xc2..=0xdf => self.lead(u32::from(b & 0x1f), 1),
            0xe0..=0xef => self.lead(u32::from(b & 0x0f), 2),
            0xf0..=0xf4 => self.lead(u32::from(b & 0x07), 3),
            _ => self.print('\u{fffd}'),
        }
    }

    fn lead(&mut self, bits: u32, need: u8) {
        self.utf8 = bits;
        self.need = need;
    }

    fn escape(&mut self, b: u8) {
        self.parse = Parse::Ground;
        match b {
            b'[' => {
                self.csi.clear();
                self.csi_bad = false;
                self.parse = Parse::Csi;
            }
            b']' | b'P' | b'_' | b'^' | b'X' => self.parse = Parse::Text,
            0x20..=0x2f => self.parse = Parse::EscapeIntermediate,
            b'7' => self.saved = (self.x, self.y),
            b'8' => self.restore(),
            b'c' => {
                let counters = (self.enters, self.leaves, self.clears, self.bytes);
                *self = Screen::new(self.cols, self.rows);
                (self.enters, self.leaves, self.clears, self.bytes) = counters;
            }
            0x1b => self.parse = Parse::Escape,
            _ => {}
        }
    }

    fn csi_byte(&mut self, b: u8) {
        match b {
            0x30..=0x3f => {
                if self.csi.len() < CSI_MAX {
                    self.csi.push(b);
                } else {
                    self.csi_bad = true;
                }
            }
            0x20..=0x2f => self.csi_bad = true,
            0x40..=0x7e => {
                self.parse = Parse::Ground;
                if !self.csi_bad {
                    self.dispatch(b);
                }
            }
            0x1b => self.parse = Parse::Escape,
            0x18 | 0x1a => self.parse = Parse::Ground,
            0x00..=0x1f => {}
            _ => {
                self.parse = Parse::Ground;
                self.ground(b);
            }
        }
    }

    fn params(body: &[u8]) -> Vec<u32> {
        let mut out = Vec::new();
        let mut value: Option<u32> = None;
        for &b in body {
            match b {
                b'0'..=b'9' => value = Some(value.unwrap_or(0).saturating_mul(10).saturating_add(u32::from(b - b'0')).min(65_535)),
                b';' | b':' => out.push(value.take().unwrap_or(0)),
                _ => {}
            }
        }
        out.push(value.unwrap_or(0));
        out
    }

    fn dispatch(&mut self, final_byte: u8) {
        let csi = core::mem::take(&mut self.csi);
        let (private, body) = match csi.first().copied() {
            Some(b'?') => (true, &csi[1..]),
            Some(b'<' | b'=' | b'>') => {
                self.csi = csi;
                return;
            }
            _ => (false, &csi[..]),
        };
        let p = Self::params(body);
        self.csi = csi;
        let first = p[0] as usize;
        let n = first.max(1);
        if private {
            if matches!(final_byte, b'h' | b'l') {
                for &mode in &p {
                    self.mode(mode, final_byte == b'h');
                }
            }
            return;
        }
        match final_byte {
            b'H' | b'f' => {
                let col = p.get(1).copied().unwrap_or(0).max(1) as usize;
                self.goto(col - 1, n - 1);
            }
            b'A' => self.goto(self.x, self.y.saturating_sub(n)),
            b'B' => self.goto(self.x, self.y + n),
            b'C' => self.goto(self.x + n, self.y),
            b'D' => self.goto(self.x.saturating_sub(n), self.y),
            b'E' => self.goto(0, self.y + n),
            b'F' => self.goto(0, self.y.saturating_sub(n)),
            b'G' | b'`' => self.goto(n - 1, self.y),
            b'd' => self.goto(self.x, n - 1),
            b'J' => self.erase_display(first),
            b'K' => self.erase_line(first),
            b'm' => self.sgr(&p),
            b's' => self.saved = (self.x, self.y),
            b'u' => self.restore(),
            _ => {}
        }
    }

    fn mode(&mut self, mode: u32, set: bool) {
        match mode {
            1049 if set => {
                self.enters += 1;
                self.saved = (self.x, self.y);
                self.alternate = vec![Cell::BLANK; self.cols * self.rows];
                self.alt = true;
                self.wrap = false;
            }
            1049 => {
                self.leaves += 1;
                if self.alt {
                    self.alt = false;
                    self.alternate = Vec::new();
                }
                self.restore();
            }
            25 => self.cursor_hidden = !set,
            2004 => self.paste = set,
            _ => {}
        }
    }

    fn restore(&mut self) {
        self.x = self.saved.0.min(self.cols - 1);
        self.y = self.saved.1.min(self.rows - 1);
        self.wrap = false;
    }

    fn goto(&mut self, x: usize, y: usize) {
        self.x = x.min(self.cols - 1);
        self.y = y.min(self.rows - 1);
        self.wrap = false;
    }

    fn erased(&self) -> Cell {
        Cell { ch: ' ', pen: Pen { bg: self.pen.bg, ..Pen::DEFAULT } }
    }

    fn fill(&mut self, from: usize, to: usize) {
        let blank = self.erased();
        let grid = self.grid_mut();
        let to = to.min(grid.len());
        if from < to {
            grid[from..to].fill(blank);
        }
    }

    fn erase_display(&mut self, how: usize) {
        let at = self.y * self.cols + self.x;
        let all = self.cols * self.rows;
        self.clears += 1;
        match how {
            0 => self.fill(at, all),
            1 => self.fill(0, at + 1),
            2 | 3 => self.fill(0, all),
            _ => {}
        }
    }

    fn erase_line(&mut self, how: usize) {
        let row = self.y * self.cols;
        match how {
            0 => self.fill(row + self.x, row + self.cols),
            1 => self.fill(row, row + self.x + 1),
            2 => self.fill(row, row + self.cols),
            _ => {}
        }
    }

    fn sgr(&mut self, p: &[u32]) {
        let mut i = 0;
        while i < p.len() {
            match p[i] {
                0 => self.pen = Pen::DEFAULT,
                1 => self.pen.attrs |= BOLD,
                2 => self.pen.attrs |= DIM,
                3 => self.pen.attrs |= ITALIC,
                4 => self.pen.attrs |= UNDERLINE,
                7 => self.pen.attrs |= INVERSE,
                22 => self.pen.attrs &= !(BOLD | DIM),
                23 => self.pen.attrs &= !ITALIC,
                24 => self.pen.attrs &= !UNDERLINE,
                27 => self.pen.attrs &= !INVERSE,
                code @ 30..=37 => self.pen.fg = Color::Indexed((code - 30) as u8),
                39 => self.pen.fg = Color::Default,
                code @ 40..=47 => self.pen.bg = Color::Indexed((code - 40) as u8),
                49 => self.pen.bg = Color::Default,
                code @ 90..=97 => self.pen.fg = Color::Indexed((code - 90 + 8) as u8),
                code @ 100..=107 => self.pen.bg = Color::Indexed((code - 100 + 8) as u8),
                code @ (38 | 48) => {
                    let byte = |k: usize| p.get(k).map(|v| (*v).min(255) as u8);
                    let color = match p.get(i + 1).copied() {
                        Some(5) => {
                            let c = byte(i + 2).map(Color::Indexed);
                            i += 2;
                            c
                        }
                        Some(2) => {
                            let c = match (byte(i + 2), byte(i + 3), byte(i + 4)) {
                                (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r, g, b)),
                                _ => None,
                            };
                            i += 4;
                            c
                        }
                        _ => return,
                    };
                    match (code, color) {
                        (38, Some(c)) => self.pen.fg = c,
                        (48, Some(c)) => self.pen.bg = c,
                        _ => return,
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn print(&mut self, ch: char) {
        if self.wrap {
            self.wrap = false;
            self.x = 0;
            self.linefeed();
        }
        let (at, pen) = (self.y * self.cols + self.x, self.pen);
        self.grid_mut()[at] = Cell { ch, pen };
        if self.x + 1 >= self.cols {
            self.wrap = true;
        } else {
            self.x += 1;
        }
    }

    fn linefeed(&mut self) {
        if self.y + 1 < self.rows {
            self.y += 1;
            return;
        }
        let cols = self.cols;
        let grid = self.grid_mut();
        let len = grid.len();
        grid.copy_within(cols.., 0);
        grid[len - cols..].fill(Cell::BLANK);
    }

    /// The vitui TUI layout on the alternate screen, located by its border glyphs: the
    /// rounded block's corners `╭` (0,2), `╮` (W-1,2), `╰` (0,H-2), `╯` (W-1,H-2) and `│`
    /// down the left border. `None` outside the alternate screen or before a frame.
    pub fn frame(&self) -> Option<Frame<'_>> {
        if !self.alt || self.cell(0, 2).ch != TOP_LEFT {
            return None;
        }
        let w = (1..self.cols).find(|&x| self.cell(x, 2).ch == TOP_RIGHT)? + 1;
        let mut y = 3;
        while y < self.rows && self.cell(0, y).ch == VERTICAL {
            y += 1;
        }
        if y >= self.rows || self.cell(0, y).ch != BOTTOM_LEFT || self.cell(w - 1, y).ch != BOTTOM_RIGHT {
            return None;
        }
        let h = y + 2;
        (h <= self.rows && h >= 7).then_some(Frame { s: self, w, h })
    }

    /// Structure-only description for FAIL details (no row text: rows may hold the prompt).
    pub fn summary(&self) -> String {
        let frame = self.frame();
        format!(
            "alt={} hidden={} paste={} size={}x{} enters={} leaves={} clears={} frame={:?} sel={:?} rows={} cols={:?}",
            self.alt,
            self.cursor_hidden,
            self.paste,
            self.cols,
            self.rows,
            self.enters,
            self.leaves,
            self.clears,
            frame.as_ref().map(|f| (f.w, f.h)),
            frame.as_ref().and_then(|f| f.selected()),
            frame.as_ref().map_or(0, |f| f.rows().len()),
            frame.as_ref().map(|f| f.header().len()),
        )
    }
}

/// A `CELL(id:gen)` value: `digits:digits`.
pub fn is_cell_identity(token: &str) -> bool {
    match token.split_once(':') {
        Some((id, generation)) => {
            !id.is_empty() && !generation.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit()) && generation.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

/// The drawn TUI area (`w` x `h`, top-left of the session): title bar row 0, status row
/// 1, the bordered block rows 2..h-2 (table header row 3, data rows 4..h-3, highlight
/// column x = 1..2), key hints row h-1.
pub struct Frame<'a> {
    s: &'a Screen,
    pub w: usize,
    pub h: usize,
}

impl Frame<'_> {
    pub fn cell(&self, x: usize, y: usize) -> Cell {
        self.s.cell(x, y)
    }

    /// Every border cell is its glyph: both side borders and the whole bottom border.
    pub fn closed(&self) -> bool {
        let (w, h) = (self.w, self.h);
        (3..h - 2).all(|y| self.cell(0, y).ch == VERTICAL && self.cell(w - 1, y).ch == VERTICAL)
            && (1..w - 1).all(|x| self.cell(x, h - 2).ch == HORIZONTAL)
    }

    pub fn title(&self) -> String {
        self.s.text(0, 0, self.w)
    }

    /// The status row's sample id: its first integer token.
    pub fn sample(&self) -> Option<u64> {
        self.s.text(1, 0, self.w).split_whitespace().find_map(|t| t.parse().ok())
    }

    /// The status row's text (row 1: sample id, `complete`/`TRUNCATED n`, units, memory).
    pub fn status(&self) -> String {
        self.s.text(1, 0, self.w)
    }

    /// Table column labels (row 3 inside the border).
    pub fn header(&self) -> Vec<String> {
        self.s.text(3, 1, self.w - 1).split_whitespace().map(String::from).collect()
    }

    /// Tokens of each visible data row (right of the highlight column), top to bottom,
    /// up to the first empty row.
    pub fn rows(&self) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        for y in 4..self.h - 2 {
            let tokens: Vec<String> = self.s.text(y, 3, self.w - 1).split_whitespace().map(String::from).collect();
            if tokens.is_empty() {
                break;
            }
            out.push(tokens);
        }
        out
    }

    /// Index of the data row that carries the highlight symbol.
    pub fn selected(&self) -> Option<usize> {
        let mut found = (4..self.h - 2).filter(|&y| self.cell(1, y).ch == HIGHLIGHT);
        let first = found.next()?;
        found.next().is_none().then_some(first - 4)
    }

    /// Identity token (first token) of the selected row.
    pub fn selected_id(&self) -> Option<String> {
        let index = self.selected()?;
        self.rows().get(index).and_then(|r| r.first().cloned())
    }

    /// A second bordered box inside the block (the field picker).
    pub fn popup(&self) -> bool {
        (3..self.h - 2).any(|y| (1..self.w - 1).any(|x| self.cell(x, y).ch == TOP_LEFT))
    }

    /// The `id:gen` of the first Cell row named `name`.
    pub fn cell_row(&self, name: &str) -> Option<String> {
        self.rows()
            .into_iter()
            .find(|r| r.len() >= 2 && is_cell_identity(&r[0]) && r[1] == name)
            .map(|r| r[0].clone())
    }

    /// The last column of every row of the area draws something (bar or border).
    pub fn last_column_inked(&self) -> bool {
        (0..self.h).all(|y| !self.cell(self.w - 1, y).is_default())
    }

    /// Row `y` has one background on all `w` cells.
    pub fn row_background(&self, y: usize) -> Option<Color> {
        let bg = self.cell(0, y).pen.bg;
        (0..self.w).all(|x| self.cell(x, y).pen.bg == bg).then_some(bg)
    }

    /// Nothing is drawn outside the area (a clamped session).
    pub fn outside_blank(&self) -> bool {
        let (cols, rows) = self.s.size();
        (0..rows).all(|y| (0..cols).all(|x| (x < self.w && y < self.h) || self.cell(x, y).ch == ' '))
    }
}
