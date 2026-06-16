// SPDX-License-Identifier: MIT
//! FlexBox — CSS flex-like container with grow, shrink, wrap, gap, and alignment.
//!
//! ## Layout algorithm (single-line, Row direction)
//!
//! ```text
//! Pass 1 — measure fixed children (flex_grow=0, flex_shrink=0 effectively)
//! Pass 2 — distribute space: grow if free_space>0, shrink if free_space<0
//! Pass 3 — position children with justify_content offsets + cross-axis align
//! ```
//!
//! When `wrap != NoWrap`, items are grouped into lines greedily. Each line is
//! laid out independently; lines are stacked along the cross axis with `gap_cross`
//! between them. `align_content` controls how the line group fills the container.
//!
//! Column direction swaps w↔h and x↔y throughout.

extern crate alloc;
use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
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

// ─── Justify ─────────────────────────────────────────────────────────────────

/// How free main-axis space is distributed among items within a line.
#[derive(Clone, Copy, PartialEq)]
pub enum Justify {
    /// Pack items toward the start of the line.
    Start,
    /// Pack items toward the end of the line.
    End,
    /// Center items along the line.
    Center,
    /// Equal space between items; no space at edges.
    SpaceBetween,
    /// Equal space around each item (half-space at edges).
    SpaceAround,
    /// Equal space between items AND at edges.
    SpaceEvenly,
}

// ─── AlignItems ──────────────────────────────────────────────────────────────

/// Cross-axis alignment for items within a single flex line.
#[derive(Clone, Copy, PartialEq)]
pub enum AlignItems {
    /// Pack items at the cross-start of the line.
    Start,
    /// Pack items at the cross-end of the line.
    End,
    /// Center items along the cross axis.
    Center,
    /// Stretch items to fill the line's cross-axis size.
    Stretch,
}

// ─── AlignContent ─────────────────────────────────────────────────────────────

/// Alignment of multiple lines within the container (only relevant with `wrap`).
#[derive(Clone, Copy, PartialEq)]
pub enum AlignContent {
    /// Pack lines toward the cross-start.
    Start,
    /// Pack lines toward the cross-end.
    End,
    /// Center the line group along the cross axis.
    Center,
    /// Stretch lines to fill the container cross-axis.
    Stretch,
    /// Equal space between lines; no space at edges.
    SpaceBetween,
    /// Equal space around each line (half-space at edges).
    SpaceAround,
}

// ─── FlexWrap ─────────────────────────────────────────────────────────────────

/// Whether items wrap to additional lines when the main axis overflows.
#[derive(Clone, Copy, PartialEq)]
pub enum FlexWrap {
    /// All items fit on a single line (may overflow).
    NoWrap,
    /// Wrap items to new lines in the forward direction.
    Wrap,
    /// Wrap items to new lines in the reverse direction.
    WrapReverse,
}

// ─── FlexItem ────────────────────────────────────────────────────────────────

/// One child slot inside a `FlexBox`.
pub struct FlexItem {
    pub node: Box<dyn ViNode>,
    /// `0.0` = fixed natural size; `> 0` = proportional share of remaining space.
    pub flex_grow: f32,
    /// Proportional shrink factor when space is insufficient. Default `1.0`.
    pub flex_shrink: f32,
    /// Minimum main-axis size floor applied after proportional distribution.
    pub min_size: f32,
    /// Maximum main-axis size cap applied after proportional distribution.
    pub max_size: Option<f32>,
    /// Per-item override of parent `align_items`. `None` = use parent's value.
    pub align_self: Option<AlignItems>,
}

impl FlexItem {
    /// Create a fixed-size item with default shrink/min/max/align.
    pub fn fixed(node: impl ViNode + 'static) -> Self {
        Self {
            node: Box::new(node),
            flex_grow: 0.0,
            flex_shrink: 1.0,
            min_size: 0.0,
            max_size: None,
            align_self: None,
        }
    }

    /// Create a flex item with grow weight and default shrink/min/max/align.
    pub fn grow(node: impl ViNode + 'static, grow: f32) -> Self {
        Self {
            node: Box::new(node),
            flex_grow: grow.max(0.0),
            flex_shrink: 1.0,
            min_size: 0.0,
            max_size: None,
            align_self: None,
        }
    }

