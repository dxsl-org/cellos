//! Cursor position and look updates, and the hardware/software repaint policy.

use ostd::syscall::sys_gpu_cursor;
use wire::display::Rect;

use crate::pointer_look::PointerLook;

/// Move the pointer, wearing `look`: move the hardware sprite, or damage the
/// software sprite's old and new places.
pub fn update(
    x: i32,
    y: i32,
    look: &PointerLook,
    hw_cursor: bool,
    mouse_x: &mut i32,
    mouse_y: &mut i32,
    pending_dirty: &mut Option<Rect>,
) {
    let old = look.rect(*mouse_x, *mouse_y);
    *mouse_x = x;
    *mouse_y = y;
    if hw_cursor {
        let _ = sys_gpu_cursor(1, core::ptr::null(), *mouse_x as u32, *mouse_y as u32, 0, 0);
        return;
    }
    damage(pending_dirty, old.union(&look.rect(x, y)));
}

/// Change the pointer at `(x, y)` from `old` to `new`: upload the hardware
/// sprite, or damage what either software sprite covers.
pub fn show(
    old: &PointerLook,
    new: &PointerLook,
    x: i32,
    y: i32,
    hw_cursor: bool,
    pending_dirty: &mut Option<Rect>,
) {
    if hw_cursor {
        let (sprite, (hot_x, hot_y)) = new.hw_sprite();
        let _ = sys_gpu_cursor(0, sprite.as_ptr(), x as u32, y as u32, hot_x, hot_y);
        return;
    }
    damage(pending_dirty, old.rect(x, y).union(&new.rect(x, y)));
}

fn damage(pending_dirty: &mut Option<Rect>, area: Rect) {
    *pending_dirty = Some(match pending_dirty.take() {
        Some(previous) => previous.union(&area),
        None => area,
    });
}
