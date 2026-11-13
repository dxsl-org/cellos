//! Cell Frame geometry, hit testing, and rasterization (CellOS Linear).
//!
//! A Cell Tab (ADR-0027) sits on the top-left of the Cell Frame and holds only
//! the app icon and the title, as in Haiku. The icon opens the window menu
//! (maximize/restore, minimize, close); a double click on the title toggles
//! maximize. Everything the compositor draws is 1px hairlines on the pixel grid with lightly rounded
//! outer corners; a soft drop shadow lives in the invisible
//! [`SHADOW_MARGIN`], and the innermost [`GRIP`] pixels of it resize.
//!
//! Vertical layout above the content: `TITLE` rows of tab, the last of which is
//! the hairline shared by the tab and the frame body.

use ostd::display::linear::{Radii, RoundRect, PANEL_RADIUS};
use wire::display::Rect;

mod menu;
mod palette;
mod panel;
mod raster;
mod tabs;
mod title;

pub use menu::{
    menu_item_at, menu_rect, menu_region, paint_menu, MenuAction, MenuEntry, MenuGlyph, MenuItems,
    MAX_MENU_ITEMS,
};
pub(crate) use palette::{chrome, ChromePalette};
pub use panel::{panel_region, panel_shape};
pub use raster::{Style, TabLabel};
use tabs::slot_at;
pub(crate) use tabs::{icon_box, menu_button_rect};
pub use tabs::{max_tab_offset, tab_rect, tab_slot, Tabs};
pub use title::TitleFont;

/// Visible hairline around the content (sides and bottom).
pub const BORDER: i32 = 1;
/// Tab height above the content, including the tab's top hairline and the
/// separator hairline shared with the frame body.
pub const TITLE: i32 = 28;
/// Band outside the visible frame that the drop shadow may paint: the shadow's
/// 10px fade plus its 3px drop (palette.rs), with a pixel to spare.
pub const SHADOW_MARGIN: i32 = 14;
/// Invisible band outside the visible frame that resizes the window.
pub const GRIP: i32 = 6;
/// Corner radius of the tab, the frame body and floating panels.
pub const RADIUS: u32 = PANEL_RADIUS;
/// Square hit area of the app icon at the tab's leading edge; it opens the
/// window menu. The 16px icon inside is drawn at 1px.
pub(super) const CONTROL: i32 = 20;
/// Inset of the icon box from the tab's leading hairline.
pub(super) const CONTROL_PAD: i32 = 4;
/// Length of the margin/border stretch that resizes diagonally near a corner.
const CORNER_GRIP: i32 = 12;

/// Content plus its hairline border; the top row is the tab separator.
pub(super) fn body_rect(content: Rect) -> Rect {
    Rect {
        x: content.x.saturating_sub(BORDER),
        y: content.y.saturating_sub(BORDER),
        w: content.w.saturating_add((BORDER * 2) as u32),
        h: content.h.saturating_add((BORDER * 2) as u32),
    }
}

/// The two shapes that make up the visible window: tab and body.
///
/// The body's top corners are square where the tab continues straight into
/// them: the left one unless the tab was slid along, the right one when the
/// tab reaches the frame's right edge.
pub(crate) fn shapes(content: Rect, tabs: Tabs) -> [RoundRect; 2] {
    let tab = tab_rect(content, tabs);
    let body = body_rect(content);
    // The body's top corner under the tab is square: the tab continues into it.
    let flush_left = tab.x <= body.x;
    let flush_right = end(tab.x, tab.w) >= end(body.x, body.w);
    [
        RoundRect::new(
            tab.x,
            tab.y,
            tab.w,
            tab.h,
            Radii {
                top_left: RADIUS,
                top_right: RADIUS,
                ..Radii::default()
            },
        ),
        RoundRect::new(
            body.x,
            body.y,
            body.w,
            body.h,
            Radii {
                top_left: if flush_left { 0 } else { RADIUS },
                top_right: if flush_right { 0 } else { RADIUS },
                bottom_right: RADIUS,
                bottom_left: RADIUS,
            },
        ),
    ]
}

/// A window action offered by the window menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Close,
    Maximize,
    Minimize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The app icon: opens the window menu.
    Menu,
    /// A tab of the stack other than the shown one: shows its window.
    Tab(usize),
    Resize(ResizeEdge),
    Title,
    Content,
}

/// The visible frame around `content`: tab strip, hairlines and content.
pub fn frame_rect(content: Rect) -> Rect {
    around_frame(content, 0)
}

/// What a window actually covers on screen: its tab strip and its body (the
/// strip beside a single tab is see-through).
pub fn visible_parts(content: Rect, tabs: Tabs) -> [Rect; 2] {
    [tab_rect(content, tabs), body_rect(content)]
}

