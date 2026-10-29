// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Style data for Ratatui apps on Cellos sessions: a truecolor palette with 256- and
//! 16-colour fallbacks, border sets and a few styled building blocks (title bar, status
//! line, selectable-list highlight, key-hint footer). Pure data on Ratatui types; nothing
//! here renders or talks to a session.
//!
//! # Colour mode
//! The session capabilities (`terminal_sdk::session::Capabilities`: frontend, size, mode,
//! event and feature masks) advertise no colour mode, so an app passes [`ColorMode`]
//! explicitly. The GUI Terminal's `cellos-vt1` screen renders `38;2`/`48;2` truecolor, so
//! [`ColorMode::TrueColor`] is right there; the fallbacks serve frontends that only map
//! the 256-colour or 16-colour palettes.
//!
//! # Glyphs
//! The GUI Terminal draws the bundled JetBrains Mono, which covers every box-drawing
//! (U+2500–257F) and block-element (U+2580–259F) character but no Braille pattern
//! (0/256 of U+2800–28FF): a Braille cell renders as the missing-glyph box. The marker
//! defaults ([`CHART_MARKER`], [`CANVAS_MARKER`]) and [`HIGHLIGHT_SYMBOL`] therefore use
//! block elements only.

use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::symbols::border;
use ratatui_core::symbols::Marker;
use ratatui_core::text::{Line, Span};
use ratatui_widgets::block::Block;
use ratatui_widgets::borders::BorderType;

use alloc::vec::Vec;

/// A 24-bit colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `0xRRGGBB`.
    pub const fn hex(value: u32) -> Self {
        Self((value >> 16) as u8, (value >> 8) as u8, value as u8)
    }

    /// Squared euclidean distance in RGB.
    const fn distance(self, other: Self) -> u32 {
        let dr = self.0.abs_diff(other.0) as u32;
        let dg = self.1.abs_diff(other.1) as u32;
        let db = self.2.abs_diff(other.2) as u32;
        dr * dr + dg * dg + db * db
    }
}

/// How palette colours reach the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// `Color::Rgb` as is (`38;2` / `48;2`).
    TrueColor,
    /// The nearest colour of the xterm 6x6x6 cube (16–231) or gray ramp (232–255).
    Indexed256,
    /// The nearest of the 16 ANSI colours, as `cellos-vt1` paints them.
    Ansi16,
}

/// The theme's colours, by role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Panel background.
    pub bg: Rgb,
    /// Bars (status line) and other raised areas.
    pub surface: Rgb,
    /// Background of the selected list row.
    pub highlight: Rgb,
    /// Body text.
    pub fg: Rgb,
    /// Secondary text (hints, labels, unfilled gauge).
    pub muted: Rgb,
    /// Titles, keys, filled gauge, selection text.
    pub accent: Rgb,
    /// Block borders.
    pub border: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
}

impl Palette {
    /// A dark palette (Catppuccin Mocha-like).
    pub const DEFAULT: Self = Self {
        bg: Rgb::hex(0x1e1e2e),
        surface: Rgb::hex(0x313244),
        highlight: Rgb::hex(0x45475a),
        fg: Rgb::hex(0xcdd6f4),
        muted: Rgb::hex(0x7f849c),
        accent: Rgb::hex(0xcba6f7),
        border: Rgb::hex(0x89b4fa),
        success: Rgb::hex(0xa6d189),
        warning: Rgb::hex(0xf9e2af),
        error: Rgb::hex(0xf38ba8),
    };
}

/// Border line sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Border {
    /// `┌─┐│└┘`
    Plain,
    /// `╭─╮│╰╯`
    Rounded,
    /// `┏━┓┃┗┛`
    Thick,
}

impl Border {
    pub const fn set(self) -> border::Set<'static> {
        match self {
            Self::Plain => border::PLAIN,
            Self::Rounded => border::ROUNDED,
            Self::Thick => border::THICK,
        }
    }

    pub const fn border_type(self) -> BorderType {
        match self {
            Self::Plain => BorderType::Plain,
            Self::Rounded => BorderType::Rounded,
            Self::Thick => BorderType::Thick,
        }
    }
}

/// Default marker for `Chart` datasets: one `█` per point (never Braille, see the module docs).
pub const CHART_MARKER: Marker = Marker::Block;
/// Default marker for `Canvas`: `█`/`▀`/`▄` half blocks (never Braille, see the module docs).
pub const CANVAS_MARKER: Marker = Marker::HalfBlock;
/// Highlight symbol of a selectable list (a block element the font covers).
pub const HIGHLIGHT_SYMBOL: &str = "\u{258c} ";

/// Levels of the xterm 6x6x6 colour cube.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The 16 ANSI colours as `cellos-vt1` paints them (SGR 30–37 / 90–97, indexes 0–15).
pub const ANSI16: [Rgb; 16] = [
    Rgb(0, 0, 0),
    Rgb(205, 49, 49),
    Rgb(13, 188, 121),
    Rgb(229, 229, 16),
    Rgb(36, 114, 200),
    Rgb(188, 63, 188),
    Rgb(17, 168, 205),
    Rgb(229, 229, 229),
    Rgb(102, 102, 102),
    Rgb(241, 76, 76),
    Rgb(35, 209, 139),
    Rgb(245, 245, 67),
    Rgb(59, 142, 234),
    Rgb(214, 112, 214),
    Rgb(41, 184, 219),
    Rgb(255, 255, 255),
];

