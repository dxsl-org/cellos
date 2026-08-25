//! Compositor-side input event handler.
//!
//! When the compositor registers as the input service's focus endpoint, all
//! dispatched `InputEvent`s arrive here prefixed with `INPUT_EVENT_OPCODE (0x10)`.
//!   - Forwards key events to the focused surface owner's cell.
//!   - Routes pointer events to the topmost or captured surface in local coordinates.
//!   - Tracks the logical mouse cursor position.
//!   - On mouse move, unions the old and new 16×16 cursor rects into
//!     `pending_dirty` so the compositor repaints both positions (no trail).

use api::display::Rect;
use api::input::{decode_event, InputEvent, INPUT_EVENT_IPC_SIZE, INPUT_EVENT_OPCODE};
use api::ipc::{InputRequest, IPC_BUF_SIZE};
use api::syscall::service;
use ostd::syscall::{sys_gpu_cursor, sys_lookup_service, sys_send, sys_try_send};

use crate::cursor_sprite::{CURSOR_H, CURSOR_W};
use crate::pointer_router::PointerRouter;
use crate::surface_table::SurfaceTable;
use crate::z_order::ZOrder;

/// Total IPC frame size for one input event (opcode byte + fixed payload).
const INPUT_FRAME_LEN: usize = 1 + INPUT_EVENT_IPC_SIZE;

/// Input routing and mouse position state owned by the compositor.
pub struct InputState {
    /// TID of the input service cell (0 = not yet connected).
    pub input_tid: usize,
    /// Registered TID for the compositor's input endpoint.
    compositor_tid: usize,
    /// Logical mouse cursor position (updated from MouseMove events).
    pub mouse_x: i32,
    pub mouse_y: i32,
    pointer: PointerRouter,
    /// True when the VirtIO GPU hardware cursor was successfully uploaded at
    /// startup.  When true, MouseMove issues `GpuCursor(move)` instead of
    /// triggering a full software repaint via `pending_dirty`.
    pub hw_cursor: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            input_tid: 0,
            compositor_tid: 0,
            mouse_x: 0,
            mouse_y: 0,
            pointer: PointerRouter::new(),
            hw_cursor: false,
        }
    }
}

/// Look up a service, yielding until it becomes available.
fn wait_for_service(id: u16) -> usize {
    loop {
        if let Some(tid) = sys_lookup_service(id) {
            return tid;
        }
        ostd::task::yield_now();
    }
}

/// Register the compositor as the input focus so that all events are routed here.
///
/// Blocks briefly at startup until both the input service and compositor's own
/// TID are registered in the service table (init does both before yielding).
pub fn connect_to_input(state: &mut InputState) {
    let input_tid = wait_for_service(service::INPUT);
    let compositor_tid = wait_for_service(service::COMPOSITOR);
    state.input_tid = input_tid;
    state.compositor_tid = compositor_tid;
    set_input_focus(input_tid, compositor_tid);
}

fn set_input_focus(input_tid: usize, compositor_tid: usize) {
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let req = InputRequest::SetFocus {
        cell_tid: compositor_tid as u32,
    };
    if let Ok(encoded) = api::ipc::encode(&req, &mut req_buf) {
        // Input can synchronously send a key to this compositor. Never wait for
        // its receive loop here, or click activation can form a two-way IPC wait.
        let _ = sys_try_send(input_tid, encoded);
    }
}

fn queue_dirty(pending_dirty: &mut Option<Rect>, dirty: Rect) {
    *pending_dirty = Some(match pending_dirty.take() {
        Some(previous) => previous.union(&dirty),
        None => dirty,
    });
}

/// Dispatch a raw IPC buffer received from the input service.
///
/// Only called when `sender == state.input_tid`. Pointer events update the
/// compositor cursor before being routed to the selected surface owner.
pub fn handle_input_event(
    buf: &[u8; 512],
    state: &mut InputState,
    table: &SurfaceTable,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    if buf[0] != INPUT_EVENT_OPCODE {
        return;
    }
    let Some(event) = decode_event(&buf[1..1 + INPUT_EVENT_IPC_SIZE]) else {
        return;
    };
    match event {
        InputEvent::Key(_) => forward_key(buf, state),
        InputEvent::MouseMove { x, y, .. } => {
            update_cursor(x, y, state, pending_dirty);
            route_pointer(event, state, table, z_order, pending_dirty);
        }
        InputEvent::MouseButton { .. } | InputEvent::MouseScroll { .. } => {
            route_pointer(event, state, table, z_order, pending_dirty);
        }
    }
}

fn route_pointer(
    event: InputEvent,
    state: &mut InputState,
    table: &SurfaceTable,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    let (input_tid, compositor_tid) = (state.input_tid, state.compositor_tid);
    state.pointer.route(
        event,
        state.mouse_x,
        state.mouse_y,
        table,
        z_order,
        |dirty| {
            queue_dirty(pending_dirty, dirty);
            set_input_focus(input_tid, compositor_tid);
        },
    );
}

/// Re-send the key-event frame to the focused surface owner.
fn forward_key(buf: &[u8; 512], state: &InputState) {
    let owner = state.pointer.focused_owner();
    if owner != 0 {
        let _ = sys_send(owner, &buf[..INPUT_FRAME_LEN]);
    }
}

/// Build a screen-space rect covering the cursor sprite at `(x, y)`.
#[inline]
fn cursor_rect(x: i32, y: i32) -> Rect {
    Rect {
        x,
        y,
        w: CURSOR_W,
        h: CURSOR_H,
    }
}

/// Update the logical cursor position and schedule its repaint.
///
/// When `state.hw_cursor` is true, the position is forwarded to the VirtIO GPU
/// hardware cursor. Otherwise, the old and new software cursor rectangles are
/// accumulated in `pending_dirty`.
fn update_cursor(x: i32, y: i32, state: &mut InputState, pending_dirty: &mut Option<Rect>) {
    let old_rect = cursor_rect(state.mouse_x, state.mouse_y);

    state.mouse_x = x;
    state.mouse_y = y;

    // One-line probe consumed by the Phase 04 integration test.
    ostd::io::println(&alloc::format!(
        "[compositor] cursor at {},{}",
        state.mouse_x,
        state.mouse_y
    ));

    if state.hw_cursor {
        // Hardware cursor: issue GpuCursor(move) — GPU scans out the cursor at
        // the new position without a full framebuffer repaint.
        let _ = sys_gpu_cursor(
            1,
            core::ptr::null(),
            state.mouse_x as u32,
            state.mouse_y as u32,
            0,
            0,
        );
    } else {
        // Software fallback: repaint the cursor area.
        let new_rect = cursor_rect(state.mouse_x, state.mouse_y);
        let combined = old_rect.union(&new_rect);
        *pending_dirty = Some(match pending_dirty.take() {
            Some(acc) => acc.union(&combined),
            None => combined,
        });
    }
}
