// SPDX-License-Identifier: MIT
//! Button v2 — clickable widget with a `Box<dyn Fn()>` callback.

extern crate alloc;
use alloc::{boxed::Box, string::String};

use crate::event::{Event, MouseButton};
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use ostd::display::linear::CONTROL_RADIUS;

const PAD: f32 = 6.0;

/// Clickable button.
///
/// `on_click` is called synchronously inside `event()` when a confirmed
/// left-click occurs (press + release on the button bounds).
pub struct Button {
    pub label: String,
    pub on_click: Box<dyn Fn()>,
    hovered: bool,
    pressed: bool,
    bounds: Rect,
}

impl Button {
    pub fn new(label: impl Into<String>, on_click: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            on_click: Box::new(on_click),
            hovered: false,
            pressed: false,
            bounds: Rect::ZERO,
        }
    }
}

impl ViNode for Button {
    fn layout(
        &mut self,
        constraints: Constraints,
        font: &mut crate::font_context::FontContext,
    ) -> Size {
        let desired = Size {
            w: font.measure(&self.label) + PAD * 2.0,
            h: font.line_height() + PAD * 2.0,
        };
        let size = constraints.constrain(desired);
        self.bounds = Rect::from_origin_size(constraints.origin, size);
        size
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let bg = if self.pressed {
            cx.theme.button_pressed()
        } else if self.hovered {
            cx.theme.button_hovered()
        } else {
            cx.theme.button_normal()
        };
        let b = self.bounds;
        cx.canvas.fill_round_rect(b, CONTROL_RADIUS, bg);
        cx.canvas
            .stroke_round_rect(b, CONTROL_RADIUS, cx.theme.border());

        let x = b.x + (b.w - cx.measure(&self.label)) * 0.5;
        let y = b.y + (b.h - cx.line_height()) * 0.5;
        let text = cx.theme.text_primary();
        cx.draw_text(Point::new(x, y), &self.label, text);
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn activate(&mut self) -> bool {
        (self.on_click)();
        true
    }

    fn event(&mut self, event: &Event) -> bool {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.bounds.contains(*pos);
                was != self.hovered
            }
            Event::MousePress {
                pos,
                button: MouseButton::Left,
            } if self.bounds.contains(*pos) => {
                self.pressed = true;
                true
            }
            Event::MouseRelease {
                pos,
                button: MouseButton::Left,
            } => {
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed && self.bounds.contains(*pos) {
                    (self.on_click)();
                    true
                } else {
                    false
                }
            }
            Event::TouchBegin { pos, .. } if self.bounds.contains(*pos) => {
                self.pressed = true;
                true
            }
            Event::TouchEnd { pos, .. } => {
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed && self.bounds.contains(*pos) {
                    (self.on_click)();
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::Button;
    use alloc::rc::Rc;
    use core::cell::Cell;

    use crate::event::{Event, MouseButton};
    use crate::layout::{Constraints, Point, Size};
    use crate::node::ViNode;
    use crate::node_widgets::style::test_support::{fills, paint_node, strokes, text_color};
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};

    #[test]
    fn paints_with_theme_button_tokens() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let mut button = Button::new("Apply", || {});
            let buf = paint_node(&mut button, theme);
            let painted = fills(&buf);
            assert_eq!(painted, [(button.bounds(), theme.button_normal())]);
            assert_eq!(strokes(&buf), [theme.border()]);
            assert_eq!(text_color(&buf, "Apply"), Some(theme.text_primary()));

            button.event(&Event::MousePress {
                pos: Point::new(1.0, 1.0),
                button: MouseButton::Left,
            });
            let buf = paint_node(&mut button, theme);
            assert_eq!(fills(&buf)[0].1, theme.button_pressed());
        }
    }

    #[test]
    fn callback_requires_a_completed_left_click_inside_bounds() {
        let calls = Rc::new(Cell::new(0u32));
        let callback_calls = Rc::clone(&calls);
        let mut button = Button::new("Apply", move || {
            callback_calls.set(callback_calls.get() + 1);
        });
        button.layout(
            Constraints::root(Size::new(100.0, 40.0)),
            &mut crate::font_context::FontContext::default(),
        );

        assert!(button.event(&Event::MousePress {
            pos: Point::new(1.0, 1.0),
            button: MouseButton::Left,
        }));
        assert!(!button.event(&Event::MouseRelease {
            pos: Point::new(99.0, 39.0),
            button: MouseButton::Left,
        }));
        assert_eq!(calls.get(), 0);

        assert!(button.event(&Event::MousePress {
            pos: Point::new(1.0, 1.0),
            button: MouseButton::Left,
        }));
        assert!(button.event(&Event::MouseRelease {
            pos: Point::new(1.0, 1.0),
            button: MouseButton::Left,
        }));
        assert_eq!(calls.get(), 1);
    }
}