/// Everything the decoration may paint: visible frame plus [`SHADOW_MARGIN`].
pub fn bounds(content: Rect) -> Rect {
    around_frame(content, SHADOW_MARGIN)
}

/// Visible frame grown by `margin` on every side.
fn around_frame(content: Rect, margin: i32) -> Rect {
    let x = content.x.saturating_sub(BORDER + margin);
    let right = end(content.x, content.w).saturating_add(BORDER + margin);
    Rect {
        x,
        y: content.y.saturating_sub(TITLE + margin),
        w: right.saturating_sub(x) as u32,
        h: content
            .h
            .saturating_add((TITLE + BORDER + margin * 2) as u32),
    }
}

/// Height of the desktop's taskbar along the bottom edge of the screen.
const TASKBAR_H: u32 = 36;

/// Content rect of a window maximized on a `screen_w` x `screen_h` screen:
/// its frame fills the screen above the taskbar.
pub fn maximized_content(screen_w: u32, screen_h: u32) -> Rect {
    content_within(Rect {
        x: 0,
        y: 0,
        w: screen_w,
        h: screen_h.saturating_sub(TASKBAR_H),
    })
}

/// Content rect whose visible frame (tab and hairlines) exactly fills `area`.
pub fn content_within(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(BORDER),
        y: area.y.saturating_add(TITLE),
        w: area.w.saturating_sub((BORDER * 2) as u32),
        h: area.h.saturating_sub((TITLE + BORDER) as u32),
    }
}

/// Columns hidden at the start and end of content row `row` so the content
/// stays inside the rounded body outline.
pub fn content_row_insets(content: Rect, tabs: Tabs, row: u32) -> (u32, u32) {
    let reach = RADIUS.saturating_sub(BORDER as u32);
    if row >= reach && row.saturating_add(reach) < content.h {
        return (0, 0);
    }
    let body = shapes(content, tabs)[1];
    let ((_, left_end), (right_start, _)) = body.outline(row.saturating_add(BORDER as u32));
    let border = BORDER as u32;
    (
        left_end.saturating_sub(border),
        body.w.saturating_sub(right_start).saturating_sub(border),
    )
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

pub fn hit_test(content: Rect, tabs: Tabs, x: i32, y: i32) -> Option<Hit> {
    // The shadow beyond the grip is see-through: clicks reach the window below.
    if !contains(around_frame(content, GRIP), x, y) {
        return None;
    }
    if contains(menu_button_rect(content, tabs), x, y) {
        return Some(Hit::Menu);
    }
    let tab = tab_rect(content, tabs);
    if contains(tab, x, y) {
        let index = slot_at(content, tabs, x);
        return Some(if index == tabs.selected {
            Hit::Title
        } else {
            Hit::Tab(index)
        });
    }
    let body = body_rect(content);
    let (tab_end, body_right, body_bottom) =
        (end(tab.x, tab.w), end(body.x, body.w), end(body.y, body.h));
    // The strip beside the tab is see-through: clicks reach the window below.
    if x >= tab_end && y < body.y {
        return None;
    }
    let content_right = end(content.x, content.w);
    let content_bottom = end(content.y, content.h);
    if x >= content.x && x < content_right && y >= content.y && y < content_bottom {
        return Some(Hit::Content);
    }

    let beside_tab = x >= tab_end;
    let mut north = y < tab.y || (beside_tab && y < content.y);
    let mut south = y >= content_bottom;
    let mut west = x < content.x;
    let mut east = x >= content_right;
    if north || south {
        west |= x < body.x + CORNER_GRIP;
        east |= (south || beside_tab) && x >= body_right - CORNER_GRIP;
    } else {
        north |= (west && y < tab.y + CORNER_GRIP) || (east && y < body.y + CORNER_GRIP);
        south |= y >= body_bottom - CORNER_GRIP;
    }
    let edge = match (north, south, west, east) {
        (true, _, true, _) => ResizeEdge::NorthWest,
        (true, _, _, true) => ResizeEdge::NorthEast,
        (_, true, true, _) => ResizeEdge::SouthWest,
        (_, true, _, true) => ResizeEdge::SouthEast,
        (true, ..) => ResizeEdge::North,
        (_, true, ..) => ResizeEdge::South,
        (_, _, true, _) => ResizeEdge::West,
        _ => ResizeEdge::East,
    };
    Some(Hit::Resize(edge))
}

/// Paint the chrome of the window whose content is `content`, inside `dirty`.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    content: Rect,
    dirty: Rect,
    style: &Style<'_>,
    font: &mut TitleFont,
) {
    raster::paint(pixels, width, height, content, dirty, style, font);
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.x && x < end(rect.x, rect.w) && y >= rect.y && y < end(rect.y, rect.h)
}

pub(super) fn end(start: i32, extent: u32) -> i32 {
    start.saturating_add(extent.min(i32::MAX as u32) as i32)
}

#[cfg(test)]
mod tests;
