// SPDX-License-Identifier: MIT
//! Dialog overlay widget — blocking modal card with title, message, and buttons.
//!
//! Construct via `Dialog::alert` (single OK button) or `Dialog::confirm`
//! (Cancel + Confirm). Both constructors accept an `OverlayActionQueue`; button
//! callbacks push `OverlayAction::Pop` to close the dialog after the user action.
//!
//! # Event handling
//!
//! Because Dialog is pushed as a **blocking** overlay, `ViApp` does not forward
//! events to the root widget while the dialog is visible. The dialog itself
//! consumes all events; clicks outside the dialog bounds also pop it.

extern crate alloc;
use alloc::{boxed::Box, string::String, vec::Vec};
use core::cell::Cell;

use crate::dirty::DirtyRegion;
use crate::event::{Event, MouseButton};
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::node_widgets::style::{self, HOVER_TINT, PRESSED_TINT};
use crate::overlay::{OverlayAction, OverlayActionQueue};
use crate::render_ctx::RenderCtx;
use crate::signal::SubscriptionHandle;
use ostd::display::linear::{CONTROL_RADIUS, PANEL_RADIUS};

// ─── DialogButton ────────────────────────────────────────────────────────────

struct DialogButton {
    label: String,
    is_primary: bool,
    /// Called on confirmed click (MouseRelease inside bounds after MousePress).
    action: Box<dyn Fn()>,
    bounds: Cell<Rect>,
    hovered: bool,
    pressed: bool,
}

// ─── Dialog ──────────────────────────────────────────────────────────────────

/// Blocking modal dialog.
///
/// Rendered as a centred card over a translucent scrim. The dialog lays itself
/// out during `ViApp`'s layout pass, so its bounds are always screen-centred
/// regardless of the window size.
pub struct Dialog {
    title: String,
    message: String,
    buttons: Vec<DialogButton>,
    bounds: Cell<Rect>,
    /// Used by `event()` to push `Pop` after a button action executes.
    queue: OverlayActionQueue,
}

impl Dialog {
    /// Simple alert dialog with a single OK button.
    ///
    /// `on_ok` is called before the dialog pops itself.
    pub fn alert(
        title: impl Into<String>,
        message: impl Into<String>,
        queue: OverlayActionQueue,
        on_ok: impl Fn() + 'static,
    ) -> Self {
        let q = queue.clone();
        Self {
            title: title.into(),
            message: message.into(),
            buttons: alloc::vec![DialogButton {
                label: "OK".into(),
                is_primary: true,
                action: Box::new(move || {
                    on_ok();
                    q.borrow_mut().push(OverlayAction::Pop);
                }),
                bounds: Cell::new(Rect::ZERO),
                hovered: false,
                pressed: false,
            }],
            bounds: Cell::new(Rect::ZERO),
            queue,
        }
    }

    /// Confirm dialog with Cancel (secondary) and Confirm (primary) buttons.
    ///
    /// `on_confirm` / `on_cancel` are called before the dialog pops itself.
    pub fn confirm(
        title: impl Into<String>,
        message: impl Into<String>,
        queue: OverlayActionQueue,
        on_confirm: impl Fn() + 'static,
        on_cancel: impl Fn() + 'static,
    ) -> Self {
        let q1 = queue.clone();
        let q2 = queue.clone();
        Self {
            title: title.into(),
            message: message.into(),
            buttons: alloc::vec![
                DialogButton {
                    label: "Cancel".into(),
                    is_primary: false,
                    action: Box::new(move || {
                        on_cancel();
                        q1.borrow_mut().push(OverlayAction::Pop);
                    }),
                    bounds: Cell::new(Rect::ZERO),
                    hovered: false,
                    pressed: false,
                },
                DialogButton {
                    label: "Confirm".into(),
                    is_primary: true,
                    action: Box::new(move || {
                        on_confirm();
                        q2.borrow_mut().push(OverlayAction::Pop);
                    }),
                    bounds: Cell::new(Rect::ZERO),
                    hovered: false,
                    pressed: false,
                },
            ],
            bounds: Cell::new(Rect::ZERO),
            queue,
        }
    }
}

impl ViNode for Dialog {
    fn layout(
        &mut self,
        constraints: Constraints,
        _font: &mut crate::font_context::FontContext,
    ) -> Size {
        // Card: 320×160 px centred on screen, clamped to available space.
        let w = 320.0f32.min(constraints.max.w - 32.0).max(120.0);
        let h = 160.0f32;
        let x = constraints.origin.x + (constraints.max.w - w) / 2.0;
        let y = constraints.origin.y + (constraints.max.h - h) / 2.0;
        self.bounds.set(Rect { x, y, w, h });

        // Buttons: right-aligned row at bottom of card (12 px margin).
        let btn_w = 80.0f32;
        let btn_h = 32.0f32;
        let btn_y = y + h - btn_h - 12.0;
        let n = self.buttons.len() as f32;
        for (i, btn) in self.buttons.iter_mut().enumerate() {
            // Stack right-to-left: primary button is rightmost.
            let slot = n - 1.0 - i as f32;
            let btn_x = x + w - 12.0 - (slot + 1.0) * (btn_w + 8.0) + 8.0;
            btn.bounds.set(Rect {
                x: btn_x,
                y: btn_y,
                w: btn_w,
                h: btn_h,
            });
        }

        Size::new(w, h)
    }

