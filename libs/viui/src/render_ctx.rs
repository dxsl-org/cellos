// SPDX-License-Identifier: MIT
//! `RenderCtx` — combined canvas + font context + theme for `ViNode::paint()`.
//!
//! Bundles a mutable canvas reference with the frame's `FontContext` and
//! active `ViTheme` so every widget has access to drawing primitives, scalable
//! glyphs, and design tokens through a single argument.
//!
//! # Why a struct instead of two arguments?
//!
//! `ViNode::paint()` is an object-safe trait method. Rust requires object-safe
//! methods to have no more than one unsized parameter (the receiver). Passing
//! two `&mut dyn …` arguments is fine syntactically but introducing `RenderCtx`
//! is cleaner, extensible, and avoids ambiguity at call sites.

use crate::canvas::{Color, ViCanvas};
use crate::font_context::FontContext;
use crate::layout::Point;
use crate::theme::ViTheme;

/// Combined draw surface + font state + active theme for one paint pass.
///
/// Passed by mutable reference through the entire widget tree during paint.
/// Containers forward `cx` directly to children — no intermediate clone or reborrow.
pub struct RenderCtx<'a> {
    /// The pixel drawing surface for this frame.
    pub canvas: &'a mut dyn ViCanvas,
    /// Font shared with the layout pass.
    pub font: &'a mut FontContext,
    /// Active design token set. Widgets read colors and spacing from here
    /// rather than hardcoding values.
    pub theme: &'a dyn ViTheme,
}

impl<'a> RenderCtx<'a> {
    /// Draw using the same font and size as layout.
    pub fn draw_text(&mut self, pos: Point, text: &str, color: Color) {
        self.canvas.draw_text_scaled(pos, text, self.font.size_px, color, &mut self.font.atlas);
    }

    pub fn draw_text_at_size(&mut self, pos: Point, text: &str, color: Color, size_px: f32) {
        self.canvas.draw_text_scaled(pos, text, size_px, color, &mut self.font.atlas);
    }

    pub fn measure(&mut self, text: &str) -> f32 {
        self.font.measure(text)
    }

    pub fn line_height(&self) -> f32 {
        self.font.line_height()
    }

    /// Re-borrow as a shorter-lived `RenderCtx`. Allows passing `cx` to child
    /// widgets when the parent still needs access afterward (split borrow).
    #[inline]
    pub fn reborrow(&mut self) -> RenderCtx<'_> {
        RenderCtx {
            canvas: self.canvas,
            font: self.font,
            theme: self.theme,
        }
    }
}
