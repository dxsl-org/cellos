//! Compositor-owned window frame geometry, hit testing, and rasterization.

use api::display::Rect;

mod raster;

pub const FRAME: i32 = 3;
pub const TITLE: i32 = 24;
pub(super) const CONTROL: i32 = 14;
pub(super) const INSET: i32 = 2;

pub fn tab_width(content_w: u32) -> u32 {
    let half = content_w / 2;
    let clamped = if half < 160 { 160 } else if half > 240 { 240 } else { half };
    clamped.min(content_w.saturating_add((FRAME * 2) as u32))
}

pub fn tab_rect(content: Rect) -> Rect {
    let tab_w = tab_width(content.w);
    Rect {
        x: content.x.saturating_sub(FRAME),
        y: content.y.saturating_sub(TITLE.saturating_add(FRAME)),
        w: tab_w,
        h: (TITLE + FRAME) as u32,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Close,
    Maximize,
    Minimize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    North,
    South,
    West,
    East,
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Control(Control),
    Resize(ResizeEdge),
    Title,
    Content,
}

pub fn bounds(content: Rect) -> Rect {
    let right = end(content.x, content.w).saturating_add(FRAME);
    let x = content.x.saturating_sub(FRAME);
    Rect {
        x,
        y: content.y.saturating_sub(FRAME.saturating_add(TITLE)),
        w: right.saturating_sub(x) as u32,
        h: content.h.saturating_add((FRAME * 2 + TITLE) as u32),
    }
}

pub fn clip_to_screen(rect: Rect, width: u32, height: u32) -> Option<Rect> {
    let x = rect.x.max(0);
    let y = rect.y.max(0);
    let right = end(rect.x, rect.w).min(width.min(i32::MAX as u32) as i32);
    let bottom = end(rect.y, rect.h).min(height.min(i32::MAX as u32) as i32);
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        w: (right - x) as u32,
        h: (bottom - y) as u32,
    })
}

pub fn hit_test(content: Rect, x: i32, y: i32) -> Option<Hit> {
    let outer = bounds(content);
    if !contains(outer, x, y) {
        return None;
    }

    let tab = tab_rect(content);

    // Check controls (Close on left of tab, Maximize/Zoom on right of tab)
    for control in [Control::Close, Control::Maximize, Control::Minimize] {
        if contains(control_rect(content, control), x, y) {
            return Some(Hit::Control(control));
        }
    }

    // Check if clicked inside the Haiku tab
    if contains(tab, x, y) {
        return Some(Hit::Title);
    }

    let tab_end = end(tab.x, tab.w);
    let content_right = end(content.x, content.w);
    let content_bottom = end(content.y, content.h);

    // BeOS / Haiku feature: open space to the right of the tab is transparent!
    if y < content.y.saturating_sub(FRAME) && x >= tab_end {
        return None;
    }

    let left = x < content.x;
    let right = x >= content_right;
    let top = y < content.y;
    let bottom = y >= content_bottom;

    match (top, bottom, left, right) {
        (true, _, true, _) => Some(Hit::Resize(ResizeEdge::NorthWest)),
        (true, _, _, true) => Some(Hit::Resize(ResizeEdge::NorthEast)),
        (_, true, true, _) => Some(Hit::Resize(ResizeEdge::SouthWest)),
        (_, true, _, true) => Some(Hit::Resize(ResizeEdge::SouthEast)),
        (true, _, _, _) => Some(Hit::Resize(ResizeEdge::North)),
        (_, true, _, _) => Some(Hit::Resize(ResizeEdge::South)),
        (_, _, true, _) => Some(Hit::Resize(ResizeEdge::West)),
        (_, _, _, true) => Some(Hit::Resize(ResizeEdge::East)),
        _ => Some(Hit::Content),
    }
}

pub fn paint(pixels: &mut [u8], width: u32, height: u32, content: Rect, dirty: Rect, active: bool) {
    raster::paint(pixels, width, height, content, dirty, active);
}

pub(super) fn control_rect(content: Rect, control: Control) -> Rect {
    let tab = tab_rect(content);
    let tab_end = end(tab.x, tab.w);
    let mid_y = tab.y.saturating_add((tab.h as i32 - CONTROL) / 2);

    match control {
        // Close box on the left side of the tab
        Control::Close => Rect {
            x: tab.x.saturating_add(6),
            y: mid_y,
            w: CONTROL as u32,
            h: CONTROL as u32,
        },
        // Zoom/Maximize button on the right side of the tab
        Control::Maximize => Rect {
            x: tab_end.saturating_sub(CONTROL + 6),
            y: mid_y,
            w: CONTROL as u32,
            h: CONTROL as u32,
        },
        Control::Minimize => Rect {
            x: tab_end.saturating_sub(CONTROL * 2 + 10),
            y: mid_y,
            w: CONTROL as u32,
            h: CONTROL as u32,
        },
    }
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.x && x < end(rect.x, rect.w) && y >= rect.y && y < end(rect.y, rect.h)
}

pub(super) fn end(start: i32, extent: u32) -> i32 {
    start.saturating_add(extent.min(i32::MAX as u32) as i32)
}
