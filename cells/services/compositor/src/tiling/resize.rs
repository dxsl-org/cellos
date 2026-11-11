//! Resizing a tile group: the edge two glued frames share moves for both.

use alloc::vec::Vec;
use api::display::{ConfigureKind, Rect};

use super::{end, group_windows, MIN_H, MIN_W};
use crate::stacking;
use crate::surface_table::frame::{Join, Side};
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::window_decoration::{self, ResizeEdge};

/// A resize of `cap` at `edge` by `(dx, dy)` from `start` (its rect when the
/// drag began). When the edge is one its tile group shares, the shared edge
/// moves for every frame on it, within everyone's minimum size; returns
/// `false` when no shared edge is involved (plain resize).
pub fn resize(
    frames: &FrameRegistry,
    table: &mut SurfaceTable,
    cap: u64,
    start: Rect,
    edge: ResizeEdge,
    (dx, dy): (i32, i32),
) -> bool {
    let Some(frame) = frames.frame_of(cap).filter(|&f| frames.is_tiled(f)) else {
        return false;
    };
    let group: Vec<(u64, Rect)> = group_windows(frames, cap)
        .into_iter()
        .filter_map(|c| table.get(c).map(|s| (c, s.intended_rect())))
        .collect();
    let s = window_decoration::frame_rect(start);
    let mut handled = false;
    let horizontal = match edge {
        ResizeEdge::East | ResizeEdge::NorthEast | ResizeEdge::SouthEast => {
            Some(end(s.x, s.w) + dx)
        }
        ResizeEdge::West | ResizeEdge::NorthWest | ResizeEdge::SouthWest => Some(s.x + dx),
        _ => None,
    };
    let vertical = match edge {
        ResizeEdge::South | ResizeEdge::SouthEast | ResizeEdge::SouthWest => {
            Some(end(s.y, s.h) + dy)
        }
        ResizeEdge::North | ResizeEdge::NorthEast | ResizeEdge::NorthWest => Some(s.y + dy),
        _ => None,
    };
    let own = table
        .get(cap)
        .map(|s| window_decoration::frame_rect(s.intended_rect()));
    if let (Some(target), Some(own)) = (horizontal, own) {
        let line = if matches!(
            edge,
            ResizeEdge::East | ResizeEdge::NorthEast | ResizeEdge::SouthEast
        ) {
            end(own.x, own.w)
        } else {
            own.x
        };
        if shares(frames, frame, Side::Right, line, &group, true) {
            move_line(table, &group, line, target, true);
            handled = true;
        }
    }
    if let (Some(target), Some(own)) = (vertical, own) {
        let line = if matches!(
            edge,
            ResizeEdge::South | ResizeEdge::SouthEast | ResizeEdge::SouthWest
        ) {
            end(own.y, own.h)
        } else {
            own.y
        };
        if shares(frames, frame, Side::Below, line, &group, false) {
            move_line(table, &group, line, target, false);
            handled = true;
        }
    }
    handled
}

/// After `cap`'s frame committed a new size: neighbours glued to it that no
/// longer meet its edges are asked to, keeping their far edges. Only the
/// shown tab of a frame speaks for its edges.
pub fn settle(frames: &FrameRegistry, table: &mut SurfaceTable, cap: u64) {
    let Some(frame) = frames.frame_of(cap).filter(|_| !frames.is_hidden(cap)) else {
        return;
    };
    let Some(own) = table
        .get(cap)
        .map(|s| window_decoration::frame_rect(s.screen_rect()))
    else {
        return;
    };
    for Join { a, b, side } in frames.joins_of(frame) {
        let (other, own_is_a) = if a == frame { (b, true) } else { (a, false) };
        let Some(other_cap) = frames.selected_cap(other) else {
            continue;
        };
        let Some(rect) = table.get(other_cap).map(|s| s.intended_rect()) else {
            continue;
        };
        let v = window_decoration::frame_rect(rect);
        let wanted = match (side, own_is_a) {
            (Side::Right, true) => with_left(v, end(own.x, own.w)),
            (Side::Right, false) => with_right(v, own.x),
            (Side::Below, true) => with_top(v, end(own.y, own.h)),
            (Side::Below, false) => with_bottom(v, own.y),
        };
        if let Some(wanted) = wanted.filter(|w| *w != v) {
            stacking::propose(table, other_cap, ConfigureKind::Resize, content_of(wanted));
        }
    }
}

