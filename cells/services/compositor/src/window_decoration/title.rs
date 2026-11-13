//! Cell Tab title text (ADR-0027: a tab shows the application's name).

use ostd::display::linear::Bgra;
use ostd::font_atlas::GlyphAtlas;
use ostd::typography::INTER_SEMIBOLD;
use wire::display::Rect;

const TITLE_PX: f32 = 13.0;
const ELLIPSIS: &str = "\u{2026}";

/// Inter SemiBold, loaded on first use. Only this face is linked into the
/// compositor (`TextFonts` would pull every bundled face).
pub struct TitleFont {
    atlas: Option<GlyphAtlas>,
}

impl TitleFont {
    pub const fn new() -> Self {
        Self { atlas: None }
    }

    fn atlas(&mut self) -> &mut GlyphAtlas {
        self.atlas
            .get_or_insert_with(|| GlyphAtlas::from_static(INTER_SEMIBOLD).expect("bundled font"))
    }

    /// Width `text` takes when drawn.
    pub(super) fn measure(&mut self, text: &str) -> u32 {
        (self.atlas().measure(text, TITLE_PX) + 0.5) as u32
    }

    /// Draw `text` left-aligned and vertically centred in `area`, clipped to
    /// `area ∩ dirty`. A title wider than `area` is cut at a character
    /// boundary and ends with an ellipsis instead of a half glyph.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw(
        &mut self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        area: Rect,
        dirty: Rect,
        text: &str,
        color: Bgra,
    ) {
        if text.is_empty() || area.w == 0 || area.h == 0 {
            return;
        }
        let Some(clip) = intersect(area, dirty) else {
            return;
        };
        let atlas = self.atlas();
        let available = area.w as f32;
        let fits = atlas.measure(text, TITLE_PX) <= available;
        let shown = if fits {
            text
        } else {
            let room = available - atlas.measure(ELLIPSIS, TITLE_PX);
            let end = text
                .char_indices()
                .map(|(at, c)| at + c.len_utf8())
                .take_while(|&end| atlas.measure(&text[..end], TITLE_PX) <= room)
                .last()
                .unwrap_or(0);
            &text[..end]
        };
        // Whole-pixel line top keeps glyph rows identical wherever the tab sits.
        let offset = (((area.h as f32 - atlas.line_height(TITLE_PX)) * 0.5).max(0.0) + 0.5) as i32;
        let y = area.y.saturating_add(offset) as f32;
        let clip = (
            clip.x,
            clip.y,
            clip.x + clip.w as i32,
            clip.y + clip.h as i32,
        );
        let stride = width as usize * 4;
        let pen = area.x as f32;
        let advance = atlas.draw_text(
            pixels, width, height, stride, pen, y, shown, TITLE_PX, color, clip,
        );
        if !fits {
            atlas.draw_text(
                pixels,
                width,
                height,
                stride,
                pen + advance,
                y,
                ELLIPSIS,
                TITLE_PX,
                color,
                clip,
            );
        }
    }
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = super::end(a.x, a.w).min(super::end(b.x, b.w));
    let bottom = super::end(a.y, a.h).min(super::end(b.y, b.h));
    (right > x && bottom > y).then(|| Rect {
        x,
        y,
        w: (right - x) as u32,
        h: (bottom - y) as u32,
    })
}
