//! Stack glue between the frame registry, the surface table and z-order.
//!
//! A frame (stack of Cell Tabs) has no geometry of its own: its shown window
//! is the frame. Every other member sits at the shown window's origin, hidden
//! (`SurfaceState::set_behind_tab`) but still `Normal`, so the reconciler can
//! configure it to the shown window's size and state one request at a time.
//! This module is the only writer of the behind-tab flag.

use wire::display::Rect;

use crate::surface_table::{FrameRegistry, Reselected, SurfaceTable};
use crate::window_decoration::{self, Tabs};
use crate::z_order::ZOrder;

/// Largest size the compositor proposes to a member (as for a user resize).
const MAX_CONTENT: u32 = 4096;
/// A detached tab lands this far below and right of its stack...
const DETACH_OFFSET: i32 = 24;
/// ...with at least this much of it left on screen.
const DETACH_KEEP: i32 = 96;

mod closing;
mod reconcile;
pub use closing::{advance_close, ask_to_close, close_all, show_next};
use reconcile::match_model;
pub use reconcile::reconcile_frame_of;
pub(crate) use reconcile::{deliver, propose};

/// The tabs of the frame showing `cap`; a window outside any stack has one.
pub fn tabs_of(frames: &FrameRegistry, cap: u64) -> Tabs {
    let Some(frame) = frames.frame_of(cap) else {
        return Tabs::SINGLE;
    };
    let members = frames.members(frame);
    Tabs {
        count: members.len().max(1),
        selected: members
            .iter()
            .position(|&member| member == cap)
            .unwrap_or(0),
        offset: frames.tab_offset(frame),
    }
}

/// Everything the frame showing or hiding `cap` may paint.
pub fn frame_bounds(frames: &FrameRegistry, table: &SurfaceTable, cap: u64) -> Option<Rect> {
    let shown = frames
        .frame_of(cap)
        .and_then(|frame| frames.selected_cap(frame))
        .unwrap_or(cap);
    table
        .get(shown)
        .map(|surface| window_decoration::bounds(surface.screen_rect()))
}

/// Put every member of `frame_id` at its shown window's origin and hide all
/// but that window. Returns the screen area the frame covers.
///
/// A frame of one window only has its flag cleared: nothing else moved.
pub fn sync(frames: &FrameRegistry, table: &mut SurfaceTable, frame_id: u32) -> Option<Rect> {
    let shown = frames.selected_cap(frame_id)?;
    let members = frames.members(frame_id);
    if members.len() < 2 {
        table.get_mut(shown)?.set_behind_tab(false);
        return None;
    }
    let origin = table.get(shown).map(|surface| (surface.x, surface.y))?;
    for cap in members {
        if let Some(surface) = table.get_mut(cap) {
            if (surface.x, surface.y) != origin {
                surface.move_to(origin.0, origin.1);
            }
            surface.set_behind_tab(cap != shown);
        }
    }
    let surface = table.get_mut(shown)?;
    surface.damage = Some(Rect {
        x: 0,
        y: 0,
        w: surface.w,
        h: surface.h,
    });
    Some(window_decoration::bounds(surface.screen_rect()))
}

/// Move every member of `cap`'s frame to `cap`'s origin (after `cap` moved),
/// then re-sync. Returns the screen area to repaint.
pub fn follow(frames: &FrameRegistry, table: &mut SurfaceTable, cap: u64) -> Option<Rect> {
    let frame = frames.frame_of(cap)?;
    let origin = table.get(cap).map(|surface| (surface.x, surface.y))?;
    let shown = frames.selected_cap(frame)?;
    let before = table
        .get(shown)
        .map(|surface| window_decoration::bounds(surface.screen_rect()));
    if let Some(surface) = table.get_mut(shown) {
        surface.move_to(origin.0, origin.1);
    }
    let after = sync(frames, table, frame)?;
    Some(before.map_or(after, |before| before.union(&after)))
}

/// [`sync`] the frame holding `cap` after `cap` moved; others follow it.
pub fn sync_frame_of(frames: &FrameRegistry, table: &mut SurfaceTable, cap: u64) {
    if let Some(frame) = frames.frame_of(cap) {
        sync(frames, table, frame);
    }
}

/// Show the window `cap` in its frame because the user picked its tab. It
/// takes the previous window's place in z-order and is asked to take its
/// size and state. Returns the screen area to repaint.
pub fn select(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    cap: u64,
) -> Option<Rect> {
    let (previous, area) = show(frames, table, z_order, cap)?;
    if let Some((state, rect)) = table.get(previous).map(|s| (s.state(), s.intended_rect())) {
        match_model(table, cap, state, rect);
    }
    Some(area)
}