/// Whether frame `frame` has a join on `side` whose shared line is `line`.
fn shares(
    frames: &FrameRegistry,
    frame: u32,
    side: Side,
    line: i32,
    group: &[(u64, Rect)],
    x_axis: bool,
) -> bool {
    let edge_at = |cap: u64, far: bool| {
        group.iter().find(|(c, _)| *c == cap).map(|(_, r)| {
            let v = window_decoration::frame_rect(*r);
            match (x_axis, far) {
                (true, true) => end(v.x, v.w),
                (true, false) => v.x,
                (false, true) => end(v.y, v.h),
                (false, false) => v.y,
            }
        })
    };
    frames
        .joins_of(frame)
        .into_iter()
        .filter(|j| j.side == side)
        .any(|j| {
            let (Some(a), Some(b)) = (frames.selected_cap(j.a), frames.selected_cap(j.b)) else {
                return false;
            };
            edge_at(a, true) == Some(line) || edge_at(b, false) == Some(line)
        })
}

/// Move the line `line` (x when `x_axis`, else y) to `target` for every
/// frame of `group` with an edge on it, clamped so all keep their minimum.
fn move_line(
    table: &mut SurfaceTable,
    group: &[(u64, Rect)],
    line: i32,
    target: i32,
    x_axis: bool,
) {
    let min = if x_axis { MIN_W } else { MIN_H } + chrome(x_axis);
    let mut low = i32::MIN;
    let mut high = i32::MAX;
    for (_, rect) in group {
        let v = window_decoration::frame_rect(*rect);
        let (start, stop) = if x_axis {
            (v.x, end(v.x, v.w))
        } else {
            (v.y, end(v.y, v.h))
        };
        if stop == line {
            low = low.max(start.saturating_add(min));
        }
        if start == line {
            high = high.min(stop.saturating_sub(min));
        }
    }
    // An impossible range (minimums overlap) moves nothing.
    if low > high {
        return;
    }
    let target = target.clamp(low, high);
    for &(cap, rect) in group {
        let v = window_decoration::frame_rect(rect);
        let wanted = match x_axis {
            true if end(v.x, v.w) == line => with_right(v, target),
            true if v.x == line => with_left(v, target),
            false if end(v.y, v.h) == line => with_bottom(v, target),
            false if v.y == line => with_top(v, target),
            _ => None,
        };
        if let Some(wanted) = wanted.filter(|w| *w != v) {
            stacking::propose(table, cap, ConfigureKind::Resize, content_of(wanted));
        }
    }
}

/// Visible-frame size taken by chrome along an axis (hairlines, tab).
fn chrome(x_axis: bool) -> i32 {
    let probe = Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };
    let v = window_decoration::frame_rect(probe);
    if x_axis {
        v.w as i32
    } else {
        v.h as i32
    }
}

/// Content rect whose visible frame is `visible`, never larger than the
/// compositor asks of any window (`MAX_CONTENT`).
fn content_of(visible: Rect) -> Rect {
    let content = window_decoration::content_within(visible);
    Rect {
        w: content.w.min(MAX_CONTENT),
        h: content.h.min(MAX_CONTENT),
        ..content
    }
}

/// Largest content the compositor proposes (as for a user resize).
const MAX_CONTENT: u32 = 4096;

fn with_left(v: Rect, x: i32) -> Option<Rect> {
    let right = end(v.x, v.w);
    (right > x).then(|| Rect {
        x,
        w: (right - x) as u32,
        ..v
    })
}

fn with_right(v: Rect, right: i32) -> Option<Rect> {
    (right > v.x).then(|| Rect {
        w: (right - v.x) as u32,
        ..v
    })
}

fn with_top(v: Rect, y: i32) -> Option<Rect> {
    let bottom = end(v.y, v.h);
    (bottom > y).then(|| Rect {
        y,
        h: (bottom - y) as u32,
        ..v
    })
}

fn with_bottom(v: Rect, bottom: i32) -> Option<Rect> {
    (bottom > v.y).then(|| Rect {
        h: (bottom - v.y) as u32,
        ..v
    })
}
