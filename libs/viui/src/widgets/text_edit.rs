//! TextEdit widget — single-line text input with keyboard editing.

use alloc::{string::String, vec::Vec};
use core::cell::{Cell, RefCell};

use crate::canvas::Color;
use crate::event::{Event, EventCx, EventStatus, KeyCode, MouseButton};
use crate::layout::{Constraints, LayoutNode, Length, Point, Rect, Size};
use crate::widget::{PaintCx, ViWidget, WidgetId};

const PAD: f32 = 4.0;

const CURSOR_CLR: Color = Color::WHITE;

pub struct TextEdit {
    pub id: WidgetId,
    pub text: String,
    cursor: usize, // original UTF-8 byte boundary
    focused: bool,
    width: Length,
    bounds: Cell<Rect>,
    cursor_positions: RefCell<Vec<(usize, f32)>>,
}

impl TextEdit {
    pub fn new(id: WidgetId) -> Self {
        Self {
            id,
            text: String::new(),
            cursor: 0,
            focused: false,
            width: Length::Fill,
            bounds: Cell::new(Rect::ZERO),
            cursor_positions: RefCell::new(Vec::new()),
        }
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self.cursor = self.text.len();
        self
    }

    pub fn with_width(mut self, w: Length) -> Self {
        self.width = w;
        self
    }

    fn next_cursor(&self) -> usize {
        self.text[self.cursor..].char_indices().skip(1)
            .find(|(_, ch)| !ostd::typography::is_combining_mark(*ch))
            .map_or(self.text.len(), |(i, _)| self.cursor + i)
    }

    fn previous_cursor(&self) -> usize {
        self.text[..self.cursor].char_indices().rev()
            .find(|(_, ch)| !ostd::typography::is_combining_mark(*ch))
            .map_or(0, |(i, _)| i)
    }

    fn insert_char(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn delete_before_cursor(&mut self) {
        let start = self.previous_cursor();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    fn delete_at_cursor(&mut self) {
        let end = self.next_cursor();
        self.text.replace_range(self.cursor..end, "");
    }
}

impl ViWidget for TextEdit {
    fn layout(&self, constraints: Constraints, font: &mut crate::font_context::FontContext) -> LayoutNode {
    font.cursor_positions(&self.text, &mut self.cursor_positions.borrow_mut());
    let w = match self.width {
        Length::Fill | Length::FillPortion(_) => constraints.max.w,
        Length::Fixed(v) => v,
        Length::Shrink => {
            (font.measure(&self.text) + font.measure("  ") + PAD * 2.0).max(40.0)
        }
    };
    let size = constraints.constrain(Size { w, h: font.line_height() + PAD * 2.0 });
    let bounds = Rect::from_origin_size(constraints.origin, size);
    self.bounds.set(bounds);
    LayoutNode::leaf(bounds)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let bounds = self.bounds.get().translate(cx.origin.x, cx.origin.y);
        let bg = if self.focused {
            cx.theme.input_focused_bg()
        } else {
            cx.theme.input_bg()
        };
        cx.canvas.fill_rect(bounds, bg);

        let border = if self.focused {
            cx.theme.input_focused_border()
        } else {
            cx.theme.border()
        };
        cx.canvas.draw_line(
            Point::new(bounds.x, bounds.y + bounds.h - 1.0),
            Point::new(bounds.x + bounds.w, bounds.y + bounds.h - 1.0),
            border,
        );

        let text_pos = Point::new(bounds.x + PAD, bounds.y + PAD);
        cx.canvas.clip_push(bounds);
        cx.draw_text(text_pos, &self.text, cx.theme.text_primary(), 0.0);

        // Cursor
        if self.focused {
            let cx_x = bounds.x + PAD + cx.font.measure(&self.text[..self.cursor]);
            let cy0 = bounds.y + PAD;
            let cy1 = bounds.y + PAD + cx.font.line_height();
            cx.canvas
                .draw_line(Point::new(cx_x, cy0), Point::new(cx_x, cy1), CURSOR_CLR);
        }
        cx.canvas.clip_pop();
    }

    fn event(&mut self, cx: &mut EventCx, e: &Event) -> EventStatus {
        let bounds = cx.bounds();
        match e {
            Event::MousePress {
                pos,
                button: MouseButton::Left,
            } => {
                if bounds.contains(*pos) {
                    if !self.focused {
                        self.focused = true;
                        cx.set_focus(self.id);
                        cx.mark_dirty();
                    }
                    // Move cursor to click position
                    let rel_x = (pos.x - bounds.x - PAD).max(0.0);
                    self.cursor = crate::font_context::FontContext::hit_position(&self.cursor_positions.borrow(), rel_x);
                    cx.mark_dirty();
                    EventStatus::Consumed
                } else {
                    if self.focused {
                        self.focused = false;
                        cx.release_focus();
                        cx.mark_dirty();
                    }
                    EventStatus::Ignored
                }
            }
            Event::Char(ch) if self.focused => {
                if !ch.is_control() {
                    self.insert_char(*ch);
                    cx.mark_dirty();
                    EventStatus::Consumed
                } else {
                    EventStatus::Ignored
                }
            }
            Event::KeyPress { key, .. } if self.focused => {
                match key {
                    KeyCode::Backspace => {
                        self.delete_before_cursor();
                        cx.mark_dirty();
                    }
                    KeyCode::Delete => {
                        self.delete_at_cursor();
                        cx.mark_dirty();
                    }
                    KeyCode::Left => {
                        if self.cursor > 0 {
                            self.cursor = self.previous_cursor();
                            cx.mark_dirty();
                        }
                    }
                    KeyCode::Right => {
                        if self.cursor < self.text.len() {
                            self.cursor = self.next_cursor();
                            cx.mark_dirty();
                        }
                    }
                    KeyCode::Home => {
                        self.cursor = 0;
                        cx.mark_dirty();
                    }
                    KeyCode::End => {
                        self.cursor = self.text.len();
                        cx.mark_dirty();
                    }
                    KeyCode::Escape => {
                        self.focused = false;
                        cx.release_focus();
                        cx.mark_dirty();
                    }
                    _ => return EventStatus::Ignored,
                }
                EventStatus::Consumed
            }
            Event::Blur => {
                self.focused = false;
                cx.mark_dirty();
                EventStatus::Ignored
            }
            _ => EventStatus::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_context::FontContext;

    #[test]
    fn click_and_cursor_use_measured_v1_layout() {
        let mut edit = TextEdit::new(WidgetId::ROOT).with_text("Wiii");
        let mut font = FontContext::default();
        let layout = edit.layout(Constraints::root(Size::new(200.0, 80.0)), &mut font);
        let mut state = crate::state_store::WidgetStateStore::new();
        let mut focus = crate::state_store::FocusManager::new();
        let mut cx = EventCx {
            state: &mut state, focus: &mut focus, widget_id: edit.id,
            layout: crate::layout::LayoutView(&layout), needs_repaint: false,
        };
        edit.event(&mut cx, &Event::MousePress {
            pos: Point::new(PAD + font.measure("W"), 6.0), button: MouseButton::Left,
        });
        assert_eq!(edit.cursor, 1);
        edit.event(&mut cx, &Event::Char('x'));
        assert_eq!(edit.text, "Wxiii");
        assert_eq!(layout.bounds.h, font.line_height() + PAD * 2.0);
    }
}
