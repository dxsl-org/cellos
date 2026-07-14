// SPDX-License-Identifier: MIT
//! ListView — scrollable list driven by `Signal<Vec<String>>`.
//!
//! ## Scroll behaviour
//! Mouse-wheel and touch-scroll update `scroll_offset`.  A 4px scrollbar thumb
//! is drawn when content exceeds the visible height.  Touch fling inertia decays
//! the scroll velocity at 0.85× per event until it falls below 0.5 px/event.
//!
//! ## Selection
//! Mouse-click or touch-begin selects the item under the pointer and fires
//! `on_select(idx)`.  `selected` is a readable/writable `Signal<Option<usize>>`
//! so callers can also set the selection programmatically.
//!
//! ## Variable item heights
//! Call `.item_heights(signal)` to enable variable-height mode.  In this mode,
//! scroll math uses a prefix-sum array; `item_at()` does a binary search.
//! When the signal is absent the fixed-height code path is unchanged.

extern crate alloc;
use alloc::{boxed::Box, vec::Vec};
use core::cell::Cell;

use crate::dirty::DirtyRegion;
use crate::event::{Event, MouseButton};
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::{Signal, SubscriptionHandle};

const DEFAULT_ITEM_H: f32 = 28.0;
const SCROLL_SPEED: f32 = 3.0;

// ── Private free-function helpers ──────────────────────────────────────────

/// Build a prefix-sum array from per-row heights.
///
/// Returns a vec of length `heights.len() + 1` where `result[i]` is the y
/// offset of row `i` (so `result[0] == 0.0` always).
fn prefix_sum(heights: &[f32]) -> alloc::vec::Vec<f32> {
    let mut acc = 0.0_f32;
    let mut result = alloc::vec::Vec::with_capacity(heights.len() + 1);
    result.push(0.0_f32);
    for &h in heights {
        acc += h;
        result.push(acc);
    }
    result
}

/// Return the row index whose vertical range contains `offset`.
///
/// Uses `partition_point` (binary search) — O(log n).
/// Precondition: `prefix` is sorted ascending (guaranteed by `prefix_sum`).
fn row_at_offset(prefix: &[f32], offset: f32) -> usize {
    let pos = prefix.partition_point(|&p| p <= offset);
    if pos == 0 {
        0
    } else {
        pos - 1
    }
}

// ── Widget ─────────────────────────────────────────────────────────────────

/// Scrollable list of string items driven by a `Signal<Vec<String>>`.
///
/// # Invariants
/// - `scroll_offset` is always clamped to `[0.0, max_scroll()]`.
/// - `bounds_cache` holds the last rect from `layout()`; `Rect::ZERO` before first layout.
/// - `item_heights` == `None` ⟹ fixed-height mode (unchanged G1 behaviour).
/// - `hovered_index` is set by `MouseMove`; clears to `None` when pointer leaves bounds.
pub struct ListView {
    /// The list of items to display.
    pub items: Signal<Vec<alloc::string::String>>,
    /// Currently selected item index, or `None`.
    pub selected: Signal<Option<usize>>,
    on_select: Option<Box<dyn Fn(usize)>>,
    item_height: f32,
    scroll_offset: Cell<f32>,
    bounds_cache: Cell<Rect>,
    /// Optional per-row height map.  `None` = fixed-height mode (default).
    item_heights: Option<Signal<Vec<f32>>>,
    /// Row index currently under the mouse pointer, for hover highlight.
    hovered_index: Cell<Option<usize>>,
    /// Current fling velocity in pixels-per-event; decays at 0.85× each event.
    touch_velocity: Cell<f32>,
    /// Y coordinate of the previous `TouchMove` event; used for delta/velocity calc.
    last_touch_y: Cell<f32>,
}

impl ListView {
    /// Create a new `ListView` driven by the given items signal.
    pub fn new(items: Signal<Vec<alloc::string::String>>) -> Self {
        Self {
            items,
            selected: Signal::new(None),
            on_select: None,
            item_height: DEFAULT_ITEM_H,
            scroll_offset: Cell::new(0.0),
            bounds_cache: Cell::new(Rect::ZERO),
            item_heights: None,
            hovered_index: Cell::new(None),
            touch_velocity: Cell::new(0.0),
            last_touch_y: Cell::new(0.0),
        }
    }

