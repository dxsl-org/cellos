use api::display::{
    compositor_ops, CloseResponse, ConfigureAck, Rect, SetTitle, SurfaceStateRequest,
};
use ostd::syscall::{sys_send, sys_try_send, SyscallResult};

use crate::input_handler::InputState;
use crate::surface_table::{StateTransition, SurfaceTable};
use crate::window_decoration;

pub(super) fn handle(
    buf: &[u8; 512],
    sender: usize,
    screen_w: u32,
    screen_h: u32,
    input: &mut InputState,
    table: &mut SurfaceTable,
    pending_dirty: &mut Option<Rect>,
) {
    match buf[0] {
        compositor_ops::SET_TITLE => set_title(buf, sender, table, pending_dirty),
        compositor_ops::CONFIGURE_ACK => acknowledge_configure(buf, sender, table, pending_dirty),
        compositor_ops::CLOSE_RESPONSE => close_response(buf, sender, table, pending_dirty),
        compositor_ops::MINIMIZE | compositor_ops::MAXIMIZE | compositor_ops::RESTORE => {
            request_state(buf, sender, screen_w, screen_h, input, table, pending_dirty)
        }
        _ => {}
    }
}

fn set_title(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable, dirty: &mut Option<Rect>) {
    let Ok(frame) = SetTitle::decode(&buf[..72]) else {
        return;
    };
    let Some(surface) = table.get_mut(frame.cap as u64) else {
        return;
    };
    if surface.owner != sender {
        return;
    }
    let Ok(title) = frame.title_str() else {
        return;
    };
    let decoration = window_decoration::bounds(surface.screen_rect());
    surface.set_title(title);
    *dirty = Some(match dirty.take() {
        Some(current) => current.union(&decoration),
        None => decoration,
    });
}

fn acknowledge_configure(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    dirty: &mut Option<Rect>,
) {
    let Some(frame) = buf.get(..12) else {
        sys_send(sender, b"\x00");
        return;
    };
    let Ok(frame) = ConfigureAck::decode(frame) else {
        sys_send(sender, b"\x00");
        return;
    };
    let Some(surface) = table.get_mut(frame.cap as u64) else {
        sys_send(sender, b"\x00");
        return;
    };
    if surface.owner != sender {
        sys_send(sender, b"\x00");
        return;
    }
    let Some((old_rect, new_rect, state)) = surface.acknowledge_configure(frame.cap, frame.serial)
    else {
        sys_send(sender, b"\x00");
        return;
    };
    // The client consumes this asynchronous state while waiting for the reply.
    if let Ok(event) = state.encode() {
        let _ = sys_send(sender, &event);
    }
    sys_send(sender, b"\x01");
    mark_dirty(dirty, old_rect, new_rect);
}

fn close_response(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    dirty: &mut Option<Rect>,
) {
    let Ok(frame) = CloseResponse::decode(&buf[..12]) else {
        return;
    };
    let Some(surface) = table.get_mut(frame.cap as u64) else {
        return;
    };
    if surface.owner != sender {
        return;
    }
    let old_rect = surface.screen_rect();
    let Some(state) = surface.close_response(frame.cap, frame.serial, frame.accept) else {
        return;
    };
    surface.damage = Some(Rect {
        x: 0,
        y: 0,
        w: surface.w,
        h: surface.h,
    });
    if let Ok(event) = state.encode() {
        let _ = sys_try_send(sender, &event);
    }
    mark_dirty(dirty, old_rect, surface.screen_rect());
}

fn request_state(
    buf: &[u8; 512],
    sender: usize,
    screen_w: u32,
    screen_h: u32,
    input: &mut InputState,
    table: &mut SurfaceTable,
    dirty: &mut Option<Rect>,
) {
    let Ok(frame) = SurfaceStateRequest::decode(&buf[..8]) else {
        return;
    };
    let Some(surface) = table.get_mut(frame.cap as u64) else {
        return;
    };
    if surface.owner != sender {
        return;
    }
    let old_rect = surface.screen_rect();
    let maximum_rect = Rect {
        x: window_decoration::FRAME,
        y: window_decoration::FRAME + window_decoration::TITLE,
        w: screen_w.saturating_sub((window_decoration::FRAME * 2) as u32),
        h: screen_h
            .saturating_sub((window_decoration::FRAME * 2 + window_decoration::TITLE) as u32),
    };
    let prior_state = surface.state();
    match surface.request_state(frame.cap, frame.opcode, maximum_rect) {
        Some(StateTransition::Configure(configure)) => {
            let delivered = configure
                .encode()
                .is_ok_and(|event| matches!(sys_try_send(sender, &event), SyscallResult::Ok(0)));
            if !delivered {
                surface.cancel_configure(configure.serial);
            }
        }
        Some(StateTransition::StateChanged(state)) => {
            let delivered = state
                .encode()
                .is_ok_and(|event| matches!(sys_try_send(sender, &event), SyscallResult::Ok(0)));
            if !delivered {
                surface.rollback_state_change(prior_state);
                return;
            }
            let visible = surface.is_visible_for_paint();
            if visible {
                surface.damage = Some(Rect {
                    x: 0,
                    y: 0,
                    w: surface.w,
                    h: surface.h,
                });
            }
            mark_dirty(dirty, old_rect, surface.screen_rect());
            if !visible {
                input.deactivate_surface(frame.cap as u64);
            }
        }
        None => {}
    }
}

fn mark_dirty(dirty: &mut Option<Rect>, old: Rect, new: Rect) {
    let old = window_decoration::bounds(old);
    let new = window_decoration::bounds(new);
    *dirty = Some(match dirty.take() {
        Some(acc) => acc.union(&old).union(&new),
        None => old.union(&new),
    });
}