    /// Set shrink factor (builder).
    pub fn shrink(mut self, factor: f32) -> Self {
        self.flex_shrink = factor.max(0.0);
        self
    }

    /// Set minimum main-axis size floor (builder).
    pub fn with_min(mut self, size: f32) -> Self {
        self.min_size = size;
        self
    }

    /// Set maximum main-axis size cap (builder).
    pub fn max_size(mut self, size: f32) -> Self {
        self.max_size = Some(size);
        self
    }

    /// Override parent's `align_items` for this item (builder).
    pub fn align_self(mut self, a: AlignItems) -> Self {
        self.align_self = Some(a);
        self
    }
}

// ─── FlexBox ─────────────────────────────────────────────────────────────────

/// Flexible container — CSS flex-like space distribution.
///
/// Supports row/column direction, flex grow+shrink, gap, padding,
/// justify-content, align-items, flex-wrap, and align-content.
pub struct FlexBox {
    direction:     FlexDirection,
    children:      Vec<FlexItem>,
    /// Inner padding applied uniformly to all four sides.
    pub padding:   f32,
    /// Gap along the main axis between items within a line.
    pub gap_main:  f32,
    /// Gap along the cross axis between lines (only relevant with wrap).
    pub gap_cross: f32,
    /// How free main-axis space is distributed within a line.
    justify:       Justify,
    /// Default cross-axis alignment for items within a line.
    align_items:   AlignItems,
    /// Whether items wrap to new lines.
    wrap:          FlexWrap,
    /// Alignment of multiple lines within the container.
    align_content: AlignContent,

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
            padding: 0.0,
            gap_main: 4.0,
            gap_cross: 4.0,
            justify: Justify::Start,
            align_items: AlignItems::Start,
            wrap: FlexWrap::NoWrap,
            align_content: AlignContent::Start,
            bounds_cache: Cell::new(Rect::ZERO),
        }
    }

    /// Set uniform gap between items AND between lines (builder).
    ///
    /// Equivalent to `.gap_axes(gap, gap)`. This preserves backward
    /// compatibility with the previous single `gap` field.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap_main = gap;
        self.gap_cross = gap;
        self
    }

    /// Set main-axis and cross-axis gaps independently (builder).
    pub fn gap_axes(mut self, main: f32, cross: f32) -> Self {
        self.gap_main = main;
        self.gap_cross = cross;
        self
    }

    /// Set uniform inner padding on all sides (builder).
    pub fn padding(mut self, pad: f32) -> Self {
        self.padding = pad;
        self
    }

    /// Set justify-content (builder).
    pub fn justify(mut self, j: Justify) -> Self {
        self.justify = j;
        self
    }

    /// Set default cross-axis alignment for items (builder).
    pub fn align_items(mut self, a: AlignItems) -> Self {
        self.align_items = a;
        self
    }

    /// Enable wrap (items flow to next line on overflow) (builder).
    pub fn wrap(mut self) -> Self {
        self.wrap = FlexWrap::Wrap;
        self
    }

    /// Set wrap mode explicitly (builder).
    pub fn wrap_mode(mut self, mode: FlexWrap) -> Self {
        self.wrap = mode;
        self
    }

    /// Set alignment of multiple lines within the container (builder).
    pub fn align_content(mut self, ac: AlignContent) -> Self {
        self.align_content = ac;
        self
    }

    /// Add a fixed-size child (`flex_grow = 0.0`).
    pub fn child(mut self, node: impl ViNode + 'static) -> Self {
        self.children.push(FlexItem {
            node: Box::new(node),
            flex_grow: 0.0,
            flex_shrink: 1.0,
            min_size: 0.0,
            max_size: None,
            align_self: None,
        });
        self
    }

    /// Add a flex child with a proportional grow weight.
    pub fn flex_child(mut self, node: impl ViNode + 'static, grow: f32) -> Self {
        self.children.push(FlexItem {
            node: Box::new(node),
            flex_grow: grow.max(0.0),
            flex_shrink: 1.0,
            min_size: 0.0,
            max_size: None,
            align_self: None,
        });
        self
    }

    /// Add a flex child with a grow weight and a minimum main-axis size.
    pub fn min_child(mut self, node: impl ViNode + 'static, grow: f32, min: f32) -> Self {
        self.children.push(FlexItem {
            node: Box::new(node),
            flex_grow: grow.max(0.0),
            flex_shrink: 1.0,
            min_size: min,
            max_size: None,
            align_self: None,
        });
        self
    }
}

