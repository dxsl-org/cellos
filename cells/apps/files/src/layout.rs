// SPDX-License-Identifier: MIT
//! Window geometry shared by rendering and mouse hit-testing, so a click always lands on
//! what was drawn: toolbar controls, the one or two listing panes, and entries in them.

use viui::surface::components::{GridView, ListView, SegmentedControl};

use crate::model::ViewMode;
use crate::split::ActivePane;
use crate::views::{self, places::PLACES_WIDTH, ContentRect};

/// Top toolbar (title and controls).
pub const TOOLBAR_HEIGHT: u32 = 44;
/// Bottom status bar.
pub const STATUS_HEIGHT: u32 = 24;
/// Strip above each split pane naming its folder; its colour marks the focused pane.
pub const PANE_HEADER_HEIGHT: u32 = 24;
/// Vertical rule between the two panes.
pub const PANE_DIVIDER: u32 = 1;
/// Labels of the view-mode switch, in [`ViewMode`] order.
pub const VIEW_MODE_LABELS: [&str; 3] = ["List", "Grid", "Cols"];
/// Label of the split toggle button.
pub const SPLIT_LABEL: &str = "Split";

const CONTROL_Y: i32 = 8;
const CONTROL_H: u32 = 28;
const VIEW_SEGMENT_W: u32 = 54;
/// Distance of the view switch's left edge from the window's right edge.
const VIEW_SWITCH_INSET: i32 = 180;
const SPLIT_BUTTON_W: u32 = 60;
const SPLIT_BUTTON_GAP: i32 = 8;

/// A row of toolbar segments drawn and hit-tested with [`SegmentedControl`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segments {
    pub x: i32,
    pub y: i32,
    pub item_w: u32,
    pub h: u32,
    pub count: usize,
}

impl Segments {
    /// Index of the segment under `(x, y)`.
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        SegmentedControl::hit_test(x, y, self.x, self.y, self.item_w, self.h, self.count)
    }
}

/// The List/Grid/Cols switch at the right of the toolbar.
pub const fn view_switch(width: u32) -> Segments {
    Segments {
        x: width as i32 - VIEW_SWITCH_INSET,
        y: CONTROL_Y,
        item_w: VIEW_SEGMENT_W,
        h: CONTROL_H,
        count: VIEW_MODE_LABELS.len(),
    }
}

/// The split toggle, just left of the view switch.
pub const fn split_button(width: u32) -> Segments {
    Segments {
        x: view_switch(width).x - SPLIT_BUTTON_GAP - SPLIT_BUTTON_W as i32,
        y: CONTROL_Y,
        item_w: SPLIT_BUTTON_W,
        h: CONTROL_H,
        count: 1,
    }
}

/// Area right of Places, between toolbar and status bar.
pub const fn content_rect(width: u32, height: u32) -> ContentRect {
    ContentRect {
        x: PLACES_WIDTH as i32,
        y: TOOLBAR_HEIGHT as i32,
        w: width.saturating_sub(PLACES_WIDTH),
        h: height.saturating_sub(TOOLBAR_HEIGHT + STATUS_HEIGHT),
    }
}

/// Frames of the left pane and, when split, the right pane. Unsplit, the left pane is
/// the whole content area.
pub fn pane_frames(width: u32, height: u32, split: bool) -> (ContentRect, Option<ContentRect>) {
    let content = content_rect(width, height);
    if !split {
        return (content, None);
    }
    let left_w = content.w.saturating_sub(PANE_DIVIDER) / 2;
    let left = ContentRect {
        w: left_w,
        ..content
    };
    let right_x = content.x + (left_w + PANE_DIVIDER) as i32;
    let right = ContentRect {
        x: right_x,
        w: content.w.saturating_sub(left_w + PANE_DIVIDER),
        ..content
    };
    (left, Some(right))
}