    /// Override the per-item height in pixels (default: 28).  Only applies in
    /// fixed-height mode (no `item_heights` signal provided).
    pub fn item_height(mut self, h: f32) -> Self {
        self.item_height = h;
        self
    }

    /// Enable variable-height mode by supplying a signal that maps each row
    /// index to its height in pixels.  The vec length should equal
    /// `items.get().len()`.  Fixed-height mode is used when this is absent.
    pub fn item_heights(mut self, h: Signal<Vec<f32>>) -> Self {
        self.item_heights = Some(h);
        self
    }

    /// Register a callback fired with the selected item index on each selection change.
    pub fn on_select(mut self, f: impl Fn(usize) + 'static) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    /// Replace the selection signal (allows external control of selected item).
    pub fn with_selected(mut self, sel: Signal<Option<usize>>) -> Self {
        self.selected = sel;
        self
    }

    /// Maximum valid scroll offset given current bounds and item count.
    fn max_scroll(&self) -> f32 {
        let b = self.bounds_cache.get();
        match &self.item_heights {
            None => {
                let content_h = self.items.get().len() as f32 * self.item_height;
                (content_h - b.h).max(0.0)
            }
            Some(heights_sig) => {
                let heights = heights_sig.get();
                let total_h: f32 = heights.iter().sum();
                (total_h - b.h).max(0.0)
            }
        }
    }

    /// Return the item index at screen position `pos`, or `None` if outside bounds.
    fn item_at(&self, pos: Point) -> Option<usize> {
        let b = self.bounds_cache.get();
        if !b.contains(pos) {
            return None;
        }
        let rel_y = pos.y - b.y + self.scroll_offset.get();
        match &self.item_heights {
            None => {
                let idx = (rel_y / self.item_height) as usize;
                let len = self.items.get().len();
                if idx < len {
                    Some(idx)
                } else {
                    None
                }
            }
            Some(heights_sig) => {
                let heights = heights_sig.get();
                let prefix = prefix_sum(&heights);
                let idx = row_at_offset(&prefix, rel_y);
                if idx < self.items.get().len() {
                    Some(idx)
                } else {
                    None
                }
            }
        }
    }
}

impl ViNode for ListView {
    fn layout(&mut self, constraints: Constraints) -> Size {
        // Constrain height to a reasonable maximum; full width.
        let desired_h = constraints.max.h.min(200.0);
        let size = constraints.constrain(Size {
            w: constraints.max.w,
            h: desired_h,
        });
        self.bounds_cache
            .set(Rect::from_origin_size(constraints.origin, size));
        size
    }

