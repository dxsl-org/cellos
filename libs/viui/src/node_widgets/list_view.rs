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
    if pos == 0 { 0 } else { pos - 1 }
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
                if idx < len { Some(idx) } else { None }
            }
            Some(heights_sig) => {
                let heights = heights_sig.get();
                let prefix = prefix_sum(&heights);
                let idx = row_at_offset(&prefix, rel_y);
                if idx < self.items.get().len() { Some(idx) } else { None }
            }
        }
    }
}

impl ViNode for ListView {
    fn layout(&mut self, constraints: Constraints) -> Size {
        // Constrain height to a reasonable maximum; full width.
        let desired_h = constraints.max.h.min(200.0);
        let size = constraints.constrain(Size { w: constraints.max.w, h: desired_h });
        self.bounds_cache.set(Rect::from_origin_size(constraints.origin, size));
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
                        let item_rect = Rect { x: b.x, y: item_y, w: b.w, h: self.item_height };

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
                        while end < items.len()
                            && end + 1 < prefix.len()
                            && prefix[end] < bottom
                        {
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
                        let item_rect = Rect { x: b.x, y: item_y, w: b.w, h: item_h };

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
                Rect { x: bar_x, y: b.y, w: bar_w, h: b.h },
                cx.theme.surface(),
            );
            cx.canvas.fill_rect(
                Rect { x: bar_x, y: thumb_y, w: bar_w, h: thumb_h },
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
            Event::MousePress { pos, button: MouseButton::Left } => {
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
                    let new_v = (-delta * 0.6 + self.touch_velocity.get() * 0.4)
                        .clamp(-50.0, 50.0);
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
