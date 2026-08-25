//! Cursor position updates and hardware/software repaint policy.

use api::display::Rect;
use ostd::syscall::sys_gpu_cursor;

use crate::cursor_sprite::{CURSOR_H, CURSOR_W};

/// Update the logical cursor and either move the hardware sprite or damage both
/// old and new software-sprite locations.
pub fn update(
    x: i32,
    y: i32,
    hw_cursor: bool,
    mouse_x: &mut i32,
    mouse_y: &mut i32,
    pending_dirty: &mut Option<Rect>,
) {
    let old = rect(*mouse_x, *mouse_y);
    *mouse_x = x;
    *mouse_y = y;
    ostd::io::println(&alloc::format!(
        "[compositor] cursor at {},{}",
        *mouse_x,
        *mouse_y
    ));
    if hw_cursor {
        let _ = sys_gpu_cursor(1, core::ptr::null(), *mouse_x as u32, *mouse_y as u32, 0, 0);
        return;
    }
    let dirty = old.union(&rect(*mouse_x, *mouse_y));
    *pending_dirty = Some(match pending_dirty.take() {
        Some(previous) => previous.union(&dirty),
        None => dirty,
    });
}

fn rect(x: i32, y: i32) -> Rect {
    Rect {
        x,
        y,
        w: CURSOR_W,
        h: CURSOR_H,
    }
}
