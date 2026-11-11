// SPDX-License-Identifier: MIT
//! Shared theme-derived colour helpers for the v2 node widgets.

use crate::canvas::Color;
use crate::theme::ViTheme;

/// Readable text colour for content drawn on `fill` (e.g. an accent button).
///
/// Picks the lighter of the theme's `bg`/`text_primary` pair for dark fills
/// and the darker one for light fills, using perceived (YIQ) brightness.
pub(crate) fn text_on(theme: &dyn ViTheme, fill: Color) -> Color {
    let (a, b) = (theme.bg(), theme.text_primary());
    let (light, dark) = if brightness(a) >= brightness(b) {
        (a, b)
    } else {
        (b, a)
    };
    if brightness(fill) >= 150 {
        dark
    } else {
        light
    }
}

/// `tint` strength for a hovered filled control.
pub(crate) const HOVER_TINT: u8 = 36;
/// `tint` strength for a pressed filled control.
pub(crate) const PRESSED_TINT: u8 = 72;

/// Hover/pressed state of a filled control: tint `fill` towards the theme's
/// text colour (lightens on dark themes, darkens on light ones).
pub(crate) fn tint(theme: &dyn ViTheme, fill: Color, alpha: u8) -> Color {
    Color::blend_over(theme.text_primary().with_alpha(alpha), fill)
}

fn brightness(c: Color) -> u32 {
    (c.r() as u32 * 299 + c.g() as u32 * 587 + c.b() as u32 * 114) / 1000
}

/// Recording helpers for widget paint tests.
#[cfg(test)]
pub(crate) mod test_support {
    extern crate alloc;
    use alloc::{string::String, vec::Vec};

    use crate::canvas::{Color, ViCanvas};
    use crate::font_context::FontContext;
    use crate::gpu_canvas::GpuCanvas;
    use crate::gpu_cmd::{GpuCmd, GpuCommandBuffer};
    use crate::layout::{Constraints, Rect, Size};
    use crate::node::ViNode;
    use crate::render_ctx::RenderCtx;
    use crate::theme::ViTheme;

    /// Record everything `paint` draws on a 400×300 canvas.
    pub(crate) fn record(paint: impl FnOnce(&mut dyn ViCanvas)) -> GpuCommandBuffer {
        let mut buf = GpuCommandBuffer::new();
        {
            let mut canvas = GpuCanvas::new(&mut buf, 400, 300);
            paint(&mut canvas);
        }
        buf
    }

    /// Lay out `node` in a 400×300 root and record its paint under `theme`.
    pub(crate) fn paint_node(node: &mut dyn ViNode, theme: &dyn ViTheme) -> GpuCommandBuffer {
        let mut font = FontContext::default();
        node.layout(Constraints::root(Size::new(400.0, 300.0)), &mut font);
        record(|canvas| {
            let mut cx = RenderCtx {
                canvas,
                font: &mut font,
                theme,
            };
            node.paint(&mut cx);
        })
    }

    /// `(text, colour)` for every recorded text command, in paint order.
    pub(crate) fn texts(buf: &GpuCommandBuffer) -> Vec<(String, Color)> {
        buf.recorded_slice()
            .iter()
            .filter_map(|r| match &r.cmd {
                GpuCmd::DrawText { text, color, .. } => Some((text.clone(), *color)),
                GpuCmd::DrawTextShort {
                    bytes, len, color, ..
                } => Some((
                    String::from(core::str::from_utf8(&bytes[..*len as usize]).unwrap()),
                    *color,
                )),
                _ => None,
            })
            .collect()
    }

    /// Colour of the first text command drawing exactly `text`.
    pub(crate) fn text_color(buf: &GpuCommandBuffer, text: &str) -> Option<Color> {
        texts(buf)
            .into_iter()
            .find(|(t, _)| t == text)
            .map(|(_, c)| c)
    }

    /// `(rect, colour)` for every recorded fill (square or rounded), in paint order.
    pub(crate) fn fills(buf: &GpuCommandBuffer) -> Vec<(Rect, Color)> {
        buf.recorded_slice()
            .iter()
            .filter_map(|r| match r.cmd {
                GpuCmd::FillRect { rect, color } | GpuCmd::FillRoundRect { rect, color, .. } => {
                    Some((rect, color))
                }
                _ => None,
            })
            .collect()
    }

    /// Colours of every recorded rounded outline, in paint order.
    pub(crate) fn strokes(buf: &GpuCommandBuffer) -> Vec<Color> {
        buf.recorded_slice()
            .iter()
            .filter_map(|r| match r.cmd {
                GpuCmd::StrokeRoundRect { color, .. } => Some(color),
                _ => None,
            })
            .collect()
    }

    /// Colours of every recorded line, in paint order.
    pub(crate) fn lines(buf: &GpuCommandBuffer) -> Vec<Color> {
        buf.recorded_slice()
            .iter()
            .filter_map(|r| match r.cmd {
                GpuCmd::DrawLine { color, .. } => Some(color),
                _ => None,
            })
            .collect()
    }
}
