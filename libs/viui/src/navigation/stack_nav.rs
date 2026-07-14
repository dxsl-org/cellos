// SPDX-License-Identifier: MIT
extern crate alloc;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::Cell;

use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::SubscriptionHandle;

use super::router::Router;

/// Slide direction used internally to know which prev/current offset to apply
/// once canvas transform support is added.
#[derive(Clone, Copy, PartialEq)]
pub enum SlideDir {
    Forward,
    Backward,
}

/// Stack navigator — displays one page at a time.
///
/// Call `push()` / `pop()` to navigate; the navigator swaps the active page.
/// Slide animation is scaffolded (`slide_t`, `tick_animation`) but painting
/// falls back to the current page only until canvas transform support lands.
///
/// Wrap `StackNavigator` as the root widget of your `ViApp`. Call `push()` and
/// `pop()` directly on the navigator (the inner `Router` is also pub for key
/// inspection via `router.current_key()`).
pub struct StackNavigator<K: Clone + PartialEq + 'static> {
    pub router: Router<K>,
    current: Box<dyn ViNode>,
    prev: Option<Box<dyn ViNode>>,
    /// Animation progress: 0.0 = transition start, 1.0 = complete.
    slide_t: f32,
    slide_dir: SlideDir,
    animating: bool,
    bounds: Cell<Rect>,
}

impl<K: Clone + PartialEq + 'static> StackNavigator<K> {
    /// Create a navigator with `initial_widget` displayed first.
    pub fn new(router: Router<K>, initial_widget: Box<dyn ViNode>) -> Self {
        Self {
            router,
            current: initial_widget,
            prev: None,
            slide_t: 1.0,
            slide_dir: SlideDir::Forward,
            animating: false,
            bounds: Cell::new(Rect::ZERO),
        }
    }

    /// Navigate forward (push). Returns `false` if the key is not registered.
    pub fn push(&mut self, key: K) -> bool {
        if let Some(new_widget) = self.router.push(key) {
            let old = core::mem::replace(&mut self.current, new_widget);
            self.prev = Some(old);
            self.slide_t = 0.0;
            self.slide_dir = SlideDir::Forward;
            self.animating = true;
            true
        } else {
            false
        }
    }

    /// Navigate back (pop). Returns `false` if already at root.
    pub fn pop(&mut self) -> bool {
        if let Some((_, new_widget)) = self.router.pop() {
            let old = core::mem::replace(&mut self.current, new_widget);
            self.prev = Some(old);
            self.slide_t = 0.0;
            self.slide_dir = SlideDir::Backward;
            self.animating = true;
            true
        } else {
            false
        }
    }

    pub fn can_pop(&self) -> bool {
        self.router.can_pop()
    }

    /// Advance slide animation by `dt_ms` milliseconds (200 ms total transition).
    ///
    /// Returns `true` while animating; returns `false` once complete and the
    /// previous page has been dropped. Call from the app's animation loop.
    pub fn tick_animation(&mut self, dt_ms: u32) -> bool {
        if !self.animating {
            return false;
        }
        self.slide_t += dt_ms as f32 / 200.0;
        if self.slide_t >= 1.0 {
            self.slide_t = 1.0;
            self.animating = false;
            self.prev = None;
        }
        self.animating
    }
}

impl<K: Clone + PartialEq + 'static> ViNode for StackNavigator<K> {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let size = self.current.layout(constraints);
        if let Some(prev) = &mut self.prev {
            prev.layout(constraints);
        }
        self.bounds
            .set(Rect::from_origin_size(constraints.origin, size));
        size
    }

    fn bounds(&self) -> Rect {
        self.bounds.get()
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        // Canvas has no transform support — paint current page only.
        // Slide animation visuals are deferred until push_transform lands on ViCanvas.
        self.current.paint(cx);
    }

    fn event(&mut self, event: &Event) -> bool {
        // Block input during transitions to avoid double-firing on both pages.
        if self.animating {
            return true;
        }
        self.current.event(event)
    }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        let mut handles = self.current.collect_dirty_handles(Rc::clone(&region));
        if let Some(prev) = &mut self.prev {
            handles.extend(prev.collect_dirty_handles(Rc::clone(&region)));
        }
        handles
    }

    fn is_focusable(&self) -> bool {
        false
    }

    fn collect_focusable_bounds(&mut self) -> Vec<Rect> {
        if self.animating {
            return Vec::new();
        }
        self.current.collect_focusable_bounds()
    }

    fn activate_at(&mut self, target: Rect) -> bool {
        self.current.activate_at(target)
    }
}