    fn bounds(&self) -> Rect {
        self.bounds_cache.get()
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let b = self.bounds_cache.get();
        let scroll = self.scroll_offset.get();

        // Background
        cx.canvas.fill_rect(b, cx.theme.bg());

        cx.canvas.clip_push(b);

        {
            let items = self.items.get();
            let sel = *self.selected.get();
            let hovered = self.hovered_index.get();

            match &self.item_heights {
                // ── Fixed-height path (unchanged G1 behaviour) ──────────────
                None => {
                    // Only render the visible row range.
                    // Adding 2 ensures partial items at top/bottom are drawn.
                    let first = (scroll / self.item_height) as usize;
                    let visible_rows = (b.h / self.item_height) as usize + 2;
                    let last = (first + visible_rows).min(items.len());

                    for i in first..last {
                        let item_y = b.y + i as f32 * self.item_height - scroll;
                        let item_rect = Rect {
                            x: b.x,
                            y: item_y,
                            w: b.w,
                            h: self.item_height,
                        };

                        if sel == Some(i) {
                            cx.canvas.fill_rect(item_rect, cx.theme.list_selected_bg());
                        } else if hovered == Some(i) {
                            cx.canvas.fill_rect(item_rect, cx.theme.surface());
                        }

                        if let Some(text) = items.get(i) {
                            let line_h = cx.line_height();
                            let ty = (item_y + (self.item_height - line_h) * 0.5).max(b.y);
                            let text_color = if sel == Some(i) {
                                cx.theme.list_selected_fg()
                            } else {
                                cx.theme.text_primary()
                            };
                            cx.draw_text(Point::new(b.x + 6.0, ty), text, text_color);
                        }
                    }
                }

                // ── Variable-height path ────────────────────────────────────
                Some(heights_sig) => {
                    let heights = heights_sig.get();
                    let prefix = prefix_sum(&heights);

                    // First visible row: last row whose top edge is <= scroll.
                    let first = row_at_offset(&prefix, scroll);
                    // Last visible row: first row whose top edge >= scroll + b.h.
                    let last = {
                        let bottom = scroll + b.h;
                        let mut end = first;
                        while end < items.len() && end + 1 < prefix.len() && prefix[end] < bottom {
                            end += 1;
                        }
                        end.min(items.len())
                    };

                    for i in first..last {
                        if i + 1 >= prefix.len() {
                            break;
                        }
                        let item_y = b.y + prefix[i] - scroll;
                        let item_h = heights.get(i).copied().unwrap_or(DEFAULT_ITEM_H);
                        let item_rect = Rect {
                            x: b.x,
                            y: item_y,
                            w: b.w,
                            h: item_h,
                        };

                        if sel == Some(i) {
                            cx.canvas.fill_rect(item_rect, cx.theme.list_selected_bg());
                        } else if hovered == Some(i) {
                            cx.canvas.fill_rect(item_rect, cx.theme.surface());
                        }

                        if let Some(text) = items.get(i) {
                            let line_h = cx.line_height();
                            let ty = (item_y + (item_h - line_h) * 0.5).max(b.y);
                            let text_color = if sel == Some(i) {
                                cx.theme.list_selected_fg()
                            } else {
                                cx.theme.text_primary()
                            };
                            cx.draw_text(Point::new(b.x + 6.0, ty), text, text_color);
                        }
                    }
                }
            }
        }

        cx.canvas.clip_pop();

        // Draw scrollbar when content overflows.  Use total content height from
        // whichever mode is active.
        let content_h = match &self.item_heights {
            None => self.items.get().len() as f32 * self.item_height,
            Some(heights_sig) => heights_sig.get().iter().sum(),
        };
        if content_h > b.h {
            let bar_w = 4.0_f32;
            let bar_x = b.x + b.w - bar_w;
            let thumb_h = (b.h / content_h * b.h).max(20.0);
            let scroll_range = content_h - b.h;
            let thumb_y = if scroll_range > 0.0 {
                b.y + (scroll / scroll_range) * (b.h - thumb_h)
            } else {
                b.y
            };
            let thumb_y = thumb_y.min(b.y + b.h - thumb_h);

            cx.canvas.fill_rect(
                Rect {
                    x: bar_x,
                    y: b.y,
                    w: bar_w,
                    h: b.h,
                },
                cx.theme.surface(),
            );
            cx.canvas.fill_rect(
                Rect {
                    x: bar_x,
                    y: thumb_y,
                    w: bar_w,
                    h: thumb_h,
                },
                cx.theme.border(),
            );
        }
    }

