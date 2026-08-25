use api::display::{compositor_ops, Rect, SurfaceRole};
use ostd::syscall::sys_send;

use crate::input_handler::InputState;
use crate::surface_table::SurfaceTable;
use crate::window_decoration;
use crate::z_order::ZOrder;

#[allow(deprecated)] // WRITE_PIXELS kept for legacy clients; new code uses ATTACH_GRANT
pub(super) fn handle(
    buf: &[u8; 512],
    sender: usize,
    input: &mut InputState,
    table: &mut SurfaceTable,
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
                    sys_send(sender, &cap.to_le_bytes());
                }
                Err(_) => {
                    sys_send(sender, &0u64.to_le_bytes());
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
        compositor_ops::MOVE_SURFACE => move_surface(buf, sender, table, pending_dirty),
        compositor_ops::RAISE_SURFACE => raise_surface(buf, sender, table, z_order, pending_dirty),
        compositor_ops::DESTROY_SURFACE => {
            destroy_surface(buf, sender, input, table, z_order, pending_dirty)
        }
        _ => {}
    }
}

fn move_surface(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    dirty: &mut Option<Rect>,
) {
    if buf.len() < 17 {
        return;
    }
    let cap = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    let x = i32::from_le_bytes(buf[9..13].try_into().unwrap());
    let y = i32::from_le_bytes(buf[13..17].try_into().unwrap());
    if let Some(surface) = table.get_mut(cap).filter(|surface| surface.owner == sender) {
        let old_rect = surface.screen_rect();
        surface.move_to(x, y);
        surface.damage = Some(Rect {
            x: 0,
            y: 0,
            w: surface.w,
            h: surface.h,
        });
        if surface.is_window_managed() {
            mark_dirty(
                dirty,
                window_decoration::bounds(old_rect),
                window_decoration::bounds(surface.screen_rect()),
            );
        }
    }
}

fn raise_surface(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    dirty: &mut Option<Rect>,
) {
    if buf.len() < 9 {
        return;
    }
    let cap = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    if !table
        .get(cap)
        .is_some_and(|s| s.owner == sender && s.role == SurfaceRole::Interactive)
    {
        return;
    }
    z_order.raise(cap);
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
        if surface.is_window_managed() {
            let decoration = window_decoration::bounds(surface.screen_rect());
            *dirty = Some(match dirty.take() {
                Some(acc) => acc.union(&decoration),
                None => decoration,
            });
        }
    }
}

fn destroy_surface(
    buf: &[u8; 512],
    sender: usize,
    input: &mut InputState,
    table: &mut SurfaceTable,
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
        sys_send(sender, b"\x01");
        return;
    }
    let freed_rect = table
        .get(cap)
        .map(|surface| window_decoration::bounds(surface.screen_rect()));
    input.remove_surface(cap);
    z_order.remove(cap);
    let _ = table.remove(cap);
    sys_send(sender, b"\x00");
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
