//! Closing tabs of a stack: the shown tab hands over to its neighbour, and
//! "Close all tabs" asks each tab in turn.

use wire::display::{Rect, WindowState};

use super::{deliver, show, sync};
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::window_decoration;
use crate::z_order::ZOrder;

/// Ask the owner of `cap` to close it (it may refuse). The window leaves the
/// screen while asked. Returns the area to repaint, or `None` when it could
/// not be asked (already closing, being configured, or undeliverable).
pub fn ask_to_close(table: &mut SurfaceTable, cap: u64) -> Option<Rect> {
    let surface = table.get_mut(cap)?;
    let (rect, close, state) = surface.begin_close(cap as u32)?;
    if !close
        .encode()
        .is_ok_and(|event| deliver(surface.owner, &event))
    {
        surface.cancel_close(close.serial);
        return None;
    }
    if let Ok(event) = state.encode() {
        deliver(surface.owner, &event);
    }
    Some(window_decoration::bounds(rect))
}

/// The shown tab `cap` just started closing: show the live tab beside it
/// (after it, else before it). Returns that tab and the area to repaint.
pub fn show_next(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    cap: u64,
) -> Option<(u64, Rect)> {
    let frame = frames.frame_of(cap)?;
    if frames.selected_cap(frame) != Some(cap) {
        return None;
    }
    let members = frames.members(frame);
    let index = members.iter().position(|&member| member == cap)?;
    let live = |member: &&u64| {
        table
            .get(**member)
            .is_some_and(|s| s.state() == WindowState::Normal)
    };
    let next = *members[index + 1..]
        .iter()
        .find(live)
        .or_else(|| members[..index].iter().rev().find(live))?;
    frames.select(next)?;
    z_order.swap(cap, next);
    let area = sync(frames, table, frame)?;
    let closing = table
        .get(cap)
        .map(|s| window_decoration::bounds(s.screen_rect()));
    Some((next, closing.map_or(area, |c| c.union(&area))))
}

/// "Close all tabs" of the stack showing `cap`: queue every tab in tab
/// order and ask the first. Each further tab is asked once the one before
/// answered ([`advance_close`]), so every owner decides about its own tab.
pub fn close_all(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    cap: u64,
) -> Option<Rect> {
    let frame = frames.frame_of(cap)?;
    let members = frames.members(frame);
    frames.set_close_queue(frame, members);
    advance_close(frames, table, z_order, frame)
}

/// Ask the next queued tab of `frame` that is still open: it is shown, then
/// asked, and the tab beside it is shown while it answers. Tabs that cannot
/// be asked are skipped. Returns the area to repaint.
pub fn advance_close(
    frames: &mut FrameRegistry,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    frame: u32,
) -> Option<Rect> {
    while let Some(next) = frames.pop_close_queue(frame) {
        if !table.get(next).is_some_and(|s| {
            s.state() == WindowState::Normal || s.state() == WindowState::Maximized
        }) {
            continue;
        }
        let shown = show(frames, table, z_order, next).map(|(_, area)| area);
        let Some(asked) = ask_to_close(table, next) else {
            #[cfg(not(test))]
            ostd::io::println(
                "[compositor] close all tabs: a tab could not be asked; it stays open",
            );
            continue;
        };
        let area = shown.map_or(asked, |shown| shown.union(&asked));
        let handed = show_next(frames, table, z_order, next).map(|(_, rect)| rect);
        return Some(handed.map_or(area, |handed| area.union(&handed)));
    }
    let _ = sync(frames, table, frame);
    None
}
