// SPDX-License-Identifier: MIT
//! Card — panel container with a background fill, border, and inner padding.

extern crate alloc;
use alloc::{boxed::Box, vec::Vec};
use core::cell::Cell;

use crate::canvas::Color;
use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::SubscriptionHandle;
use ostd::display::linear::PANEL_RADIUS;

/// Container that draws a tinted panel behind its child widget.
///
/// `padding` shrinks the child's allocated area on all sides.
/// The child's bounds are set relative to the card's padded interior.
pub struct Card {
    child: Box<dyn ViNode>,
    padding: f32,
    /// Panel fill; `None` uses the theme's `surface`.
    bg_color: Option<Color>,
    /// Outline; `None` uses the theme's `border`.
    border_color: Option<Color>,
    bounds_cache: Cell<Rect>,
}

impl Card {
    /// Wrap `child` with default padding (8 px) and the theme's panel colours.
    pub fn new(child: Box<dyn ViNode>) -> Self {
        Self {
            child,
            padding: 8.0,
            bg_color: None,
            border_color: None,
            bounds_cache: Cell::new(Rect::ZERO),
        }
    }

    /// Uniform padding on all sides.
    pub fn padding(mut self, p: f32) -> Self {
        self.padding = p;
        self
    }

    /// Background fill color.
    pub fn bg(mut self, c: Color) -> Self {
        self.bg_color = Some(c);
        self
    }

    /// Border color.
    pub fn border(mut self, c: Color) -> Self {
        self.border_color = Some(c);
        self
    }
}

impl ViNode for Card {
    fn layout(
        &mut self,
        constraints: Constraints,
        font: &mut crate::font_context::FontContext,
    ) -> Size {
        let pad = self.padding;

        // Give child the interior area — origin shifted in by padding.
        let inner = Constraints {
            origin: Point::new(constraints.origin.x + pad, constraints.origin.y + pad),
            min: Size { w: 0.0, h: 0.0 },
            max: Size {
                w: (constraints.max.w - pad * 2.0).max(0.0),
                h: (constraints.max.h - pad * 2.0).max(0.0),
            },
        };
        let inner_size = self.child.layout(inner, font);

        let size = constraints.constrain(Size {
            w: inner_size.w + pad * 2.0,
            h: inner_size.h + pad * 2.0,
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
        let bg = self.bg_color.unwrap_or_else(|| cx.theme.surface());
        let border = self.border_color.unwrap_or_else(|| cx.theme.border());
        cx.canvas.fill_round_rect(b, PANEL_RADIUS, bg);
        cx.canvas.stroke_round_rect(b, PANEL_RADIUS, border);
        // Child paints inside the padded interior
        self.child.paint(cx);
    }

    fn event(&mut self, event: &Event) -> bool {
        self.child.event(event)
    }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        self.child.collect_dirty_handles(region)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::Card;
    use alloc::boxed::Box;

    use crate::canvas::Color;
    use crate::node::ViNode;
    use crate::node_widgets::space::Space;
    use crate::node_widgets::style::test_support::{fills, paint_node, strokes};
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};

    #[test]
    fn panel_uses_theme_surface_and_border_unless_overridden() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let mut card = Card::new(Box::new(Space::new(20.0, 20.0)));
            let buf = paint_node(&mut card, theme);
            assert_eq!(fills(&buf), [(card.bounds(), theme.surface())]);
            assert_eq!(strokes(&buf), [theme.border()]);

            let (bg, border) = (Color::rgb(1, 2, 3), Color::rgb(4, 5, 6));
            let mut card = Card::new(Box::new(Space::new(20.0, 20.0)))
                .bg(bg)
                .border(border);
            let buf = paint_node(&mut card, theme);
            assert_eq!(fills(&buf)[0].1, bg);
            assert_eq!(strokes(&buf), [border]);
        }
    }
}
