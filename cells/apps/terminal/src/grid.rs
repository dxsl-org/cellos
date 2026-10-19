// SPDX-License-Identifier: MIT
//! Monospace terminal grid and scrollback buffer for GUI Terminal.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use viui::canvas::Color;

#[allow(dead_code)]
pub const DEFAULT_COLS: usize = 80;
#[allow(dead_code)]
pub const DEFAULT_ROWS: usize = 24;
pub const MAX_SCROLLBACK: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::rgb(220, 225, 235), // Crisp light text
            bg: Color::rgb(18, 20, 26),    // Dark terminal background
        }
    }
}

pub struct TerminalGrid {
    pub cols: usize,
    pub rows: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub cells: Vec<TerminalCell>,
    pub scrollback: Vec<Vec<TerminalCell>>,
    pub cur_fg: Color,
    pub cur_bg: Color,
}

impl TerminalGrid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let total = cols * rows;
        let cells = vec![TerminalCell::default(); total];
        Self {
            cols,
            rows,
            cursor_x: 0,
            cursor_y: 0,
            cells,
            scrollback: Vec::new(),
            cur_fg: Color::rgb(220, 225, 235),
            cur_bg: Color::rgb(18, 20, 26),
        }
    }

    pub fn cell_at(&self, col: usize, row: usize) -> Option<&TerminalCell> {
        if col < self.cols && row < self.rows {
            Some(&self.cells[row * self.cols + col])
        } else {
            None
        }
    }

    pub fn write_char(&mut self, ch: char) {
        match ch {
            '\r' => {
                self.cursor_x = 0;
            }
            '\n' => {
                self.cursor_x = 0;
                self.newline();
            }
            '\t' => {
                let next_tab = (self.cursor_x + 8) / 8 * 8;
                self.cursor_x = next_tab.min(self.cols.saturating_sub(1));
            }
            '\x08' => {
                // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                    let idx = self.cursor_y * self.cols + self.cursor_x;
                    self.cells[idx] = TerminalCell {
                        ch: ' ',
                        fg: self.cur_fg,
                        bg: self.cur_bg,
                    };
                }
            }
            _ => {
                if !ch.is_control() {
                    if self.cursor_x >= self.cols {
                        self.cursor_x = 0;
                        self.newline();
                    }
                    let idx = self.cursor_y * self.cols + self.cursor_x;
                    if idx < self.cells.len() {
                        self.cells[idx] = TerminalCell {
                            ch,
                            fg: self.cur_fg,
                            bg: self.cur_bg,
                        };
                        self.cursor_x += 1;
                    }
                }
            }
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.write_char(ch);
        }
    }

    pub fn newline(&mut self) {
        if self.cursor_y + 1 < self.rows {
            self.cursor_y += 1;
        } else {
            // Scroll up: save top row to scrollback
            let top_row: Vec<TerminalCell> = self.cells[..self.cols].to_vec();
            if self.scrollback.len() >= MAX_SCROLLBACK {
                self.scrollback.remove(0);
            }
            self.scrollback.push(top_row);

            // Shift cells up by 1 row
            self.cells.copy_within(self.cols.., 0);

            // Clear bottom row
            let last_row_start = (self.rows - 1) * self.cols;
            for cell in &mut self.cells[last_row_start..] {
                *cell = TerminalCell {
                    ch: ' ',
                    fg: self.cur_fg,
                    bg: self.cur_bg,
                };
            }
        }
    }

    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            *cell = TerminalCell {
                ch: ' ',
                fg: self.cur_fg,
                bg: self.cur_bg,
            };
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        if new_cols == 0 || new_rows == 0 {
            return;
        }
        let total = new_cols * new_rows;
        let mut new_cells = vec![TerminalCell::default(); total];

        let min_cols = self.cols.min(new_cols);
        let min_rows = self.rows.min(new_rows);

        for r in 0..min_rows {
            for c in 0..min_cols {
                new_cells[r * new_cols + c] = self.cells[r * self.cols + c];
            }
        }

        self.cols = new_cols;
        self.rows = new_rows;
        self.cells = new_cells;
        self.cursor_x = self.cursor_x.min(new_cols.saturating_sub(1));
        self.cursor_y = self.cursor_y.min(new_rows.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use super::*;

    #[test]
    fn test_grid_write_and_scroll() {
        let mut grid = TerminalGrid::new(10, 3);
        grid.write_str("Hello\nWorld\nLine3\nLine4");

        // Line4 caused scrolling
        assert_eq!(grid.scrollback.len(), 1);
        assert_eq!(grid.cursor_y, 2);

        // First visible row is now "World"
        let row0: String = (0..5).map(|c| grid.cell_at(c, 0).unwrap().ch).collect();
        assert_eq!(row0, "World");

        // Second row is "Line3"
        let row1: String = (0..5).map(|c| grid.cell_at(c, 1).unwrap().ch).collect();
        assert_eq!(row1, "Line3");

        // Third row is "Line4"
        let row2: String = (0..5).map(|c| grid.cell_at(c, 2).unwrap().ch).collect();
        assert_eq!(row2, "Line4");
    }

    #[test]
    fn test_grid_backspace_and_clear() {
        let mut grid = TerminalGrid::new(10, 3);
        grid.write_str("Test");
        assert_eq!(grid.cursor_x, 4);

        grid.write_char('\x08'); // backspace
        assert_eq!(grid.cursor_x, 3);
        assert_eq!(grid.cell_at(3, 0).unwrap().ch, ' ');

        grid.clear();
        assert_eq!(grid.cursor_x, 0);
        assert_eq!(grid.cursor_y, 0);
        assert_eq!(grid.cell_at(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn test_grid_resize() {
        let mut grid = TerminalGrid::new(10, 3);
        grid.write_str("12345");
        grid.resize(20, 5);
        assert_eq!(grid.cols, 20);
        assert_eq!(grid.rows, 5);
        let s: String = (0..5).map(|c| grid.cell_at(c, 0).unwrap().ch).collect();
        assert_eq!(s, "12345");
    }
}
