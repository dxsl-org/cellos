//! Cell Tab geometry: one tab per window, side by side across a stack.

use wire::display::Rect;

use super::{body_rect, end, BORDER, CONTROL, CONTROL_PAD, RADIUS, TITLE};

/// Half the content width within 160..=240, but never narrower than the frame
/// by less than two corner radii: a tab ending inside the body's rounded
/// top-right corner would leave a notch, so it spans the frame instead.
pub fn tab_width(content_w: u32) -> u32 {
    let body_w = content_w.saturating_add((BORDER * 2) as u32);
    let tab = (content_w / 2).clamp(160, 240).min(body_w);
    if body_w - tab < RADIUS * 2 {
        body_w
    } else {
        tab
    }
}

/// The Cell Tabs of one frame: `count` stacked windows, `selected` shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tabs {
    pub count: usize,
    pub selected: usize,
    /// How far a lone tab was slid right along the frame's top edge (Haiku's
    /// Shift+drag); a stack's tabs always span the frame.
    pub offset: u32,
}

impl Tabs {
    /// A frame holding one window.
    pub const SINGLE: Self = Self {
        count: 1,
        selected: 0,
        offset: 0,
    };

    /// `count` stacked tabs, `selected` shown.
    #[cfg(test)]
    pub const fn stack(count: usize, selected: usize) -> Self {
        Self {
            count,
            selected,
            offset: 0,
        }
    }
}

/// All tabs together: one tab as wide as [`tab_width`], or for a stack the
/// whole width of the frame, shared out between the tabs (as in Haiku).
pub fn tab_rect(content: Rect, tabs: Tabs) -> Rect {
    let body = body_rect(content);
    let (w, offset) = if tabs.count > 1 {
        (body.w, 0)
    } else {
        (
            tab_width(content.w),
            snapped_offset(tabs.offset, max_tab_offset(content)),
        )
    };
    Rect {
        x: body.x.saturating_add(offset.min(i32::MAX as u32) as i32),
        y: content.y.saturating_sub(TITLE),
        w,
        h: TITLE as u32,
    }
}

/// A lone tab sits flush with an edge or clear of both rounded corners: an
/// end inside a corner's curve would leave a notch in the outline.
fn snapped_offset(offset: u32, max: u32) -> u32 {
    let clear = RADIUS * 2;
    if offset < clear || max < clear * 2 {
        0
    } else if offset + clear > max {
        max
    } else {
        offset
    }
}

/// Furthest a lone tab may slide: its end meets the frame's right edge.
pub fn max_tab_offset(content: Rect) -> u32 {
    body_rect(content).w.saturating_sub(tab_width(content.w))
}

/// Tab `index` of the strip. Neighbours share their 1px dividing hairline;
/// the last tab takes the remainder so the strip ends with the frame.
pub fn tab_slot(content: Rect, tabs: Tabs, index: usize) -> Rect {
    let strip = tab_rect(content, tabs);
    let count = tabs.count.max(1) as u32;
    let index = index.min(tabs.count.max(1) - 1) as u32;
    let each = strip.w / count;
    let x = strip.x.saturating_add((each * index) as i32);
    let w = if index + 1 == count {
        strip.w - each * index
    } else {
        each + 1
    };
    Rect { x, w, ..strip }
}

/// Index of the tab under `x` in the strip of `tabs`.
pub(super) fn slot_at(content: Rect, tabs: Tabs, x: i32) -> usize {
    (0..tabs.count)
        .find(|&index| {
            let slot = tab_slot(content, tabs, index);
            x < end(slot.x, slot.w).saturating_sub(1) || index + 1 == tabs.count
        })
        .unwrap_or(0)
}

/// The app icon box at the shown tab's leading edge, centred in the tab rows
/// above the shared separator. Pressing it opens the window menu.
pub(crate) fn menu_button_rect(content: Rect, tabs: Tabs) -> Rect {
    icon_box(tab_slot(content, tabs, tabs.selected))
}

/// The app icon box of the tab `tab`.
pub(crate) fn icon_box(tab: Rect) -> Rect {
    Rect {
        x: tab.x.saturating_add(CONTROL_PAD),
        y: tab.y.saturating_add((TITLE - 1 - CONTROL) / 2),
        w: CONTROL as u32,
        h: CONTROL as u32,
    }
}