/// Show `cap` in its frame in place of the shown window, which it returns
/// with the screen area to repaint. Asks nothing of `cap`.
fn show(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    cap: u64,
) -> Option<(u64, Rect)> {
    let previous = frames.select(cap)?;
    z_order.swap(previous, cap);
    let before = frame_bounds(frames, table, previous);
    let frame = frames.frame_of(cap)?;
    let after = sync(frames, table, frame)?;
    Some((
        previous,
        before.map_or(after, |before| before.union(&after)),
    ))
}

/// Pull the shown window `cap` out of its stack into a window of its own,
/// at `at` or else a little below and right of where it was, kept on the
/// `screen`. The stack shows the tab beside it in its place. Returns the
/// area to repaint.
pub fn detach(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    cap: u64,
    screen: (u32, u32),
    at: Option<(i32, i32)>,
) -> Option<Rect> {
    let before = frame_bounds(frames, table, cap)?;
    let (_, change) = frames.detach(cap)?;
    if let Some(next) = change.selected {
        z_order.swap(cap, next);
    }
    let stack = sync(frames, table, change.frame_id);
    let surface = table.get_mut(cap)?;
    let max_x = (screen.0 as i32)
        .saturating_sub(DETACH_KEEP)
        .max(window_decoration::BORDER);
    let max_y = (screen.1 as i32)
        .saturating_sub(DETACH_KEEP)
        .max(window_decoration::TITLE);
    let (x, y) = at.unwrap_or((
        surface.x.saturating_add(DETACH_OFFSET),
        surface.y.saturating_add(DETACH_OFFSET),
    ));
    let x = x.clamp(window_decoration::BORDER, max_x);
    let y = y.clamp(window_decoration::TITLE, max_y);
    surface.move_to(x, y);
    surface.set_behind_tab(false);
    surface.damage = Some(Rect {
        x: 0,
        y: 0,
        w: surface.w,
        h: surface.h,
    });
    let after = window_decoration::bounds(surface.screen_rect());
    let area = before.union(&after);
    Some(stack.map_or(area, |stack| area.union(&stack)))
}

/// The user dropped the frame showing `source` onto the frame showing
/// `target`: its tabs join the end of the target's strip, and its shown
/// window is shown in the target's place, asked for the target's size and
/// state. Returns the screen area to repaint.
pub fn stack(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    source: u64,
    target: u64,
) -> Option<Rect> {
    let (from, onto) = (frames.frame_of(source)?, frames.frame_of(target)?);
    let (moving, staying) = (frames.selected_cap(from)?, frames.selected_cap(onto)?);
    let model = table
        .get(staying)
        .map(|surface| (surface.state(), surface.intended_rect()))?;
    let before = frame_bounds(frames, table, moving);
    if !frames.stack(from, onto) {
        return None;
    }
    table.get_mut(moving)?.move_to(model.1.x, model.1.y);
    z_order.swap(staying, moving);
    let after = sync(frames, table, onto)?;
    match_model(table, moving, model.0, model.1);
    Some(before.map_or(after, |before| before.union(&after)))
}

/// After the shown window `old` of a frame left (destroyed or closing), show
/// `change.selected` in its place in z-order. Returns the area to repaint.
pub fn reselected(
    frames: &FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    old: u64,
    change: Reselected,
) -> Option<Rect> {
    let shown = change.selected?;
    z_order.swap(old, shown);
    sync(frames, table, change.frame_id)
}

/// Forget the window `cap`, which is about to be removed from the table.
/// When it was the shown tab, the tab beside it is shown instead.
/// When it was the tab "Close all tabs" was waiting on (closing, unanswered),
/// the next queued tab is asked now.
pub fn forget(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    cap: u64,
) -> Option<Rect> {
    let was_shown = frames.frame_of(cap).is_some() && !frames.is_hidden(cap);
    let awaited = frames.frame_of(cap).filter(|&frame| {
        frames.is_closing_all(frame) && table.get(cap).is_some_and(|s| s.awaits_close_answer())
    });
    let change = frames.remove_surface(cap)?;
    let mut area = if was_shown {
        reselected(frames, table, z_order, cap, change)
    } else {
        None
    };
    if let Some(frame) = awaited {
        if let Some(next) = advance_close(frames, table, z_order, frame) {
            area = Some(area.map_or(next, |a| a.union(&next)));
        }
    }
    area
}

#[cfg(test)]
mod tests;
