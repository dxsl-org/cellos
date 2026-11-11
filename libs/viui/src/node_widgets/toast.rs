// SPDX-License-Identifier: MIT
//! Toast notification widget — transient, auto-dismiss, non-blocking overlay.
//!
//! Toasts are positioned at the bottom-center of the screen. Auto-dismiss is
//! driven by `ToastEntry::elapsed_ms`, which is incremented by `ViApp::tick_with_dt`.
//! Set `duration_ms = 0` for a manual-dismiss-only toast.

extern crate alloc;
use alloc::string::String;
use core::cell::Cell;

use crate::canvas::Color;
use crate::dirty::DirtyRegion;
use crate::event::Event;
use crate::layout::{Constraints, Point, Rect, Size};
use crate::node::ViNode;
use crate::render_ctx::RenderCtx;
use crate::signal::SubscriptionHandle;
use crate::theme::ViTheme;
use ostd::display::linear::PANEL_RADIUS;

/// Severity stripe: a small pill inset from the left edge, clear of the
/// panel's rounded corners.
const STRIPE_W: f32 = 3.0;
const STRIPE_INSET: f32 = 4.0;

// ─── ToastKind ───────────────────────────────────────────────────────────────

/// Semantic category of a toast — controls the severity stripe colour.
#[derive(Clone)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

// ─── ToastConfig ─────────────────────────────────────────────────────────────

/// Parameters for a transient toast notification.
#[derive(Clone)]
pub struct ToastConfig {
    pub message: String,
    pub kind: ToastKind,
    /// Auto-dismiss after this many milliseconds. `0` = manual dismiss only.
    pub duration_ms: u32,
}

// ─── ToastEntry ──────────────────────────────────────────────────────────────

/// Live toast tracked by `ViApp` — wraps the config with timer state and a
/// pre-built widget so `paint()` does not need to allocate each frame.
pub struct ToastEntry {
    pub config: ToastConfig,
    pub elapsed_ms: u32,
    /// Pre-built widget. Laid out every frame by `ViApp` in Path A.
    pub widget: Toast,
}

// ─── Toast ───────────────────────────────────────────────────────────────────

/// Rendered toast overlay widget.
///
/// Positioned at the bottom-center of the screen constraints. The widget is
/// non-interactive: all events pass through.
pub struct Toast {
    message: String,
    kind: ToastKind,
    bounds: Cell<Rect>,
}

impl Toast {
    /// Construct from a `ToastConfig`.
    pub fn new(config: &ToastConfig) -> Self {
        Self {
            message: config.message.clone(),
            kind: config.kind.clone(),
            bounds: Cell::new(Rect::ZERO),
        }
    }

    /// Severity stripe colour from the theme's status tokens.
    fn severity_color(&self, theme: &dyn ViTheme) -> Color {
        match self.kind {
            ToastKind::Info => theme.badge_attention(),
            ToastKind::Success => theme.badge_success(),
            ToastKind::Warning => theme.badge_dirty(),
            ToastKind::Error => theme.badge_error(),
        }
    }
}

impl ViNode for Toast {
    fn layout(
        &mut self,
        constraints: Constraints,
        _font: &mut crate::font_context::FontContext,
    ) -> Size {
        let w = constraints.max.w.min(400.0);
        let h = 44.0f32;
        // Centre horizontally; sit 16 px above the bottom edge.
        let x = constraints.origin.x + (constraints.max.w - w) / 2.0;
        let y = constraints.origin.y + constraints.max.h - h - 16.0;
        self.bounds.set(Rect { x, y, w, h });
        Size::new(w, h)
    }

    fn bounds(&self) -> Rect {
        self.bounds.get()
    }

    fn paint(&self, cx: &mut RenderCtx<'_>) {
        let b = self.bounds.get();
        let theme = cx.theme;
        cx.canvas.fill_round_rect(b, PANEL_RADIUS, theme.surface());
        let corner = PANEL_RADIUS as f32;
        let stripe = Rect {
            x: b.x + STRIPE_INSET,
            y: b.y + corner,
            w: STRIPE_W,
            h: (b.h - corner * 2.0).max(0.0),
        };
        cx.canvas
            .fill_round_rect(stripe, STRIPE_W as u32 / 2, self.severity_color(theme));
        cx.canvas.stroke_round_rect(b, PANEL_RADIUS, theme.border());
        let text_y = b.y + (b.h - cx.line_height()) * 0.5;
        cx.draw_text(
            Point::new(b.x + 12.0, text_y),
            &self.message,
            theme.text_primary(),
        );
    }

    /// Toasts are non-interactive — never consume events.
    fn event(&mut self, _event: &Event) -> bool {
        false
    }

    fn collect_dirty_handles(
        &mut self,
        _region: DirtyRegion,
    ) -> alloc::vec::Vec<SubscriptionHandle> {
        alloc::vec::Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Toast, ToastConfig, ToastKind};

    use crate::node_widgets::style::test_support::{fills, paint_node, strokes, text_color};
    use crate::theme::{ViTheme, DARK_THEME, LIGHT_THEME};

    #[test]
    fn panel_text_and_severity_use_theme_tokens() {
        for theme in [&LIGHT_THEME as &dyn ViTheme, &DARK_THEME] {
            for (kind, severity) in [
                (ToastKind::Info, theme.badge_attention()),
                (ToastKind::Success, theme.badge_success()),
                (ToastKind::Warning, theme.badge_dirty()),
                (ToastKind::Error, theme.badge_error()),
            ] {
                let mut toast = Toast::new(&ToastConfig {
                    message: "Saved".into(),
                    kind,
                    duration_ms: 0,
                });
                let buf = paint_node(&mut toast, theme);
                let painted = fills(&buf);
                assert_eq!(painted[0].1, theme.surface());
                assert_eq!(painted[1].1, severity);
                assert_eq!(strokes(&buf), [theme.border()]);
                assert_eq!(text_color(&buf, "Saved"), Some(theme.text_primary()));
            }
        }
    }
}
