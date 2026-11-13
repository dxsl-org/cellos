//! Pointer routing and compositor-owned window decoration gestures.

use wire::display::Rect;
use wire::input::InputEvent;

pub use stack_drag::{is_modifier_key, DragFeedback, DragMode};

use crate::stacking;
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::window_decoration::{self, Hit, ResizeEdge};
use crate::z_order::ZOrder;

mod actions;
mod control;
#[cfg(test)]
mod gesture_tests;
mod input;
mod resize;
mod stack_drag;
mod target;
#[cfg(test)]
mod tests;
mod window_menu;

/// Second press on the same title within this window toggles maximize.
const DOUBLE_CLICK_MS: u64 = 400;
/// ...and within this many pixels of the first press.
const DOUBLE_CLICK_SLOP: i32 = 4;

#[derive(Clone, Copy)]
pub(super) enum Capture {
    Client(u64),
    Drag {
        cap: u64,
        start_x: i32,
        start_y: i32,
        rect: Rect,
        mode: DragMode,
    },
    Resize {
        cap: u64,
        start_x: i32,
        start_y: i32,
        rect: Rect,
        edge: ResizeEdge,
    },
    /// A press inside the open window menu; the item is chosen on release.
    Menu,
}

pub(crate) struct RouteContext<'a, F> {
    maximum: Rect,
    screen: (u32, u32),
    now_ms: u64,
    table: &'a mut SurfaceTable,
    frames: &'a mut FrameRegistry,
    z_order: &'a mut ZOrder,
    damage: F,
}

impl<'a, F> RouteContext<'a, F> {
    pub(crate) fn new(
        maximum: Rect,
        screen: (u32, u32),
        now_ms: u64,
        table: &'a mut SurfaceTable,
        frames: &'a mut FrameRegistry,
        z_order: &'a mut ZOrder,
        damage: F,
    ) -> Self {
        Self {
            maximum,
            screen,
            now_ms,
            table,
            frames,
            z_order,
            damage,
        }
    }
}

/// The app icon under the pointer; `open` while its window menu shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuButtonHover {
    pub cap: u64,
    pub open: bool,
}

/// The open window menu of window `cap`. The window cannot move while it is
/// open (any press outside closes it), so `rect` stays valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenMenu {
    pub cap: u64,
    pub rect: Rect,
    /// What the menu offers, fixed when it opened.
    pub items: window_decoration::MenuItems,
    /// Index into `items` under the pointer.
    pub hover: Option<usize>,
}

/// Tracks keyboard focus, selected decoration, pointer capture, hover and the
/// window menu.
pub struct PointerRouter {
    pub(super) focused_owner: usize,
    pub(super) selected_cap: Option<u64>,
    pub(super) capture: Option<Capture>,
    pub(super) menu: Option<OpenMenu>,
    /// Last title press `(cap, ms, x, y)`, for double-click detection.
    pub(super) last_title_press: Option<(u64, u64, i32, i32)>,
    hover: Option<MenuButtonHover>,
    /// Screen area left stale by a menu closed without a damage sink
    /// (its window went away); taken by the main loop.
    stale: Option<Rect>,
    /// Modifier keys held, as of the last key event (Win/Shift tab drags).
    pub(super) modifiers: wire::input::Modifiers,
    /// Ghost and target of a Win+drag in progress.
    pub(super) feedback: Option<DragFeedback>,
}

impl PointerRouter {
    pub const fn new() -> Self {
        Self {
            focused_owner: 0,
            selected_cap: None,
            capture: None,
            menu: None,
            last_title_press: None,
            hover: None,
            stale: None,
            modifiers: wire::input::Modifiers(0),
            feedback: None,
        }
    }

    /// Track the modifier keys from a key event.
    pub fn note_key(&mut self, key: &wire::input::KeyEvent) {
        self.modifiers = stack_drag::modifiers_after(key);
    }

    /// Ghost and target of a Win+drag in progress, for painting.
    pub const fn drag_feedback(&self) -> Option<DragFeedback> {
        self.feedback
    }

    /// Esc: abandon a Win+drag in progress. Returns the area to repaint.
    pub fn cancel_stack_drag(&mut self) -> Option<Rect> {
        stack_drag::cancel(self)
    }

    pub const fn focused_owner(&self) -> usize {
        self.focused_owner
    }
    pub fn set_focus(&mut self, owner: usize, cap: Option<u64>) {
        self.focused_owner = owner;
        self.selected_cap = cap;
    }

    pub const fn selected_cap(&self) -> Option<u64> {
        self.selected_cap
    }

    pub const fn menu_button_hover(&self) -> Option<MenuButtonHover> {
        self.hover
    }

    pub const fn open_menu(&self) -> Option<OpenMenu> {
        self.menu
    }

    /// Close the window menu; returns the screen area to repaint.
    pub fn close_menu(&mut self) -> Option<Rect> {
        let menu = self.menu.take()?;
        if matches!(self.capture, Some(Capture::Menu)) {
            self.capture = None;
        }
        Some(window_decoration::menu_region(menu.rect))
    }

    /// Area left stale since the last call (see [`Self::forget`]).
    pub fn take_stale(&mut self) -> Option<Rect> {
        self.stale.take()
    }

    /// Recompute the app icon under `(x, y)` after an event was routed.
    /// Returns the screen area to repaint when the hovered icon or its menu
    /// state changed. Drags, resizes and client captures show no hover.
    pub fn update_hover(
        &mut self,
        x: i32,
        y: i32,
        table: &SurfaceTable,
        frames: &FrameRegistry,
        z_order: &ZOrder,
    ) -> Option<Rect> {
        let next = match self.capture {
            Some(Capture::Drag { .. } | Capture::Resize { .. } | Capture::Client(_)) => None,
            _ => target::at(x, y, table, frames, z_order)
                .filter(|target| target.hit == Hit::Menu)
                .map(|target| MenuButtonHover {
                    cap: target.cap,
                    open: self.menu.is_some_and(|menu| menu.cap == target.cap),
                }),
        };
        if next == self.hover {
            return None;
        }
        let area = |hover: MenuButtonHover| {
            table.get(hover.cap).map(|surface| {
                window_decoration::menu_button_rect(
                    surface.screen_rect(),
                    stacking::tabs_of(frames, hover.cap),
                )
            })
        };
        let damage = [self.hover, next]
            .into_iter()
            .flatten()
            .filter_map(area)
            .reduce(|a, b| a.union(&b));
        self.hover = next;
        damage
    }

    pub fn forget(&mut self, cap: u64) {
        if self.hover.is_some_and(|hover| hover.cap == cap) {
            self.hover = None;
        }
        if self.menu.is_some_and(|menu| menu.cap == cap) {
            self.stale = self.close_menu();
        }
        if self
            .last_title_press
            .is_some_and(|(pressed, ..)| pressed == cap)
        {
            self.last_title_press = None;
        }
        if self.selected_cap == Some(cap) {
            self.selected_cap = None;
            self.focused_owner = 0;
        }
        if capture_cap(self.capture) == Some(cap) {
            self.capture = None;
            if let Some(feedback) = self.feedback.take() {
                self.stale = Some(
                    self.stale
                        .map_or(feedback.area(), |s| s.union(&feedback.area())),
                );
            }
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
        | Some(Capture::Resize { cap, .. }) => Some(cap),
        Some(Capture::Menu) | None => None,
    }
}