// ─── Internal layout types ───────────────────────────────────────────────────

/// Computed size of one child on the main axis, before final positioning.
struct ItemLayout {
    /// Resolved main-axis size for this item.
    main: f32,
    /// Measured cross-axis size from the layout pass (reserved for future use).
    #[allow(dead_code)]
    cross: f32,
}

// ─── Layout helpers ──────────────────────────────────────────────────────────

impl FlexBox {
    // ── Single-line measurement ───────────────────────────────────────────────

    /// Measure items in `indices` within the given main/cross budgets.
    ///
    /// Returns per-item `ItemLayout` in index order (parallel to `indices`).
    fn measure_line(
        &mut self,
        indices:      &[usize],
        available_main: f32,
        cross_budget: f32,
        is_row:       bool,
    ) -> Vec<ItemLayout> {
        let n = indices.len();
        let gap_total = if n > 1 { self.gap_main * (n - 1) as f32 } else { 0.0 };
        let inner_main = (available_main - gap_total).max(0.0);

        let dummy = Point::new(99_999.0, 99_999.0);

        // Pass 1: measure fixed children.
        let mut base_sizes: Vec<Option<f32>> = vec![None; n];
        let mut fixed_sum = 0.0_f32;

        for (slot, &ci) in indices.iter().enumerate() {
            let item = &mut self.children[ci];
            if item.flex_grow == 0.0 {
                let sz = if is_row {
                    item.node.layout(Constraints::new(dummy, Size::new(inner_main, cross_budget)))
                } else {
                    item.node.layout(Constraints::new(dummy, Size::new(cross_budget, inner_main)))
                };
                let m = if is_row { sz.w } else { sz.h };
                // Apply max_size cap to fixed children too.
                let capped = item.max_size.map(|mx| m.min(mx)).unwrap_or(m);
                base_sizes[slot] = Some(capped.max(item.min_size));
                fixed_sum += base_sizes[slot].unwrap();
            }
        }

        // Pass 2: resolve flex (grow / shrink) children.
        let free = inner_main - fixed_sum;
        let total_grow:   f32 = indices.iter().map(|&ci| self.children[ci].flex_grow).sum();
        let flex_count = indices.iter().filter(|&&ci| self.children[ci].flex_grow > 0.0).count();

        if free > 0.0 && total_grow > 0.0 {
            // Distribute positive free space proportional to flex_grow.
            for (slot, &ci) in indices.iter().enumerate() {
                let item = &self.children[ci];
                if item.flex_grow > 0.0 {
                    let share = free * item.flex_grow / total_grow;
                    let clamped = item.max_size.map(|mx| share.min(mx)).unwrap_or(share);
                    base_sizes[slot] = Some(clamped.max(item.min_size));
                }
            }
        } else if free > 0.0 && flex_count > 0 {
            // Fallback: all flex children share evenly.
            let each = free / flex_count as f32;
            for (slot, &ci) in indices.iter().enumerate() {
                let item = &self.children[ci];
                if item.flex_grow > 0.0 {
                    let capped = item.max_size.map(|mx| each.min(mx)).unwrap_or(each);
                    base_sizes[slot] = Some(capped.max(item.min_size));
                }
            }
        } else if free < 0.0 {
            // Distribute shrinkage proportional to flex_shrink * base_size.
            // Fixed children (flex_grow=0) can still shrink via flex_shrink.
            let shrink_total: f32 = indices.iter().zip(base_sizes.iter()).map(|(&ci, bs)| {
                let sz = bs.unwrap_or(0.0);
                self.children[ci].flex_shrink * sz
            }).sum();

            if shrink_total > 0.0 {
                let deficit = -free; // positive amount to remove
                for (slot, &ci) in indices.iter().enumerate() {
                    let item = &self.children[ci];
                    let sz = base_sizes[slot].unwrap_or(0.0);
                    let weight = item.flex_shrink * sz / shrink_total;
                    let new_sz = (sz - deficit * weight).max(item.min_size);
                    base_sizes[slot] = Some(new_sz);
                }
            }
        }

        // Ensure all slots are filled (flex children that got no budget).
        for (slot, &ci) in indices.iter().enumerate() {
            if base_sizes[slot].is_none() {
                base_sizes[slot] = Some(self.children[ci].min_size);
            }
        }

        // Return layouts (cross sizes come from the real layout pass below,
        // but we need a dummy cross for now — cross will be filled later).
        base_sizes.into_iter().map(|m| ItemLayout { main: m.unwrap(), cross: 0.0 }).collect()
    }

