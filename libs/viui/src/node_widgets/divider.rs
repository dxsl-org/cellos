// SPDX-License-Identifier: MIT
//! Divider — horizontal or vertical separator line.

extern crate alloc;
use alloc::vec::Vec;
use core::cell::Cell;

use crate::canvas::Color;
use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Axis, Constraints, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::SubscriptionHandle;

/// Single-pixel separator line.
///
/// Horizontal variant expands to full container width, 1 px tall.
/// Vertical variant expands to full container height, 1 px wide.
pub struct Divider {
    axis: Axis,
    /// Line colour; `None` uses the theme's `divider`.
    color: Option<Color>,
    bounds_cache: Cell<Rect>,
}

impl Divider {
    /// 1 px horizontal rule spanning full width.
    pub fn horizontal() -> Self {
        Self {
            axis: Axis::Horizontal,
            color: None,
            bounds_cache: Cell::new(Rect::ZERO),
        }
    }

    /// 1 px vertical rule spanning full height.
    pub fn vertical() -> Self {
        Self {
            axis: Axis::Vertical,
            color: None,
            bounds_cache: Cell::new(Rect::ZERO),
        }
    }

    /// Override the default separator color.
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }
}

impl ViNode for Divider {
    fn layout(
        &mut self,
        constraints: Constraints,
        _font: &mut crate::font_context::FontContext,
    ) -> Size {
        let size = match self.axis {
            Axis::Horizontal => constraints.constrain(Size {
                w: constraints.max.w,
                h: 1.0,
            }),
            Axis::Vertical => constraints.constrain(Size {
                w: 1.0,
                h: constraints.max.h,
            }),
        };
        self.bounds_cache
            .set(Rect::from_origin_size(constraints.origin, size));
        size
    }

    fn bounds(&self) -> Rect {
        self.bounds_cache.get()
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let color = self.color.unwrap_or_else(|| cx.theme.divider());
        cx.canvas.fill_rect(self.bounds_cache.get(), color);
    }

    // Dividers are non-interactive.
    fn event(&mut self, _event: &Event) -> bool {
        false
    }

    fn collect_dirty_handles(&mut self, _region: DirtyRegion) -> Vec<SubscriptionHandle> {
        alloc::vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::Divider;

    use crate::canvas::Color;
    use crate::node_widgets::style::test_support::{fills, paint_node};
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};

    #[test]
    fn line_uses_theme_divider_unless_overridden() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let painted = fills(&paint_node(&mut Divider::horizontal(), theme));
            assert_eq!(painted[0].1, theme.divider());
            let red = Color::rgb(200, 0, 0);
            let painted = fills(&paint_node(&mut Divider::vertical().color(red), theme));
            assert_eq!(painted[0].1, red);
        }
    }
}
