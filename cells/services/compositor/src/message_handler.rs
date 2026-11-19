//! Authenticated display IPC dispatch and owner-exit cleanup.

mod grant;
mod surface;
mod view;
mod view_frame;
mod window;
use abi::syscall::service;
use ostd::service::provider_tid;
use ostd::syscall::sys_grant_unregister;
use wire::display::Rect;
use wire::proto::compositor::{OWNER_EXITED_LEN, OWNER_EXITED_RELAY};

use crate::appearance::Appearance;
use crate::input_handler::InputState;
use crate::surface_table::SurfaceTable;
use crate::z_order::ZOrder;

pub(crate) struct MessageContext<'a> {
    pub(crate) screen_w: u32,
    pub(crate) screen_h: u32,
    pub(crate) input: &'a mut InputState,
    pub(crate) table: &'a mut SurfaceTable,
    pub(crate) frames: &'a mut crate::surface_table::FrameRegistry,
    pub(crate) z_order: &'a mut ZOrder,
    pub(crate) pending_dirty: &'a mut Option<Rect>,
    pub(crate) appearance: &'a mut Appearance,
}

/// Dispatch one IPC message from a consumer cell or the supervisor.
pub(crate) fn handle_message(buf: &[u8; 512], sender: usize, context: MessageContext<'_>) {
    let MessageContext {
        screen_w,
        screen_h,
        input,
        table,
        frames,
        z_order,
        pending_dirty,
        appearance,
    } = context;
    if buf.is_empty() {
        return;
    }
    if buf[0] == OWNER_EXITED_RELAY && sender == provider_tid(service::SUPERVISOR).unwrap_or(0) {
        let dead_tid = usize::from_le_bytes(
            buf[1..OWNER_EXITED_LEN]
                .try_into()
                .unwrap_or([0; core::mem::size_of::<usize>()]),
        );
        cleanup_owner(dead_tid, input, table, frames, z_order, pending_dirty);
        return;
    }

    #[allow(deprecated)] // WRITE_PIXELS kept for legacy clients; new code uses ATTACH_GRANT
    match buf[0] {
        wire::display::compositor_ops::CREATE_SURFACE
        | wire::display::compositor_ops::WRITE_PIXELS
        | wire::display::compositor_ops::DAMAGE_SURFACE
        | wire::display::compositor_ops::MOVE_SURFACE
        | wire::display::compositor_ops::RAISE_SURFACE
        | wire::display::compositor_ops::DESTROY_SURFACE => {
            surface::handle(buf, sender, input, table, frames, z_order, pending_dirty)
        }
        wire::display::compositor_ops::SET_TITLE
        | wire::display::compositor_ops::SET_ICON
        | wire::display::compositor_ops::CONFIGURE_ACK
        | wire::display::compositor_ops::CLOSE_RESPONSE
        | wire::display::compositor_ops::MINIMIZE
        | wire::display::compositor_ops::MAXIMIZE
        | wire::display::compositor_ops::RESTORE => window::handle(
            buf,
            sender,
            MessageContext {
                screen_w,
                screen_h,
                input,
                table,
                frames,
                z_order,
                pending_dirty,
                appearance,
            },
        ),
        wire::display::compositor_ops::ATTACH_GRANT
        | wire::display::compositor_ops::DETACH_REPLACED_GRANT
        | wire::display::compositor_ops::DAMAGE_NOTIFY
        | wire::display::compositor_ops::DETACH_GRANT
        | wire::display::compositor_ops::GET_SCREEN_SIZE => {
            grant::handle(buf, sender, table, frames, appearance, (screen_w, screen_h))
        }
        wire::display::compositor_ops::CREATE_VIEW
        | wire::display::compositor_ops::DESTROY_VIEW
        | wire::display::compositor_ops::SELECT_VIEW
        | wire::display::compositor_ops::GROUP_VIEW
        | wire::display::compositor_ops::DETACH_VIEW
        | wire::display::compositor_ops::SNAP_FRAME
        | wire::display::compositor_ops::TOGGLE_FULLSCREEN => view::handle(
            buf,
            sender,
            MessageContext {
                screen_w,
                screen_h,
                input,
                table,
                frames,
                z_order,
                pending_dirty,
                appearance,
            },
        ),
        wire::display::compositor_ops::SET_APPEARANCE => {
            let frame = &buf[..wire::display::SET_APPEARANCE_WIRE_SIZE];
            let changed = wire::display::SetAppearance::decode(frame)
                .is_ok_and(|request| appearance.apply(&request, sender, table));
            if changed {
                // Every frame and panel shadow on screen changes colour, and
                // every client toolkit repaints in the new palette.
                *pending_dirty = Some(Rect {
                    x: 0,
                    y: 0,
                    w: screen_w,
                    h: screen_h,
                });
                appearance.broadcast(table);
            }
        }
        _ => {}
    }
}

/// Remove every surface, view and registered-grant frame created by `dead_tid`
/// and queue the freed screen area for repaint. Returns how many surfaces were
/// removed. Idempotent: a tid with nothing left removes nothing.
pub(crate) fn cleanup_owner(
    dead_tid: usize,
    input: &mut InputState,
    table: &mut SurfaceTable,
    frames: &mut crate::surface_table::FrameRegistry,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) -> usize {
    let mut removed = 0;
    for cap in table.caps_owned_by(dead_tid) {
        let Some(surface) = table.get(cap) else {
            continue;
        };
        let freed_rect = crate::window_decoration::bounds(surface.screen_rect());
        let grant_ids = surface.grant_ids();
        // A tab of the dead app that another app's stack showed gives way to
        // the tab beside it.
        if let Some(area) = crate::stacking::forget(frames, table, z_order, cap) {
            *pending_dirty = Some(pending_dirty.map_or(area, |acc| acc.union(&area)));
        }
        input.remove_surface(cap);
        z_order.remove(cap);
        let _ = table.remove(cap);
        removed += 1;
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
    // Views whose surface is already gone.
    frames.cleanup_owner(dead_tid);
    removed
}