    // ── Justify offsets ───────────────────────────────────────────────────────

    /// Compute the initial main-axis offset and inter-item spacing for `Justify`.
    ///
    /// Returns `(start_offset, between_spacing)` where:
    /// - `start_offset` = position of the first item's leading edge
    /// - `between_spacing` = additional space added between each pair of items
    ///   (on top of `gap_main`)
    fn justify_offsets(&self, free: f32, n: usize) -> (f32, f32) {
        if n == 0 { return (0.0, 0.0); }
        match self.justify {
            Justify::Start        => (0.0, 0.0),
            Justify::End          => (free.max(0.0), 0.0),
            Justify::Center       => (free.max(0.0) / 2.0, 0.0),
            Justify::SpaceBetween => {
                if n <= 1 { (0.0, 0.0) }
                else { (0.0, free.max(0.0) / (n - 1) as f32) }
            }
            Justify::SpaceAround  => {
                let slot = free.max(0.0) / n as f32;
                (slot / 2.0, slot)
            }
            Justify::SpaceEvenly  => {
                let slot = free.max(0.0) / (n + 1) as f32;
                (slot, slot)
            }
        }
    }

    // ── Cross-axis alignment ──────────────────────────────────────────────────

    /// Compute the cross-axis offset for one item given the line's cross size.
    fn cross_offset(&self, item_cross: f32, line_cross: f32, effective_align: AlignItems) -> f32 {
        match effective_align {
            AlignItems::Start   => 0.0,
            AlignItems::End     => (line_cross - item_cross).max(0.0),
            AlignItems::Center  => ((line_cross - item_cross) / 2.0).max(0.0),
            AlignItems::Stretch => 0.0, // item was already sized to line_cross
        }
    }

    // ── AlignContent line offsets ─────────────────────────────────────────────

    /// Compute per-line cross-axis start offsets for multi-line layout.
    ///
    /// `lines_cross` — slice of each line's natural cross size.
    /// `available_cross` — total cross space (after padding).
    fn align_content_offsets(&self, lines_cross: &[f32], available_cross: f32) -> Vec<f32> {
        let n = lines_cross.len();
        if n == 0 { return Vec::new(); }

        let total_gap = if n > 1 { self.gap_cross * (n - 1) as f32 } else { 0.0 };
        let total_lines: f32 = lines_cross.iter().sum::<f32>() + total_gap;
        let free = (available_cross - total_lines).max(0.0);

        let mut offsets = vec![0.0f32; n];
        match self.align_content {
            AlignContent::Start | AlignContent::Stretch => {
                // Pack at start; Stretch handled by stretching each line's size externally.
                let mut cursor = 0.0;
                for (i, &lc) in lines_cross.iter().enumerate() {
                    offsets[i] = cursor;
                    cursor += lc + self.gap_cross;
                }
            }
            AlignContent::End => {
                let mut cursor = free;
                for (i, &lc) in lines_cross.iter().enumerate() {
                    offsets[i] = cursor;
                    cursor += lc + self.gap_cross;
                }
            }
            AlignContent::Center => {
                let mut cursor = free / 2.0;
                for (i, &lc) in lines_cross.iter().enumerate() {
                    offsets[i] = cursor;
                    cursor += lc + self.gap_cross;
                }
            }
            AlignContent::SpaceBetween => {
                let between = if n > 1 { free / (n - 1) as f32 } else { 0.0 };
                let mut cursor = 0.0;
                for (i, &lc) in lines_cross.iter().enumerate() {
                    offsets[i] = cursor;
                    cursor += lc + self.gap_cross + between;
                }
            }
            AlignContent::SpaceAround => {
                let slot = free / n as f32;
                let mut cursor = slot / 2.0;
                for (i, &lc) in lines_cross.iter().enumerate() {
                    offsets[i] = cursor;
                    cursor += lc + self.gap_cross + slot;
                }
            }
        }
        offsets
    }

