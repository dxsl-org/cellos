//! Frame-level IPC view ops: regroup a view, snap or fullscreen a frame.

use api::display::{compositor_ops, FrameRef, Rect, ViewOpResult, ViewRef};

use super::view::{send_error, send_response};
use crate::stacking;
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::z_order::ZOrder;

/// GROUP_VIEW: move the caller's view into the caller's own frame.
pub(super) fn group(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    if buf.len() < 25 {
        send_error(
            sender,
            compositor_ops::GROUP_VIEW,
            ViewOpResult::InvalidToken,
        );
        return;
    }
    let target_frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    let target_frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
    let source_frame_id = u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]);
    let source_frame_gen = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
    let view_id = u32::from_le_bytes([buf[17], buf[18], buf[19], buf[20]]);
    let view_inc = u32::from_le_bytes([buf[21], buf[22], buf[23], buf[24]]);

    let target_origin = frames
        .selected_cap(target_frame_id)
        .and_then(|shown| table.get(shown))
        .map(|shown| (shown.x, shown.y));
    let source_shown = frames.selected_cap(source_frame_id);
    match frames.group_view(
        FrameRef {
            frame_id: target_frame_id,
            generation: target_frame_gen,
        },
        FrameRef {
            frame_id: source_frame_id,
            generation: source_frame_gen,
        },
        ViewRef {
            view_id,
            incarnation: view_inc,
        },
        sender,
    ) {
        Ok(resp) => {
            keep_place(frames, z_order, source_frame_id, source_shown);
            if let Some(cap) = frames.surface_for_view(view_id) {
                if let Some((tx, ty)) = target_origin {
                    if let Some(s) = table.get_mut(cap) {
                        s.move_to(tx, ty);
                    }
                }
                for frame in [target_frame_id, source_frame_id] {
                    if let Some(rect) = stacking::sync(frames, table, frame) {
                        *pending_dirty = Some(pending_dirty.map_or(rect, |acc| acc.union(&rect)));
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

/// DETACH_VIEW: move the caller's view into a new frame at a position.
pub(super) fn detach(
    buf: &[u8; 512],
    sender: usize,
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    if buf.len() < 25 {
        send_error(
            sender,
            compositor_ops::DETACH_VIEW,
            ViewOpResult::InvalidToken,
        );
        return;
    }
    let source_frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    let source_frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
    let view_id = u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]);
    let view_inc = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
    let x = i32::from_le_bytes([buf[17], buf[18], buf[19], buf[20]]);
    let y = i32::from_le_bytes([buf[21], buf[22], buf[23], buf[24]]);
    let source_shown = frames.selected_cap(source_frame_id);

    match frames.detach_view(
        FrameRef {
            frame_id: source_frame_id,
            generation: source_frame_gen,
        },
        ViewRef {
            view_id,
            incarnation: view_inc,
        },
        x,
        y,
        sender,
    ) {
        Ok(resp) => {
            keep_place(frames, z_order, source_frame_id, source_shown);
            if let Some(cap) = frames.surface_for_view(view_id) {
                if let Some(s) = table.get_mut(cap) {
                    s.move_to(x, y);
                    let rect = crate::window_decoration::bounds(s.screen_rect());
                    *pending_dirty = Some(match pending_dirty.take() {
                        Some(acc) => acc.union(&rect),
                        None => rect,
                    });
                }
                for frame in [resp.frame_id, source_frame_id] {
                    if let Some(rect) = stacking::sync(frames, table, frame) {
                        *pending_dirty = Some(pending_dirty.map_or(rect, |acc| acc.union(&rect)));
                    }
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

/// SNAP_FRAME: move the caller's own frame to a screen zone.
pub(super) fn snap(
    buf: &[u8; 512],
    sender: usize,
    screen: (u32, u32),
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    pending_dirty: &mut Option<Rect>,
) {
    if buf.len() < 12 {
        send_error(
            sender,
            compositor_ops::SNAP_FRAME,
            ViewOpResult::InvalidToken,
        );
        return;
    }
    let frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    let frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
    let zone_byte = buf[9];
    let Some(zone) = api::display::SnapZone::from_u8(zone_byte) else {
        send_error(
            sender,
            compositor_ops::SNAP_FRAME,
            ViewOpResult::InvalidToken,
        );
        return;
    };

    // Work area excludes bottom taskbar (36 DIP)
    let work_area = Rect {
        x: 0,
        y: 0,
        w: screen.0,
        h: screen.1.saturating_sub(36),
    };

    match frames.snap_frame(frame_id, frame_gen, zone, work_area, sender) {
        Ok((resp, rect)) => {
            if let Some(v) = frames.selected_view_for_frame(frame_id) {
                if let Some(cap) = frames.surface_for_view(v.view_id) {
                    if let Some(s) = table.get_mut(cap) {
                        s.move_to(rect.x, rect.y);
                        *pending_dirty = Some(match pending_dirty.take() {
                            Some(acc) => acc.union(&rect),
                            None => rect,
                        });
                    }
                    stacking::follow(frames, table, cap);
                    if let Some(frame) = frames.frame_of(cap) {
                        frames.untile(frame);
                    }
                }
            }
            send_response(sender, &resp);
        }
        Err(err) => {
            send_error(sender, compositor_ops::SNAP_FRAME, err);
        }
    }
}

/// TOGGLE_FULLSCREEN of the caller's own frame.
pub(super) fn fullscreen(
    buf: &[u8; 512],
    sender: usize,
    screen: (u32, u32),
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    pending_dirty: &mut Option<Rect>,
) {
    if buf.len() < 12 {
        send_error(
            sender,
            compositor_ops::TOGGLE_FULLSCREEN,
            ViewOpResult::InvalidToken,
        );
        return;
    }
    let frame_id = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    let frame_gen = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
    let enable = buf[9] != 0;

    match frames.toggle_fullscreen(frame_id, frame_gen, enable, screen.0, screen.1, sender) {
        Ok((resp, rect)) => {
            if let Some(v) = frames.selected_view_for_frame(frame_id) {
                if let Some(cap) = frames.surface_for_view(v.view_id) {
                    if let Some(s) = table.get_mut(cap) {
                        s.move_to(rect.x, rect.y);
                        *pending_dirty = Some(match pending_dirty.take() {
                            Some(acc) => acc.union(&rect),
                            None => rect,
                        });
                    }
                    stacking::follow(frames, table, cap);
                    if let Some(frame) = frames.frame_of(cap) {
                        frames.untile(frame);
                    }
                }
            }
            send_response(sender, &resp);
        }
        Err(err) => {
            send_error(sender, compositor_ops::TOGGLE_FULLSCREEN, err);
        }
    }
}

/// The tab a regroup now shows in `frame` takes the place in z-order of the
/// one it showed before (`shown_before`), as for every compositor gesture.
fn keep_place(frames: &FrameRegistry, z_order: &mut ZOrder, frame: u32, shown_before: Option<u64>) {
    if let (Some(before), Some(now)) = (shown_before, frames.selected_cap(frame)) {
        if before != now {
            z_order.swap(before, now);
        }
    }
}
