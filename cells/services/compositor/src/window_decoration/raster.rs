use api::display::Rect;

use super::{clip_to_screen, control_rect, end, Control, TITLE};

const FRAME_COLOR: [u8; 4] = [0x20, 0x20, 0x24, 0xff];
const TAB_ACTIVE: [u8; 4] = [0x20, 0xd2, 0xfd, 0xff]; // Haiku Golden Yellow (BGRA: R=253, G=210, B=32)
const TAB_ACTIVE_HL: [u8; 4] = [0x60, 0xe8, 0xff, 0xff]; // Lighter gold highlight
const TAB_INACTIVE: [u8; 4] = [0xc4, 0xbf, 0xb8, 0xff]; // Haiku Inactive Muted Slate
const TAB_INACTIVE_HL: [u8; 4] = [0xdc, 0xd8, 0xd4, 0xff];
const BTN_BORDER: [u8; 4] = [0x24, 0x20, 0x1c, 0xff];
const BTN_ACTIVE_BG: [u8; 4] = [0x18, 0xb8, 0xe0, 0xff];
const BTN_INACTIVE_BG: [u8; 4] = [0xa8, 0xa4, 0xa0, 0xff];
const GLYPH: [u8; 4] = [0x20, 0x20, 0x20, 0xff];

pub(super) fn paint(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    content: Rect,
    dirty: Rect,
    active: bool,
) {
    let outer = super::bounds(content);
    let content_right = end(content.x, content.w);
    let content_bottom = end(content.y, content.h);
    let tab = super::tab_rect(content);
    let tab_end = end(tab.x, tab.w);
    let frame_right = end(outer.x, outer.w);

    // 1. Left border
    paint_rect(
        pixels,
        width,
        height,
        Rect {
            x: outer.x,
            y: content.y,
            w: content.x.saturating_sub(outer.x) as u32,
            h: content.h,
        },
        dirty,
        FRAME_COLOR,
    );

    // 2. Right border
    paint_rect(
        pixels,
        width,
        height,
        Rect {
            x: content_right,
            y: content.y,
            w: frame_right.saturating_sub(content_right) as u32,
            h: content.h,
        },
        dirty,
        FRAME_COLOR,
    );

    // 3. Bottom border
    paint_rect(
        pixels,
        width,
        height,
        Rect {
            x: outer.x,
            y: content_bottom,
            w: outer.w,
            h: end(outer.y, outer.h).saturating_sub(content_bottom) as u32,
        },
        dirty,
        FRAME_COLOR,
    );

    // 4. Top border to the right of the tab (thin frame border only; space above is empty)
    if frame_right > tab_end {
        paint_rect(
            pixels,
            width,
            height,
            Rect {
                x: tab_end,
                y: content.y.saturating_sub(super::FRAME),
                w: (frame_right - tab_end) as u32,
                h: super::FRAME as u32,
            },
            dirty,
            FRAME_COLOR,
        );
    }

    // 5. Paint Haiku Tab sitting on top of the window frame
    let tab_bg = if active { TAB_ACTIVE } else { TAB_INACTIVE };
    let tab_hl = if active { TAB_ACTIVE_HL } else { TAB_INACTIVE_HL };
    paint_rect(pixels, width, height, tab, dirty, tab_bg);

    // Tab top highlight line (1px)
    paint_rect(
        pixels,
        width,
        height,
        Rect { x: tab.x, y: tab.y, w: tab.w, h: 1 },
        dirty,
        tab_hl,
    );
    // Tab left border (1px)
    paint_rect(
        pixels,
        width,
        height,
        Rect { x: tab.x, y: tab.y, w: 1, h: tab.h },
        dirty,
        FRAME_COLOR,
    );
    // Tab right border (1px)
    paint_rect(
        pixels,
        width,
        height,
        Rect { x: tab_end.saturating_sub(1), y: tab.y, w: 1, h: tab.h },
        dirty,
        FRAME_COLOR,
    );
    // Tab top line (1px)
    paint_rect(
        pixels,
        width,
        height,
        Rect { x: tab.x, y: tab.y, w: tab.w, h: 1 },
        dirty,
        FRAME_COLOR,
    );

    // 6. Paint BeOS/Haiku close box and zoom box on tab
    for control in [Control::Close, Control::Maximize] {
        let rect = control_rect(content, control);
        // Button border
        paint_rect(pixels, width, height, rect, dirty, BTN_BORDER);
        // Button interior
        if rect.w > 2 && rect.h > 2 {
            paint_rect(
                pixels,
                width,
                height,
                Rect {
                    x: rect.x + 1,
                    y: rect.y + 1,
                    w: rect.w - 2,
                    h: rect.h - 2,
                },
                dirty,
                if active { BTN_ACTIVE_BG } else { BTN_INACTIVE_BG },
            );
        }
        paint_symbol(pixels, width, height, rect, dirty, control);
    }
}
fn paint_symbol(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    dirty: Rect,
    control: Control,
) {
    let mid_x = rect.x.saturating_add(super::CONTROL / 2);
    let mid_y = rect.y.saturating_add(super::CONTROL / 2);
    match control {
        Control::Close => {
            // BeOS classic inner square
            paint_rect(
                pixels,
                width,
                height,
                Rect {
                    x: mid_x - 3,
                    y: mid_y - 3,
                    w: 6,
                    h: 6,
                },
                dirty,
                GLYPH,
            );
        }
        Control::Maximize => {
            // BeOS zoom glyph (two horizontal bars)
            paint_rect(
                pixels,
                width,
                height,
                Rect {
                    x: mid_x - 3,
                    y: mid_y - 3,
                    w: 6,
                    h: 2,
                },
                dirty,
                GLYPH,
            );
            paint_rect(
                pixels,
                width,
                height,
                Rect {
                    x: mid_x - 3,
                    y: mid_y + 1,
                    w: 6,
                    h: 2,
                },
                dirty,
                GLYPH,
            );
        }
        Control::Minimize => {}
    }
}

