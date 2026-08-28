//! Pointer routing and compositor-owned window decoration gestures.

use api::display::Rect;
use api::input::InputEvent;

use crate::surface_table::SurfaceTable;
use crate::window_decoration::{Control, ResizeEdge};
use crate::z_order::ZOrder;

mod control;
mod input;
mod resize;
mod target;

#[derive(Clone, Copy)]
pub(super) enum Capture {
    Client(u64),
    Drag {
        cap: u64,
        start_x: i32,
        start_y: i32,
        rect: Rect,
    },
    Resize {
        cap: u64,
        start_x: i32,
        start_y: i32,
        rect: Rect,
        edge: ResizeEdge,
    },
    Control {
        cap: u64,
        control: Control,
    },
}

pub(crate) struct RouteContext<'a, F> {
    maximum: Rect,
    table: &'a mut SurfaceTable,
    z_order: &'a mut ZOrder,
    damage: F,
}

impl<'a, F> RouteContext<'a, F> {
    pub(crate) fn new(
        maximum: Rect,
        table: &'a mut SurfaceTable,
        z_order: &'a mut ZOrder,
        damage: F,
    ) -> Self {
        Self {
            maximum,
            table,
            z_order,
            damage,
        }
    }
}

/// Tracks keyboard focus, selected decoration, and pointer capture.
pub struct PointerRouter {
    pub(super) focused_owner: usize,
    pub(super) selected_cap: Option<u64>,
    pub(super) capture: Option<Capture>,
}

impl PointerRouter {
    pub const fn new() -> Self {
        Self {
            focused_owner: 0,
            selected_cap: None,
            capture: None,
        }
    }

    pub const fn focused_owner(&self) -> usize {
        self.focused_owner
    }

    pub const fn selected_cap(&self) -> Option<u64> {
        self.selected_cap
    }

    pub fn forget(&mut self, cap: u64) {
        if self.selected_cap == Some(cap) {
            self.selected_cap = None;
            self.focused_owner = 0;
        }
        if capture_cap(self.capture) == Some(cap) {
            self.capture = None;
        }
    }

    pub fn disable(&mut self, cap: u64) {
        self.forget(cap);
        self.focused_owner = 0;
    }

    pub(crate) fn route<F>(
        &mut self,
        event: InputEvent,
        x: i32,
        y: i32,
        mut context: RouteContext<'_, F>,
    ) where
        F: FnMut(Rect),
    {
        input::route(self, event, x, y, &mut context);
    }
}

fn capture_cap(capture: Option<Capture>) -> Option<u64> {
    match capture {
        Some(Capture::Client(cap))
        | Some(Capture::Drag { cap, .. })
        | Some(Capture::Resize { cap, .. })
        | Some(Capture::Control { cap, .. }) => Some(cap),
        None => None,
    }
}
