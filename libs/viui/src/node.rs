// SPDX-License-Identifier: MIT
//! `ViNode` trait — v2 widget interface for the Reactive Signal Tree.
//!
//! Compared to v1 `ViWidget`:
//! - No `Msg` generic — callbacks live in widget structs as `Box<dyn Fn()>`
//! - No `WidgetStateStore` — state lives in `Signal<T>` fields
//! - `layout()` returns `Size` (containers recurse into children directly)
//! - `paint()` takes `&mut dyn ViCanvas` directly (no `PaintCx` wrapper)
//! - `event()` returns `bool` instead of `EventStatus`

extern crate alloc;
use alloc::vec::Vec;

use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Rect, Size};
use crate::render_ctx::RenderCtx;
use crate::signal::SubscriptionHandle;

/// v2 widget trait — Reactive Signal Tree node.
///
/// All implementations must cache their final `Rect` from `layout()` and
/// return it from `bounds()` — `paint()` and `event()` use it for positioning
/// and hit-testing respectively.
pub trait ViNode: 'static {
    /// Compute layout given available space; store the result as cached bounds.
    ///
    /// `constraints.origin` is the top-left of the assigned slot.
    /// Returns the actual size consumed (≤ `constraints.max`).
    fn layout(&mut self, constraints: Constraints) -> Size;

    /// Cached bounds from the last `layout()` call. Returns `Rect::ZERO` before
    /// first layout.
    fn bounds(&self) -> Rect;

    /// Paint into `cx` using the cached bounds.
    ///
    /// `cx` carries both the draw surface (`canvas`) and the scalable font
    /// state (`font`). Containers must forward `cx` to children via
    /// `cx.reborrow()` or by passing each child a disjoint sub-borrow.
    fn paint(&self, cx: &mut RenderCtx<'_>);

    /// Handle an input event. Returns `true` if consumed (stops bubbling to parent).
    fn event(&mut self, event: &Event) -> bool;

    /// Subscribe all internal `Signal`s to mark `region` dirty when they change.
    ///
    /// Called by `ViApp` after each layout pass. The returned handles keep
    /// subscriptions alive; `ViApp` stores them and drops them before the next
    /// layout so closures capture fresh bounds. Containers must recurse into
    /// children. Default: returns empty vec (widgets with no Signals).
    fn collect_dirty_handles(&mut self, _region: DirtyRegion) -> Vec<SubscriptionHandle> {
        Vec::new()
    }

    /// Returns true if this widget can receive keyboard focus.
    ///
    /// Leaf interactive widgets (Button, CheckBox, Slider, TextEdit) override
    /// this to return `true`. Containers and decorative widgets leave the default.
    fn is_focusable(&self) -> bool { false }

    /// Collect screen rects of all focusable descendants (and self, if focusable).
    ///
    /// Called by `ViApp` after each layout pass to build the Tab-order list.
    /// Containers must override to recurse into children. Default: returns `self.bounds()`
    /// when `is_focusable()` is true, otherwise empty.
    fn collect_focusable_bounds(&mut self) -> alloc::vec::Vec<crate::layout::Rect> {
        if self.is_focusable() {
            alloc::vec![self.bounds()]
        } else {
            alloc::vec::Vec::new()
        }
    }

    /// Activate the widget (called when Enter is pressed while this widget is focused).
    ///
    /// Returns `true` if the activation was consumed (triggers layout re-check).
    /// Button: fires `on_click`. CheckBox: toggles state. Default: no-op → false.
    fn activate(&mut self) -> bool { false }

    /// Walk the tree and activate the widget whose bounds exactly match `target`.
    ///
    /// Default impl handles leaf widgets: if focusable and bounds match, calls
    /// `activate()`. Containers must override to recurse into children.
    fn activate_at(&mut self, target: crate::layout::Rect) -> bool {
        if self.is_focusable() && self.bounds() == target {
            self.activate()
        } else {
            false
        }
    }
}
