//! Authenticated session view and frame IPC dispatch.

use api::display::{
    compositor_ops, CreateViewRequest, Rect, SurfaceRole, ViewOpResponse, ViewOpResult,
    CREATE_VIEW_REQUEST_WIRE_SIZE, VIEW_OP_RESPONSE_WIRE_SIZE,
};
use ostd::syscall::sys_send;

use crate::input_handler::InputState;
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::z_order::ZOrder;

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
        compositor_ops::CREATE_VIEW => {
            if buf.len() < CREATE_VIEW_REQUEST_WIRE_SIZE {
                send_error(sender, compositor_ops::CREATE_VIEW, ViewOpResult::InvalidToken);
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

            // Allocate backing surface in SurfaceTable
            let (fx, fy) = if frame_id != 0 {
                frames.frame_position(frame_id).unwrap_or((100, 100))
            } else {
                (100, 100)
            };
            let cap = match table.create(fx, fy, w, h, sender, role) {
                Ok(c) => c,
                Err(_) => {
                    send_error(sender, compositor_ops::CREATE_VIEW, ViewOpResult::CapacityExhausted);
                    return;
                }
            };

            match frames.create_view(&req, sender, cap) {
                Ok(resp) => {
                    z_order.push(cap);
                    if role == SurfaceRole::Interactive {
                        input.set_focus(sender, Some(cap));
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
                send_error(sender, compositor_ops::DESTROY_VIEW, ViewOpResult::InvalidToken);
                return;
            }
            let frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let view_id = u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]);
            let view_inc = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);

            match frames.destroy_view(frame_id, frame_gen, view_id, view_inc, sender) {
                Ok((resp, surface_cap)) => {
                    let freed = if let Some(s) = table.get(surface_cap) {
                        crate::window_decoration::bounds(s.screen_rect())
                    } else {
                        crate::window_decoration::bounds(Rect { x: 100, y: 100, w: 800, h: 600 })
                    };
                    input.remove_surface(surface_cap);
                    table.remove(surface_cap);
                    z_order.remove(surface_cap);
                    if let Some(next_v) = frames.selected_view_for_frame(frame_id) {
                        if let Some(next_cap) = frames.surface_for_view(next_v.view_id) {
                            let owner = table.get(next_cap).map_or(sender, |s| s.owner);
                            input.set_focus(owner, Some(next_cap));
                            z_order.raise(next_cap);
                        }
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
                send_error(sender, compositor_ops::SELECT_VIEW, ViewOpResult::InvalidToken);
                return;
            }
            let frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let view_id = u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]);
            let view_inc = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);

            match frames.select_view(frame_id, frame_gen, view_id, view_inc) {
                Ok(resp) => {
                    if let Some(cap) = frames.surface_for_view(view_id) {
                        z_order.raise(cap);
                        let owner = table.get(cap).map_or(sender, |s| s.owner);
                        input.set_focus(owner, Some(cap));
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
            if buf.len() < 25 {
                send_error(sender, compositor_ops::GROUP_VIEW, ViewOpResult::InvalidToken);
                return;
            }
            let target_frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let target_frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let source_frame_id = u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]);
            let source_frame_gen = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
            let view_id = u32::from_le_bytes([buf[17], buf[18], buf[19], buf[20]]);
            let view_inc = u32::from_le_bytes([buf[21], buf[22], buf[23], buf[24]]);

            match frames.group_view(
                target_frame_id,
                target_frame_gen,
                source_frame_id,
                source_frame_gen,
                view_id,
                view_inc,
                sender,
            ) {
                Ok(resp) => {
                    if let Some(cap) = frames.surface_for_view(view_id) {
                        if let Some((tx, ty)) = frames.frame_position(target_frame_id) {
                            if let Some(s) = table.get_mut(cap) {
                                s.move_to(tx, ty);
                                let rect = crate::window_decoration::bounds(s.screen_rect());
                                *pending_dirty = Some(match pending_dirty.take() {
                                    Some(acc) => acc.union(&rect),
                                    None => rect,
                                });
                            }
                        }
                        z_order.raise(cap);
                    }
                    send_response(sender, &resp);
                }
                Err(err) => {
                    send_error(sender, compositor_ops::GROUP_VIEW, err);
                }
            }
        }
        compositor_ops::DETACH_VIEW => {
            if buf.len() < 25 {
                send_error(sender, compositor_ops::DETACH_VIEW, ViewOpResult::InvalidToken);
                return;
            }
            let source_frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let source_frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let view_id = u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]);
            let view_inc = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
            let x = i32::from_le_bytes([buf[17], buf[18], buf[19], buf[20]]);
            let y = i32::from_le_bytes([buf[21], buf[22], buf[23], buf[24]]);

            match frames.detach_view(
                source_frame_id,
                source_frame_gen,
                view_id,
                view_inc,
                x,
                y,
                sender,
            ) {
                Ok(resp) => {
                    if let Some(cap) = frames.surface_for_view(view_id) {
                        if let Some(s) = table.get_mut(cap) {
                            s.move_to(x, y);
                            let rect = crate::window_decoration::bounds(s.screen_rect());
                            *pending_dirty = Some(match pending_dirty.take() {
                                Some(acc) => acc.union(&rect),
                                None => rect,
                            });
                        }
                        z_order.raise(cap);
                    }
                    send_response(sender, &resp);
                }
                Err(err) => {
                    send_error(sender, compositor_ops::DETACH_VIEW, err);
                }
            }
        }
        _ => {}
    }
}

fn send_response(recipient: usize, resp: &ViewOpResponse) {
    let mut reply = [0u8; VIEW_OP_RESPONSE_WIRE_SIZE];
    reply[0] = resp.opcode;
    reply[1] = resp.result as u8;
    reply[2..4].copy_from_slice(&resp._pad);
    reply[4..8].copy_from_slice(&resp.frame_id.to_le_bytes());
    reply[8..12].copy_from_slice(&resp.frame_generation.to_le_bytes());
    reply[12..16].copy_from_slice(&resp.view_id.to_le_bytes());
    reply[16..20].copy_from_slice(&resp.view_incarnation.to_le_bytes());
    sys_send(recipient, &reply);
}

fn send_error(recipient: usize, opcode: u8, err: ViewOpResult) {
    let mut reply = [0u8; VIEW_OP_RESPONSE_WIRE_SIZE];
    reply[0] = opcode;
    reply[1] = err as u8;
    sys_send(recipient, &reply);
}
