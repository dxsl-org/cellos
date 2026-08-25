//! Surface-aware pointer routing for the compositor.

use api::input::{
    encode_event, InputEvent, KeyState, MouseButton, INPUT_EVENT_IPC_SIZE, INPUT_EVENT_OPCODE,
};
use ostd::syscall::sys_try_send;

use crate::surface_table::SurfaceTable;
use crate::z_order::ZOrder;

#[derive(Clone, Copy)]
struct PointerTarget {
    cap: u64,
    owner: usize,
    origin_x: i32,
    origin_y: i32,
}

/// Tracks keyboard focus and left-button capture across pointer events.
pub struct PointerRouter {
    focused_owner: usize,
    capture: Option<u64>,
}

impl PointerRouter {
    /// Create a router with no focused or captured surface.
    pub const fn new() -> Self {
        Self {
            focused_owner: 0,
            capture: None,
        }
    }

    /// Return the owner selected by the most recent left-button press.
    pub const fn focused_owner(&self) -> usize {
        self.focused_owner
    }

    /// Route one pointer event using the current screen-space cursor position.
    ///
    /// Moves follow the captured surface while the left button is held; otherwise
    /// they target the topmost surface under the cursor. Coordinates sent to the
    /// owner are translated into that surface's local coordinate system.
    pub fn route<F>(
        &mut self,
        event: InputEvent,
        screen_x: i32,
        screen_y: i32,
        table: &SurfaceTable,
        z_order: &mut ZOrder,
        mut activate: F,
    ) where
        F: FnMut(api::display::Rect),
    {
        match event {
            InputEvent::MouseMove { .. } => {
                let target = match self.capture {
                    Some(cap) => target_for_cap(cap, table),
                    None => hit_test(screen_x, screen_y, table, z_order),
                };
                if let Some(target) = target {
                    send_local(target, event);
                }
            }
            InputEvent::MouseButton { button, state } => {
                self.route_button(
                    button,
                    state,
                    event,
                    screen_x,
                    screen_y,
                    table,
                    z_order,
                    &mut activate,
                );
            }
            InputEvent::MouseScroll { .. } => {
                if let Some(target) = hit_test(screen_x, screen_y, table, z_order) {
                    send_position(target, screen_x, screen_y);
                    send_local(target, event);
                }
            }
            InputEvent::Key(_) => {}
        }
    }

    fn route_button<F>(
        &mut self,
        button: MouseButton,
        state: KeyState,
        event: InputEvent,
        screen_x: i32,
        screen_y: i32,
        table: &SurfaceTable,
        z_order: &mut ZOrder,
        activate: &mut F,
    ) where
        F: FnMut(api::display::Rect),
    {
        let left = button == MouseButton::Left;
        let target = if left && state == KeyState::Released {
            self.capture.and_then(|cap| target_for_cap(cap, table))
        } else {
            hit_test(screen_x, screen_y, table, z_order)
        };

        if left && state == KeyState::Pressed {
            self.capture = target.map(|target| target.cap);
            if let Some(target) = target {
                self.focused_owner = target.owner;
                z_order.raise(target.cap);
                if let Some(surface) = table.get(target.cap) {
                    activate(surface.screen_rect());
                }
            }
        }
        if let Some(target) = target {
            send_position(target, screen_x, screen_y);
            send_local(target, event);
        }
        if left && state == KeyState::Released {
            self.capture = None;
        }
    }
}

fn target_for_cap(cap: u64, table: &SurfaceTable) -> Option<PointerTarget> {
    table.get(cap).map(|surface| PointerTarget {
        cap,
        owner: surface.owner,
        origin_x: surface.x,
        origin_y: surface.y,
    })
}

fn hit_test(x: i32, y: i32, table: &SurfaceTable, z_order: &ZOrder) -> Option<PointerTarget> {
    z_order.iter_top_to_bottom().find_map(|cap| {
        let surface = table.get(cap)?;
        if surface.role != api::display::SurfaceRole::Interactive {
            return None;
        }
        let rect = surface.screen_rect();
        (x >= rect.x && x < rect.x + rect.w as i32 && y >= rect.y && y < rect.y + rect.h as i32)
            .then_some(PointerTarget {
                cap,
                owner: surface.owner,
                origin_x: surface.x,
                origin_y: surface.y,
            })
    })
}

fn send_position(target: PointerTarget, screen_x: i32, screen_y: i32) {
    send_local(
        target,
        InputEvent::MouseMove {
            x: screen_x,
            y: screen_y,
            dx: 0,
            dy: 0,
        },
    );
}

fn send_local(target: PointerTarget, event: InputEvent) {
    let event = match event {
        InputEvent::MouseMove { x, y, dx, dy } => InputEvent::MouseMove {
            x: x - target.origin_x,
            y: y - target.origin_y,
            dx,
            dy,
        },
        event => event,
    };
    let mut payload = [0u8; INPUT_EVENT_IPC_SIZE];
    encode_event(&event, &mut payload);
    let mut frame = [0u8; 1 + INPUT_EVENT_IPC_SIZE];
    frame[0] = INPUT_EVENT_OPCODE;
    frame[1..].copy_from_slice(&payload);
    let _ = sys_try_send(target.owner, &frame);
}
