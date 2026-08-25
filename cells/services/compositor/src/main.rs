#![no_std]
#![no_main]
// #[no_mangle] on main() requires removing global forbid — all submodules stay unsafe-free.

//! Compositor Service Cell.
//!
//! Manages a z-ordered set of cell surfaces, blends them into a screen
//! framebuffer, and flushes dirty regions to the VirtIO GPU via the
//! `GpuFlush` kernel syscall.
//!
//! Input routing: on startup the compositor registers as the input service's
//! focus endpoint. All `InputEvent` IPC frames are dispatched to
//! `input_handler`, which forwards keyboard events to the focused surface
//! owner and updates the cursor position on mouse move.

extern crate alloc;

mod cursor_sprite;
mod cursor_state;
mod framebuffer;
mod input_handler;
mod message_handler;
mod pointer_router;
mod render;
mod surface_table;
mod window_decoration;
mod z_order;

use api::display::Rect;
use framebuffer::ScreenFb;
use input_handler::{connect_to_input, handle_input_event, InputState};
use ostd::io::println;
use ostd::syscall::{sys_get_resolution, sys_get_time, sys_gpu_cursor, sys_recv, SyscallResult};
use render::render_frame;
use surface_table::SurfaceTable;
use z_order::ZOrder;

/// IPC opcode prefix byte identifying an input event from the input service.
/// Must match `input_handler::INPUT_EVENT_OPCODE` (0x10).
const INPUT_EVENT_OPCODE: u8 = 0x10;

/// Build a 64×64 BGRA8888 sprite for the VirtIO GPU hardware cursor.
///
/// Stamps the 16×16 software cursor into the top-left corner; all other pixels
/// are transparent (0x00_00_00_00). The 64×64 size is fixed by the VirtIO GPU
/// spec (`CURSOR_RECT` in virtio-drivers `gpu.rs:145-148`).
fn build_hw_cursor_sprite() -> [u8; 64 * 64 * 4] {
    let mut buf = [0u8; 64 * 64 * 4];
    for row in 0..cursor_sprite::CURSOR_H as usize {
        for col in 0..cursor_sprite::CURSOR_W as usize {
            if let Some(px) = cursor_sprite::cursor_pixel(row as u32, col as u32) {
                let off = (row * 64 + col) * 4;
                buf[off..off + 4].copy_from_slice(&px);
            }
        }
    }
    buf
}

#[no_mangle]
pub fn main() {
    println("[compositor] Compositor v0.2: software blending, VirtIO GPU, input routing");

    let (w, h) = framebuffer::default_screen_size();
    let mut fb = ScreenFb::new(w, h);
    let mut table = SurfaceTable::new();
    let mut z_order = ZOrder::new();
    let mut input = InputState::new();
    // Compositor-initiated repaint region: set by cursor moves, surface destroy/raise.
    // Consumed (taken) on each render_frame call.
    let mut pending_dirty: Option<Rect> = None;

    // Register as input focus so keyboard + mouse events flow to us.
    connect_to_input(&mut input);

    // Attempt to upload the hardware cursor sprite (64×64 BGRA8888).
    // The 16×16 software sprite is placed in the top-left; the rest is transparent.
    // If the GPU cursor is unavailable (setup_cursor returns error), we fall back to
    // the Phase 01 software cursor path for the session.
    {
        let (hot_x, hot_y) = cursor_sprite::hotspot();
        let sprite = build_hw_cursor_sprite();
        let ok = sys_gpu_cursor(0, sprite.as_ptr(), 0, 0, hot_x as u32, hot_y as u32).is_ok();
        input.hw_cursor = ok;
        if ok {
            println("[compositor] hardware cursor active");
        } else {
            println("[compositor] hardware cursor unavailable — using software cursor");
        }
    }

    let mut buf = [0u8; 512];
    // Resolution hotplug: check every 5 s; reconstruct ScreenFb on change.
    let mut last_res_check_ms: u64 = 0;

    loop {
        // The receive syscall only writes the payload; clear its reusable tail so
        // fixed-frame decoders never inspect bytes from an earlier sender.
        buf.fill(0);

        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(sender) if sender > 0 => {
                if input.input_tid != 0 && sender == input.input_tid && buf[0] == INPUT_EVENT_OPCODE
                {
                    // On MouseMove, update_cursor sets pending_dirty = union(old, new)
                    // so the frame is repainted at the interval tick.
                    handle_input_event(
                        &buf,
                        &mut input,
                        Rect {
                            x: window_decoration::FRAME,
                            y: window_decoration::FRAME + window_decoration::TITLE,
                            w: fb
                                .width
                                .saturating_sub((window_decoration::FRAME * 2) as u32),
                            h: fb.height.saturating_sub(
                                (window_decoration::FRAME * 2 + window_decoration::TITLE) as u32,
                            ),
                        },
                        &mut table,
                        &mut z_order,
                        &mut pending_dirty,
                    );
                } else {
                    message_handler::handle_message(
                        &buf,
                        sender,
                        fb.width,
                        fb.height,
                        &mut input,
                        &mut table,
                        &mut z_order,
                        &mut pending_dirty,
                    );
                }
            }
            _ => ostd::task::yield_now(),
        }

        // Damage-driven: render when a surface reports damage or cursor/compositor dirty.
        if table.has_damage() || pending_dirty.is_some() {
            let _ = render_frame(
                &mut fb,
                &mut table,
                &z_order,
                pending_dirty.take(),
                input.selected_cap(),
                input.mouse_x,
                input.mouse_y,
            );
        }

        // Display hotplug: poll GPU resolution every 5 s; if it changed, rebuild ScreenFb
        // and mark the full screen dirty so all surfaces are re-blended at the new size.
        let now_ms = sys_get_time();
        if now_ms.wrapping_sub(last_res_check_ms) >= 5_000 {
            last_res_check_ms = now_ms;
            let (new_w, new_h) = sys_get_resolution();
            if new_w != fb.width || new_h != fb.height {
                println("[compositor] resolution changed — rebuilding framebuffer");
                fb = ScreenFb::new(new_w, new_h);
                // Repaint entire screen so all surfaces are composited at the new size.
                pending_dirty = Some(Rect {
                    x: 0,
                    y: 0,
                    w: new_w,
                    h: new_h,
                });
            }
        }
    }
}