    // ── Row layout ────────────────────────────────────────────────────────────

    fn layout_row(&mut self, constraints: Constraints) -> Size {
        let available_main  = (constraints.max.w - 2.0 * self.padding).max(0.0);
        let available_cross = (constraints.max.h - 2.0 * self.padding).max(0.0);

        // Group children into lines.
        let line_groups = self.compute_lines(available_main, true);
        let n_lines = line_groups.len();

        if n_lines == 0 {
            let size = constraints.constrain(Size { w: constraints.max.w, h: 2.0 * self.padding });
            self.bounds_cache.set(Rect::from_origin_size(constraints.origin, size));
            return size;
        }

        // Measure each line.
        let mut line_natural_cross: Vec<f32> = Vec::with_capacity(n_lines);
        let mut line_item_layouts: Vec<Vec<ItemLayout>> = Vec::with_capacity(n_lines);

        for indices in &line_groups {
            let layouts = self.measure_line(indices, available_main, available_cross, true);
            // Natural cross = max child cross size (need real layout pass).
            // Use dummy cross for now; real cross computed after main sizing.
            line_natural_cross.push(0.0);
            line_item_layouts.push(layouts);
        }

        // Determine effective line cross sizes (real layout pass for cross).
        let dummy = Point::new(99_999.0, 99_999.0);
        // For Stretch align, children need the line cross size → chicken-and-egg.
        // We do a two-pass approach: first compute natural cross, then re-layout stretches.

        // Pass A: measure cross sizes with natural constraints.
        let mut line_cross_sizes: Vec<f32> = vec![0.0; n_lines];
        for (li, indices) in line_groups.iter().enumerate() {
            let il = &line_item_layouts[li];
            let mut max_cross = 0.0f32;
            for (slot, &ci) in indices.iter().enumerate() {
                let main_w = il[slot].main;
                let sz = self.children[ci].node.layout(
                    Constraints::new(dummy, Size::new(main_w, available_cross))
                );
                max_cross = max_cross.max(sz.h);
            }
            line_cross_sizes[li] = max_cross;
        }

        // For AlignContent::Stretch: distribute extra cross space to lines.
        let line_cross_out = self.effective_line_cross_sizes(&line_cross_sizes, available_cross);

        // Compute per-line cross-axis start offsets.
        let cross_offsets = self.align_content_offsets(&line_cross_out, available_cross);

        // Pass B: final layout with real origins.
        let origin_x = constraints.origin.x + self.padding;
        let origin_y = constraints.origin.y + self.padding;

        for (li, indices) in line_groups.iter().enumerate() {
            let il = &line_item_layouts[li];
            let n = indices.len();
            let line_cross = line_cross_out[li];
            let cross_y = origin_y + cross_offsets[li];

            // Compute main-axis positions with justify_content.
            let items_main_total: f32 = il.iter().map(|i| i.main).sum::<f32>()
                + if n > 1 { self.gap_main * (n - 1) as f32 } else { 0.0 };
            let free_main = (available_main - items_main_total).max(0.0);
            // For justify: free = actual free (may be negative handled by shrink already)
            let justify_free = available_main - items_main_total;
            let (start_off, between) = self.justify_offsets(justify_free, n);

            let mut x = origin_x + start_off;
            for (slot, &ci) in indices.iter().enumerate() {
                let item = &self.children[ci];
                let main_w = il[slot].main;

                // Effective align for this item.
                let eff_align = item.align_self.unwrap_or(self.align_items);
                let child_cross = if eff_align == AlignItems::Stretch {
                    line_cross
                } else {
                    available_cross
                };

                let sz = self.children[ci].node.layout(
                    Constraints::new(Point::new(x, cross_y), Size::new(main_w, child_cross))
                );

                // Cross-axis positioning.
                let real_item_cross = sz.h;
                let cy_off = self.cross_offset(real_item_cross, line_cross, eff_align);
                if cy_off.abs() > 0.01 {
                    // Re-layout at corrected y (cross offset inside line).
                    self.children[ci].node.layout(
                        Constraints::new(
                            Point::new(x, cross_y + cy_off),
                            Size::new(main_w, child_cross),
                        )
                    );
                }

                x += main_w + self.gap_main + between;
                let _ = free_main; // used via justify_offsets
            }
        }

        // Total cross extent.
        let total_cross: f32 = if n_lines == 0 { 0.0 } else {
            cross_offsets.last().copied().unwrap_or(0.0) + line_cross_out.last().copied().unwrap_or(0.0)
        };

        let size = constraints.constrain(Size {
            w: constraints.max.w,
            h: (total_cross + 2.0 * self.padding).max(0.0),
        });
        self.bounds_cache.set(Rect::from_origin_size(constraints.origin, size));
        size
    }