/// Ratatui's names for [`ANSI16`], by index.
const ANSI16_COLORS: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

/// Index of the cube level nearest to `value` (the lower one on a tie).
fn cube_level(value: u8) -> usize {
    let mut best = 0;
    for (i, level) in CUBE.iter().enumerate() {
        if level.abs_diff(value) < CUBE[best].abs_diff(value) {
            best = i;
        }
    }
    best
}

/// The xterm-256 index nearest to `rgb` (squared RGB distance) among the cube (16–231)
/// and the gray ramp (232–255); the 16 user-configurable colours are never chosen. The
/// cube wins a tie with the ramp, a lower ramp level a tie within it.
pub fn nearest_256(rgb: Rgb) -> u8 {
    let (r, g, b) = (cube_level(rgb.0), cube_level(rgb.1), cube_level(rgb.2));
    let cube = Rgb(CUBE[r], CUBE[g], CUBE[b]);
    let mut best = (rgb.distance(cube), (16 + 36 * r + 6 * g + b) as u8);
    for step in 0..24u8 {
        let level = 8 + 10 * step;
        let distance = rgb.distance(Rgb(level, level, level));
        if distance < best.0 {
            best = (distance, 232 + step);
        }
    }
    best.1
}

/// The [`ANSI16`] index nearest to `rgb` (squared RGB distance; the lower index on a tie).
pub fn nearest_16(rgb: Rgb) -> u8 {
    let mut best = 0;
    for (i, colour) in ANSI16.iter().enumerate() {
        if rgb.distance(*colour) < rgb.distance(ANSI16[best]) {
            best = i;
        }
    }
    best as u8
}

/// A palette in one colour mode with one border set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub palette: Palette,
    pub mode: ColorMode,
    pub border: Border,
}

impl Theme {
    /// [`Palette::DEFAULT`] with rounded borders.
    pub const fn new(mode: ColorMode) -> Self {
        Self { palette: Palette::DEFAULT, mode, border: Border::Rounded }
    }

    /// `rgb` in this theme's colour mode.
    pub fn color(&self, rgb: Rgb) -> Color {
        match self.mode {
            ColorMode::TrueColor => Color::Rgb(rgb.0, rgb.1, rgb.2),
            ColorMode::Indexed256 => Color::Indexed(nearest_256(rgb)),
            ColorMode::Ansi16 => ANSI16_COLORS[nearest_16(rgb) as usize],
        }
    }

    /// Body text on the panel background.
    pub fn base(&self) -> Style {
        Style::new().fg(self.color(self.palette.fg)).bg(self.color(self.palette.bg))
    }

    pub fn border_style(&self) -> Style {
        Style::new().fg(self.color(self.palette.border))
    }

    /// Block titles: bold accent.
    pub fn title_style(&self) -> Style {
        Style::new().fg(self.color(self.palette.accent)).add_modifier(Modifier::BOLD)
    }

    /// A bordered block in this theme's border set, on the panel background, titled `title`.
    pub fn block<'a>(&self, title: &'a str) -> Block<'a> {
        Block::bordered()
            .border_set(self.border.set())
            .border_style(self.border_style())
            .title(Line::styled(title, self.title_style()))
            .style(self.base())
    }

    /// The title bar: bold panel-background text on the accent colour.
    pub fn title_bar_style(&self) -> Style {
        Style::new()
            .fg(self.color(self.palette.bg))
            .bg(self.color(self.palette.accent))
            .add_modifier(Modifier::BOLD)
    }

    /// `text` as a title bar; rendered as a widget it fills its whole row.
    pub fn title_bar<'a>(&self, text: &'a str) -> Line<'a> {
        Line::styled(text, self.title_bar_style())
    }

    /// The status line: body text on the surface colour.
    pub fn status_line_style(&self) -> Style {
        Style::new().fg(self.color(self.palette.fg)).bg(self.color(self.palette.surface))
    }

    /// `text` as a status line; rendered as a widget it fills its whole row.
    pub fn status_line<'a>(&self, text: &'a str) -> Line<'a> {
        Line::styled(text, self.status_line_style())
    }

    /// The selected row of a list: bold accent on the highlight colour. Use with
    /// [`HIGHLIGHT_SYMBOL`].
    pub fn highlight_style(&self) -> Style {
        Style::new()
            .fg(self.color(self.palette.accent))
            .bg(self.color(self.palette.highlight))
            .add_modifier(Modifier::BOLD)
    }

    pub fn key_style(&self) -> Style {
        Style::new().fg(self.color(self.palette.accent)).add_modifier(Modifier::BOLD)
    }

    pub fn muted_style(&self) -> Style {
        Style::new().fg(self.color(self.palette.muted))
    }

    /// A key-hint footer: `key label  key label ...`, keys bold accent, the rest muted.
    pub fn key_hints<'a>(&self, hints: &[(&'a str, &'a str)]) -> Line<'a> {
        let mut spans = Vec::with_capacity(hints.len() * 4);
        for (i, (key, label)) in hints.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(*key, self.key_style()));
            spans.push(Span::raw(" "));
            spans.push(Span::raw(*label));
        }
        Line::from(spans).style(self.muted_style())
    }
}

#[cfg(test)]
mod tests;
