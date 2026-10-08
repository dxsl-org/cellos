// SPDX-License-Identifier: MIT
//! TextEdit — single-line text input with cursor and keyboard editing.

extern crate alloc;
use alloc::{boxed::Box, string::String, vec::Vec};
use core::cell::Cell;

use crate::canvas::Color;
use crate::dirty::DirtyRegion;
use crate::event::{Event, KeyCode};
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::{Signal, SubscriptionHandle};

const PADDING: f32 = 4.0;

/// Single-line text input field.
///
/// Receives keyboard events while focused (the event routing layer is expected
/// to dispatch `Focus`/`Blur` lifecycle events and `KeyPress`/`Char` direct
/// events to the focused widget).
///
/// # Cursor
///
/// Cursor position is a byte offset into the UTF-8 string. Navigation
/// (`Left`/`Right`/`Home`/`End`) advances by full Unicode scalar values.
/// Callback invoked with the submitted line on Enter.
type SubmitHandler = Box<dyn Fn(&str)>;

pub struct TextEdit {
    /// Current text content.
    pub text: Signal<String>,
    /// Hint shown when the field is empty and unfocused.
    pub placeholder: Signal<String>,
    on_submit: Option<SubmitHandler>,
    /// Byte offset of the cursor in `text`.
    cursor_pos: Cell<usize>,
    focused: Cell<bool>,
    bounds_cache: Cell<Rect>,
    cursor_positions: Vec<(usize, f32)>,
}

impl TextEdit {
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            placeholder: Signal::new(String::new()),
            on_submit: None,
            cursor_pos: Cell::new(0),
            focused: Cell::new(false),
            bounds_cache: Cell::new(Rect::ZERO),
            cursor_positions: Vec::new(),
        }
    }

    /// Attach a reactive placeholder signal.
    pub fn placeholder(mut self, s: Signal<String>) -> Self {
        self.placeholder = s;
        self
    }

    /// Attach a static placeholder string.
    pub fn placeholder_str(self, s: impl Into<String>) -> Self {
        self.placeholder(Signal::new(s.into()))
    }

    /// Callback fired with the current text when the user presses Enter.
    pub fn on_submit(mut self, f: impl Fn(&str) + 'static) -> Self {
        self.on_submit = Some(Box::new(f));
        self
    }

    /// Advance by a base scalar and all its following combining marks.
    fn advance_cursor(text: &str, pos: usize) -> usize {
        text[pos..].char_indices().skip(1)
            .find(|(_, ch)| !ostd::typography::is_combining_mark(*ch))
            .map_or(text.len(), |(i, _)| pos + i)
    }

    fn retreat_cursor(text: &str, pos: usize) -> usize {
        text[..pos].char_indices().rev()
            .find(|(_, ch)| !ostd::typography::is_combining_mark(*ch))
            .map_or(0, |(i, _)| i)
    }
}