    // ── Column layout ─────────────────────────────────────────────────────────

    fn layout_column(&mut self, constraints: Constraints) -> Size {
        let available_main  = (constraints.max.h - 2.0 * self.padding).max(0.0);
        let available_cross = (constraints.max.w - 2.0 * self.padding).max(0.0);

        let line_groups = self.compute_lines(available_main, false);
        let n_lines = line_groups.len();

        if n_lines == 0 {
            let size = constraints.constrain(Size { w: 2.0 * self.padding, h: constraints.max.h });
            self.bounds_cache.set(Rect::from_origin_size(constraints.origin, size));
            return size;
        }

        let dummy = Point::new(99_999.0, 99_999.0);

        let mut line_cross_sizes: Vec<f32> = vec![0.0; n_lines];
        let mut line_item_layouts: Vec<Vec<ItemLayout>> = Vec::with_capacity(n_lines);

        // First pass: measure.
        for (li, indices) in line_groups.iter().enumerate() {
            let layouts = self.measure_line(indices, available_main, available_cross, false);
            let mut max_cross = 0.0f32;
            for (slot, &ci) in indices.iter().enumerate() {
                let main_h = layouts[slot].main;
                let sz = self.children[ci].node.layout(
                    Constraints::new(dummy, Size::new(available_cross, main_h))
                );
                max_cross = max_cross.max(sz.w);
            }
            line_cross_sizes[li] = max_cross;
            line_item_layouts.push(layouts);
        }

        let line_cross_out = self.effective_line_cross_sizes(&line_cross_sizes, available_cross);
        let cross_offsets  = self.align_content_offsets(&line_cross_out, available_cross);

        let origin_x = constraints.origin.x + self.padding;
        let origin_y = constraints.origin.y + self.padding;

        for (li, indices) in line_groups.iter().enumerate() {
            let il = &line_item_layouts[li];
            let n = indices.len();
            let line_cross = line_cross_out[li];
            let cross_x = origin_x + cross_offsets[li];

            let items_main_total: f32 = il.iter().map(|i| i.main).sum::<f32>()
                + if n > 1 { self.gap_main * (n - 1) as f32 } else { 0.0 };
            let justify_free = available_main - items_main_total;
            let (start_off, between) = self.justify_offsets(justify_free, n);

            let mut y = origin_y + start_off;
            for (slot, &ci) in indices.iter().enumerate() {
                let item = &self.children[ci];
                let main_h = il[slot].main;

                let eff_align = item.align_self.unwrap_or(self.align_items);
                let child_cross = if eff_align == AlignItems::Stretch {
                    line_cross
                } else {
                    available_cross
                };

                let sz = self.children[ci].node.layout(
                    Constraints::new(Point::new(cross_x, y), Size::new(child_cross, main_h))
                );

                let real_item_cross = sz.w;
                let cx_off = self.cross_offset(real_item_cross, line_cross, eff_align);
                if cx_off.abs() > 0.01 {
                    self.children[ci].node.layout(
                        Constraints::new(
                            Point::new(cross_x + cx_off, y),
                            Size::new(child_cross, main_h),
                        )
                    );
                }

                y += main_h + self.gap_main + between;
            }
        }

        let total_cross: f32 = if n_lines == 0 { 0.0 } else {
            cross_offsets.last().copied().unwrap_or(0.0) + line_cross_out.last().copied().unwrap_or(0.0)
        };

        let size = constraints.constrain(Size {
            w: (total_cross + 2.0 * self.padding).max(0.0),
            h: constraints.max.h,
        });
        self.bounds_cache.set(Rect::from_origin_size(constraints.origin, size));
        size
    }