    fn event(&mut self, event: &Event) -> bool {
        // Apply touch fling decay at the start of every event call.
        // This approximates per-frame decay without requiring a timer.
        let v = self.touch_velocity.get();
        if v.abs() > 0.5 {
            let new_off = (self.scroll_offset.get() + v).clamp(0.0, self.max_scroll());
            self.scroll_offset.set(new_off);
            self.touch_velocity.set((v * 0.85).clamp(-50.0, 50.0));
        }

        let b = self.bounds_cache.get();
        match event {
            Event::Scroll { pos, delta_y } if b.contains(*pos) => {
                let new_off = (self.scroll_offset.get() - delta_y * SCROLL_SPEED)
                    .clamp(0.0, self.max_scroll());
                self.scroll_offset.set(new_off);
                true
            }
            Event::MouseMove { pos } => {
                // Update hover index; returns None automatically when pointer is outside bounds.
                // Do NOT consume — parent may also need this event.
                self.hovered_index.set(self.item_at(*pos));
                false
            }
            Event::MousePress {
                pos,
                button: MouseButton::Left,
            } => {
                if let Some(idx) = self.item_at(*pos) {
                    self.selected.set(Some(idx));
                    if let Some(cb) = &self.on_select {
                        cb(idx);
                    }
                    true
                } else {
                    false
                }
            }
            Event::TouchBegin { pos, .. } => {
                // Reset fling state on a new touch contact.
                self.touch_velocity.set(0.0);
                self.last_touch_y.set(pos.y);

                if let Some(idx) = self.item_at(*pos) {
                    self.selected.set(Some(idx));
                    if let Some(cb) = &self.on_select {
                        cb(idx);
                    }
                    true
                } else {
                    false
                }
            }
            Event::TouchMove { pos, finger_id: 0 } => {
                let last_y = self.last_touch_y.get();
                let delta = pos.y - last_y;
                self.last_touch_y.set(pos.y);

                // Exponential moving average keeps velocity smooth across jittery input.
                // Only update velocity after the first sample (last_y != 0.0).
                if last_y != 0.0 {
                    let new_v = (-delta * 0.6 + self.touch_velocity.get() * 0.4).clamp(-50.0, 50.0);
                    self.touch_velocity.set(new_v);
                }

                let new_off = (self.scroll_offset.get() - delta).clamp(0.0, self.max_scroll());
                self.scroll_offset.set(new_off);
                true
            }
            _ => false,
        }
    }

    /// Subscribe items + selected + (optionally) heights signals; mark bounds dirty on any change.
    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        // Clone region *before* any move into closures so each subscription gets
        // its own independent reference.
        let bounds = self.bounds_cache.get();
        let r1 = region.clone();
        let r2 = region.clone();

        let h1 = self.items.subscribe(move || {
            r1.borrow_mut().mark(bounds);
        });
        let h2 = self.selected.subscribe(move || {
            r2.borrow_mut().mark(bounds);
        });
        let mut handles = alloc::vec![h1, h2];

        if let Some(heights_sig) = &self.item_heights {
            let r3 = region.clone();
            let h3 = heights_sig.subscribe(move || {
                r3.borrow_mut().mark(bounds);
            });
            handles.push(h3);
        }

        handles
    }
}

// ── VirtualListView ────────────────────────────────────────────────────────

/// Data source for [`VirtualListView`].
///
/// Implementors supply items by index under a **fixed item height** contract.
/// Fixed height is required for O(1) scroll-offset-to-first-visible computation.
/// For variable-height items, use the non-virtual [`ListView`].
pub trait ListDataProvider: 'static {
    /// Total number of items in the list.
    fn item_count(&self) -> usize;

    /// Fixed pixel height of every item.
    fn item_height(&self) -> f32;

    /// Build the widget for the item at `index`.
    ///
    /// Called when a slot is (re)bound to a new data index.
    /// Precondition: `index < item_count()`.
    fn build_item(&self, index: usize) -> Box<dyn crate::node::ViNode>;
}

/// [`ListDataProvider`] backed by a `Signal<Vec<T>>` and a builder closure.
///
/// The builder is called with a reference to each item and its list index.
/// Cloning the signal is cheap — all clones share the same underlying cell.
pub struct VecProvider<T: Clone + 'static> {
    items: Signal<Vec<T>>,
    height: f32,
    builder: Box<dyn Fn(&T, usize) -> Box<dyn crate::node::ViNode>>,
}

impl<T: Clone + 'static> VecProvider<T> {
    pub fn new(
        items: Signal<Vec<T>>,
        item_height: f32,
        builder: impl Fn(&T, usize) -> Box<dyn crate::node::ViNode> + 'static,
    ) -> Self {
        Self {
            items,
            height: item_height,
            builder: Box::new(builder),
        }
    }
}