fn paint_rect(pixels: &mut [u8], width: u32, height: u32, rect: Rect, dirty: Rect, color: [u8; 4]) {
    let Some(rect) = intersect(rect, dirty).and_then(|r| clip_to_screen(r, width, height)) else {
        return;
    };
    let stride = width as usize * 4;
    for y in rect.y as usize..rect.y.saturating_add(rect.h as i32) as usize {
        let start = y.saturating_mul(stride).saturating_add(rect.x as usize * 4);
        let end = start.saturating_add(rect.w as usize * 4).min(pixels.len());
        for pixel in pixels[start..end].chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = end(a.x, a.w).min(end(b.x, b.w));
    let bottom = end(a.y, a.h).min(end(b.y, b.h));
    if right <= x || bottom <= y {
        None
    } else {
        Some(Rect {
            x,
            y,
            w: (right - x) as u32,
            h: (bottom - y) as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disjoint_rects_never_underflow_to_bogus_decorations() {
        let title = Rect { x: 124, y: 32, w: 1032, h: 28 };
        let cursor_dirty = Rect { x: 200, y: 150, w: 16, h: 16 };
        assert_eq!(intersect(title, cursor_dirty), None);

        let mut pixels = alloc::vec![0u8; 1280 * 800 * 4];
        paint_rect(&mut pixels, 1280, 800, title, cursor_dirty, TAB_ACTIVE);
        assert!(pixels.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn overlapping_rects_compute_exact_intersection() {
        let a = Rect { x: 10, y: 20, w: 100, h: 50 };
        let b = Rect { x: 50, y: 40, w: 100, h: 50 };
        assert_eq!(intersect(a, b), Some(Rect { x: 50, y: 40, w: 60, h: 30 }));
    }
}
