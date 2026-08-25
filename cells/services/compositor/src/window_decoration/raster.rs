use api::display::Rect;

use super::{clip_to_screen, control_rect, end, Control, TITLE};

const FRAME_COLOR: [u8; 4] = [0x3b, 0x34, 0x2d, 0xff];
const TITLE_INACTIVE: [u8; 4] = [0x56, 0x4d, 0x42, 0xff];
const TITLE_ACTIVE: [u8; 4] = [0x83, 0x65, 0x31, 0xff];
const CLOSE: [u8; 4] = [0x45, 0x45, 0xcb, 0xff];
const MAXIMIZE: [u8; 4] = [0x53, 0xa4, 0x36, 0xff];
const MINIMIZE: [u8; 4] = [0x53, 0xa4, 0xd6, 0xff];
const SYMBOL: [u8; 4] = [0xf0, 0xf0, 0xf0, 0xff];

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
    paint_rect(
        pixels,
        width,
        height,
        Rect {
            x: outer.x,
            y: outer.y,
            w: outer.w,
            h: content.y.saturating_sub(outer.y) as u32,
        },
        dirty,
        FRAME_COLOR,
    );
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
    paint_rect(
        pixels,
        width,
        height,
        Rect {
            x: content_right,
            y: content.y,
            w: end(outer.x, outer.w).saturating_sub(content_right) as u32,
            h: content.h,
        },
        dirty,
        FRAME_COLOR,
    );
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
    let title = Rect {
        x: outer.x,
        y: content.y.saturating_sub(TITLE),
        w: outer.w,
        h: TITLE as u32,
    };
    paint_rect(
        pixels,
        width,
        height,
        title,
        dirty,
        if active { TITLE_ACTIVE } else { TITLE_INACTIVE },
    );
    for control in [Control::Close, Control::Maximize, Control::Minimize] {
        let rect = control_rect(content, control);
        paint_rect(
            pixels,
            width,
            height,
            rect,
            dirty,
            match control {
                Control::Close => CLOSE,
                Control::Maximize => MAXIMIZE,
                Control::Minimize => MINIMIZE,
            },
        );
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
    let symbol = match control {
        Control::Close => Rect {
            x: mid_x.saturating_sub(4),
            y: mid_y,
            w: 8,
            h: 1,
        },
        Control::Maximize => Rect {
            x: mid_x.saturating_sub(4),
            y: mid_y.saturating_sub(4),
            w: 8,
            h: 1,
        },
        Control::Minimize => Rect {
            x: mid_x.saturating_sub(4),
            y: mid_y.saturating_add(3),
            w: 8,
            h: 1,
        },
    };
    paint_rect(pixels, width, height, symbol, dirty, SYMBOL);
}

fn paint_rect(pixels: &mut [u8], width: u32, height: u32, rect: Rect, dirty: Rect, color: [u8; 4]) {
    let Some(rect) = clip_to_screen(intersect(rect, dirty), width, height) else {
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

fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = end(a.x, a.w).min(end(b.x, b.w));
    let bottom = end(a.y, a.h).min(end(b.y, b.h));
    Rect {
        x,
        y,
        w: right.saturating_sub(x) as u32,
        h: bottom.saturating_sub(y) as u32,
    }
}
