//! Compositor-owned window frame geometry, hit testing, and rasterization.

use api::display::Rect;

mod raster;

pub const FRAME: i32 = 4;
pub const TITLE: i32 = 20;
pub(super) const CONTROL: i32 = 16;
pub(super) const INSET: i32 = 2;

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
    let frame_left = content.x.saturating_sub(FRAME);
    let control_left = right.saturating_sub(INSET + CONTROL * 3);
    let x = frame_left.min(control_left);
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
    for control in [Control::Close, Control::Maximize, Control::Minimize] {
        if contains(control_rect(content, control), x, y) {
            return Some(Hit::Control(control));
        }
    }
    let left = x < content.x;
    let right = x >= end(content.x, content.w);
    let top = y < content.y.saturating_sub(TITLE);
    let bottom = y >= end(content.y, content.h);
    match (top, bottom, left, right) {
        (true, _, true, _) => Some(Hit::Resize(ResizeEdge::NorthWest)),
        (true, _, _, true) => Some(Hit::Resize(ResizeEdge::NorthEast)),
        (_, true, true, _) => Some(Hit::Resize(ResizeEdge::SouthWest)),
        (_, true, _, true) => Some(Hit::Resize(ResizeEdge::SouthEast)),
        (true, _, _, _) => Some(Hit::Resize(ResizeEdge::North)),
        (_, true, _, _) => Some(Hit::Resize(ResizeEdge::South)),
        (_, _, true, _) => Some(Hit::Resize(ResizeEdge::West)),
        (_, _, _, true) => Some(Hit::Resize(ResizeEdge::East)),
        _ if y < content.y => Some(Hit::Title),
        _ => Some(Hit::Content),
    }
}

pub fn paint(pixels: &mut [u8], width: u32, height: u32, content: Rect, dirty: Rect, active: bool) {
    raster::paint(pixels, width, height, content, dirty, active);
}

pub(super) fn control_rect(content: Rect, control: Control) -> Rect {
    let right = end(content.x, content.w)
        .saturating_add(FRAME)
        .saturating_sub(INSET);
    let index = match control {
        Control::Close => 0,
        Control::Maximize => 1,
        Control::Minimize => 2,
    };
    Rect {
        x: right.saturating_sub(CONTROL * (index + 1)),
        y: content.y.saturating_sub(TITLE).saturating_add(INSET),
        w: CONTROL as u32,
        h: CONTROL as u32,
    }
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.x && x < end(rect.x, rect.w) && y >= rect.y && y < end(rect.y, rect.h)
}

pub(super) fn end(start: i32, extent: u32) -> i32 {
    start.saturating_add(extent.min(i32::MAX as u32) as i32)
}
