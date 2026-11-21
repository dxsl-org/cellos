#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
// The root cannot forbid unsafe_code: `main` is #[no_mangle], and
// surface_table/pixels.rs blits from a leased client Grant through a raw slice.

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

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

mod appearance;
mod cursor_sprite;
mod cursor_state;
mod framebuffer;
mod input_handler;
mod message_handler;
mod owner_watch;
mod pointer_router;
mod render;
mod stacking;
mod surface_table;
mod tiling;
mod window_decoration;
mod z_order;

use framebuffer::ScreenFb;
use input_handler::{connect_to_input, handle_input_event, InputState, ScreenBounds};
use ostd::io::println;
use ostd::service::{serve, Handler, Owner, Request, ServeConfig, Wait, WatchPolicy};
use ostd::syscall::{sys_get_resolution, sys_get_time, sys_gpu_cursor};
use render::render_frame;
use surface_table::SurfaceTable;
use wire::display::Rect;
use wire::proto::input::INPUT_EVENT_OPCODE;
use z_order::ZOrder;

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

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(10 * 1024 * 1024);

#[cfg(not(test))]
#[cfg(target_os = "none")]
#[no_mangle]
pub fn main() {
    init_custom_heap();
    println("[compositor] Compositor v0.2: software blending, VirtIO GPU, input routing");
    let (w, h) = framebuffer::default_screen_size();
    let mut compositor = Compositor {
        fb: ScreenFb::new(w, h),
        table: SurfaceTable::new(),
        frames: crate::surface_table::FrameRegistry::new(),
        z_order: ZOrder::new(),
        input: InputState::new(),
        appearance: appearance::Appearance::new(),
        pending_dirty: None,
        last_res_check_ms: 0,
        owners: BTreeMap::new(),
    };

    // Register as input focus so keyboard + mouse events flow to us.
    connect_to_input(&mut compositor.input);

    // Attempt to upload the hardware cursor sprite (64×64 BGRA8888).
    // The 16×16 software sprite is placed in the top-left; the rest is transparent.
    // If the GPU cursor is unavailable (setup_cursor returns error), we fall back to
    // the Phase 01 software cursor path for the session.
    {
        let (hot_x, hot_y) = cursor_sprite::hotspot();
        let sprite = build_hw_cursor_sprite();
        let ok = sys_gpu_cursor(0, sprite.as_ptr(), 0, 0, hot_x as u32, hot_y as u32).is_ok();
        compositor.input.hw_cursor = ok;
        if ok {
            println("[compositor] hardware cursor active");
        } else {
            println("[compositor] hardware cursor unavailable — using software cursor");
        }
    }

    let config = ServeConfig::with_heartbeat().watching(
        WatchPolicy::OnRequest(|payload| owner_watch::creates_owned_state(payload[0])),
        owner_watch::MAX_WATCHED_CELLS,
    );
    serve(config, &mut compositor);
}

/// The compositor's state between receives, served by [`serve`].
#[cfg(not(test))]
#[cfg(target_os = "none")]
struct Compositor {
    fb: ScreenFb,
    table: SurfaceTable,
    frames: surface_table::FrameRegistry,
    z_order: ZOrder,
    input: InputState,
    appearance: appearance::Appearance,
    // Compositor-initiated repaint region: set by cursor moves, surface destroy/raise.
    // Consumed (taken) on each render_frame call.
    pending_dirty: Option<Rect>,
    // Resolution hotplug: check every 5 s; reconstruct ScreenFb on change.
    last_res_check_ms: u64,
    /// Surfaces and views are keyed by the creating **thread** tid, while a death
    /// names the Cell incarnation: the threads that created state for each owner.
    owners: BTreeMap<Owner, Vec<usize>>,
}

#[cfg(not(test))]
#[cfg(target_os = "none")]
impl Compositor {
    /// Remember that `tid` creates state on behalf of `owner`, so its death
    /// reclaims it. Mirrors what `serve` can watch: never the kernel's own
    /// identity, at most [`owner_watch::MAX_WATCHED_CELLS`] owners and
    /// [`owner_watch::MAX_TIDS_PER_CELL`] tids each; beyond that the state is
    /// served but not reclaimed on death.
    fn record_creator(&mut self, owner: Owner, tid: usize) {
        if owner.cell_id == 0 || owner.generation == 0 {
            return;
        }
        if !self.owners.contains_key(&owner) && self.owners.len() >= owner_watch::MAX_WATCHED_CELLS
        {
            return;
        }
        let tids = self.owners.entry(owner).or_default();
        if !tids.contains(&tid) && tids.len() < owner_watch::MAX_TIDS_PER_CELL {
            tids.push(tid);
        }
    }
}