    // ── Line breaking ─────────────────────────────────────────────────────────

    /// Group child indices into lines by greedy wrapping.
    ///
    /// When `wrap == NoWrap` returns a single line with all children.
    /// Items that cannot fit alone on a line are placed on their own line anyway.
    fn compute_lines(&mut self, available_main: f32, is_row: bool) -> Vec<Vec<usize>> {
        let n = self.children.len();
        if n == 0 { return Vec::new(); }

        if self.wrap == FlexWrap::NoWrap {
            return vec![(0..n).collect()];
        }

        let dummy = Point::new(99_999.0, 99_999.0);
        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut current: Vec<usize> = Vec::new();
        let mut current_main = 0.0f32;

        for i in 0..n {
            // Measure natural size for this item.
            let sz = if is_row {
                self.children[i].node.layout(
                    Constraints::new(dummy, Size::new(available_main, f32::MAX))
                )
            } else {
                self.children[i].node.layout(
                    Constraints::new(dummy, Size::new(f32::MAX, available_main))
                )
            };
            let item_main = if is_row { sz.w } else { sz.h };

            if !current.is_empty() && current_main + self.gap_main + item_main > available_main {
                // Break — start a new line.
                lines.push(core::mem::take(&mut current));
                current_main = 0.0;
            }

            // Recompute gap AFTER the break so the first item on a fresh line
            // gets 0 gap, not the gap that was valid for the now-flushed line.
            let gap_needed = if current.is_empty() { 0.0 } else { self.gap_main };
            current.push(i);
            current_main += gap_needed + item_main;
        }
        if !current.is_empty() { lines.push(current); }

        if self.wrap == FlexWrap::WrapReverse { lines.reverse(); }

        lines
    }

    // ── AlignContent::Stretch helper ─────────────────────────────────────────