    fn bounds(&self) -> Rect {
        self.bounds.get()
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let b = self.bounds.get();
        let theme = cx.theme;

        // Card panel with a 1 px outline.
        cx.canvas.fill_round_rect(b, PANEL_RADIUS, theme.surface());
        cx.canvas.stroke_round_rect(b, PANEL_RADIUS, theme.border());

        cx.draw_text(
            Point::new(b.x + 16.0, b.y + 16.0),
            &self.title,
            theme.text_primary(),
        );
        cx.draw_text(
            Point::new(b.x + 16.0, b.y + 48.0),
            &self.message,
            theme.text_secondary(),
        );

        for btn in &self.buttons {
            let bb = btn.bounds.get();
            let (bg, fg) = if btn.is_primary {
                let accent = theme.accent();
                let bg = if btn.pressed {
                    style::tint(theme, accent, PRESSED_TINT)
                } else if btn.hovered {
                    style::tint(theme, accent, HOVER_TINT)
                } else {
                    accent
                };
                (bg, style::text_on(theme, accent))
            } else {
                let bg = if btn.pressed {
                    theme.button_pressed()
                } else if btn.hovered {
                    theme.button_hovered()
                } else {
                    theme.button_normal()
                };
                (bg, theme.text_primary())
            };
            cx.canvas.fill_round_rect(bb, CONTROL_RADIUS, bg);
            if !btn.is_primary {
                cx.canvas
                    .stroke_round_rect(bb, CONTROL_RADIUS, theme.border());
            }

            let text_x = bb.x + (bb.w - cx.measure(&btn.label)) / 2.0;
            let text_y = bb.y + (bb.h - cx.line_height()) / 2.0;
            cx.draw_text(Point::new(text_x, text_y), &btn.label, fg);
        }
    }

    /// Blocking dialog: consume all events.
    ///
    /// Clicks outside the card dismiss the dialog. Button presses/releases
    /// are tracked to give visual feedback and trigger the action on release.
    fn event(&mut self, event: &Event) -> bool {
        match event {
            Event::MouseMove { pos } => {
                for btn in &mut self.buttons {
                    btn.hovered = btn.bounds.get().contains(*pos);
                }
                // Consume: prevent the root from acting on mouse moves over the dim.
                true
            }
            Event::MousePress {
                pos,
                button: MouseButton::Left,
            } => {
                if self.bounds.get().contains(*pos) {
                    for btn in &mut self.buttons {
                        if btn.bounds.get().contains(*pos) {
                            btn.pressed = true;
                        }
                    }
                } else {
                    // Click outside → dismiss.
                    self.queue.borrow_mut().push(OverlayAction::Pop);
                }
                true // always consume
            }
            Event::MouseRelease {
                pos,
                button: MouseButton::Left,
            } => {
                for btn in &mut self.buttons {
                    let was_pressed = btn.pressed;
                    btn.pressed = false;
                    if was_pressed && btn.bounds.get().contains(*pos) {
                        (btn.action)();
                        return true;
                    }
                }
                true
            }
            // Blocking overlay: consume everything so nothing leaks to root.
            _ => true,
        }
    }

    fn collect_dirty_handles(&mut self, _region: DirtyRegion) -> Vec<SubscriptionHandle> {
        Vec::new()
    }

    fn is_focusable(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::Dialog;

    use crate::event::Event;
    use crate::node::ViNode;
    use crate::node_widgets::style::test_support::{fills, paint_node, strokes, text_color};
    use crate::node_widgets::style::{text_on, tint, HOVER_TINT};
    use crate::overlay::new_action_queue;
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};

    #[test]
    fn card_text_and_buttons_use_theme_tokens() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            let mut dialog =
                Dialog::confirm("Delete?", "Gone for good", new_action_queue(), || {}, || {});
            let buf = paint_node(&mut dialog, theme);
            let painted = fills(&buf);
            assert_eq!(painted[0], (dialog.bounds(), theme.surface()));
            assert_eq!(painted[1].1, theme.button_normal());
            assert_eq!(painted[2].1, theme.accent());
            assert_eq!(painted.len(), 3);
            // Card outline, then the secondary button's outline.
            assert_eq!(strokes(&buf), [theme.border(); 2]);
            assert_eq!(text_color(&buf, "Delete?"), Some(theme.text_primary()));
            assert_eq!(
                text_color(&buf, "Gone for good"),
                Some(theme.text_secondary())
            );
            assert_eq!(text_color(&buf, "Cancel"), Some(theme.text_primary()));
            assert_eq!(
                text_color(&buf, "Confirm"),
                Some(text_on(theme, theme.accent()))
            );

            let confirm = dialog.buttons[1].bounds.get();
            dialog.event(&Event::MouseMove {
                pos: crate::layout::Point::new(confirm.x + 1.0, confirm.y + 1.0),
            });
            let painted = fills(&paint_node(&mut dialog, theme));
            assert_eq!(painted[2].1, tint(theme, theme.accent(), HOVER_TINT));
        }
    }
}
