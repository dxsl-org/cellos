//! Tile groups on screen: frames glued edge to edge move, raise and hide
//! together, and the edge two of them share resizes both (Haiku's Stack &
//! Tile). The registry knows only which frames are glued along which side;
//! every edge is read off the windows' own geometry, so the shared edges are
//! the "tabstops" of the group and cannot drift from the windows.

use alloc::vec::Vec;
use wire::display::{Rect, WindowState};

use crate::stacking;
use crate::surface_table::frame::Side;
use crate::surface_table::{FrameRegistry, StateTransition, SurfaceTable};
use crate::window_decoration;
use crate::z_order::ZOrder;

/// A dropped frame glues to a parallel edge at most this far away...
const SNAP: u32 = 16;
/// ...when they overlap along it by at least half the shorter one.
const MIN_OVERLAP_PERCENT: i64 = 50;
/// Smallest content a tiled frame is resized to (as for any user resize).
const MIN_W: i32 = 64;
const MIN_H: i32 = 48;

/// Where a dropped frame would glue: to `side` of frame `a` (or `a` to its
/// side when `dropped_first`), with the dropped content moved to `content`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glue {
    pub target: u64,
    pub side: Side,
    /// The dropped frame is `a` of the join (left of / above the target).
    pub dropped_first: bool,
    pub content: Rect,
    /// The shared edge, for highlighting.
    pub edge: Rect,
}

/// The shown window of each frame of `cap`'s tile group (`cap` first).
pub fn group_windows(frames: &FrameRegistry, cap: u64) -> Vec<u64> {
    let Some(frame) = frames.frame_of(cap) else {
        return alloc::vec![cap];
    };
    frames
        .tile_group(frame)
        .into_iter()
        .filter_map(|f| frames.selected_cap(f))
        .collect()
}

/// Where the frame of `cap`, dropped with its content at `content`, would
/// glue: the nearest parallel edge of another shown frame within [`SNAP`].
pub fn glue_at(
    frames: &FrameRegistry,
    table: &SurfaceTable,
    z_order: &ZOrder,
    cap: u64,
    content: Rect,
) -> Option<Glue> {
    let own = frames.frame_of(cap)?;
    let g = window_decoration::frame_rect(content);
    let mut best: Option<(u32, Glue)> = None;
    for other in z_order.iter_top_to_bottom() {
        let Some(frame) = frames
            .frame_of(other)
            .filter(|&f| f != own && frames.selected_cap(f) == Some(other))
        else {
            continue;
        };
        let Some(t) = table
            .get(other)
            .filter(|s| s.is_shown())
            .map(|s| window_decoration::frame_rect(s.screen_rect()))
        else {
            continue;
        };
        let _ = frame;
        for (gap, side, dropped_first, shift, edge) in candidates(g, t) {
            let gap = gap.unsigned_abs();
            if gap <= SNAP && best.is_none_or(|(b, _)| gap < b) {
                let content = Rect {
                    x: content.x.saturating_add(shift.0),
                    y: content.y.saturating_add(shift.1),
                    ..content
                };
                best = Some((
                    gap,
                    Glue {
                        target: other,
                        side,
                        dropped_first,
                        content,
                        edge,
                    },
                ));
            }
        }
    }
    best.map(|(_, glue)| glue)
}

/// Glue the frame of `cap` (already at its snapped place) as `glue` says.
pub fn glue(frames: &mut FrameRegistry, cap: u64, glue: Glue) -> bool {
    let (Some(dropped), Some(target)) = (frames.frame_of(cap), frames.frame_of(glue.target)) else {
        return false;
    };
    let (a, b) = if glue.dropped_first {
        (dropped, target)
    } else {
        (target, dropped)
    };
    frames.glue(a, b, glue.side)
}

/// Move every other frame of `cap`'s tile group by `delta` (after `cap`'s
/// frame moved by it). Returns the area to repaint.
pub fn follow(
    frames: &FrameRegistry,
    table: &mut SurfaceTable,
    cap: u64,
    delta: (i32, i32),
) -> Option<Rect> {
    if delta == (0, 0) {
        return None;
    }
    let mut area: Option<Rect> = None;
    for other in group_windows(frames, cap)
        .into_iter()
        .filter(|&other| other != cap)
    {
        let Some(surface) = table.get_mut(other) else {
            continue;
        };
        let before = window_decoration::bounds(surface.screen_rect());
        surface.move_to(
            surface.x.saturating_add(delta.0),
            surface.y.saturating_add(delta.1),
        );
        let after = window_decoration::bounds(surface.screen_rect());
        stacking::sync_frame_of(frames, table, other);
        let moved = before.union(&after);
        area = Some(area.map_or(moved, |a| a.union(&moved)));
    }
    area
}

