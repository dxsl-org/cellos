// SPDX-License-Identifier: MIT
pub mod columns;
pub mod places;

use viui::canvas::Color;

/// Screen rectangle of the main content area a view renders into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl ContentRect {
    /// Whether `(x, y)` lies inside (right and bottom edges exclusive).
    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && (x - self.x) < self.w as i32 && (y - self.y) < self.h as i32
    }
}

/// Colours shared by the Files view renderers.
#[derive(Clone, Copy)]
pub struct ViewPalette {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub border: Color,
}
