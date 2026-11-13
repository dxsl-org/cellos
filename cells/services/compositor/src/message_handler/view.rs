//! Authenticated session view and frame IPC dispatch.

use ostd::ipc::reply_to;
use wire::display::{
    compositor_ops, CreateViewRequest, Rect, SurfaceRole, ViewOpResponse, ViewOpResult,
    CREATE_VIEW_REQUEST_WIRE_SIZE, VIEW_OP_RESPONSE_WIRE_SIZE,
};

use super::{view_frame, MessageContext};
use crate::stacking;

pub(super) fn handle(buf: &[u8; 512], sender: usize, context: MessageContext<'_>) {
    let MessageContext {
        screen_w,
        screen_h,
        input,
        table,
        frames,
        z_order,
        pending_dirty,
        ..
    } = context;
    match buf[0] {
        compositor_ops::CREATE_VIEW => {
            if buf.len() < CREATE_VIEW_REQUEST_WIRE_SIZE {
                send_error(
                    sender,
                    compositor_ops::CREATE_VIEW,
                    ViewOpResult::InvalidToken,
                );
                return;
            }
            let role = SurfaceRole::from_u8(buf[1]);
            let frame_id = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
            let frame_generation = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
            let w = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
            let h = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]);
            let min_w = u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]);
            let min_h = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]);
            let recipient_incarnation = u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]);
            let file_grant_id = u64::from_le_bytes([
                buf[32], buf[33], buf[34], buf[35], buf[36], buf[37], buf[38], buf[39],
            ]);
            let request_generation = u32::from_le_bytes([buf[40], buf[41], buf[42], buf[43]]);
            let mut launch_token = [0u8; 16];
            launch_token.copy_from_slice(&buf[48..64]);

            let req = CreateViewRequest {
                opcode: compositor_ops::CREATE_VIEW,
                role,
                _pad: [0; 2],
                frame_id,
                frame_generation,
                w,
                h,
                min_w,
                min_h,
                recipient_incarnation,
                file_grant_id,
                request_generation,
                launch_token,
                _reserved: 0,
            };

            // `recipient_incarnation`, `file_grant_id` and `launch_token` are
            // carried for clients but confer no authority: only the frame's
            // owner may add a view to an existing frame (`FrameRegistry`).
            // Allocate backing surface in SurfaceTable, behind the frame's shown tab.
            let (fx, fy) = frames
                .selected_cap(frame_id)
                .and_then(|shown| table.get(shown))
                .map_or((100, 100), |shown| (shown.x, shown.y));
            let cap = match table.create(fx, fy, w, h, sender, role) {
                Ok(c) => c,
                Err(_) => {
                    send_error(
                        sender,
                        compositor_ops::CREATE_VIEW,
                        ViewOpResult::CapacityExhausted,
                    );
                    return;
                }
            };

            match frames.create_view(&req, sender, cap) {
                Ok(resp) => {
                    z_order.push(cap);
                    stacking::sync(frames, table, resp.frame_id);
                    // The table may have downgraded a refused Panel request.
                    if table
                        .get(cap)
                        .is_some_and(|surface| surface.role == SurfaceRole::Interactive)
                    {
                        input.focus_new_window(sender, cap);
                    }
                    send_response(sender, &resp);
                }
                Err(err) => {
                    table.remove(cap);
                    send_error(sender, compositor_ops::CREATE_VIEW, err);
                }
            }
        }
        compositor_ops::DESTROY_VIEW => {
            if buf.len() < 17 {
                send_error(
                    sender,
                    compositor_ops::DESTROY_VIEW,
                    ViewOpResult::InvalidToken,
                );
                return;
            }
            let frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let view_id = u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]);
            let view_inc = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);

            let was_shown = frames
                .surface_for_view(view_id)
                .is_some_and(|cap| !frames.is_hidden(cap));
            match frames.destroy_view(frame_id, frame_gen, view_id, view_inc, sender) {
                Ok((resp, surface_cap, reselected)) => {
                    let freed = if let Some(s) = table.get(surface_cap) {
                        crate::window_decoration::bounds(s.screen_rect())
                    } else {
                        crate::window_decoration::bounds(Rect {
                            x: 100,
                            y: 100,
                            w: 800,
                            h: 600,
                        })
                    };
                    if was_shown {
                        stacking::reselected(frames, table, z_order, surface_cap, reselected);
                    }
                    input.remove_surface(surface_cap);
                    table.remove(surface_cap);
                    z_order.remove(surface_cap);
                    // Focus follows only to the caller's own next tab.
                    if let Some(next_cap) = reselected
                        .selected
                        .filter(|&next| table.get(next).is_some_and(|s| s.owner == sender))
                    {
                        input.request_focus(sender, next_cap);
                        z_order.raise(next_cap);
                    }
                    *pending_dirty = Some(match pending_dirty.take() {
                        Some(acc) => acc.union(&freed),
                        None => freed,
                    });
                    send_response(sender, &resp);
                }
                Err(err) => {
                    send_error(sender, compositor_ops::DESTROY_VIEW, err);
                }
            }
        }
        compositor_ops::SELECT_VIEW => {
            if buf.len() < 17 {
                send_error(
                    sender,
                    compositor_ops::SELECT_VIEW,
                    ViewOpResult::InvalidToken,
                );
                return;
            }
            let frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let view_id = u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]);
            let view_inc = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);

            let previous = frames.selected_cap(frame_id);
            match frames.select_view(frame_id, frame_gen, view_id, view_inc, sender) {
                Ok(resp) => {
                    if let Some(cap) = frames.surface_for_view(view_id) {
                        if let Some(previous) = previous {
                            z_order.swap(previous, cap);
                        }
                        stacking::sync(frames, table, frame_id);
                        z_order.raise(cap);
                        input.request_focus(sender, cap);
                        if let Some(s) = table.get(cap) {
                            let rect = crate::window_decoration::bounds(s.screen_rect());
                            *pending_dirty = Some(match pending_dirty.take() {
                                Some(acc) => acc.union(&rect),
                                None => rect,
                            });
                        }
                    }
                    send_response(sender, &resp);
                }
                Err(err) => {
                    send_error(sender, compositor_ops::SELECT_VIEW, err);
                }
            }
        }
        compositor_ops::GROUP_VIEW => {
            view_frame::group(buf, sender, table, frames, z_order, pending_dirty)
        }
        compositor_ops::DETACH_VIEW => {
            view_frame::detach(buf, sender, table, frames, z_order, pending_dirty)
        }
        compositor_ops::SNAP_FRAME => view_frame::snap(
            buf,
            sender,
            (screen_w, screen_h),
            table,
            frames,
            pending_dirty,
        ),
        compositor_ops::TOGGLE_FULLSCREEN => view_frame::fullscreen(
            buf,
            sender,
            (screen_w, screen_h),
            table,
            frames,
            pending_dirty,
        ),
        _ => {}
    }
}

pub(super) fn send_response(recipient: usize, resp: &ViewOpResponse) {
    let mut reply = [0u8; VIEW_OP_RESPONSE_WIRE_SIZE];
    reply[0] = resp.opcode;
    reply[1] = resp.result as u8;
    reply[2..4].copy_from_slice(&resp._pad);
    reply[4..8].copy_from_slice(&resp.frame_id.to_le_bytes());
    reply[8..12].copy_from_slice(&resp.frame_generation.to_le_bytes());
    reply[12..16].copy_from_slice(&resp.view_id.to_le_bytes());
    reply[16..20].copy_from_slice(&resp.view_incarnation.to_le_bytes());
    let _ = reply_to(recipient, &reply);
}

pub(super) fn send_error(recipient: usize, opcode: u8, err: ViewOpResult) {
    let mut reply = [0u8; VIEW_OP_RESPONSE_WIRE_SIZE];
    reply[0] = opcode;
    reply[1] = err as u8;
    let _ = reply_to(recipient, &reply);
}
