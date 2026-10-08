//! `GlyphAtlas` — on-demand outline rasterization for ViUI.
//!
//! Uses ab_glyph with its no_std/libm backend. Bundled font data is borrowed;
//! only requested glyph bitmaps remain resident, not all font outlines.
//!
//! The bounded bitmap cache is keyed by `(glyph_id, px_bits)`; unsupported
//! scalars share one missing-glyph entry. Cached paints do not allocate.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use ab_glyph::{point, Font, FontArc, FontRef, FontVec};
use crate::typography::normalized_chars;

// ─── Public types ────────────────────────────────────────────────────────────

/// Per-glyph layout and rasterization metrics.
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

// A long-lived console or document must not retain every scalar/size ever seen.
const MAX_CACHED_GLYPHS: usize = 512;
const MAX_CACHE_BYTES: usize = 256 * 1024;
pub const MAX_FONT_PX: f32 = 128.0;

pub fn supported_size(px: f32) -> bool {
    px.is_finite() && px > 0.0 && px <= MAX_FONT_PX
}

fn require_supported_size(px: f32) {
    assert!(supported_size(px), "font size must be finite and in (0, 128] pixels");
}

/// Scalable outline rasterizer with an application-owned glyph bitmap cache.
///
/// Rasterized bitmaps are cached by `(codepoint, size_bits)` — repeated draws
/// for the same character at the same size are essentially free.
///
/// # Usage
/// ```no_run
/// use ostd::font_atlas::GlyphAtlas;
///
/// # fn rasterize(font_bytes: &[u8]) {
/// let mut atlas = GlyphAtlas::new(font_bytes).expect("valid font");
/// let (metrics, bitmap) = atlas.rasterize('A', 16.0);
/// # let _ = (metrics, bitmap);
/// # }
/// ```
pub struct GlyphAtlas {
    font: FontArc,
    units_per_em: f32,
    cache: BTreeMap<(u32, u32), CachedGlyph>,
    cache_bytes: usize,
}

impl GlyphAtlas {
    /// Load an owned copy of arbitrary TrueType/OpenType data.
    /// Bundled static assets should use `from_static` to avoid copying.
    pub fn new(font_bytes: &[u8]) -> Option<Self> {
        FontVec::try_from_vec(font_bytes.to_vec()).ok()
            .and_then(|font| Self::from_font(FontArc::new(font)))
    }

    /// Borrow embedded static data; no duplicate TTF buffer or eager outlines.
    pub fn from_static(font_bytes: &'static [u8]) -> Option<Self> {
        FontRef::try_from_slice(font_bytes).ok()
            .and_then(|font| Self::from_font(FontArc::new(font)))
    }

    fn from_font(font: FontArc) -> Option<Self> {
        let units_per_em = font.units_per_em()?;
        Some(Self { font, units_per_em, cache: BTreeMap::new(), cache_bytes: 0 })
    }

    fn scale(&self, px: f32) -> f32 {
        require_supported_size(px);
        // API sizes are pixels per em, whereas ab_glyph uses ascent-descent.
        px * self.font.height_unscaled() / self.units_per_em
    }