impl<T: Clone + 'static> ListDataProvider for VecProvider<T> {
    fn item_count(&self) -> usize {
        self.items.get().len()
    }
    fn item_height(&self) -> f32 {
        self.height
    }
    fn build_item(&self, index: usize) -> Box<dyn crate::node::ViNode> {
        let items = self.items.get();
        (self.builder)(&items[index], index)
    }
}

/// One recycled display slot inside [`VirtualListView`].
struct VirtualSlot {
    /// The widget currently occupying this slot.
    widget: Box<dyn crate::node::ViNode>,
    /// Which data index the widget is currently rendering, or `None` if empty.
    bound_idx: Option<usize>,
}

/// Virtual scrolling list — allocates only O(visible) slots regardless of item count.
///
/// Requires a fixed item height (see [`ListDataProvider::item_height`]).
/// Supports 10 000+ items with O(visible) layout and paint cost.
///
/// # Slot recycling
/// Slots are created once during the first layout pass and rebound (widget rebuilt)
/// only when a slot scrolls to a new data index.  Scrolling by less than one item
/// height leaves all slot–index bindings unchanged.
///
/// # Scrollbar
/// A 6 px thumb-and-track scrollbar is drawn on the right edge when total content
/// height exceeds the viewport height.
pub struct VirtualListView {
    provider: Box<dyn ListDataProvider>,
    scroll_y: f32,
    scroll_vel: f32,
    last_touch_y: f32,
    slot_count: usize,
    slots: Vec<VirtualSlot>,
    selected: Signal<Option<usize>>,
    on_select: Option<Box<dyn Fn(usize)>>,
    bounds_cache: Cell<Rect>,
}

impl VirtualListView {
    /// Create a new virtual list driven by `provider`.
    pub fn new(provider: Box<dyn ListDataProvider>) -> Self {
        Self {
            provider,
            scroll_y: 0.0,
            scroll_vel: 0.0,
            last_touch_y: 0.0,
            slot_count: 0,
            slots: Vec::new(),
            selected: Signal::new(None),
            on_select: None,
            bounds_cache: Cell::new(Rect::ZERO),
        }
    }

    /// Register a selection callback fired with the clicked item index.
    pub fn on_select(mut self, f: impl Fn(usize) + 'static) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    /// Clone of the internal `selected` signal — subscribe externally to react.
    pub fn selected_signal(&self) -> Signal<Option<usize>> {
        self.selected.clone()
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    fn first_visible_idx(&self) -> usize {
        let h = self.provider.item_height();
        if h <= 0.0 {
            return 0;
        }
        (self.scroll_y / h) as usize
    }

    fn total_content_height(&self) -> f32 {
        self.provider.item_count() as f32 * self.provider.item_height()
    }

    fn max_scroll(&self) -> f32 {
        let b = self.bounds_cache.get();
        (self.total_content_height() - b.h).max(0.0)
    }

    fn clamp_scroll(&mut self) {
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
    }

    /// Ensure `slots` has enough entries to fill `viewport_h` plus a 2-slot buffer.
    ///
    /// Called from `layout()` — only resizes if slot count changed.
    fn ensure_slots(&mut self, viewport_h: f32) {
        let item_h = self.provider.item_height();
        if item_h <= 0.0 {
            return;
        }
        // +2: one partially visible slot at each edge.
        // No f32::ceil in no_std — compute ceiling via integer division.
        let needed =
            (viewport_h as usize + item_h as usize).saturating_sub(1) / item_h as usize + 2;
        if self.slot_count == needed {
            return;
        }

        self.slot_count = needed;
        self.slots.clear();
        let count = self.provider.item_count();
        for i in 0..needed {
            // Bootstrap with sequential indices so slots are pre-populated
            // and don't all start at idx 0 (avoids duplicate binding on first pass).
            let idx = if i < count { Some(i) } else { None };
            let widget = if i < count {
                self.provider.build_item(i)
            } else {
                // Placeholder for out-of-range slots — never painted.
                self.provider
                    .build_item(0.min(count.saturating_sub(1)).max(0))
            };
            self.slots.push(VirtualSlot {
                widget,
                bound_idx: idx,
            });
        }
    }

    /// Rebind each slot to the correct data index for the current scroll position.
    ///
    /// Only rebuilds the slot widget when the target index differs from the
    /// currently bound index — no-op for slots that didn't scroll to a new row.
    fn rebind_slots(&mut self) {
        let first = self.first_visible_idx();
        let count = self.provider.item_count();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            let target = first + i;
            if target >= count {
                slot.bound_idx = None;
                continue;
            }
            if slot.bound_idx != Some(target) {
                slot.widget = self.provider.build_item(target);
                slot.bound_idx = Some(target);
            }
        }
    }
}

impl crate::node::ViNode for VirtualListView {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let size = constraints.constrain(Size {
            w: constraints.max.w,
            h: constraints.max.h,
        });
        let b = Rect::from_origin_size(constraints.origin, size);
        self.bounds_cache.set(b);

