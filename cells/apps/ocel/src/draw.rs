// SPDX-License-Identifier: MIT
//! Drawing primitives for Ocel Document Viewer.

use ostd::display::linear::{Canvas, Radii, RoundRect};
use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};
use viui::theme::SystemPalette;

pub use viui::canvas::Color;
pub use viui::surface::components::*;
pub use viui::surface::{
    draw_icon_centered, fill_rect, fill_round_rect, stroke_round_rect, LineIcon, CONTROL_RADIUS,
    PANEL_RADIUS,
};

/// Semantic text roles the document parsers stamp on spans. The values are
/// tags, not paint: [`ink::resolve`] maps them to the painting theme, so a
/// parsed document re-tints whenever the reader or system palette changes.
pub mod ink {
    use super::Color;
    use viui::theme::ViTheme;

    /// Secondary text: captions, metadata, image placeholders, quotes.
    pub const MUTED: Color = Color::rgb(166, 173, 200);
    /// Emphasised text: top-level headings, prices, pagination and menu links.
    pub const ACCENT: Color = Color::rgb(137, 180, 250);

    /// Colour that paints a span tagged `c` under `theme`; other colours pass through.
    pub fn resolve(theme: &dyn ViTheme, c: Color) -> Color {
        if c == ACCENT {
            theme.accent()
        } else if c == MUTED {
            theme.reader_subtext()
        } else {
            c
        }
    }
}

/// Syntax-highlighting roles and the per-palette colours that paint them.
/// Code spans carry the role tags below (the Dark palette's values);
/// [`CodePalette::resolve`] maps them onto the document palette at paint
/// time, so open code re-tints whenever the reader or system palette changes.
pub mod code_ink {
    use super::{Color, SystemPalette};

