//! Checkbox widget — toggle with label.

use alloc::string::String;

use crate::event::{Event, EventCx, EventStatus, MouseButton};
use crate::layout::{Constraints, LayoutNode, Point, Rect, Size};
use crate::widget::{PaintCx, ViWidget, WidgetId};
use crate::widgets::label::Label;
use ostd::display::linear::CONTROL_RADIUS;

const BOX_SIZE: f32 = 14.0;
const GAP: f32 = 6.0;

pub struct Checkbox {
    pub id: WidgetId,
    pub checked: bool,
    label: Label,
    hovered: bool,
    bounds: core::cell::Cell<Rect>,
}

impl Checkbox {
    pub fn new(id: WidgetId, checked: bool, label: impl Into<String>) -> Self {
        Self {
            id,
            checked,
            label: Label::new(label),
            hovered: false,
            bounds: core::cell::Cell::new(Rect::ZERO),
        }
    }

    /// Consume and return the latest checked state.
    pub fn is_checked(&self) -> bool {
        self.checked
    }
}

impl ViWidget for Checkbox {
    fn layout(
        &self,
        constraints: Constraints,
        font: &mut crate::font_context::FontContext,
    ) -> LayoutNode {
        let label_size = self.label.measure(font);
        let desired = Size {
            w: BOX_SIZE + GAP + label_size.w,
            h: BOX_SIZE.max(label_size.h),
        };
        let size = constraints.constrain(desired);
        let bounds = Rect::from_origin_size(constraints.origin, size);
        self.bounds.set(bounds);
        LayoutNode::leaf(bounds)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let bounds = self.bounds.get().translate(cx.origin.x, cx.origin.y);
        let orig = Point::new(bounds.x, bounds.y + (bounds.h - BOX_SIZE) * 0.5);
        let box_rect = Rect::new(orig.x, orig.y, BOX_SIZE, BOX_SIZE);

        let bg = if self.hovered {
            cx.theme.button_hovered()
        } else {
            cx.theme.input_bg()
        };
        cx.canvas.fill_round_rect(box_rect, CONTROL_RADIUS, bg);
        cx.canvas
            .stroke_round_rect(box_rect, CONTROL_RADIUS, cx.theme.border());

        if self.checked {
            // Checkmark: two diagonal lines forming a tick
            let check = cx.theme.check_color();
            let m = 3.0; // margin
            cx.canvas.draw_line(
                Point::new(orig.x + m, orig.y + BOX_SIZE / 2.0),
                Point::new(orig.x + BOX_SIZE / 2.0 - 1.0, orig.y + BOX_SIZE - m),
                check,
            );
            cx.canvas.draw_line(
                Point::new(orig.x + BOX_SIZE / 2.0 - 1.0, orig.y + BOX_SIZE - m),
                Point::new(orig.x + BOX_SIZE - m, orig.y + m),
                check,
            );
        }

        let text_pos = Point::new(
            bounds.x + BOX_SIZE + GAP,
            bounds.y + (bounds.h - cx.font.line_height()) * 0.5,
        );
        cx.draw_text(text_pos, &self.label.text, cx.theme.text_primary(), 0.0);
    }

    fn event(&mut self, cx: &mut EventCx, e: &Event) -> EventStatus {
        let bounds = cx.bounds();
        match e {
            Event::MouseMove { pos } => {
                let h = bounds.contains(*pos);
                if h != self.hovered {
                    self.hovered = h;
                    cx.mark_dirty();
                }
                EventStatus::Ignored
            }
            Event::MousePress {
                pos,
                button: MouseButton::Left,
            } => {
                if bounds.contains(*pos) {
                    self.checked = !self.checked;
                    cx.mark_dirty();
                    EventStatus::Consumed
                } else {
                    EventStatus::Ignored
                }
            }
            _ => EventStatus::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Checkbox;

    use crate::font_context::FontContext;
    use crate::layout::{Constraints, Size};
    use crate::node_widgets::style::test_support::{fills, lines, record, strokes, text_color};
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};
    use crate::widget::{PaintCx, ViWidget, WidgetId};

    #[test]
    fn box_check_and_label_use_theme_tokens() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let cb = Checkbox::new(WidgetId(1), true, "Sync");
            let mut font = FontContext::default();
            cb.layout(Constraints::root(Size::new(200.0, 40.0)), &mut font);
            let buf = record(|canvas| cb.paint(&mut PaintCx::with_theme(canvas, theme, &mut font)));
            let painted = fills(&buf);
            assert_eq!(painted[0].1, theme.input_bg());
            assert_eq!(strokes(&buf), [theme.border()]);
            assert_eq!(lines(&buf), [theme.check_color(); 2]);
            assert_eq!(text_color(&buf, "Sync"), Some(theme.text_primary()));
        }
    }
}
