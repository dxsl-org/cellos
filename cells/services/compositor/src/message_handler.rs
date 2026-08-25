//! Authenticated display IPC dispatch and owner-exit cleanup.

mod grant;
mod surface;
mod window;

use api::display::Rect;
use api::syscall::service;
use ostd::syscall::{sys_grant_unregister, sys_lookup_service};

use crate::input_handler::InputState;
use crate::surface_table::SurfaceTable;
use crate::z_order::ZOrder;

const OWNER_EXITED_OPCODE: u8 = 0xE2;

/// Dispatch one IPC message from a consumer cell or the supervisor.
pub(crate) fn handle_message(
    buf: &[u8; 512],
    sender: usize,
    screen_w: u32,
    screen_h: u32,
    input: &mut InputState,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    if buf.is_empty() {
        return;
    }
    if buf[0] == OWNER_EXITED_OPCODE
        && sender == sys_lookup_service(service::SUPERVISOR).unwrap_or(0)
    {
        let dead_tid = usize::from_le_bytes(
            buf[1..1 + core::mem::size_of::<usize>()]
                .try_into()
                .unwrap_or([0; core::mem::size_of::<usize>()]),
        );
        cleanup_owner(dead_tid, input, table, z_order, pending_dirty);
        return;
    }

    #[allow(deprecated)] // WRITE_PIXELS kept for legacy clients; new code uses ATTACH_GRANT
    match buf[0] {
        api::display::compositor_ops::CREATE_SURFACE
        | api::display::compositor_ops::WRITE_PIXELS
        | api::display::compositor_ops::DAMAGE_SURFACE
        | api::display::compositor_ops::MOVE_SURFACE
        | api::display::compositor_ops::RAISE_SURFACE
        | api::display::compositor_ops::DESTROY_SURFACE => {
            surface::handle(buf, sender, input, table, z_order, pending_dirty)
        }
        api::display::compositor_ops::SET_TITLE
        | api::display::compositor_ops::CONFIGURE_ACK
        | api::display::compositor_ops::CLOSE_RESPONSE
        | api::display::compositor_ops::MINIMIZE
        | api::display::compositor_ops::MAXIMIZE
        | api::display::compositor_ops::RESTORE => {
            window::handle(buf, sender, screen_w, screen_h, input, table, pending_dirty)
        }
        api::display::compositor_ops::ATTACH_GRANT
        | api::display::compositor_ops::DETACH_REPLACED_GRANT
        | api::display::compositor_ops::DAMAGE_NOTIFY
        | api::display::compositor_ops::DETACH_GRANT
        | api::display::compositor_ops::GET_SCREEN_SIZE => grant::handle(buf, sender, table),
        _ => {}
    }
}

fn cleanup_owner(
    dead_tid: usize,
    input: &mut InputState,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    for cap in table.caps_owned_by(dead_tid) {
        let Some(surface) = table.get(cap) else {
            continue;
        };
        let freed_rect = crate::window_decoration::bounds(surface.screen_rect());
        let grant_ids = surface.grant_ids();
        input.remove_surface(cap);
        z_order.remove(cap);
        let _ = table.remove(cap);
        for index in 0..grant_ids.len() {
            let Some(reg_id) = grant_ids[index] else {
                continue;
            };
            if !grant_ids[..index].contains(&Some(reg_id)) {
                let _ = sys_grant_unregister(reg_id);
            }
        }
        *pending_dirty = Some(match pending_dirty.take() {
            Some(acc) => acc.union(&freed_rect),
            None => freed_rect,
        });
    }
}
