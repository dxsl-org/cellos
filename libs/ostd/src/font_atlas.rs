//! `GlyphAtlas` — fontdue-backed scalable glyph rasterizer for ViUI.
//!
//! fontdue 0.9 is `no_std`-compatible when built with
//! `default-features = false, features = ["hashbrown"]`.
//!
//! The atlas caches rendered bitmaps in a `BTreeMap` keyed by `(codepoint, px_bits)`
//! so subsequent draws for the same (char, size) pair skip re-rasterization entirely.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use fontdue::{Font, FontSettings};

// ─── Public types ────────────────────────────────────────────────────────────

/// Per-glyph layout and rasterization metrics (mirrors `fontdue::Metrics`).
///
/// Y axis is **math convention** (up = positive):
/// - `ymin` = bottom of bounding box relative to baseline (≤ 0 for descenders)
/// - top of bounding box = `ymin + height`
///
/// In screen coords (y-down): glyph_top_screen = `baseline_y - (ymin + height)`.
#[derive(Copy, Clone, Debug)]
pub struct GlyphMetrics {
    /// Horizontal offset from pen position to glyph left edge (pixels).
    pub xmin: i32,
    /// Distance from baseline to glyph bottom (math y-up; negative for descenders).
    pub ymin: i32,
    /// Rasterized bitmap width in pixels.
    pub width: usize,
    /// Rasterized bitmap height in pixels.
    pub height: usize,
    /// Advance width — how far to move the pen after this glyph.
    pub advance_width: f32,
}

// ─── GlyphAtlas ──────────────────────────────────────────────────────────────

struct CachedGlyph {
    metrics: GlyphMetrics,
    /// 1 byte per pixel, linear coverage (0 = transparent, 255 = solid).
    bitmap: Vec<u8>,
}

/// Scalable glyph rasterizer wrapping a `fontdue::Font`.
///
/// Rasterized bitmaps are cached by `(codepoint, size_bits)` — repeated draws
/// for the same character at the same size are essentially free.
///
/// # Usage
/// ```no_run
/// let atlas = GlyphAtlas::new(include_bytes!("../assets/font.ttf")).unwrap();
/// let (metrics, bitmap) = atlas.rasterize('A', 16.0);
/// ```
pub struct GlyphAtlas {
    font: Font,
    cache: BTreeMap<(u32, u32), CachedGlyph>,
}

impl GlyphAtlas {
    /// Load a TrueType/OpenType font from raw bytes.
    ///
    /// Returns `None` when the font data is invalid.
    pub fn new(font_bytes: &[u8]) -> Option<Self> {
        Font::from_bytes(font_bytes, FontSettings::default())
            .ok()
            .map(|font| Self {
                font,
                cache: BTreeMap::new(),
            })
    }

    /// Rasterize `c` at `px` pixels tall.
    ///
    /// Returns `(GlyphMetrics, coverage_bitmap)`.  The bitmap is 1 byte per pixel
    /// stored row-major, top-left origin, width × height bytes total.
    /// Repeated calls for the same `(c, px)` return the cached result.
    pub fn rasterize(&mut self, c: char, px: f32) -> (GlyphMetrics, &[u8]) {
        let key = (c as u32, px.to_bits());
        if !self.cache.contains_key(&key) {
            let (m, bitmap) = self.font.rasterize(c, px);
            self.cache.insert(
                key,
                CachedGlyph {
                    metrics: GlyphMetrics {
                        xmin: m.xmin,
                        ymin: m.ymin,
                        width: m.width,
                        height: m.height,
                        advance_width: m.advance_width,
                    },
                    bitmap,
                },
            );
        }
        let g = self.cache.get(&key).unwrap();
        (g.metrics, &g.bitmap)
    }

    /// Metrics for `c` at `px` without rasterizing (no bitmap allocation).
    pub fn metrics(&self, c: char, px: f32) -> GlyphMetrics {
        let m = self.font.metrics(c, px);
        GlyphMetrics {
            xmin: m.xmin,
            ymin: m.ymin,
            width: m.width,
            height: m.height,
            advance_width: m.advance_width,
        }
    }

    /// Distance from baseline to the ascender line in screen pixels (positive, y-down).
    ///
    /// Use as: `baseline_screen_y = origin_y + atlas.ascender(px)`.
    pub fn ascender(&self, px: f32) -> f32 {
        self.font
            .horizontal_line_metrics(px)
            .map(|lm| lm.ascent)
            .unwrap_or(px * 0.8)
    }

    /// Total line height (ascent − descent + line_gap) in pixels.
    pub fn line_height(&self, px: f32) -> f32 {
        self.font
            .horizontal_line_metrics(px)
            .map(|lm| lm.ascent - lm.descent + lm.line_gap)
            .unwrap_or(px * 1.2)
    }

    /// Horizontal advance for `c` at `px` (fast — hits fontdue internal cache, no BTreeMap).
    pub fn advance(&self, c: char, px: f32) -> f32 {
        self.font.metrics(c, px).advance_width
    }
}
