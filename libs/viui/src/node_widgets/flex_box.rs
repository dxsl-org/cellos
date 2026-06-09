// SPDX-License-Identifier: MIT
//! FlexBox — flexible space-distributing container (row or column direction).
//!
//! Fixed children (`flex_grow = 0.0`) receive their natural size from a
//! measurement pass; remaining space is distributed proportionally among
//! flex children by their `flex_grow` weight, floored by `min_size`.
//!
//! ## Layout algorithm (Row)
//!
//! ```text
//! Pass 1 — measure fixed children (dummy origin 99999, 99999)
//! Pass 2 — lay out all children with real origins
//!   remaining = available_main - fixed_sum
//!   flex child width = (remaining * flex_grow / total_grow).max(min_size)
//! ```
//!
//! Column direction swaps w↔h and x↔y throughout.

extern crate alloc;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::Cell;

use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::SubscriptionHandle;

// ─── Direction ───────────────────────────────────────────────────────────────

/// Main-axis direction for a `FlexBox`.
pub enum FlexDirection {
    /// Children arranged left-to-right (horizontal main axis).
    Row,
    /// Children arranged top-to-bottom (vertical main axis).
    Column,
}

// ─── FlexItem ────────────────────────────────────────────────────────────────

/// One child slot inside a `FlexBox`.
pub struct FlexItem {
    pub node: Box<dyn ViNode>,
    /// `0.0` = fixed natural size; `> 0` = proportional share of remaining space.
    pub flex_grow: f32,
    /// Minimum main-axis size floor applied after proportional distribution.
    pub min_size: f32,
}

// ─── FlexBox ─────────────────────────────────────────────────────────────────

/// Flexible container — CSS flex-like space distribution.
///
/// Supports both row and column directions. Fixed children (`flex_grow=0.0`)
/// get their natural size measured first; the remaining main-axis space is
/// divided proportionally among flex children by `flex_grow` weight.
pub struct FlexBox {
    direction: FlexDirection,
    children: Vec<FlexItem>,
    /// Gap (pixels) between children along the main axis.
    pub gap: f32,
    /// Inner padding applied to all four sides.
    pub padding: f32,
    bounds_cache: Cell<Rect>,
}

// ─── Builder ─────────────────────────────────────────────────────────────────

impl FlexBox {
    /// Create a horizontal (`Row`) flex container.
    pub fn row() -> Self {
        Self::new(FlexDirection::Row)
    }

    /// Create a vertical (`Column`) flex container.
    pub fn column() -> Self {
        Self::new(FlexDirection::Column)
    }

    fn new(direction: FlexDirection) -> Self {
        Self {
            direction,
            children: Vec::new(),
            gap: 4.0,
            padding: 0.0,
            bounds_cache: Cell::new(Rect::ZERO),
        }
    }

    /// Set gap between children (builder pattern).
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Set uniform inner padding on all sides (builder pattern).
    pub fn padding(mut self, pad: f32) -> Self {
        self.padding = pad;
        self
    }

    /// Add a fixed-size child (`flex_grow = 0.0`).
    pub fn child(mut self, node: impl ViNode + 'static) -> Self {
        self.children.push(FlexItem {
            node: Box::new(node),
            flex_grow: 0.0,
            min_size: 0.0,
        });
        self
    }

    /// Add a flex child with a proportional grow weight.
    pub fn flex_child(mut self, node: impl ViNode + 'static, grow: f32) -> Self {
        self.children.push(FlexItem {
            node: Box::new(node),
            flex_grow: grow.max(0.0),
            min_size: 0.0,
        });
        self
    }

    /// Add a flex child with a grow weight and a minimum main-axis size.
    pub fn min_child(mut self, node: impl ViNode + 'static, grow: f32, min: f32) -> Self {
        self.children.push(FlexItem {
            node: Box::new(node),
            flex_grow: grow.max(0.0),
            min_size: min,
        });
        self
    }
}

// ─── Layout helpers ──────────────────────────────────────────────────────────