    /// Code surface and token colours for one document palette. Every token
    /// keeps at least 4.5:1 contrast against `surface`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CodePalette {
        pub surface: Color,
        pub plain: Color,
        pub comment: Color,
        pub keyword: Color,
        pub string: Color,
        pub number: Color,
        /// Inline code and code blocks without a known language.
        pub inline: Color,
    }

    pub const DARK: CodePalette = CodePalette {
        surface: Color::rgb(49, 50, 68),
        plain: Color::rgb(205, 214, 244),
        comment: Color::rgb(166, 173, 200),
        keyword: Color::rgb(137, 180, 250),
        string: Color::rgb(166, 227, 161),
        number: Color::rgb(250, 179, 135),
        inline: Color::rgb(148, 226, 213),
    };

    pub const LIGHT: CodePalette = CodePalette {
        surface: Color::rgb(238, 240, 244),
        plain: Color::rgb(36, 41, 47),
        comment: Color::rgb(87, 96, 106),
        keyword: Color::rgb(111, 66, 193),
        string: Color::rgb(17, 99, 41),
        number: Color::rgb(5, 80, 174),
        inline: Color::rgb(149, 56, 0),
    };

    pub const PAPER: CodePalette = CodePalette {
        surface: Color::rgb(233, 229, 218),
        plain: Color::rgb(43, 43, 42),
        comment: Color::rgb(95, 90, 80),
        keyword: Color::rgb(140, 60, 30),
        string: Color::rgb(61, 95, 30),
        number: Color::rgb(30, 80, 130),
        inline: Color::rgb(122, 74, 18),
    };

    pub const PLAIN: Color = DARK.plain;
    pub const COMMENT: Color = DARK.comment;
    pub const KEYWORD: Color = DARK.keyword;
    pub const STRING: Color = DARK.string;
    pub const NUMBER: Color = DARK.number;
    /// Inline code and code blocks without a known language.
    pub const INLINE: Color = DARK.inline;

    impl CodePalette {
        /// Code palette painted under the document palette `palette`.
        pub const fn for_palette(palette: SystemPalette) -> &'static Self {
            match palette {
                SystemPalette::Light => &LIGHT,
                SystemPalette::Dark => &DARK,
                SystemPalette::Paper => &PAPER,
            }
        }

        /// Colour that paints a code span tagged `c`; other colours pass through.
        pub fn resolve(&self, c: Color) -> Color {
            match c {
                PLAIN => self.plain,
                COMMENT => self.comment,
                KEYWORD => self.keyword,
                STRING => self.string,
                NUMBER => self.number,
                INLINE => self.inline,
                other => other,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReaderThemeMode {
    #[default]
    FollowSystem,
    Light,
    Dark,
    Paper,
}

impl ReaderThemeMode {
    pub const fn next(self) -> Self {
        match self {
            Self::FollowSystem => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::Paper,
            Self::Paper => Self::FollowSystem,
        }
    }

    /// Effective palette, with FollowSystem mapped to `system`.
    pub fn resolved_palette(self, system: SystemPalette) -> SystemPalette {
        match self {
            Self::FollowSystem => system,
            Self::Light => SystemPalette::Light,
            Self::Dark => SystemPalette::Dark,
            Self::Paper => SystemPalette::Paper,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::FollowSystem => "Follow System",
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Paper => "Paper",
        }
    }
}

/// Rounded box clipped to the caller's viewport: `fill`, then a 1px `stroke`
/// covering the same outline.
pub fn clipped_round_rect(
    surf: &mut ViSurface,
    (x, y, w, h): (i32, i32, u32, u32),
    radius: u32,
    fill: Option<Color>,
    stroke: Option<Color>,
    clip: (i32, i32, i32, i32),
) {
    if clip.2 <= clip.0 || clip.3 <= clip.1 {
        return;
    }
    let (width, height, stride) = (surf.width(), surf.height(), surf.stride());
    let mut canvas = Canvas::new(surf.pixels_mut(), width, height, stride).clip_to(
        clip.0,
        clip.1,
        (clip.2 - clip.0) as u32,
        (clip.3 - clip.1) as u32,
    );
    let shape = RoundRect::new(x, y, w, h, Radii::all(radius));
    if let Some(color) = fill {
        canvas.fill_round(&shape, color.as_bgra_bytes());
    }
    if let Some(color) = stroke {
        canvas.stroke_round(&shape, color.as_bgra_bytes());
    }
}

/// Paint measured, grayscale-antialiased text within a caller's viewport.
// reason: thin wrapper over `TextFonts::draw_text` with many drawing call sites; bundling the
// position/style/clip args would only churn callers.
#[allow(clippy::too_many_arguments)]
pub fn text(
    fonts: &mut TextFonts,
    surf: &mut ViSurface,
    x: f32,
    y: f32,
    value: &str,
    face: FontFace,
    px: f32,
    color: Color,
    clip: (i32, i32, i32, i32),
) -> f32 {
    let width = surf.width();
    let height = surf.height();
    let stride = surf.stride();
    fonts.draw_text(
        surf.pixels_mut(),
        width,
        height,
        stride,
        x,
        y,
        value,
        face,
        px,
        color.as_bgra_bytes(),
        clip,
    )
}

pub fn clipped_rect(
    surf: &mut ViSurface,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: Color,
    clip: (i32, i32, i32, i32),
) {
    let x0 = x.max(clip.0);
    let y0 = y.max(clip.1);
    let x1 = x.saturating_add(w as i32).min(clip.2);
    let y1 = y.saturating_add(h as i32).min(clip.3);
    if x1 > x0 && y1 > y0 {
        fill_rect(surf, x0, y0, (x1 - x0) as u32, (y1 - y0) as u32, color);
    }
}

// reason: low-level blit helper with multiple drawing call sites; bundling geometry and source
// args would only churn callers.
#[allow(clippy::too_many_arguments)]
pub fn draw_image_clipped(
    surf: &mut ViSurface,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    pixels: &[u8],
    src_stride: usize,
    source_height: u32,
    clip: (i32, i32, i32, i32),
) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let dst_stride = surf.stride();
    let dst_pixels = surf.pixels_mut();

    let x0 = x.max(0).max(clip.0);
    let y0 = y.max(0).max(clip.1);
    let x1 = (x + w as i32).min(sw).min(clip.2);
    let y1 = (y + h as i32).min(sh).min(clip.3);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let source_width = src_stride / 4;

    for py in y0..y1 {
        let src_y = ((py - y) as u64 * source_height as u64 / h as u64) as usize;
        let src_row = src_y * src_stride;
        let dst_row = py as usize * dst_stride;

        for px in x0..x1 {
            let src_x = ((px - x) as u64 * source_width as u64 / w as u64) as usize;
            let src_offset = src_row + src_x * 4;
            let dst_offset = dst_row + px as usize * 4;

            if src_offset + 3 < pixels.len() && dst_offset + 3 < dst_pixels.len() {
                let sa = pixels[src_offset + 3] as u32;
                if sa == 255 {
                    dst_pixels[dst_offset..dst_offset + 4]
                        .copy_from_slice(&pixels[src_offset..src_offset + 4]);
                } else if sa > 0 {
                    let inv = 255 - sa;
                    let sb = pixels[src_offset] as u32;
                    let sg = pixels[src_offset + 1] as u32;
                    let sr = pixels[src_offset + 2] as u32;

                    let db = dst_pixels[dst_offset] as u32;
                    let dg = dst_pixels[dst_offset + 1] as u32;
                    let dr = dst_pixels[dst_offset + 2] as u32;

                    dst_pixels[dst_offset] = ((sb * sa + db * inv) / 255) as u8;
                    dst_pixels[dst_offset + 1] = ((sg * sa + dg * inv) / 255) as u8;
                    dst_pixels[dst_offset + 2] = ((sr * sa + dr * inv) / 255) as u8;
                    dst_pixels[dst_offset + 3] = 255;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::code_ink::{CodePalette, DARK, LIGHT, PAPER};
    use super::Color;

    /// WCAG 2.1 relative luminance of an sRGB colour.
    fn luminance(c: Color) -> f64 {
        let channel = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    /// WCAG 2.1 contrast ratio between two colours.
    fn contrast(a: Color, b: Color) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn every_code_token_keeps_wcag_aa_contrast_on_its_surface() {
        let palettes: [(&str, CodePalette); 3] =
            [("Dark", DARK), ("Light", LIGHT), ("Paper", PAPER)];
        for (name, p) in palettes {
            for (role, ink) in [
                ("plain", p.plain),
                ("comment", p.comment),
                ("keyword", p.keyword),
                ("string", p.string),
                ("number", p.number),
                ("inline", p.inline),
            ] {
                let ratio = contrast(ink, p.surface);
                assert!(
                    ratio >= 4.5,
                    "{name} {role} contrast {ratio:.2}:1 is below 4.5:1"
                );
            }
        }
    }
}
