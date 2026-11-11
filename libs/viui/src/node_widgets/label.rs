// SPDX-License-Identifier: MIT
//! Label v2 — text widget driven by a `Signal<String>`.

use crate::canvas::Color;
use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::{Signal, SubscriptionHandle};

extern crate alloc;
use alloc::{string::String, vec::Vec};

/// Text display widget.
///
/// `text` is a `Signal<String>` — call `signal.set()` from anywhere to update
/// the displayed text. The app runner will repaint on the next tick.
pub struct Label {
    pub text: Signal<String>,
    /// Text colour; `None` uses the theme's `text_primary`.
    pub color: Option<Color>,
    bounds: Rect,
}

impl Label {
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            color: None,
            bounds: Rect::ZERO,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl ViNode for Label {
    fn layout(
        &mut self,
        constraints: Constraints,
        font: &mut crate::font_context::FontContext,
    ) -> Size {
        let desired = Size {
            w: font.measure(&self.text.get()),
            h: font.line_height(),
        };
        let size = constraints.constrain(desired);
        self.bounds = Rect::from_origin_size(constraints.origin, size);
        size
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let pos = crate::layout::Point::new(self.bounds.x, self.bounds.y);
        let color = self.color.unwrap_or_else(|| cx.theme.text_primary());
        cx.draw_text(pos, &self.text.get(), color);
    }

    fn event(&mut self, _event: &Event) -> bool {
        false
    }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        let rect = self.bounds;
        let h = self.text.subscribe(move || {
            region.borrow_mut().mark_layout(rect);
        });
        alloc::vec![h]
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::Label;
    use alloc::rc::Rc;
    use core::cell::RefCell;

    use crate::canvas::Color;
    use crate::dirty::DirtyRect;
    use crate::layout::{Constraints, Size};
    use crate::node::ViNode;
    use crate::node_widgets::style::test_support::{paint_node, text_color};
    use crate::signal::Signal;
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};

    #[test]
    fn text_follows_the_theme_unless_overridden() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let mut label = Label::new(Signal::new("Status".into()));
            let buf = paint_node(&mut label, theme);
            assert_eq!(text_color(&buf, "Status"), Some(theme.text_primary()));

            let red = Color::rgb(200, 0, 0);
            let mut label = Label::new(Signal::new("Status".into())).with_color(red);
            let buf = paint_node(&mut label, theme);
            assert_eq!(text_color(&buf, "Status"), Some(red));
        }
    }

    #[test]
    fn signal_update_marks_the_label_bounds_dirty() {
        let text = Signal::new(alloc::string::String::from("idle"));
        let mut label = Label::new(text.clone());
        label.layout(
            Constraints::root(Size::new(80.0, 24.0)),
            &mut crate::font_context::FontContext::default(),
        );
        let dirty = Rc::new(RefCell::new(DirtyRect::new()));
        let _handles = label.collect_dirty_handles(Rc::clone(&dirty));

        text.set(alloc::string::String::from("ready"));

        assert_eq!(dirty.borrow_mut().take(), Some(label.bounds()));
    }
}
