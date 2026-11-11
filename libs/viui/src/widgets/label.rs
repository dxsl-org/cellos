//! Label widget — static text display.

use alloc::string::String;

use crate::canvas::Color;
use crate::event::{Event, EventCx, EventStatus};
use crate::layout::{Constraints, LayoutNode, Rect, Size};
use crate::widget::{PaintCx, ViWidget};

pub struct Label {
    pub text: String,
    /// Text colour; `None` uses the theme's `text_primary`.
    pub color: Option<Color>,
    /// Explicit size in px; `0` inherits the paint context's font size.
    pub size_px: u16,
    bounds: core::cell::Cell<Rect>,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
            size_px: 0,
            bounds: core::cell::Cell::new(Rect::ZERO),
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Measure with the font and size used to paint this label.
    pub fn measure(&self, font: &mut crate::font_context::FontContext) -> Size {
        let px = if self.size_px > 0 {
            self.size_px as f32
        } else {
            font.size_px
        };
        Size {
            w: font.atlas.measure(&self.text, px),
            h: font.atlas.line_height(px),
        }
    }
}

impl ViWidget for Label {
    fn layout(
        &self,
        constraints: Constraints,
        font: &mut crate::font_context::FontContext,
    ) -> LayoutNode {
        let desired = self.measure(font);
        let size = constraints.constrain(desired);
        let bounds = Rect::from_origin_size(constraints.origin, size);
        self.bounds.set(bounds);
        LayoutNode::leaf(bounds)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let pos = self.bounds.get().origin().offset(cx.origin.x, cx.origin.y);
        let color = self.color.unwrap_or_else(|| cx.theme.text_primary());
        cx.draw_text(pos, &self.text, color, self.size_px as f32);
    }

    fn event(&mut self, _cx: &mut EventCx, _e: &Event) -> EventStatus {
        EventStatus::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::Label;

    use crate::canvas::Color;
    use crate::font_context::FontContext;
    use crate::layout::{Constraints, Size};
    use crate::node_widgets::style::test_support::{record, text_color};
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};
    use crate::widget::{PaintCx, ViWidget};

    fn paint(label: &Label, theme: &dyn ViTheme) -> crate::gpu_cmd::GpuCommandBuffer {
        let mut font = FontContext::default();
        label.layout(Constraints::root(Size::new(200.0, 40.0)), &mut font);
        record(|canvas| label.paint(&mut PaintCx::with_theme(canvas, theme, &mut font)))
    }

    #[test]
    fn text_follows_the_theme_unless_overridden() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let buf = paint(&Label::new("Hello"), theme);
            assert_eq!(text_color(&buf, "Hello"), Some(theme.text_primary()));
            let red = Color::rgb(200, 0, 0);
            let buf = paint(&Label::new("Hello").with_color(red), theme);
            assert_eq!(text_color(&buf, "Hello"), Some(red));
        }
    }
}