/// Raise every frame of `cap`'s tile group above all other windows, `cap` on top.
pub fn raise(frames: &FrameRegistry, z_order: &mut ZOrder, cap: u64) {
    let group = group_windows(frames, cap);
    if group.len() < 2 {
        return;
    }
    for &other in group.iter().filter(|&&other| other != cap) {
        z_order.raise(other);
    }
    z_order.raise(cap);
}

/// The window `cap` of a tile group came back from minimized: the windows
/// the user minimized together with it come back too (not those their own
/// apps minimized). Returns the area to repaint.
pub fn restore_group(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    cap: u64,
) -> Option<Rect> {
    if !frames
        .frame_of(cap)
        .is_some_and(|frame| frames.take_group_minimized(frame))
    {
        return None;
    }
    let mut area: Option<Rect> = None;
    for other in group_windows(frames, cap)
        .into_iter()
        .filter(|&other| other != cap)
    {
        if !frames
            .frame_of(other)
            .is_some_and(|frame| frames.take_group_minimized(frame))
        {
            continue;
        }
        let Some(surface) = table
            .get_mut(other)
            .filter(|s| s.state() == WindowState::Minimized)
        else {
            continue;
        };
        let restore = wire::display::compositor_ops::RESTORE;
        if let Some(StateTransition::StateChanged(state)) =
            surface.request_state(other as u32, restore, surface.screen_rect())
        {
            if !state
                .encode()
                .is_ok_and(|event| stacking::deliver(surface.owner, &event))
            {
                surface.rollback_state_change(WindowState::Minimized);
                continue;
            }
            surface.damage = Some(Rect {
                x: 0,
                y: 0,
                w: surface.w,
                h: surface.h,
            });
            let shown = window_decoration::bounds(surface.screen_rect());
            area = Some(area.map_or(shown, |a| a.union(&shown)));
        }
    }
    area
}

/// Every way `g` (dropped) can glue to `t`: (signed gap, side, dropped
/// first, shift moving `g` flush, shared edge). Client windows may sit
/// anywhere in `i32`, so every difference saturates.
fn candidates(g: Rect, t: Rect) -> [(i32, Side, bool, (i32, i32), Rect); 4] {
    let (gr, gb, tr, tb) = (end(g.x, g.w), end(g.y, g.h), end(t.x, t.w), end(t.y, t.h));
    let v_ok = overlaps(g.y, gb, t.y, tb);
    let h_ok = overlaps(g.x, gr, t.x, tr);
    let far = i32::MAX / 2;
    let pick = |ok: bool, gap: i32| if ok { gap } else { far };
    let span = |from: i32, to: i32| to.saturating_sub(from).max(1) as u32;
    let column = |x: i32| Rect {
        x: x.saturating_sub(1),
        y: g.y.max(t.y),
        w: 2,
        h: span(g.y.max(t.y), gb.min(tb)),
    };
    let row = |y: i32| Rect {
        x: g.x.max(t.x),
        y: y.saturating_sub(1),
        w: span(g.x.max(t.x), gr.min(tr)),
        h: 2,
    };
    let (right, left) = (t.x.saturating_sub(gr), g.x.saturating_sub(tr));
    let (below, above) = (t.y.saturating_sub(gb), g.y.saturating_sub(tb));
    [
        (
            pick(v_ok, right),
            Side::Right,
            true,
            (right, 0),
            column(t.x),
        ),
        (
            pick(v_ok, left),
            Side::Right,
            false,
            (left.saturating_neg(), 0),
            column(tr),
        ),
        (pick(h_ok, below), Side::Below, true, (0, below), row(t.y)),
        (
            pick(h_ok, above),
            Side::Below,
            false,
            (0, above.saturating_neg()),
            row(tb),
        ),
    ]
}

/// Whether two spans overlap by at least half the shorter one.
fn overlaps(a0: i32, a1: i32, b0: i32, b1: i32) -> bool {
    let overlap = i64::from(a1.min(b1)) - i64::from(a0.max(b0));
    let shorter = (i64::from(a1) - i64::from(a0)).min(i64::from(b1) - i64::from(b0));
    overlap > 0 && overlap * 100 >= shorter * MIN_OVERLAP_PERCENT
}

fn end(start: i32, extent: u32) -> i32 {
    start.saturating_add(extent.min(i32::MAX as u32) as i32)
}

#[cfg(test)]
mod tests;

mod resize;
pub use resize::{resize, settle};
