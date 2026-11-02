// SPDX-License-Identifier: MIT
pub mod columns;
pub mod grid;
pub mod list;
pub mod places;

use viui::canvas::Color;

/// Screen rectangle of the main content area a view renders into.
#[derive(Clone, Copy)]
pub struct ContentRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Colours shared by the Files view renderers.
#[derive(Clone, Copy)]
pub struct ViewPalette {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub border: Color,
}
