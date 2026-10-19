// SPDX-License-Identifier: MIT
//! Drawing primitives for Ocel Document Viewer.

use ostd::typography::{FontFace, TextFonts};
use ostd::display::ViSurface;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    #[allow(dead_code)]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn as_bgra_bytes(self) -> [u8; 4] {
        [self.b, self.g, self.r, self.a]
    }
}

pub mod theme {
    use super::Color;

    pub const BG_DARK: Color = Color::rgb(30, 30, 46); // Mocha Base
    pub const BG_TOOLBAR: Color = Color::rgb(24, 24, 37); // Mantle
    pub const BG_INPUT: Color = Color::rgb(49, 50, 68); // Surface0
    pub const TEXT_PRIMARY: Color = Color::rgb(205, 214, 244);
    pub const TEXT_MUTED: Color = Color::rgb(166, 173, 200);
    pub const ACCENT_BLUE: Color = Color::rgb(137, 180, 250);
    pub const ACCENT_CYAN: Color = Color::rgb(148, 226, 213);
    pub const BORDER: Color = Color::rgb(69, 71, 90);
}

use core::sync::atomic::{AtomicU8, Ordering};

static ACTIVE_SYSTEM_PALETTE: AtomicU8 = AtomicU8::new(viui::theme::SystemPalette::Light as u8);

pub fn set_active_system_palette(palette: viui::theme::SystemPalette) {
    ACTIVE_SYSTEM_PALETTE.store(palette as u8, Ordering::Relaxed);
}

pub fn active_system_palette() -> viui::theme::SystemPalette {
    viui::theme::SystemPalette::from_u8(ACTIVE_SYSTEM_PALETTE.load(Ordering::Relaxed))
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

    /// Resolve effective palette, mapping FollowSystem to the active system palette.
    pub fn resolved_palette(self, system: viui::theme::SystemPalette) -> viui::theme::SystemPalette {
        match self {
            Self::FollowSystem => system,
            Self::Light => viui::theme::SystemPalette::Light,
            Self::Dark => viui::theme::SystemPalette::Dark,
            Self::Paper => viui::theme::SystemPalette::Paper,
        }
    }

    /// Obtain the shared ViTheme reference for the resolved palette.
    pub fn resolved_theme(self, system: viui::theme::SystemPalette) -> &'static dyn viui::theme::ViTheme {
        match self.resolved_palette(system) {
            viui::theme::SystemPalette::Light => &viui::theme::LIGHT_THEME,
            viui::theme::SystemPalette::Dark => &viui::theme::DARK_THEME,
            viui::theme::SystemPalette::Paper => &viui::theme::PAPER_THEME,
        }
    }

    pub fn bg_color(self) -> Color {
        self.resolved_bg(active_system_palette())
    }

    pub fn resolved_bg(self, system: viui::theme::SystemPalette) -> Color {
        let c = self.resolved_theme(system).reader_bg();
        Color::rgb(c.r(), c.g(), c.b())
    }

    pub fn text_color(self) -> Color {
        self.resolved_text(active_system_palette())
    }

    pub fn resolved_text(self, system: viui::theme::SystemPalette) -> Color {
        let c = self.resolved_theme(system).reader_text();
        Color::rgb(c.r(), c.g(), c.b())
    }

    pub fn muted_color(self) -> Color {
        self.resolved_muted(active_system_palette())
    }

    pub fn resolved_muted(self, system: viui::theme::SystemPalette) -> Color {
        let c = self.resolved_theme(system).reader_subtext();
        Color::rgb(c.r(), c.g(), c.b())
    }

    pub fn accent_color(self) -> Color {
        self.resolved_accent(active_system_palette())
    }

    pub fn resolved_accent(self, system: viui::theme::SystemPalette) -> Color {
        let c = self.resolved_theme(system).accent();
        Color::rgb(c.r(), c.g(), c.b())
    }

    pub fn link_color(self) -> Color {
        self.resolved_link(active_system_palette())
    }

    pub fn resolved_link(self, system: viui::theme::SystemPalette) -> Color {
        let c = self.resolved_theme(system).reader_link();
        Color::rgb(c.r(), c.g(), c.b())
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::FollowSystem => "Follow System",
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Paper => "Paper",
        }
    }

    /// Remap legacy dark-theme span colors to the corresponding semantic color of this reader mode.
    pub fn remap_span_color(self, c: Color) -> Color {
        if c == theme::ACCENT_BLUE {
            self.accent_color()
        } else if c == theme::ACCENT_CYAN {
            self.text_color()
        } else if c == theme::TEXT_MUTED {
            self.muted_color()
        } else if c == theme::TEXT_PRIMARY {
            self.text_color()
        } else {
            c
        }
    }
}

pub fn fill_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, color: Color) {
    let sw = surf.width() as i32;
    let sh = surf.height() as i32;
    let stride = surf.stride();
    let pixels = surf.pixels_mut();

    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = x.saturating_add(w.min(i32::MAX as u32) as i32).min(sw);
    let y1 = y.saturating_add(h.min(i32::MAX as u32) as i32).min(sh);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let [b, g, r, a] = color.as_bgra_bytes();
    if a == 0 { return; }
    if a == 255 {
        let color = [b, g, r, a];
        for py in y0..y1 {
            let start = py as usize * stride + x0 as usize * 4;
            let end = py as usize * stride + x1 as usize * 4;
            for pixel in pixels[start..end].chunks_exact_mut(4) {
                pixel.copy_from_slice(&color);
            }
        }
        return;
    }
    for py in y0..y1 {
        let row_start = py as usize * stride;
        for px in x0..x1 {
            let offset = row_start + px as usize * 4;
            blend_pixel(&mut pixels[offset..offset + 4], b, g, r, a);
        }
    }
}