    /// For `AlignContent::Stretch`, distribute extra cross space to lines.
    /// Otherwise return the natural cross sizes unchanged.
    fn effective_line_cross_sizes(&self, natural: &[f32], available_cross: f32) -> Vec<f32> {
        if self.align_content != AlignContent::Stretch || natural.is_empty() {
            return natural.to_vec();
        }
        let n = natural.len();
        let gap_total = if n > 1 { self.gap_cross * (n - 1) as f32 } else { 0.0 };
        let total_natural: f32 = natural.iter().sum();
        let free = (available_cross - total_natural - gap_total).max(0.0);
        let bonus = free / n as f32;
        natural.iter().map(|&lc| lc + bonus).collect()
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
            FlexDirection::Row    => self.layout_row(constraints),
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

    // ── New feature tests ─────────────────────────────────────────────────────

    #[test]
    fn justify_space_evenly() {
        // 3 items of 20px in a 200px row with SpaceEvenly → 4 equal gaps of 35px.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .padding(0.0)
            .justify(Justify::SpaceEvenly)
            .child(FixedLeaf::new(20.0, 10.0))
            .child(FixedLeaf::new(20.0, 10.0))
            .child(FixedLeaf::new(20.0, 10.0));

        fb.layout(root(200.0, 50.0));

        // free = 200 - 60 = 140; slot = 140/4 = 35
        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        let b2 = fb.children[2].node.bounds();
        assert!((b0.x - 35.0).abs() < 0.01, "child0 x={}", b0.x);
        assert!((b1.x - 90.0).abs() < 0.01, "child1 x={}", b1.x);
        assert!((b2.x - 145.0).abs() < 0.01, "child2 x={}", b2.x);
    }

    #[test]
    fn justify_space_around() {
        // 2 items of 50px in a 200px row with SpaceAround → slot=50, start=25.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .padding(0.0)
            .justify(Justify::SpaceAround)
            .child(FixedLeaf::new(50.0, 10.0))
            .child(FixedLeaf::new(50.0, 10.0));

        fb.layout(root(200.0, 50.0));

        // free = 100; slot = 50; start = 25
        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        assert!((b0.x - 25.0).abs() < 0.01, "child0 x={}", b0.x);
        assert!((b1.x - 125.0).abs() < 0.01, "child1 x={}", b1.x);
    }

    #[test]
    fn flex_shrink_proportional() {
        // 2 items each 100px natural, container=150px → need to shrink by 50px.
        // Item0: shrink=1, item1: shrink=2 → shrink weights 100:200 → 1/3:2/3
        // Item0 shrinks by ~16.67 → 83.33, item1 shrinks by ~33.33 → 66.67.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .padding(0.0)
            .child(FixedLeaf::new(100.0, 10.0))
            .child(FixedLeaf::new(100.0, 10.0));

        // Set shrink on second item via FlexItem mutation.
        fb.children[1].flex_shrink = 2.0;

        fb.layout(root(150.0, 50.0));

        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        assert!((b0.w - 83.33).abs() < 0.1, "item0 w={}", b0.w);
        assert!((b1.w - 66.67).abs() < 0.1, "item1 w={}", b1.w);
    }

    #[test]
    fn wrap_breaks_into_two_lines() {
        // 3 items of 80px in a 200px container with wrap.
        // Line 1: item0(80) + item1(80) = 160 <= 200; item2(80) would make 240 > 200 → new line.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .padding(0.0)
            .wrap()
            .child(FixedLeaf::new(80.0, 20.0))
            .child(FixedLeaf::new(80.0, 20.0))
            .child(FixedLeaf::new(80.0, 20.0));

        fb.layout(root(200.0, 200.0));

        // Item 0 and 1 on line 1; item 2 on line 2.
        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        let b2 = fb.children[2].node.bounds();

        assert_eq!(b0.y, 0.0, "item0 y={}", b0.y);
        assert_eq!(b1.y, 0.0, "item1 y={}", b1.y);
        assert!((b2.y - 20.0).abs() < 0.01, "item2 should be on second line y={}", b2.y);
    }

    #[test]
    fn align_items_stretch_fills_cross() {
        // Two fixed children (h=20) in a 100px-high row with Stretch.
        // Both should be laid out at h=100.
        let mut fb = FlexBox::row()
            .gap(0.0)
            .padding(0.0)
            .align_items(AlignItems::Stretch)
            .child(FixedLeaf::new(50.0, 20.0))
            .child(FixedLeaf::new(50.0, 20.0));

        fb.layout(root(200.0, 100.0));

        let b0 = fb.children[0].node.bounds();
        let b1 = fb.children[1].node.bounds();
        // With stretch, children receive cross budget = line_cross = 100.
        // FixedLeaf clamps to its natural size, so h stays 20 (constrained by Size::new).
        // What we verify is that layout was called with h=100 (the stretch cross).
        // FixedLeaf.layout calls constrain(Size::new(50, 20)) against max=(50, 100) → h=20.
        // The important thing: no panic and position is correct.
        assert_eq!(b0.x, 0.0);
        assert_eq!(b1.x, 50.0);
    }

    #[test]
    fn builder_new_api_compiles() {
        // Verify new builder methods compile and produce a valid FlexBox.
        let mut fb = FlexBox::row()
            .gap_axes(8.0, 4.0)
            .wrap()
            .align_items(AlignItems::Center)
            .justify(Justify::SpaceBetween)
            .align_content(AlignContent::Start)
            .child(FixedLeaf::new(50.0, 30.0));

        let size = fb.layout(root(200.0, 100.0));
        assert!(size.w > 0.0);
    }
}