    /// Rasterize `c` at `px` pixels tall.
    ///
    /// Returns `(GlyphMetrics, coverage_bitmap)`.  The bitmap is 1 byte per pixel
    /// stored row-major, top-left origin, width × height bytes total.
    /// Repeated calls for the same `(c, px)` return the cached result.
    pub fn rasterize(&mut self, c: char, px: f32) -> (GlyphMetrics, &[u8]) {
        // Unsupported scalars share the font's missing-glyph cache entry.
        let key = (self.font.glyph_id(c).0 as u32, px.to_bits());
        if !self.cache.contains_key(&key)
            && (self.cache.len() >= MAX_CACHED_GLYPHS || self.cache_bytes >= MAX_CACHE_BYTES)
        {
            self.cache.clear();
            self.cache_bytes = 0;
        }
        let font = &self.font;
        let scale = self.scale(px);
        let units_per_em = self.units_per_em;
        let cache_bytes = &mut self.cache_bytes;
        let g = self.cache.entry(key).or_insert_with(|| {
            let id = font.glyph_id(c);
            let advance_width = font.h_advance_unscaled(id) * px / units_per_em;
            let mut metrics = GlyphMetrics {
                xmin: 0, ymin: 0, width: 0, height: 0, advance_width,
            };
            let mut bitmap = Vec::new();
            if px > 0.0 {
                if let Some(outlined) = font.outline_glyph(id.with_scale_and_position(scale, point(0.0, 0.0))) {
                    let bounds = outlined.px_bounds();
                    metrics.xmin = bounds.min.x as i32;
                    metrics.ymin = -(bounds.max.y as i32);
                    metrics.width = bounds.width() as usize;
                    metrics.height = bounds.height() as usize;
                    let pixel_count = metrics.width.checked_mul(metrics.height)
                        .filter(|&count| count <= 64 * 1024)
                        .expect("glyph exceeds the 64KiB rasterization budget");
                    bitmap.resize(pixel_count, 0);
                    outlined.draw(|x, y, coverage| {
                        bitmap[y as usize * metrics.width + x as usize] =
                            (coverage * 255.0 + 0.5) as u8;
                    });
                }
            }
            *cache_bytes += bitmap.len() + core::mem::size_of::<CachedGlyph>();
            CachedGlyph { metrics, bitmap }
        });
        (g.metrics, &g.bitmap)
    }


    /// Distance from baseline to the ascender line in screen pixels (positive, y-down).
    ///
    /// Use as: `baseline_screen_y = origin_y + atlas.ascender(px)`.
    pub fn ascender(&self, px: f32) -> f32 {
        require_supported_size(px);
        self.font.ascent_unscaled() * px / self.units_per_em
    }

    /// Total line height (ascent − descent + line_gap) in pixels.
    pub fn line_height(&self, px: f32) -> f32 {
        require_supported_size(px);
        (self.font.height_unscaled() + self.font.line_gap_unscaled()) * px / self.units_per_em
    }

    /// Horizontal advance without outlining or allocating.
    pub fn advance(&self, c: char, px: f32) -> f32 {
        require_supported_size(px);
        self.font.h_advance_unscaled(self.font.glyph_id(c)) * px / self.units_per_em
    }

    /// Pair adjustment in pixels; identical in measuring and painting.
    pub fn kerning(&self, left: char, right: char, px: f32) -> f32 {
        require_supported_size(px);
        self.font.kern_unscaled(self.font.glyph_id(left), self.font.glyph_id(right)) * px / self.units_per_em
    }

    /// Advance of one NFC-normalized line, including pair kerning.
    pub fn measure(&self, text: &str, px: f32) -> f32 {
        let mut width = 0.0;
        let mut previous = None;
        for c in normalized_chars(text) {
            if let Some(left) = previous {
                width += self.kerning(left, c, px);
            }
            width += self.advance(c, px);
            previous = Some(c);
        }
        width
    }

    /// Paint a normalized line using cached grayscale coverage and BGRA
    /// source-over compositing. `y` is the line top, not the glyph top.
    /// Returns the same pen advance as `measure`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(
        &mut self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        stride: usize,
        x: f32,
        y: f32,
        text: &str,
        px: f32,
        color: [u8; 4],
        clip: (i32, i32, i32, i32),
    ) -> f32 {
        if stride == 0 || color[3] == 0 {
            return self.measure(text, px);
        }
        let surface_w = (width as usize).min(stride / 4).min(i32::MAX as usize) as i32;
        let surface_h = (height as usize).min(pixels.len() / stride).min(i32::MAX as usize) as i32;
        let clip = (
            clip.0.max(0), clip.1.max(0),
            clip.2.min(surface_w), clip.3.min(surface_h),
        );
        if clip.0 >= clip.2 || clip.1 >= clip.3 {
            return self.measure(text, px);
        }
        let baseline = y + self.ascender(px);
        let mut pen = x;
        let mut previous = None;
        for c in normalized_chars(text) {
            if let Some(left) = previous {
                pen += self.kerning(left, c, px);
            }
            let (metrics, bitmap) = self.rasterize(c, px);
            let gx = round_pixel(pen + metrics.xmin as f32);
            let gy = round_pixel(baseline - (metrics.ymin as f32 + metrics.height as f32));
            let x0 = gx.max(clip.0);
            let y0 = gy.max(clip.1);
            let x1 = gx.saturating_add(metrics.width.min(i32::MAX as usize) as i32).min(clip.2);
            let y1 = gy.saturating_add(metrics.height.min(i32::MAX as usize) as i32).min(clip.3);
            for screen_y in y0..y1 {
                let source_row = (screen_y - gy) as usize * metrics.width;
                let destination_row = screen_y as usize * stride;
                for screen_x in x0..x1 {
                    let coverage = bitmap[source_row + (screen_x - gx) as usize];
                    let alpha = (color[3] as u32 * coverage as u32 + 127) / 255;
                    if alpha == 0 {
                        continue;
                    }
                    let offset = destination_row + screen_x as usize * 4;
                    blend_coverage(&mut pixels[offset..offset + 4], color, alpha);
                }
            }
            pen += metrics.advance_width;
            previous = Some(c);
        }
        pen - x
    }
}