fn blend_pixel(pixel: &mut [u8], b: u8, g: u8, r: u8, a: u8) {
    if a == 255 {
        pixel.copy_from_slice(&[b, g, r, 255]);
    } else if a != 0 {
        let alpha = a as u32;
        let inverse = 255 - alpha;
        for (dst, src) in pixel[..3].iter_mut().zip([b, g, r]) {
            *dst = ((src as u32 * alpha + *dst as u32 * inverse + 127) / 255) as u8;
        }
        pixel[3] = (alpha + pixel[3] as u32 * inverse / 255) as u8;
    }
}

pub fn stroke_rect(surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32, border: Color) {
    if w == 0 || h == 0 {
        return;
    }
    fill_rect(surf, x, y, w, 1, border);
    fill_rect(surf, x, y + h as i32 - 1, w, 1, border);
    fill_rect(surf, x, y, 1, h, border);
    fill_rect(surf, x + w as i32 - 1, y, 1, h, border);
}
pub fn draw_book_icon(surf: &mut ViSurface, cx: i32, cy: i32, color: Color) {
    // Left page outline
    fill_rect(surf, cx - 7, cy - 5, 6, 1, color);
    fill_rect(surf, cx - 7, cy + 5, 6, 1, color);
    fill_rect(surf, cx - 7, cy - 5, 1, 11, color);
    fill_rect(surf, cx - 1, cy - 5, 1, 11, color);
    // Left page lines
    fill_rect(surf, cx - 5, cy - 2, 3, 1, color);
    fill_rect(surf, cx - 5, cy + 1, 3, 1, color);

    // Right page outline
    fill_rect(surf, cx + 1, cy - 5, 6, 1, color);
    fill_rect(surf, cx + 1, cy + 5, 6, 1, color);
    fill_rect(surf, cx + 1, cy - 5, 1, 11, color);
    fill_rect(surf, cx + 7, cy - 5, 1, 11, color);
    // Right page lines
    fill_rect(surf, cx + 3, cy - 2, 3, 1, color);
    fill_rect(surf, cx + 3, cy + 1, 3, 1, color);

    // Center spine
    fill_rect(surf, cx, cy - 5, 1, 12, color);
}

pub fn draw_globe_icon(surf: &mut ViSurface, cx: i32, cy: i32, color: Color) {
    // Outer circle
    fill_rect(surf, cx - 3, cy - 6, 7, 1, color);
    fill_rect(surf, cx - 3, cy + 6, 7, 1, color);
    fill_rect(surf, cx - 6, cy - 3, 1, 7, color);
    fill_rect(surf, cx + 6, cy - 3, 1, 7, color);
    // Corner pixels
    fill_rect(surf, cx - 5, cy - 5, 2, 1, color);
    fill_rect(surf, cx + 4, cy - 5, 2, 1, color);
    fill_rect(surf, cx - 5, cy + 5, 2, 1, color);
    fill_rect(surf, cx + 4, cy + 5, 2, 1, color);
    fill_rect(surf, cx - 5, cy - 4, 1, 2, color);
    fill_rect(surf, cx + 5, cy - 4, 1, 2, color);
    fill_rect(surf, cx - 5, cy + 3, 1, 2, color);
    fill_rect(surf, cx + 5, cy + 3, 1, 2, color);

    // Equator line
    fill_rect(surf, cx - 5, cy, 11, 1, color);

    // Prime meridian vertical line
    fill_rect(surf, cx, cy - 5, 1, 11, color);

    // Meridian inner curves
    fill_rect(surf, cx - 3, cy - 4, 1, 9, color);
    fill_rect(surf, cx + 3, cy - 4, 1, 9, color);
}

/// Paint measured, grayscale-antialiased text within a caller's viewport.
pub fn text(
    fonts: &mut TextFonts, surf: &mut ViSurface, x: f32, y: f32,
    value: &str, face: FontFace, px: f32, color: Color,
    clip: (i32, i32, i32, i32),
) -> f32 {
    let width = surf.width();
    let height = surf.height();
    let stride = surf.stride();
    fonts.draw_text(surf.pixels_mut(), width, height, stride, x, y, value,
        face, px, color.as_bgra_bytes(), clip)
}

pub fn clipped_rect(
    surf: &mut ViSurface, x: i32, y: i32, w: u32, h: u32,
    color: Color, clip: (i32, i32, i32, i32),
) {
    let x0 = x.max(clip.0);
    let y0 = y.max(clip.1);
    let x1 = x.saturating_add(w as i32).min(clip.2);
    let y1 = y.saturating_add(h as i32).min(clip.3);
    if x1 > x0 && y1 > y0 {
        fill_rect(surf, x0, y0, (x1 - x0) as u32, (y1 - y0) as u32, color);
    }
}

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