impl FlexBox {
    /// Two-pass Row layout. Returns `(final_size, bounds)`.
    fn layout_row(&mut self, constraints: Constraints) -> Size {
        let n = self.children.len();
        let gap_total = if n > 1 { self.gap * (n - 1) as f32 } else { 0.0 };
        let available_main = (constraints.max.w - 2.0 * self.padding - gap_total).max(0.0);
        let cross_max = (constraints.max.h - 2.0 * self.padding).max(0.0);

        // ── Pass 1: measure fixed children ───────────────────────────────
        let dummy_origin = Point::new(99999.0, 99999.0);
        let mut fixed_widths: Vec<Option<f32>> = (0..n).map(|_| None).collect();
        let mut fixed_sum = 0.0_f32;

        for (i, item) in self.children.iter_mut().enumerate() {
            if item.flex_grow == 0.0 {
                let sz = item.node.layout(Constraints::new(
                    dummy_origin,
                    Size::new(available_main, cross_max),
                ));
                fixed_widths[i] = Some(sz.w);
                fixed_sum += sz.w;
            }
        }

        // ── Distribute remaining space to flex children ───────────────────
        let remaining = (available_main - fixed_sum).max(0.0);
        let total_grow: f32 = self.children.iter().map(|c| c.flex_grow).sum();
        let flex_count = self.children.iter().filter(|c| c.flex_grow > 0.0).count();

        // ── Pass 2: layout all children with real origins ─────────────────
        let mut x = constraints.origin.x + self.padding;
        let base_y = constraints.origin.y + self.padding;
        let mut max_cross = 0.0_f32;

        for (i, item) in self.children.iter_mut().enumerate() {
            let child_w = if item.flex_grow == 0.0 {
                fixed_widths[i].unwrap_or(0.0)
            } else if total_grow > 0.0 {
                (remaining * item.flex_grow / total_grow).max(item.min_size)
            } else {
                // All flex children share evenly when total_grow is 0 (fallback).
                if flex_count > 0 {
                    (remaining / flex_count as f32).max(item.min_size)
                } else {
                    item.min_size
                }
            };

            let sz = item.node.layout(Constraints::new(
                Point::new(x, base_y),
                Size::new(child_w, cross_max),
            ));
            max_cross = max_cross.max(sz.h);
            x += child_w + self.gap;
        }

        let size = constraints.constrain(Size {
            w: constraints.max.w,
            h: (max_cross + 2.0 * self.padding).max(0.0),
        });
        self.bounds_cache.set(Rect::from_origin_size(constraints.origin, size));
        size
    }

    /// Two-pass Column layout (w↔h / x↔y swapped relative to Row).
    fn layout_column(&mut self, constraints: Constraints) -> Size {
        let n = self.children.len();
        let gap_total = if n > 1 { self.gap * (n - 1) as f32 } else { 0.0 };
        let available_main = (constraints.max.h - 2.0 * self.padding - gap_total).max(0.0);
        let cross_max = (constraints.max.w - 2.0 * self.padding).max(0.0);

        // ── Pass 1: measure fixed children ───────────────────────────────
        let dummy_origin = Point::new(99999.0, 99999.0);
        let mut fixed_heights: Vec<Option<f32>> = (0..n).map(|_| None).collect();
        let mut fixed_sum = 0.0_f32;

        for (i, item) in self.children.iter_mut().enumerate() {
            if item.flex_grow == 0.0 {
                let sz = item.node.layout(Constraints::new(
                    dummy_origin,
                    Size::new(cross_max, available_main),
                ));
                fixed_heights[i] = Some(sz.h);
                fixed_sum += sz.h;
            }
        }

        // ── Distribute remaining space to flex children ───────────────────
        let remaining = (available_main - fixed_sum).max(0.0);
        let total_grow: f32 = self.children.iter().map(|c| c.flex_grow).sum();
        let flex_count = self.children.iter().filter(|c| c.flex_grow > 0.0).count();

        // ── Pass 2: layout all children with real origins ─────────────────
        let base_x = constraints.origin.x + self.padding;
        let mut y = constraints.origin.y + self.padding;
        let mut max_cross = 0.0_f32;

        for (i, item) in self.children.iter_mut().enumerate() {
            let child_h = if item.flex_grow == 0.0 {
                fixed_heights[i].unwrap_or(0.0)
            } else if total_grow > 0.0 {
                (remaining * item.flex_grow / total_grow).max(item.min_size)
            } else {
                if flex_count > 0 {
                    (remaining / flex_count as f32).max(item.min_size)
                } else {
                    item.min_size
                }
            };

            let sz = item.node.layout(Constraints::new(
                Point::new(base_x, y),
                Size::new(cross_max, child_h),
            ));
            max_cross = max_cross.max(sz.w);
            y += child_h + self.gap;
        }

        let size = constraints.constrain(Size {
            w: (max_cross + 2.0 * self.padding).max(0.0),
            h: constraints.max.h,
        });
        self.bounds_cache.set(Rect::from_origin_size(constraints.origin, size));
        size
    }
}