fn round_pixel(value: f32) -> i32 {
    if value >= 0.0 { (value + 0.5) as i32 } else { (value - 0.5) as i32 }
}

fn blend_coverage(destination: &mut [u8], color: [u8; 4], alpha: u32) {
    if alpha == 255 {
        destination.copy_from_slice(&[color[0], color[1], color[2], 255]);
        return;
    }
    let inverse = 255 - alpha;
    let destination_alpha = destination[3] as u32;
    if destination_alpha == 255 {
        for channel in 0..3 {
            destination[channel] = ((color[channel] as u32 * alpha
                + destination[channel] as u32 * inverse + 127) / 255) as u8;
        }
        return;
    }
    let alpha_numerator = alpha * 255 + destination_alpha * inverse;
    for channel in 0..3 {
        destination[channel] = ((color[channel] as u32 * alpha * 255
            + destination[channel] as u32 * destination_alpha * inverse
            + alpha_numerator / 2) / alpha_numerator) as u8;
    }
    destination[3] = ((alpha_numerator + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typography::{FontFace, TextFonts};
    use alloc::string::String;
    use alloc::vec;
    use unicode_normalization::UnicodeNormalization;

    const VIETNAMESE: &str = "ăâđêôơưáàảãạắằẳẵặấầẩẫậéèẻẽẹếềểễệíìỉĩịóòỏõọốồổỗộớờởỡợúùủũụứừửữựýỳỷỹỵĂÂĐÊÔƠƯÁÀẢÃẠẮẰẲẴẶẤẦẨẪẬÉÈẺẼẸẾỀỂỄỆÍÌỈĨỊÓÒỎÕỌỐỒỔỖỘỚỜỞỠỢÚÙỦŨỤỨỪỬỮỰÝỲỶỸỴ";

    #[test]
    fn bundled_faces_cover_vietnamese_and_keep_terminal_fixed_pitch() {
        for face in [
            FontFace::UiRegular, FontFace::UiSemibold, FontFace::UiItalic,
            FontFace::UiSemiboldItalic, FontFace::MonoRegular, FontFace::MonoBold,
        ] {
            let atlas = GlyphAtlas::from_static(face.bytes()).unwrap();
            for c in VIETNAMESE.chars() {
                assert_ne!(atlas.font.glyph_id(c).0, 0, "{face:?} missing {c}");
            }
            if matches!(face, FontFace::MonoRegular | FontFace::MonoBold) {
                let advance = atlas.advance('M', 16.0);
                for c in "iW 0ếĐ".chars() {
                    assert_eq!(atlas.advance(c, 16.0), advance, "{face:?}: {c}");
                }
            } else {
                assert!(atlas.measure("WWWW", 16.0) > atlas.measure("iiii", 16.0));
            }
        }
    }

    #[test]
    fn decomposed_vietnamese_measures_and_paints_identically() {
        let mut fonts = TextFonts::new();
        let text = "Tiếng Việt: ắ ậ ễ ỡ ự Đ";
        let decomposed: String = text.nfd().collect();
        let face = FontFace::UiRegular;
        assert_eq!(fonts.measure(face, text, 18.0), fonts.measure(face, &decomposed, 18.0));
        let mut composed_pixels = vec![255; 420 * 48 * 4];
        let mut decomposed_pixels = composed_pixels.clone();
        let measured = fonts.measure(face, text, 18.0);
        for (pixels, input) in [
            (&mut composed_pixels, text),
            (&mut decomposed_pixels, decomposed.as_str()),
        ] {
            let advance = fonts.draw_text(
                pixels, 420, 48, 420 * 4, 4.0, 4.0, input, face, 18.0,
                [30, 60, 90, 255], (0, 0, 420, 48),
            );
            assert!((advance - measured).abs() < 0.001);
        }
        assert_eq!(composed_pixels, decomposed_pixels);
        assert!(composed_pixels.chunks_exact(4).any(|p| p[0] > 30 && p[0] < 255));
        assert!(composed_pixels.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn clipping_preserves_outside_pixels_and_stride_padding() {
        let mut atlas = GlyphAtlas::from_static(FontFace::UiItalic.bytes()).unwrap();
        let width = 48;
        let height = 32;
        let stride = width * 4 + 12;
        let mut pixels = vec![37; stride * height];
        let before = pixels.clone();
        atlas.draw_text(
            &mut pixels, width as u32, height as u32, stride, -9.0, -3.0,
            "Tiếng Việt", 24.0, [20, 100, 220, 180], (5, 6, 35, 25),
        );
        for y in 0..height {
            for x in 0..width {
                if !(5..35).contains(&x) || !(6..25).contains(&y) {
                    let offset = y * stride + x * 4;
                    assert_eq!(&pixels[offset..offset + 4], &before[offset..offset + 4]);
                }
            }
            assert_eq!(&pixels[y * stride + width * 4..(y + 1) * stride],
                &before[y * stride + width * 4..(y + 1) * stride]);
        }
        assert!(pixels[6 * stride + 5 * 4..25 * stride].iter()
            .zip(&before[6 * stride + 5 * 4..25 * stride]).any(|(a, b)| a != b));
    }

    #[test]
    fn coverage_composes_straight_alpha_without_dark_fringes() {
        let color = [30, 90, 210, 255];
        let mut transparent = [0, 0, 0, 0];
        blend_coverage(&mut transparent, color, 128);
        assert_eq!(transparent, [30, 90, 210, 128]);
        let mut opaque = [255, 255, 255, 255];
        blend_coverage(&mut opaque, color, 128);
        assert_eq!(opaque, [142, 172, 232, 255]);
        blend_coverage(&mut opaque, color, 255);
        assert_eq!(opaque, color);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::typography::INTER_REGULAR;

    #[test]
    fn long_unicode_stream_reclaims_cache_and_shares_missing_glyph() {
        let mut atlas = GlyphAtlas::from_static(INTER_REGULAR).unwrap();
        for value in 0x20..0x2000 {
            if let Some(c) = char::from_u32(value) {
                atlas.rasterize(c, 16.0);
                assert!(atlas.cache.len() <= MAX_CACHED_GLYPHS);
                assert!(atlas.cache_bytes < MAX_CACHE_BYTES + 4096);
            }
        }
        atlas.cache.clear();
        atlas.cache_bytes = 0;
        for value in 0xf0000..0xf1000 {
            atlas.rasterize(char::from_u32(value).unwrap(), 16.0);
        }
        assert_eq!(atlas.cache.len(), 1);
        let (metrics, bitmap) = atlas.rasterize('ế', 16.0);
        assert!(metrics.width > 0 && metrics.height > 0);
        assert!(bitmap.iter().any(|&coverage| coverage > 0 && coverage < 255));
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;
    use crate::typography::INTER_REGULAR;

    #[test]
    fn invalid_sizes_are_rejected_before_raster_allocation() {
        for px in [0.0, -1.0, f32::NAN, f32::INFINITY, 100_000.0] {
            let mut atlas = GlyphAtlas::from_static(INTER_REGULAR).unwrap();
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                atlas.rasterize('A', px);
            })).is_err());
            assert!(atlas.cache.is_empty());
        }
        let mut atlas = GlyphAtlas::from_static(INTER_REGULAR).unwrap();
        let (metrics, bitmap) = atlas.rasterize('ế', MAX_FONT_PX);
        assert_eq!(bitmap.len(), metrics.width * metrics.height);
        assert!(bitmap.len() <= 64 * 1024);
    }
}