#[cfg(not(test))]
#[cfg(target_os = "none")]
impl Handler for Compositor {
    fn on_request(&mut self, request: Request<'_>) {
        let sender = request.sender();
        let Some(buf) = request.payload().first_chunk::<512>() else {
            return;
        };
        if self.input.input_tid != 0
            && sender == self.input.input_tid
            && buf[0] == INPUT_EVENT_OPCODE
        {
            // On MouseMove, update_cursor sets pending_dirty = union(old, new)
            // so the frame is repainted at the interval tick.
            handle_input_event(
                buf,
                &mut self.input,
                ScreenBounds {
                    width: self.fb.width,
                    height: self.fb.height,
                    maximum: window_decoration::maximized_content(self.fb.width, self.fb.height),
                    now_ms: sys_get_time(),
                },
                &mut self.table,
                &mut self.frames,
                &mut self.z_order,
                &mut self.pending_dirty,
            );
            return;
        }
        // `serve` already watched the owner: the kernel grants the watch only
        // while this message is the one being served.
        if owner_watch::creates_owned_state(buf[0]) && request.owner_watched() {
            self.record_creator(request.owner(), sender);
        }
        message_handler::handle_message(
            buf,
            sender,
            message_handler::MessageContext {
                screen_w: self.fb.width,
                screen_h: self.fb.height,
                input: &mut self.input,
                table: &mut self.table,
                frames: &mut self.frames,
                z_order: &mut self.z_order,
                pending_dirty: &mut self.pending_dirty,
                appearance: &mut self.appearance,
            },
        );
    }

    fn on_owner_death(&mut self, owner: Owner) {
        let dead = owner_watch::Released {
            cell_id: owner.cell_id,
            generation: owner.generation,
            tids: self.owners.remove(&owner).unwrap_or_default(),
        };
        let removed = owner_watch::reclaim(
            &dead,
            &mut self.input,
            &mut self.table,
            &mut self.frames,
            &mut self.z_order,
            &mut self.pending_dirty,
        );
        if let Some(line) = owner_watch::death_line(&dead, removed) {
            println(&line);
        }
    }

    fn on_turn(&mut self) {
        if let Some(stale) = self.input.take_stale() {
            self.pending_dirty = Some(
                self.pending_dirty
                    .map_or(stale, |dirty| dirty.union(&stale)),
            );
        }
        // Damage-driven: render when a surface reports damage or cursor/compositor dirty.
        if self.table.has_damage() || self.pending_dirty.is_some() {
            let _ = render_frame(
                &mut self.fb,
                &mut self.table,
                &self.frames,
                &self.z_order,
                self.pending_dirty.take(),
                &render::ChromeState {
                    selected: self.input.selected_cap(),
                    menu_button: self.input.menu_button_hover(),
                    menu: self.input.open_menu(),
                    drag: self.input.drag_feedback(),
                    palette: self.appearance.palette(),
                },
                self.input.mouse_x,
                self.input.mouse_y,
            );
        }

        // Display hotplug: poll GPU resolution every 5 s; if it changed, rebuild ScreenFb
        // and mark the full screen dirty so all surfaces are re-blended at the new size.
        let now_ms = sys_get_time();
        if now_ms.wrapping_sub(self.last_res_check_ms) >= 5_000 {
            self.last_res_check_ms = now_ms;
            let (new_w, new_h) = sys_get_resolution();
            if new_w != self.fb.width || new_h != self.fb.height {
                println("[compositor] resolution changed — rebuilding framebuffer");
                self.fb = ScreenFb::new(new_w, new_h);
                // Repaint entire screen so all surfaces are composited at the new size.
                self.pending_dirty = Some(Rect {
                    x: 0,
                    y: 0,
                    w: new_w,
                    h: new_h,
                });
            }
        }
    }

    /// Wake at least every 5 s (500 ticks of 10 ms) so the resolution check in
    /// [`Self::on_turn`] also runs while no client is talking.
    fn on_idle(&mut self) -> Wait {
        Wait::Ticks(500)
    }
}
