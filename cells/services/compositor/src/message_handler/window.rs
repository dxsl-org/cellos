use api::display::{
    compositor_ops, CloseResponse, ConfigureAck, Rect, SetIcon, SetTitle, SurfaceStateRequest,
    WindowState,
};
use api::services::proto::compositor::{REPLY_FALSE, REPLY_TRUE};
use ostd::ipc::{post, reply_to};
use ostd::syscall::{sys_try_send, SyscallResult};

use super::MessageContext;
use crate::input_handler::InputState;
use crate::stacking;
use crate::surface_table::{FrameRegistry, StateTransition, SurfaceTable};
use crate::window_decoration;

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
        compositor_ops::SET_TITLE => set_title(buf, sender, table, frames, pending_dirty),
        compositor_ops::SET_ICON => set_icon(buf, sender, table, frames, pending_dirty),
        compositor_ops::CONFIGURE_ACK => {
            if let Some(cap) = acknowledge_configure(buf, sender, table, pending_dirty) {
                // The shown tab sets the frame's size; the others catch up.
                if let Some(area) = frames
                    .frame_of(cap)
                    .and_then(|frame| stacking::sync(frames, table, frame))
                {
                    add_dirty(pending_dirty, area);
                }
                stacking::reconcile_frame_of(frames, table, cap, (screen_w, screen_h));
                // Glued neighbours follow the edges this frame now has.
                crate::tiling::settle(frames, table, cap);
            }
        }
        compositor_ops::CLOSE_RESPONSE => {
            if let Some(cap) = close_response(buf, sender, table, pending_dirty) {
                // "Close all tabs" asks the next tab once this one answered.
                if let Some(frame) = frames
                    .frame_of(cap)
                    .filter(|&frame| frames.is_closing_all(frame))
                {
                    if let Some(area) = stacking::advance_close(frames, table, z_order, frame) {
                        add_dirty(pending_dirty, area);
                    }
                }
                stacking::reconcile_frame_of(frames, table, cap, (screen_w, screen_h));
            }
        }
        compositor_ops::MINIMIZE | compositor_ops::MAXIMIZE | compositor_ops::RESTORE => {
            request_state(
                buf,
                sender,
                (screen_w, screen_h),
                input,
                table,
                frames,
                pending_dirty,
            )
        }
        _ => {}
    }
}

/// Owner-only; an icon value this build does not know is refused whole.
fn set_icon(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    frames: &FrameRegistry,
    dirty: &mut Option<Rect>,
) {
    let Ok(frame) = SetIcon::decode(buf) else {
        return;
    };
    let Some(icon) = ostd::display::linear::icons::LineIcon::from_u8(frame.icon) else {
        return;
    };
    let Some(surface) = table
        .get_mut(u64::from(frame.cap))
        .filter(|surface| surface.owner == sender)
    else {
        return;
    };
    surface.set_icon(icon);
    if let Some(area) = stacking::frame_bounds(frames, table, u64::from(frame.cap)) {
        add_dirty(dirty, area);
    }
}

fn set_title(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    frames: &FrameRegistry,
    dirty: &mut Option<Rect>,
) {
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
    surface.set_title(title);
    if let Some(area) = stacking::frame_bounds(frames, table, frame.cap as u64) {
        add_dirty(dirty, area);
    }
}

/// Returns the cap whose configure committed.
fn acknowledge_configure(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    dirty: &mut Option<Rect>,
) -> Option<u64> {
    let Some(frame) = buf.get(..12) else {
        let _ = reply_to(sender, &[REPLY_FALSE]);
        return None;
    };
    let Ok(frame) = ConfigureAck::decode(frame) else {
        let _ = reply_to(sender, &[REPLY_FALSE]);
        return None;
    };
    let Some(surface) = table.get_mut(frame.cap as u64) else {
        let _ = reply_to(sender, &[REPLY_FALSE]);
        return None;
    };
    if surface.owner != sender {
        let _ = reply_to(sender, &[REPLY_FALSE]);
        return None;
    }
    let Some((old_rect, new_rect, state)) = surface.acknowledge_configure(frame.cap, frame.serial)
    else {
        let _ = reply_to(sender, &[REPLY_FALSE]);
        return None;
    };
    // The client consumes this asynchronous state while waiting for the reply.
    if let Ok(event) = state.encode() {
        let _ = post(sender, &event);
    }
    let _ = reply_to(sender, &[REPLY_TRUE]);
    mark_dirty(dirty, old_rect, new_rect);
    Some(frame.cap as u64)
}

fn close_response(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    dirty: &mut Option<Rect>,
) -> Option<u64> {
    let frame = CloseResponse::decode(&buf[..12]).ok()?;
    let surface = table
        .get_mut(frame.cap as u64)
        .filter(|surface| surface.owner == sender)?;
    let old_rect = surface.screen_rect();
    let awaited = surface.awaits_close_answer();
    // A refusal restores the window; an acceptance leaves it closing until
    // its owner destroys it.
    if let Some(state) = surface.close_response(frame.cap, frame.serial, frame.accept) {
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
    (awaited && !surface.awaits_close_answer()).then_some(frame.cap as u64)
}

/// An owner's own minimize/maximize/restore. The shown tab of a stack
/// speaks for the whole frame (the other tabs follow it); a tab behind it has
/// no state of its own to change.
fn request_state(
    buf: &[u8; 512],
    sender: usize,
    (screen_w, screen_h): (u32, u32),
    input: &mut InputState,
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    dirty: &mut Option<Rect>,
) {
    let Ok(frame) = SurfaceStateRequest::decode(&buf[..8]) else {
        return;
    };
    let cap = frame.cap as u64;
    // A tiled window maximizes only once the user untiles it. The shown tab
    // of a frame shared with other apps speaks for their windows too, so it
    // may only come back from minimized; the user drives the rest.
    let tiled = frames.frame_of(cap).is_some_and(|f| frames.is_tiled(f));
    let shared = frames.is_shared(cap);
    let from_minimized = table
        .get(cap)
        .is_some_and(|s| s.state() == WindowState::Minimized);
    let refused = (tiled && frame.opcode == compositor_ops::MAXIMIZE)
        || (shared && !(frame.opcode == compositor_ops::RESTORE && from_minimized));
    if frames.is_hidden(cap) || refused {
        return;
    }
    let Some(surface) = table.get_mut(cap) else {
        return;
    };
    if surface.owner != sender {
        return;
    }
    let old_rect = surface.screen_rect();
    let maximum_rect = window_decoration::content_within(Rect {
        x: 0,
        y: 0,
        w: screen_w,
        h: screen_h,
    });
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
                input.deactivate_surface(cap);
            } else if prior_state == WindowState::Minimized {
                // Its tile group went down with it and comes back with it.
                if let Some(area) = crate::tiling::restore_group(frames, table, cap) {
                    add_dirty(dirty, area);
                }
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

fn add_dirty(dirty: &mut Option<Rect>, area: Rect) {
    *dirty = Some(dirty.map_or(area, |acc| acc.union(&area)));
}
