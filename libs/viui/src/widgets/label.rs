//! Label widget — static text display.

use alloc::string::String;

use crate::canvas::{Color, TextStyle};
use crate::event::{Event, EventCx, EventStatus};
use crate::layout::{Constraints, LayoutNode, Rect, Size};
use crate::widget::{PaintCx, ViWidget};


pub struct Label {
    pub text: String,
    pub style: TextStyle,
    bounds: core::cell::Cell<Rect>,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextStyle::DEFAULT,
            bounds: core::cell::Cell::new(Rect::ZERO),
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.style.color = color;
        self
    }

    /// Measure with the font and size used to paint this label.
    pub fn measure(&self, font: &mut crate::font_context::FontContext) -> Size {
        let px = if self.style.size_px > 0 { self.style.size_px as f32 } else { font.size_px };
        Size { w: font.atlas.measure(&self.text, px), h: font.atlas.line_height(px) }
    }
}

impl ViWidget for Label {
    fn layout(&self, constraints: Constraints, font: &mut crate::font_context::FontContext) -> LayoutNode {
    let desired = self.measure(font);
    let size = constraints.constrain(desired);
    let bounds = Rect::from_origin_size(constraints.origin, size);
    self.bounds.set(bounds);
    LayoutNode::leaf(bounds)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let pos = self.bounds.get().origin().offset(cx.origin.x, cx.origin.y);
        cx.draw_text(pos, &self.text, self.style.color, self.style.size_px as f32);
    }

    fn event(&mut self, _cx: &mut EventCx, _e: &Event) -> EventStatus {
        EventStatus::Ignored
    }
}