/// Where a pane's entries are drawn: below its header strip when split.
pub const fn listing_rect(frame: ContentRect, split: bool) -> ContentRect {
    if !split {
        return frame;
    }
    let header = if frame.h < PANE_HEADER_HEIGHT {
        frame.h
    } else {
        PANE_HEADER_HEIGHT
    };
    ContentRect {
        y: frame.y + header as i32,
        h: frame.h - header,
        ..frame
    }
}

/// The pane under `(x, y)` and its frame; the divider and the chrome belong to none.
pub fn pane_at(
    width: u32,
    height: u32,
    split: bool,
    x: i32,
    y: i32,
) -> Option<(ActivePane, ContentRect)> {
    let (left, right) = pane_frames(width, height, split);
    if left.contains(x, y) {
        return Some((ActivePane::Left, left));
    }
    right
        .filter(|frame| frame.contains(x, y))
        .map(|frame| (ActivePane::Right, frame))
}

/// Entry index under `(x, y)` in a listing drawn into `rect`. Entries the renderer cut
/// off at the bottom are not hit: clicking blank space must not select an unseen row.
pub fn listing_hit(
    mode: ViewMode,
    rect: ContentRect,
    count: usize,
    x: i32,
    y: i32,
) -> Option<usize> {
    if !rect.contains(x, y) {
        return None;
    }
    match mode {
        ViewMode::List => {
            let visible = rect.h.saturating_sub(ListView::HEADER_HEIGHT) / ListView::ROW_HEIGHT;
            ListView::hit_test(x, y, rect.x, rect.y, rect.w, count.min(visible as usize))
        }
        ViewMode::Grid => {
            let idx = GridView::hit_test(x, y, rect.x, rect.y, rect.w, count)?;
            let cols = (rect.w.saturating_sub(GridView::CARD_GAP)
                / (GridView::CARD_W + GridView::CARD_GAP))
                .max(1) as usize;
            // Mirrors GridView::render: 12 px top padding, rows stop when a card would overflow.
            let row = (idx / cols) as u32;
            let bottom = 12 + row * (GridView::CARD_H + GridView::CARD_GAP) + GridView::CARD_H;
            (bottom <= rect.h).then_some(idx)
        }
        ViewMode::Columns => {
            let visible = (rect.h / views::columns::ROW_H) as usize;
            views::columns::handle_click(count.min(visible), rect.x, rect.y, x, y)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 800;
    const H: u32 = 600;

    #[test]
    fn unsplit_pane_is_the_whole_content_area() {
        let (left, right) = pane_frames(W, H, false);
        assert_eq!(left, content_rect(W, H));
        assert_eq!(
            left,
            ContentRect {
                x: 160,
                y: 44,
                w: 640,
                h: 532
            }
        );
        assert_eq!(right, None);
        assert_eq!(listing_rect(left, false), left);
    }

    #[test]
    fn split_panes_sit_side_by_side_and_tile_the_content_area() {
        let (left, right) = pane_frames(W, H, true);
        let right = right.expect("right pane when split");
        assert_eq!(
            left,
            ContentRect {
                x: 160,
                y: 44,
                w: 319,
                h: 532
            }
        );
        assert_eq!(
            right,
            ContentRect {
                x: 480,
                y: 44,
                w: 320,
                h: 532
            }
        );
        assert_eq!(left.w + PANE_DIVIDER + right.w, content_rect(W, H).w);
        assert_eq!(right.x + right.w as i32, W as i32);

        let listing = listing_rect(right, true);
        assert_eq!(
            listing,
            ContentRect {
                x: 480,
                y: 68,
                w: 320,
                h: 508
            }
        );
    }

    #[test]
    fn pane_at_picks_the_pane_and_skips_the_divider_and_chrome() {
        assert_eq!(
            pane_at(W, H, true, 200, 100).map(|p| p.0),
            Some(ActivePane::Left)
        );
        assert_eq!(
            pane_at(W, H, true, 478, 100).map(|p| p.0),
            Some(ActivePane::Left)
        );
        assert_eq!(pane_at(W, H, true, 479, 100), None, "divider column");
        assert_eq!(
            pane_at(W, H, true, 480, 100).map(|p| p.0),
            Some(ActivePane::Right)
        );
        assert_eq!(
            pane_at(W, H, true, 799, 575).map(|p| p.0),
            Some(ActivePane::Right)
        );
        assert_eq!(pane_at(W, H, true, 100, 100), None, "places sidebar");
        assert_eq!(pane_at(W, H, true, 600, 20), None, "toolbar");
        assert_eq!(pane_at(W, H, true, 600, 590), None, "status bar");
        // Unsplit, the right half is still the (only) left pane.
        assert_eq!(
            pane_at(W, H, false, 600, 100).map(|p| p.0),
            Some(ActivePane::Left)
        );
    }

    #[test]
    fn listing_hits_rows_in_each_view_mode_and_never_unseen_ones() {
        let rect = listing_rect(pane_frames(W, H, true).1.unwrap(), true);
        // List: 28 px column header, then 32 px rows.
        assert_eq!(listing_hit(ViewMode::List, rect, 5, 500, rect.y + 10), None);
        assert_eq!(
            listing_hit(ViewMode::List, rect, 5, 500, rect.y + 28),
            Some(0)
        );
        assert_eq!(
            listing_hit(ViewMode::List, rect, 5, 500, rect.y + 28 + 32 * 4),
            Some(4)
        );
        assert_eq!(
            listing_hit(ViewMode::List, rect, 5, 500, rect.y + 28 + 32 * 5),
            None
        );
        // Unsplit the listing is 532 px: (532 - 28) / 32 = 15 rows drawn; the partial
        // 16th row below them is blank space and selects nothing.
        let whole = content_rect(W, H);
        assert_eq!(
            listing_hit(ViewMode::List, whole, 99, 500, whole.y + 28 + 32 * 14),
            Some(14)
        );
        assert_eq!(
            listing_hit(ViewMode::List, whole, 99, 500, whole.y + 28 + 32 * 15),
            None
        );
        // Left of the pane is the other pane, not this listing.
        assert_eq!(listing_hit(ViewMode::List, rect, 5, 470, rect.y + 40), None);

        // Grid: 320 px wide → 2 columns of 104 px cards + 12 px gaps.
        assert_eq!(
            listing_hit(ViewMode::Grid, rect, 4, rect.x + 20, rect.y + 20),
            Some(0)
        );
        assert_eq!(
            listing_hit(ViewMode::Grid, rect, 4, rect.x + 140, rect.y + 20),
            Some(1)
        );
        assert_eq!(
            listing_hit(ViewMode::Grid, rect, 4, rect.x + 20, rect.y + 130),
            Some(2)
        );
        // 508 px: rows end at 12 + 108k + 96; row 4 (bottom 540) is cut off.
        assert_eq!(
            listing_hit(ViewMode::Grid, rect, 99, rect.x + 20, rect.y + 450),
            None
        );

        // Columns: 28 px rows in the first 180 px column.
        assert_eq!(
            listing_hit(ViewMode::Columns, rect, 3, rect.x + 30, rect.y + 60),
            Some(2)
        );
        assert_eq!(
            listing_hit(ViewMode::Columns, rect, 3, rect.x + 200, rect.y + 5),
            None
        );
    }

    #[test]
    fn toolbar_controls_do_not_overlap() {
        let view = view_switch(W);
        let split = split_button(W);
        assert_eq!(view.hit(view.x + 1, 20), Some(0));
        assert_eq!(view.hit(view.x - 1, 20), None);
        assert_eq!(split.hit(split.x + 1, 20), Some(0));
        assert_eq!(split.hit(view.x + 1, 20), None);
        assert!(split.x + split.item_w as i32 <= view.x);
        assert_eq!(split.hit(split.x + 1, 40), None, "below the button");
    }
}