impl ViNode for TextEdit {
    fn layout(&mut self, constraints: Constraints, font: &mut crate::font_context::FontContext) -> Size {
    font.cursor_positions(&self.text.get(), &mut self.cursor_positions);
    let cursor = self.cursor_pos.get().min(self.text.get().len());
    self.cursor_pos.set(self.cursor_positions.iter().rev().find(|p| p.0 <= cursor).map_or(0, |p| p.0));
    let size = constraints.constrain(Size {
        w: constraints.max.w,
        h: font.line_height() + PADDING * 2.0,
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

        // Background — slightly brighter when focused
        let bg = if self.focused.get() {
            Color::rgb(20, 20, 50)
        } else {
            Color::rgb(25, 25, 38)
        };
        cx.canvas.fill_rect(b, bg);

        // Border — accent color when focused
        let border = if self.focused.get() {
            Color::rgb(80, 120, 220)
        } else {
            Color::rgb(70, 70, 95)
        };
        let x0 = b.x;
        let y0 = b.y;
        let x1 = b.x + b.w;
        let y1 = b.y + b.h;
        cx.canvas
            .draw_line(Point::new(x0, y0), Point::new(x1, y0), border); // top
        cx.canvas
            .draw_line(Point::new(x1, y0), Point::new(x1, y1), border); // right
        cx.canvas
            .draw_line(Point::new(x1, y1), Point::new(x0, y1), border); // bottom
        cx.canvas
            .draw_line(Point::new(x0, y1), Point::new(x0, y0), border); // left

        let ty = b.y + (b.h - cx.line_height()) * 0.5;

        // Clip text content to the inner area
        cx.canvas.clip_push(Rect {
            x: b.x + 1.0,
            y: b.y + 1.0,
            w: b.w - 2.0,
            h: b.h - 2.0,
        });

        let text = self.text.get();
        if text.is_empty() && !self.focused.get() {
            // Show placeholder
            let ph = self.placeholder.get();
            if !ph.is_empty() {
                cx.draw_text(
                    Point::new(b.x + PADDING, ty.max(b.y)),
                    &ph,
                    Color::rgb(100, 100, 130),
                );
            }
        } else {
            // Draw the text content
            cx.draw_text(
                Point::new(b.x + PADDING, ty.max(b.y)),
                &text,
                Color::rgb(220, 220, 230),
            );

            // Draw cursor bar while focused
            if self.focused.get() {
                let cursor_byte = self.cursor_pos.get().min(text.len());
                let cursor_x = b.x + PADDING + cx.measure(&text[..cursor_byte]);
                cx.canvas.draw_line(
                    Point::new(cursor_x, b.y + 3.0),
                    Point::new(cursor_x, b.y + b.h - 3.0),
                    Color::WHITE,
                );
            }
        }

        cx.canvas.clip_pop();
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn event(&mut self, event: &Event) -> bool {
        match event {
            Event::Focus => {
                self.focused.set(true);
                true
            }
            Event::Blur => {
                self.focused.set(false);
                true
            }

            // Character input — insert at cursor
            Event::Char(ch) if self.focused.get() => {
                let pos = self.cursor_pos.get();
                let byte_len = ch.len_utf8();
                self.text.update(|s| {
                    s.insert(pos, *ch);
                });
                self.cursor_pos.set(pos + byte_len);
                true
            }

            // Structural keys
            Event::KeyPress { key, .. } if self.focused.get() => {
                match key {
                    KeyCode::Backspace => {
                        let pos = self.cursor_pos.get();
                        if pos > 0 {
                            // Need a snapshot for retreat calculation before mutating
                            let new_pos = {
                                let text = self.text.get();
                                Self::retreat_cursor(&text, pos)
                            };
                            self.text.update(|s| {
                                s.replace_range(new_pos..pos, "");
                            });
                            self.cursor_pos.set(new_pos);
                        }
                        true
                    }
                    KeyCode::Delete => {
                        let pos = self.cursor_pos.get();
                        let len = self.text.get().len();
                        if pos < len {
                            let end = Self::advance_cursor(&self.text.get(), pos);
                            self.text.update(|s| {
                                s.replace_range(pos..end, "");
                            });
                        }
                        true
                    }
                    KeyCode::Left => {
                        let pos = self.cursor_pos.get();
                        let new_pos = {
                            let text = self.text.get();
                            Self::retreat_cursor(&text, pos)
                        };
                        self.cursor_pos.set(new_pos);
                        true
                    }
                    KeyCode::Right => {
                        let pos = self.cursor_pos.get();
                        let new_pos = {
                            let text = self.text.get();
                            Self::advance_cursor(&text, pos)
                        };
                        self.cursor_pos.set(new_pos);
                        true
                    }
                    KeyCode::Home => {
                        self.cursor_pos.set(0);
                        true
                    }
                    KeyCode::End => {
                        let len = self.text.get().len();
                        self.cursor_pos.set(len);
                        true
                    }
                    KeyCode::Enter => {
                        // Clone text before calling callback to release the Ref
                        let text_snap: String = (*self.text.get()).clone();
                        if let Some(cb) = &self.on_submit {
                            cb(&text_snap);
                        }
                        true
                    }
                    _ => false,
                }
            }

            // Gain focus on click
            Event::MousePress { pos, .. } if self.bounds_cache.get().contains(*pos) => {
                self.focused.set(true);
                self.cursor_pos.set(crate::font_context::FontContext::hit_position(
                    &self.cursor_positions, pos.x - self.bounds_cache.get().x - PADDING,
                ));
                true
            }

            _ => false,
        }
    }

    fn collect_dirty_handles(&mut self, region: DirtyRegion) -> Vec<SubscriptionHandle> {
        let bounds = self.bounds_cache.get();
        let h = self.text.subscribe(move || {
            region.borrow_mut().mark_layout(bounds);
        });
        alloc::vec![h]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_context::FontContext;

    #[test]
    fn clicking_proportional_text_places_cursor_at_measured_boundary() {
        let text = Signal::new(String::from("Wiii"));
        let mut edit = TextEdit::new(text.clone());
        let mut font = FontContext::default();
        edit.layout(Constraints::root(Size::new(200.0, 80.0)), &mut font);
        edit.event(&Event::MousePress {
            pos: Point::new(PADDING + font.measure("W"), 6.0),
            button: crate::event::MouseButton::Left,
        });
        assert_eq!(edit.cursor_pos.get(), 1);
        edit.event(&Event::Char('x'));
        assert_eq!(&*text.get(), "Wxiii");
    }

    #[test]
    fn vietnamese_navigation_and_deletion_keep_combining_sequence_whole() {
        let text = Signal::new(String::from("e\u{0302}\u{0301}W"));
        let mut edit = TextEdit::new(text.clone());
        edit.focused.set(true);
        edit.cursor_pos.set("e\u{0302}\u{0301}".len());
        edit.event(&Event::KeyPress { key: KeyCode::Backspace, modifiers: crate::event::Modifiers::default() });
        assert_eq!(&*text.get(), "W");
        assert_eq!(edit.cursor_pos.get(), 0);
    }
}
