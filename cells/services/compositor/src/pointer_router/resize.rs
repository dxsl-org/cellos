use api::display::{ConfigureKind, Rect};
use ostd::syscall::{sys_try_send, SyscallResult};

use crate::surface_table::SurfaceTable;
use crate::window_decoration::ResizeEdge;

const MIN_CONTENT_W: u32 = 64;
const MIN_CONTENT_H: u32 = 48;
const MAX_CONTENT: i32 = 4096;

pub(super) fn propose(
    cap: u64,
    rect: Rect,
    edge: ResizeEdge,
    dx: i32,
    dy: i32,
    table: &mut SurfaceTable,
) {
    let proposed = rect_for(rect, edge, dx, dy);
    if let Some(surface) = table.get_mut(cap) {
        if let Some(event) = surface.begin_configure(cap as u32, ConfigureKind::Resize, proposed) {
            let delivered = event.encode().is_ok_and(|bytes| {
                matches!(sys_try_send(surface.owner, &bytes), SyscallResult::Ok(0))
            });
            if !delivered {
                surface.cancel_configure(event.serial);
            }
        }
    }
}

fn rect_for(rect: Rect, edge: ResizeEdge, dx: i32, dy: i32) -> Rect {
    let right = end(rect.x, rect.w);
    let bottom = end(rect.y, rect.h);
    let move_west = matches!(
        edge,
        ResizeEdge::West | ResizeEdge::NorthWest | ResizeEdge::SouthWest
    );
    let move_east = matches!(
        edge,
        ResizeEdge::East | ResizeEdge::NorthEast | ResizeEdge::SouthEast
    );
    let move_north = matches!(
        edge,
        ResizeEdge::North | ResizeEdge::NorthWest | ResizeEdge::NorthEast
    );
    let move_south = matches!(
        edge,
        ResizeEdge::South | ResizeEdge::SouthWest | ResizeEdge::SouthEast
    );
    let x = if move_west {
        rect.x
            .saturating_add(dx)
            .max(right.saturating_sub(MAX_CONTENT))
            .min(right.saturating_sub(MIN_CONTENT_W as i32))
    } else {
        rect.x
    };
    let y = if move_north {
        rect.y
            .saturating_add(dy)
            .max(bottom.saturating_sub(MAX_CONTENT))
            .min(bottom.saturating_sub(MIN_CONTENT_H as i32))
    } else {
        rect.y
    };
    let new_right = if move_east {
        right
            .saturating_add(dx)
            .max(rect.x.saturating_add(MIN_CONTENT_W as i32))
            .min(rect.x.saturating_add(MAX_CONTENT))
    } else {
        right
    };
    let new_bottom = if move_south {
        bottom
            .saturating_add(dy)
            .max(rect.y.saturating_add(MIN_CONTENT_H as i32))
            .min(rect.y.saturating_add(MAX_CONTENT))
    } else {
        bottom
    };
    Rect {
        x,
        y,
        w: new_right
            .saturating_sub(x)
            .clamp(MIN_CONTENT_W as i32, MAX_CONTENT) as u32,
        h: new_bottom
            .saturating_sub(y)
            .clamp(MIN_CONTENT_H as i32, MAX_CONTENT) as u32,
    }
}

fn end(start: i32, extent: u32) -> i32 {
    start.saturating_add(extent.min(i32::MAX as u32) as i32)
}
