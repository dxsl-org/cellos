use api::display::{compositor_ops, Rect, SurfaceRole};
use api::services::proto::compositor::{DESTROY_OK, DESTROY_REFUSED};
use ostd::ipc::reply_to;

use crate::input_handler::InputState;
use crate::stacking;
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::window_decoration;
use crate::z_order::ZOrder;

#[allow(deprecated)] // WRITE_PIXELS kept for legacy clients; new code uses ATTACH_GRANT
pub(super) fn handle(
    buf: &[u8; 512],
    sender: usize,
    input: &mut InputState,
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    match buf[0] {
        compositor_ops::CREATE_SURFACE => {
            if buf.len() < 10 {
                return;
            }
            let sw = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let sh = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let role = SurfaceRole::from_u8(buf[9]);
            match table.create(0, 0, sw, sh, sender, role) {
                Ok(cap) => {
                    z_order.push(cap);
                    // The table may have downgraded a refused Panel request.
                    if table
                        .get(cap)
                        .is_some_and(|surface| surface.role == SurfaceRole::Interactive)
                    {
                        frames.adopt(cap, sender);
                        input.focus_new_window(sender, cap);
                    }
                    let _ = reply_to(sender, &cap.to_le_bytes());
                }
                Err(_) => {
                    let _ = reply_to(sender, &0u64.to_le_bytes());
                }
            }
        }
        compositor_ops::WRITE_PIXELS => {
            if buf.len() < 25 {
                return;
            }
            let cap = u64::from_le_bytes(buf[1..9].try_into().unwrap());
            let x = i32::from_le_bytes(buf[9..13].try_into().unwrap());
            let y = i32::from_le_bytes(buf[13..17].try_into().unwrap());
            let pw = u32::from_le_bytes(buf[17..21].try_into().unwrap());
            let ph = u32::from_le_bytes(buf[21..25].try_into().unwrap());
            if let Some(s) = table.get_mut(cap).filter(|s| s.owner == sender) {
                s.write_pixels(x, y, pw, ph, &buf[25..]);
            }
        }
        compositor_ops::DAMAGE_SURFACE => {
            if buf.len() < 25 {
                return;
            }
            let cap = u64::from_le_bytes(buf[1..9].try_into().unwrap());
            let x = i32::from_le_bytes(buf[9..13].try_into().unwrap());
            let y = i32::from_le_bytes(buf[13..17].try_into().unwrap());
            let w = u32::from_le_bytes(buf[17..21].try_into().unwrap());
            let h = u32::from_le_bytes(buf[21..25].try_into().unwrap());
            if let Some(s) = table.get_mut(cap).filter(|s| s.owner == sender) {
                let damage = Rect { x, y, w, h };
                s.damage = Some(match s.damage {
                    Some(existing) => existing.union(&damage),
                    None => damage,
                });
            }
        }
        compositor_ops::MOVE_SURFACE => move_surface(buf, sender, table, frames, pending_dirty),
        compositor_ops::RAISE_SURFACE => {
            raise_surface(buf, sender, input, table, frames, z_order, pending_dirty)
        }
        compositor_ops::DESTROY_SURFACE => {
            destroy_surface(buf, sender, input, table, frames, z_order, pending_dirty)
        }
        _ => {}
    }
}

/// A window in a stack moves its whole frame with it.
fn move_surface(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    dirty: &mut Option<Rect>,
) {
    if buf.len() < 17 {
        return;
    }
    let cap = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    let x = i32::from_le_bytes(buf[9..13].try_into().unwrap());
    let y = i32::from_le_bytes(buf[13..17].try_into().unwrap());
    // A tab behind another sits where its frame is; only the shown window
    // places the frame.
    if frames.is_hidden(cap) {
        return;
    }
    let Some(surface) = table.get_mut(cap).filter(|surface| surface.owner == sender) else {
        return;
    };
    let old_region = surface.paint_region();
    surface.move_to(x, y);
    surface.damage = Some(Rect {
        x: 0,
        y: 0,
        w: surface.w,
        h: surface.h,
    });
    mark_dirty(dirty, old_region, surface.paint_region());
    // A window its app moves on its own leaves its tile group.
    if let Some(frame) = frames.frame_of(cap) {
        frames.untile(frame);
    }
    if let Some(moved) = stacking::follow(frames, table, cap) {
        mark_dirty(dirty, moved, moved);
    }
}

/// Raise the caller's own window. A window behind another tab of its stack
/// is shown only when the caller may take focus anyway (it holds the
/// keyboard, or nobody does), so a hidden tab cannot cover another app's tab.
fn raise_surface(
    buf: &[u8; 512],
    sender: usize,
    input: &mut InputState,
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    z_order: &mut ZOrder,
    dirty: &mut Option<Rect>,
) {
    if buf.len() < 9 {
        return;
    }
    let cap = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    if !table.get(cap).is_some_and(|s| {
        s.owner == sender && (s.role == SurfaceRole::Interactive || s.role == SurfaceRole::Panel)
    }) {
        return;
    }
    if frames.is_hidden(cap) {
        if !input.may_focus(sender) {
            return;
        }
        if let Some(area) = stacking::select(frames, table, z_order, cap) {
            mark_dirty(dirty, area, area);
        }
    }
    z_order.raise(cap);
    input.request_focus(sender, cap);
    if let Some(surface) = table.get_mut(cap) {
        let full = Rect {
            x: 0,
            y: 0,
            w: surface.w,
            h: surface.h,
        };
        surface.damage = Some(match surface.damage {
            Some(damage) => damage.union(&full),
            None => full,
        });
        let region = surface.paint_region();
        *dirty = Some(match dirty.take() {
            Some(acc) => acc.union(&region),
            None => region,
        });
    }
}

fn destroy_surface(
    buf: &[u8; 512],
    sender: usize,
    input: &mut InputState,
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    z_order: &mut ZOrder,
    dirty: &mut Option<Rect>,
) {
    if buf.len() < 9 {
        return;
    }
    let cap = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    if !table
        .get(cap)
        .is_some_and(|surface| surface.owner == sender)
    {
        let _ = reply_to(sender, &[DESTROY_REFUSED]);
        return;
    }
    let freed_rect = table
        .get(cap)
        .map(|surface| window_decoration::bounds(surface.screen_rect()));
    if let Some(area) = stacking::forget(frames, table, z_order, cap) {
        mark_dirty(dirty, area, area);
    }
    input.remove_surface(cap);
    z_order.remove(cap);
    let _ = table.remove(cap);
    let _ = reply_to(sender, &[DESTROY_OK]);
    if let Some(rect) = freed_rect {
        *dirty = Some(match dirty.take() {
            Some(acc) => acc.union(&rect),
            None => rect,
        });
    }
}

fn mark_dirty(dirty: &mut Option<Rect>, old: Rect, new: Rect) {
    *dirty = Some(match dirty.take() {
        Some(acc) => acc.union(&old).union(&new),
        None => old.union(&new),
    });
}
