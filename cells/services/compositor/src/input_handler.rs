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
use ostd::syscall::{sys_lookup_service, sys_try_send};

use crate::cursor_state;
use crate::pointer_router::{PointerRouter, RouteContext};
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
    /// Set by a left press on a `Panel` (the single-owner desktop shell, e.g.
    /// a taskbar launcher) and cleared by the next pointer press or key: until
    /// then one newly created window may take focus from the panel, so an app
    /// the user just launched from the shell gets the keyboard.
    launch_armed: bool,
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
            launch_armed: false,
        }
    }

    pub const fn selected_cap(&self) -> Option<u64> {
        self.pointer.selected_cap()
    }

    pub fn remove_surface(&mut self, cap: u64) {
        self.pointer.forget(cap);
    }

    /// Remove a surface that remains allocated but must no longer receive input.
    pub fn deactivate_surface(&mut self, cap: u64) {
        self.pointer.disable(cap);
    }
    /// Give keyboard focus to a client's own surface on a client request
    /// (raise, view selection). A client may never take focus from another
    /// client: only a pointer press, which the compositor routes itself, moves
    /// focus between owners. Nobody focused means keys reach the console shell,
    /// so a surface may still take focus then; it is on top and the user sees
    /// where keys go.
    pub fn request_focus(&mut self, owner: usize, cap: u64) -> bool {
        let holder = self.pointer.focused_owner();
        if holder != 0 && holder != owner {
            return false;
        }
        self.pointer.set_focus(owner, Some(cap));
        true
    }

    /// Focus rule for a newly created window: [`Self::request_focus`], plus one
    /// window right after the user pressed on the desktop shell's panel (the
    /// launch gesture). The first new window after that press spends the grant.
    pub fn focus_new_window(&mut self, owner: usize, cap: u64) -> bool {
        let armed = core::mem::take(&mut self.launch_armed);
        if self.request_focus(owner, cap) {
            return true;
        }
        if !armed {
            return false;
        }
        self.pointer.set_focus(owner, Some(cap));
        true
    }

    /// Arm or clear the launch grant after a pointer press at the cursor.
    fn note_press(&mut self, left: bool, table: &SurfaceTable) {
        let (x, y) = (self.mouse_x, self.mouse_y);
        self.launch_armed = left
            && self
                .pointer
                .selected_cap()
                .and_then(|cap| table.get(cap))
                .is_some_and(|surface| {
                    surface.role == api::display::SurfaceRole::Panel
                        && surface.owner == self.pointer.focused_owner()
                        && surface.screen_rect().contains_point(x, y)
                });
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

/// Screen geometry used to clamp and route pointer events.
pub struct ScreenBounds {
    /// Physical screen width in pixels.
    pub width: u32,
    /// Physical screen height in pixels.
    pub height: u32,
    /// Area available to maximized windows.
    pub maximum: Rect,
}

/// Dispatch a raw IPC buffer received from the input service.
///
/// Only called when `sender == state.input_tid`. Pointer events update the
/// compositor cursor before being routed to the selected surface owner.
pub fn handle_input_event(
    buf: &[u8; 512],
    state: &mut InputState,
    screen: ScreenBounds,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    let ScreenBounds {
        width: screen_w,
        height: screen_h,
        maximum,
    } = screen;
    if buf[0] != INPUT_EVENT_OPCODE {
        return;
    }
    let Some(event) = decode_event(&buf[1..1 + INPUT_EVENT_IPC_SIZE]) else {
        return;
    };
    match event {
        InputEvent::Key(_) => forward_key(buf, state, table, screen_w, screen_h),
        InputEvent::MouseMove { x, y, dx, dy } => {
            // Mouse pointer can move across the ENTIRE physical screen (0..width-1, 0..height-1),
            // including into the bottom taskbar panel.
            let x = x.clamp(0, screen_w.saturating_sub(1).min(i32::MAX as u32) as i32);
            let y = y.clamp(0, screen_h.saturating_sub(1).min(i32::MAX as u32) as i32);
            let event = InputEvent::MouseMove { x, y, dx, dy };
            cursor_state::update(
                x,
                y,
                state.hw_cursor,
                &mut state.mouse_x,
                &mut state.mouse_y,
                pending_dirty,
            );
            route_pointer(event, state, maximum, table, z_order, pending_dirty);
        }
        InputEvent::MouseButton { .. } | InputEvent::MouseScroll { .. } => {
            route_pointer(event, state, maximum, table, z_order, pending_dirty);
        }
    }
}

fn route_pointer(
    event: InputEvent,
    state: &mut InputState,
    maximum: Rect,
    table: &mut SurfaceTable,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) {
    let (input_tid, compositor_tid) = (state.input_tid, state.compositor_tid);
    let old_decoration = state
        .pointer
        .selected_cap()
        .and_then(|cap| table.get(cap))
        .map(|surface| crate::window_decoration::bounds(surface.screen_rect()));
    state.pointer.route(
        event,
        state.mouse_x,
        state.mouse_y,
        RouteContext::new(maximum, table, z_order, |decoration| {
            queue_dirty(
                pending_dirty,
                old_decoration.map_or(decoration, |old| old.union(&decoration)),
            );
            set_input_focus(input_tid, compositor_tid);
        }),
    );
    if let InputEvent::MouseButton {
        button,
        state: api::input::KeyState::Pressed,
    } = event
    {
        state.note_press(button == api::input::MouseButton::Left, table);
    }
}

fn find_shell_tid() -> usize {
    let mut procs = [api::syscall::ProcessInfo::default(); 32];
    if let Ok(count) = ostd::syscall::sys_get_procs(&mut procs) {
        for p in procs.iter().take(count) {
            if &p.name[..7] == b"cellbox" {
                return p.id;
            }
        }
    }
    0
}

/// Re-send the key-event frame to the focused surface owner, or fallback to shell.
///
/// Keys follow the selected surface only while it still accepts input and is at
/// least partly on screen; otherwise focus is dropped, so a client cannot hold
/// the keyboard with a window moved out of sight.
fn forward_key(
    buf: &[u8; 512],
    state: &mut InputState,
    table: &SurfaceTable,
    screen_w: u32,
    screen_h: u32,
) {
    state.launch_armed = false;
    let owner = state.pointer.focused_owner();
    if owner != 0 {
        let visible = state
            .pointer
            .selected_cap()
            .and_then(|cap| table.get(cap))
            .is_some_and(|surface| {
                surface.owner == owner
                    && surface.accepts_input()
                    && on_screen(surface.screen_rect(), screen_w, screen_h)
            });
        if !visible {
            state.pointer.set_focus(0, None);
            return;
        }
    }
    let target = if owner != 0 { owner } else { find_shell_tid() };
    if target != 0 {
        let _ = sys_try_send(target, &buf[..INPUT_FRAME_LEN]);
    }
}

/// Whether any pixel of `rect` lies on the screen. Widened so client-chosen
/// positions near `i32::MAX` cannot overflow.
fn on_screen(rect: Rect, screen_w: u32, screen_h: u32) -> bool {
    let (x, y) = (i64::from(rect.x), i64::from(rect.y));
    x < i64::from(screen_w)
        && y < i64::from(screen_h)
        && x + i64::from(rect.w) > 0
        && y + i64::from(rect.h) > 0
}

#[cfg(test)]
mod tests {
    use api::display::SurfaceRole;

    use super::*;

    #[test]
    fn a_client_cannot_take_focus_from_another_client() {
        let mut input = InputState::new();
        assert!(input.request_focus(10, 1));
        assert!(!input.request_focus(20, 2));
        assert_eq!(input.selected_cap(), Some(1));
        // The holder may move focus among its own surfaces.
        assert!(input.request_focus(10, 3));
        assert_eq!(input.selected_cap(), Some(3));
    }

    #[test]
    fn only_one_new_window_after_a_panel_press_takes_focus() {
        let mut table = SurfaceTable::new();
        let panel = table
            .create(0, 0, 64, 16, 10, SurfaceRole::Panel)
            .expect("panel");
        let mut input = InputState::new();
        assert!(input.request_focus(10, panel));
        // Without a launch gesture a new window cannot take the panel's focus.
        assert!(!input.focus_new_window(20, 2));
        (input.mouse_x, input.mouse_y) = (4, 4);
        input.note_press(true, &table);
        assert!(input.focus_new_window(20, 2));
        assert_eq!(input.selected_cap(), Some(2));
        assert!(!input.focus_new_window(30, 3));
        assert_eq!(input.selected_cap(), Some(2));
    }

    #[test]
    fn keys_never_follow_a_focused_surface_moved_off_screen() {
        let mut table = SurfaceTable::new();
        let cap = table
            .create(0, 0, 8, 8, 10, SurfaceRole::Interactive)
            .expect("surface");
        let mut input = InputState::new();
        assert!(input.request_focus(10, cap));
        table.get_mut(cap).unwrap().move_to(i32::MAX, -20);

        forward_key(&[0; 512], &mut input, &table, 64, 64);

        assert_eq!(input.selected_cap(), None);
        assert_eq!(input.pointer.focused_owner(), 0);
    }
}
