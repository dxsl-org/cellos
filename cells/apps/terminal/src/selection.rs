// SPDX-License-Identifier: MIT
//! Mouse text selection over the visible grid.
//!
//! A selection runs in reading order from one cell to another, inclusive, like a
//! stream of text: the first and last rows are partial, the rows between are whole.
//! It lives in grid coordinates only, so output that scrolls the screen leaves it on the
//! same cells; a resize clears it ([`Selector::clear`]).

extern crate alloc;
use alloc::string::String;

use crate::grid::{TerminalGrid, Width};
use crate::{PAD_X, PAD_Y};

/// One grid cell. Field order makes the derived ordering reading order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

/// Where the grid sits in the surface: cell (0, 0) at (`PAD_X`, `PAD_Y`).
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub cell_w: f32,
    pub cell_h: f32,
    pub cols: usize,
    pub rows: usize,
}

impl Geometry {
    /// The cell under a surface-local pixel; `None` outside the grid.
    pub fn cell_at(&self, x: i32, y: i32) -> Option<Cell> {
        let fx = (x as f32 - PAD_X) / self.cell_w;
        let fy = (y as f32 - PAD_Y) / self.cell_h;
        if fx < 0.0 || fy < 0.0 {
            return None;
        }
        let cell = Cell {
            row: fy as usize,
            col: fx as usize,
        };
        (cell.col < self.cols && cell.row < self.rows).then_some(cell)
    }

    /// The grid cell nearest a pixel, so a drag past the edge selects up to the edge.
    pub fn clamped_cell(&self, x: i32, y: i32) -> Cell {
        // Float-to-int casts saturate: anything left of or above the grid is 0.
        let col = ((x as f32 - PAD_X) / self.cell_w) as usize;
        let row = ((y as f32 - PAD_Y) / self.cell_h) as usize;
        Cell {
            row: row.min(self.rows.saturating_sub(1)),
            col: col.min(self.cols.saturating_sub(1)),
        }
    }
}

/// A selection between the cell the drag started on and the cell it is on now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Cell,
    pub head: Cell,
}

impl Selection {
    /// First and last selected cell in reading order, whichever way the drag went.
    pub fn bounds(&self) -> (Cell, Cell) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn contains(&self, col: usize, row: usize) -> bool {
        let (first, last) = self.bounds();
        (first..=last).contains(&Cell { row, col })
    }

    /// The selected text: rows joined with `\n`, each row's trailing spaces trimmed, a
    /// wide character once (its trailing half adds nothing), combining marks kept.
    pub fn text(&self, grid: &TerminalGrid) -> String {
        let (first, last) = self.bounds();
        let mut out = String::new();
        for row in first.row..=last.row.min(grid.rows.saturating_sub(1)) {
            if row != first.row {
                out.push('\n');
            }
            let start = if row == first.row { first.col } else { 0 };
            let end = if row == last.row {
                last.col
            } else {
                grid.cols.saturating_sub(1)
            };
            let line_start = out.len();
            for col in start..=end {
                let Some(cell) = grid.cell_at(col, row) else {
                    break;
                };
                if cell.width == Width::Trail {
                    continue;
                }
                out.push(cell.ch);
                if let Some(mark) = cell.mark {
                    out.push(mark);
                }
            }
            let trimmed = line_start + out[line_start..].trim_end_matches(' ').len();
            out.truncate(trimmed);
        }
        out
    }
}

/// Left-button drag state: press starts a selection on the cell under the pointer,
/// moves extend it, release ends the drag.
#[derive(Debug, Default)]
pub struct Selector {
    /// Last surface-local pointer position; button events carry none.
    pointer: Option<(i32, i32)>,
    dragging: bool,
    /// The head has left the anchor cell at least once during this drag. Until then
    /// nothing is shown, and a release clears the selection (a plain click).
    moved: bool,
    selection: Option<Selection>,
}

impl Selector {
    /// The selection to show and copy.
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref().filter(|_| self.moved)
    }

    /// Pointer motion; returns whether the shown selection changed.
    pub fn on_move(&mut self, x: i32, y: i32, geom: Geometry) -> bool {
        self.pointer = Some((x, y));
        let Some(selection) = self.selection.as_mut().filter(|_| self.dragging) else {
            return false;
        };
        let head = geom.clamped_cell(x, y);
        if head == selection.head {
            return false;
        }
        selection.head = head;
        self.moved |= head != selection.anchor;
        self.moved
    }

    /// Left press: a new selection anchored at the pointer's cell (none outside the
    /// grid). Returns whether the shown selection changed.
    pub fn on_press(&mut self, geom: Geometry) -> bool {
        let shown = self.clear();
        if let Some(cell) = self.pointer.and_then(|(x, y)| geom.cell_at(x, y)) {
            self.selection = Some(Selection {
                anchor: cell,
                head: cell,
            });
            self.dragging = true;
        }
        shown
    }

    /// Left release: ends the drag; a click that never moved off its cell selects
    /// nothing (and showed nothing, so the screen needs no repaint).
    pub fn on_release(&mut self) {
        self.dragging = false;
        if !self.moved {
            self.selection = None;
        }
    }

    /// Drop the selection; returns whether one was shown.
    pub fn clear(&mut self) -> bool {
        let shown = self.selection().is_some();
        self.selection = None;
        self.dragging = false;
        self.moved = false;
        shown
    }
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
