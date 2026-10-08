// SPDX-License-Identifier: MIT
//! Shared scalable typography for layout and paint. GUI defaults to bundled Inter 16px.

use ostd::font_atlas::GlyphAtlas;

pub struct FontContext {
    pub atlas: GlyphAtlas,
    pub size_px: f32,
}

impl Default for FontContext {
    fn default() -> Self {
        Self {
            atlas: GlyphAtlas::from_static(ostd::typography::INTER_REGULAR)
                .expect("bundled Inter must be a valid static TrueType font"),
            size_px: 16.0,
        }
    }
}

impl FontContext {
    /// Load a custom font used by both layout and paint.
    pub fn with_font(font_bytes: &[u8], size_px: f32) -> Option<Self> {
        if !ostd::font_atlas::supported_size(size_px) {
            return None;
        }
        GlyphAtlas::new(font_bytes).map(|atlas| Self { atlas, size_px })
    }

    pub fn measure(&mut self, text: &str) -> f32 {
        self.atlas.measure(text, self.size_px)
    }

    pub fn line_height(&self) -> f32 {
        self.atlas.line_height(self.size_px)
    }

    /// Populate original UTF-8 cluster boundaries with normalized, kerned pen advances.
    /// Storage is reused across layouts. Vietnamese combining sequences are indivisible.
    pub fn cursor_positions(&mut self, text: &str, positions: &mut alloc::vec::Vec<(usize, f32)>) {
        positions.clear();
        positions.push((0, 0.0));
        let mut start = 0;
        let mut pen = 0.0;
        let mut previous = None;
        for end in text.char_indices()
            .filter_map(|(i, ch)| if i > 0 && !ostd::typography::is_combining_mark(ch) { Some(i) } else { None })
            .chain(core::iter::once(text.len()))
        {
            for ch in ostd::typography::normalized_chars(&text[start..end]) {
                if let Some(left) = previous { pen += self.atlas.kerning(left, ch, self.size_px); }
                pen += self.atlas.advance(ch, self.size_px);
                previous = Some(ch);
            }
            if end > 0 { positions.push((end, pen)); }
            start = end;
        }
    }

    pub fn hit_position(positions: &[(usize, f32)], x: f32) -> usize {
        for pair in positions.windows(2) {
            if x < (pair[0].1 + pair[1].1) * 0.5 { return pair[0].0; }
        }
        positions.last().map_or(0, |p| p.0)
    }
}

#[cfg(test)]
mod tests {
    use super::FontContext;
    use crate::canvas::{Color, FramebufferCanvas, ViCanvas};
    use crate::layout::{Constraints, Point, Rect, Size};
    use crate::node::ViNode;
    use crate::signal::Signal;

    #[test]
    fn proportional_layout_updates_for_equal_byte_length_text() {
        let mut font = FontContext::default();
        let text = Signal::new(alloc::string::String::from("iii"));
        let mut label = crate::node_widgets::label::Label::new(text.clone());
        let constraints = Constraints::root(Size::new(300.0, 100.0));
        let narrow = label.layout(constraints, &mut font);
        text.set(alloc::string::String::from("WWW"));
        let wide = label.layout(constraints, &mut font);
        assert!(wide.w > narrow.w);
        assert_eq!(wide.w, font.measure("WWW"));
        assert_eq!(wide.h, font.line_height());
    }

    #[test]
    fn custom_size_and_v1_layout_share_font_metrics() {
        use crate::widget::ViWidget;
        let mut font = FontContext::with_font(ostd::typography::INTER_REGULAR, 24.0).unwrap();
        let label = crate::widgets::label::Label::new("Tiếng Việt");
        let node = label.layout(Constraints::root(Size::new(500.0, 100.0)), &mut font);
        assert_eq!(node.bounds.w, font.measure("Tiếng Việt"));
        assert_eq!(node.bounds.h, font.line_height());
    }

    #[test]
    fn cursor_boundaries_match_nfc_advances_without_splitting_vietnamese_marks() {
        let mut font = FontContext::default();
        let mut positions = alloc::vec::Vec::new();
        let text = "e\u{0302}\u{0301} Wi";
        font.cursor_positions(text, &mut positions);
        assert_eq!(positions[1].0, "e\u{0302}\u{0301}".len());
        assert_eq!(positions[1].1, font.measure("ế"));
        assert_eq!(positions.last().unwrap().1, font.measure(text));
        for &(byte, x) in &positions {
            assert_eq!(FontContext::hit_position(&positions, x), byte);
        }
    }

    #[test]
    fn canvas_text_is_normalized_antialiased_clipped_and_bgra() {
        let mut font = FontContext::default();
        let mut red = alloc::vec![0; 100 * 40 * 4];
        let mut blue = red.clone();
        for (pixels, text, color) in [
            (&mut red, "ế", Color::rgb(255, 0, 0)),
            (&mut blue, "e\u{0302}\u{0301}", Color::rgb(0, 0, 255)),
        ] {
            let mut canvas = FramebufferCanvas::new(pixels, 400, 100, 40);
            canvas.clip_push(Rect::new(4.0, 0.0, 10.0, 40.0));
            canvas.draw_text_scaled(Point::new(2.0, 2.0), text, 16.0, color, &mut font.atlas);
        }
        let mut partial_coverage = false;
        let mut painted = false;
        for (index, (r, b)) in red.chunks_exact(4).zip(blue.chunks_exact(4)).enumerate() {
            assert_eq!(r[3], b[3]);
            assert_eq!(r[2], b[0]);
            assert_eq!(r[0], 0);
            assert_eq!(b[2], 0);
            if index % 100 < 4 || index % 100 >= 14 { assert_eq!(r[3], 0); }
            painted |= r[3] > 0;
            partial_coverage |= r[3] > 0 && r[3] < 255;
        }
        assert!(painted && partial_coverage);
    }
}