        self.ensure_slots(b.h);
        self.rebind_slots();

        // Position each slot at its absolute screen-space Y coordinate.
        let item_h = self.provider.item_height();
        let first = self.first_visible_idx();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.bound_idx.is_none() {
                continue;
            }
            let slot_y = b.y + (first + i) as f32 * item_h - self.scroll_y;
            let slot_constraints =
                Constraints::new(Point::new(b.x, slot_y), Size::new(b.w, item_h));
            slot.widget.layout(slot_constraints);
        }

        size
    }

    fn bounds(&self) -> Rect {
        self.bounds_cache.get()
    }

    fn paint(&self, cx: &mut crate::render_ctx::RenderCtx<'_>) {
        let b = self.bounds_cache.get();

        // Background fill.
        cx.canvas.fill_rect(b, cx.theme.bg());

        cx.canvas.clip_push(b);

        let sel = *self.selected.get();
        for slot in &self.slots {
            let idx = match slot.bound_idx {
                Some(i) => i,
                None => continue,
            };
            let sb = slot.widget.bounds();
            // Skip slots scrolled fully outside the viewport.
            if sb.y + sb.h < b.y || sb.y > b.y + b.h {
                continue;
            }

            if sel == Some(idx) {
                cx.canvas.fill_rect(sb, cx.theme.list_selected_bg());
            }
            slot.widget.paint(cx);
        }

        cx.canvas.clip_pop();

        // Scrollbar — drawn outside clip so it appears on top of content.
        let total_h = self.total_content_height();
        if total_h > b.h {
            let bar_w = 6.0_f32;
            let bar_x = b.x + b.w - bar_w;
            let thumb_h = (b.h / total_h * b.h).max(20.0);
            let scroll_range = total_h - b.h;
            let thumb_y = if scroll_range > 0.0 {
                b.y + (self.scroll_y / scroll_range) * (b.h - thumb_h)
            } else {
                b.y
            };
            let thumb_y = thumb_y.min(b.y + b.h - thumb_h);

            // Track
            cx.canvas.fill_rect(
                Rect {
                    x: bar_x,
                    y: b.y,
                    w: bar_w,
                    h: b.h,
                },
                cx.theme.surface(),
            );
            // Thumb
            cx.canvas.fill_rect(
                Rect {
                    x: bar_x,
                    y: thumb_y,
                    w: bar_w,
                    h: thumb_h,
                },
                cx.theme.border(),
            );
        }
    }

    fn event(&mut self, event: &crate::event::Event) -> bool {
        // Decay touch fling at the start of every event call.
        let v = self.scroll_vel;
        if v.abs() > 0.5 {
            self.scroll_y = (self.scroll_y + v).clamp(0.0, self.max_scroll());
            self.scroll_vel = (v * 0.85).clamp(-50.0, 50.0);
            self.rebind_slots();
        }

        let b = self.bounds_cache.get();
        match event {
            crate::event::Event::Scroll { pos, delta_y } if b.contains(*pos) => {
                self.scroll_y -= delta_y * SCROLL_SPEED;
                self.clamp_scroll();
                self.rebind_slots();
                true
            }
            crate::event::Event::MousePress {
                pos,
                button: crate::event::MouseButton::Left,
            } => {
                if !b.contains(*pos) {
                    return false;
                }
                let item_h = self.provider.item_height();
                if item_h > 0.0 {
                    let clicked = ((pos.y - b.y + self.scroll_y) / item_h) as usize;
                    if clicked < self.provider.item_count() {
                        self.selected.set(Some(clicked));
                        if let Some(cb) = &self.on_select {
                            cb(clicked);
                        }
                    }
                }
                true
            }
            crate::event::Event::TouchBegin { pos, .. } => {
                if !b.contains(*pos) {
                    return false;
                }
                self.scroll_vel = 0.0;
                self.last_touch_y = pos.y;
                let item_h = self.provider.item_height();
                if item_h > 0.0 {
                    let clicked = ((pos.y - b.y + self.scroll_y) / item_h) as usize;
                    if clicked < self.provider.item_count() {
                        self.selected.set(Some(clicked));
                        if let Some(cb) = &self.on_select {
                            cb(clicked);
                        }
                    }
                }
                true
            }
            crate::event::Event::TouchMove { pos, finger_id: 0 } => {
                if !b.contains(*pos) {
                    return false;
                }
                let last_y = self.last_touch_y;
                let delta = pos.y - last_y;
                self.last_touch_y = pos.y;
                if last_y != 0.0 {
                    self.scroll_vel = (-delta * 0.6 + self.scroll_vel * 0.4).clamp(-50.0, 50.0);
                }
                self.scroll_y -= delta;
                self.clamp_scroll();
                self.rebind_slots();
                true
            }
            _ => false,
        }
    }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        let bounds = self.bounds_cache.get();
        let r = region.clone();
        let sub = self.selected.subscribe(move || {
            r.borrow_mut().mark(bounds);
        });
        alloc::vec![sub]
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{prefix_sum, row_at_offset};

    #[test]
    fn prefix_sum_empty() {
        let p = prefix_sum(&[]);
        assert_eq!(p, &[0.0_f32]);
    }

    #[test]
    fn prefix_sum_uniform() {
        let p = prefix_sum(&[10.0, 10.0, 10.0]);
        assert_eq!(p, &[0.0, 10.0, 20.0, 30.0]);
    }

    #[test]
    fn prefix_sum_variable() {
        let p = prefix_sum(&[20.0, 40.0, 10.0]);
        assert_eq!(p, &[0.0, 20.0, 60.0, 70.0]);
    }

    #[test]
    fn row_at_offset_first_row() {
        let p = prefix_sum(&[20.0, 40.0, 10.0]);
        assert_eq!(row_at_offset(&p, 0.0), 0);
        assert_eq!(row_at_offset(&p, 10.0), 0);
        assert_eq!(row_at_offset(&p, 19.9), 0);
    }

    #[test]
    fn row_at_offset_second_row() {
        let p = prefix_sum(&[20.0, 40.0, 10.0]);
        assert_eq!(row_at_offset(&p, 20.0), 1);
        assert_eq!(row_at_offset(&p, 59.9), 1);
    }

    #[test]
    fn row_at_offset_third_row() {
        let p = prefix_sum(&[20.0, 40.0, 10.0]);
        assert_eq!(row_at_offset(&p, 60.0), 2);
        assert_eq!(row_at_offset(&p, 69.9), 2);
    }

    #[test]
    fn row_at_offset_beyond_last() {
        // When offset is past total content, returns last valid row.
        let p = prefix_sum(&[20.0, 40.0, 10.0]);
        // offset == total height (70.0) → partition_point returns 4 (past end) → 3
        // row 3 doesn't exist in a 3-item list, but that's guarded by item_at's len check.
        let idx = row_at_offset(&p, 70.0);
        assert!(idx <= 3);
    }
}