// ─── ViNode ───────────────────────────────────────────────────────────────────

impl ViNode for FlexBox {
    fn layout(&mut self, constraints: Constraints) -> Size {
        if self.children.is_empty() {
            let size = constraints.constrain(Size {
                w: constraints.max.w,
                h: 2.0 * self.padding,
            });
            self.bounds_cache.set(Rect::from_origin_size(constraints.origin, size));
            return size;
        }

        match self.direction {
            FlexDirection::Row => self.layout_row(constraints),
            FlexDirection::Column => self.layout_column(constraints),
        }
    }

    fn bounds(&self) -> Rect {
        self.bounds_cache.get()
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        for item in &self.children {
            item.node.paint(cx);
        }
    }

    fn event(&mut self, event: &Event) -> bool {
        for item in self.children.iter_mut().rev() {
            if item.node.event(event) {
                return true;
            }
        }
        false
    }

    fn collect_focusable_bounds(&mut self) -> alloc::vec::Vec<crate::layout::Rect> {
        self.children.iter_mut()
            .flat_map(|c| c.node.collect_focusable_bounds())
            .collect()
    }

    fn activate_at(&mut self, target: crate::layout::Rect) -> bool {
        self.children.iter_mut().any(|c| c.node.activate_at(target))
    }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        let mut handles = Vec::new();
        for item in &mut self.children {
            handles.extend(item.node.collect_dirty_handles(Rc::clone(&region)));
        }
        handles
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Constraints, Point, Size};

    // Minimal leaf widget used only in tests.
    struct FixedLeaf {
        w: f32,
        h: f32,
        bounds: Rect,
    }

    impl FixedLeaf {
        fn new(w: f32, h: f32) -> Self {
            Self { w, h, bounds: Rect::ZERO }
        }
    }

    impl ViNode for FixedLeaf {
        fn layout(&mut self, constraints: Constraints) -> Size {
            let size = constraints.constrain(Size::new(self.w, self.h));
            self.bounds = Rect::from_origin_size(constraints.origin, size);
            size
        }
        fn bounds(&self) -> Rect { self.bounds }
        fn paint(&self, _cx: &mut RenderCtx<'_>) {}
        fn event(&mut self, _event: &Event) -> bool { false }
    }

    fn root(w: f32, h: f32) -> Constraints {
        Constraints::new(Point::ZERO, Size::new(w, h))
    }

    // ── Row tests ─────────────────────────────────────────────────────────────

    #[test]
    fn row_two_fixed_one_flex() {
        // 200px wide, 2 fixed children (30px each), 1 flex child gets remainder.
        // gap=0, padding=0 → flex child should get 200 - 30 - 30 = 140 px.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .padding(0.0)
            .child(FixedLeaf::new(30.0, 20.0))
            .child(FixedLeaf::new(30.0, 20.0))
            .flex_child(FixedLeaf::new(999.0, 20.0), 1.0);

        let size = fb.layout(root(200.0, 100.0));
        assert_eq!(size.w, 200.0, "row should fill full width");

        // Flex child is the third item — its layout bounds should be 140px wide.
        let flex_bounds = fb.children[2].node.bounds();
        assert!(
            (flex_bounds.w - 140.0).abs() < 0.01,
            "flex child w = {}, expected 140", flex_bounds.w
        );
        assert_eq!(flex_bounds.x, 60.0, "flex child should start at x=60");
    }

    #[test]
    fn row_equal_flex_children() {
        // Two flex(1) children in a 200px row → each gets 100px.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .padding(0.0)
            .flex_child(FixedLeaf::new(50.0, 20.0), 1.0)
            .flex_child(FixedLeaf::new(50.0, 20.0), 1.0);

        fb.layout(root(200.0, 100.0));

        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        assert!((b0.w - 100.0).abs() < 0.01, "child 0 w={}", b0.w);
        assert!((b1.w - 100.0).abs() < 0.01, "child 1 w={}", b1.w);
        assert_eq!(b0.x, 0.0);
        assert!((b1.x - 100.0).abs() < 0.01, "child 1 x={}", b1.x);
    }

    #[test]
    fn row_weighted_flex() {
        // flex(1) + flex(2) in 300px → 100px + 200px.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .padding(0.0)
            .flex_child(FixedLeaf::new(10.0, 10.0), 1.0)
            .flex_child(FixedLeaf::new(10.0, 10.0), 2.0);

        fb.layout(root(300.0, 50.0));

        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        assert!((b0.w - 100.0).abs() < 0.01, "child 0 w={}", b0.w);
        assert!((b1.w - 200.0).abs() < 0.01, "child 1 w={}", b1.w);
    }

    #[test]
    fn row_padding_and_gap() {
        // 200px, padding=10, gap=5, two equal flex(1) children.
        // inner = 200 - 20 (pad) - 5 (gap) = 175 → each child 87.5px.
        let mut fb = FlexBox::row()
            .padding(10.0)
            .gap(5.0)
            .flex_child(FixedLeaf::new(10.0, 20.0), 1.0)
            .flex_child(FixedLeaf::new(10.0, 20.0), 1.0);

        let size = fb.layout(root(200.0, 100.0));

        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();

        assert!((b0.w - 87.5).abs() < 0.01, "child 0 w={}", b0.w);
        assert!((b1.w - 87.5).abs() < 0.01, "child 1 w={}", b1.w);
        // First child starts at padding offset.
        assert_eq!(b0.x, 10.0);
        // Second child starts at 10 + 87.5 + 5 = 102.5.
        assert!((b1.x - 102.5).abs() < 0.01, "child 1 x={}", b1.x);
        // Height should include padding.
        assert!(size.h >= 2.0 * 10.0, "height too small: {}", size.h);
    }

    // ── Column tests ──────────────────────────────────────────────────────────

    #[test]
    fn column_equal_flex_children() {
        // Two flex(1) children in a 200px-tall column → each gets 100px.
        let mut fb = FlexBox::column()
            .gap(0.0)
            .padding(0.0)
            .flex_child(FixedLeaf::new(50.0, 20.0), 1.0)
            .flex_child(FixedLeaf::new(50.0, 20.0), 1.0);

        fb.layout(root(100.0, 200.0));

        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        assert!((b0.h - 100.0).abs() < 0.01, "child 0 h={}", b0.h);
        assert!((b1.h - 100.0).abs() < 0.01, "child 1 h={}", b1.h);
        assert_eq!(b0.y, 0.0);
        assert!((b1.y - 100.0).abs() < 0.01, "child 1 y={}", b1.y);
    }

    #[test]
    fn column_fixed_plus_flex() {
        // 200px height, 1 fixed (40px) + 1 flex(1) → flex gets 160px.
        let mut fb = FlexBox::column()
            .gap(0.0)
            .padding(0.0)
            .child(FixedLeaf::new(50.0, 40.0))
            .flex_child(FixedLeaf::new(50.0, 10.0), 1.0);

        fb.layout(root(100.0, 200.0));

        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        assert!((b0.h - 40.0).abs() < 0.01, "fixed h={}", b0.h);
        assert!((b1.h - 160.0).abs() < 0.01, "flex h={}", b1.h);
        assert!((b1.y - 40.0).abs() < 0.01, "flex y={}", b1.y);
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn empty_flexbox_returns_padded_size() {
        let mut fb = FlexBox::row().padding(8.0);
        let size = fb.layout(root(200.0, 100.0));
        assert_eq!(size.h, 16.0, "empty row h should be 2*padding=16");
    }

    #[test]
    fn single_child_no_gap() {
        let mut fb = FlexBox::row()
            .gap(10.0)
            .child(FixedLeaf::new(50.0, 30.0));

        fb.layout(root(200.0, 100.0));
        // Single child — no gap should be applied.
        let b = fb.children[0].node.bounds();
        assert_eq!(b.x, 0.0, "single child should be at x=0");
    }

    #[test]
    fn min_size_respected() {
        // flex(1) with min_size=80 in a 100px container after a 90px fixed child.
        // remaining = 100 - 90 = 10; allocated = 10; but min = 80 → should get 80.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .child(FixedLeaf::new(90.0, 10.0))
            .min_child(FixedLeaf::new(10.0, 10.0), 1.0, 80.0);

        fb.layout(root(100.0, 50.0));

        let b1 = fb.children[1].node.bounds();
        assert!(b1.w >= 80.0, "flex child should get at least min_size=80, got {}", b1.w);
    }

    #[test]
    fn row_bounds_cache_set_correctly() {
        let mut fb = FlexBox::row().child(FixedLeaf::new(50.0, 30.0));
        fb.layout(Constraints::new(Point::new(10.0, 20.0), Size::new(200.0, 100.0)));
        let b = fb.bounds();
        assert_eq!(b.x, 10.0);
        assert_eq!(b.y, 20.0);
    }
}
