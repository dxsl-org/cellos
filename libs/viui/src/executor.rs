// SPDX-License-Identifier: MIT
//! `CommandExecutor` trait + `CpuExecutor` — CPU playback backend.
//!
//! `CommandExecutor` decouples the command recorder (`GpuCanvas`) from the
//! actual drawing strategy, letting `GpuRenderer` drive either a CPU
//! rasterizer today or a hardware GPU backend in G2+.
//!
//! # Damage-rect filtering
//! When `damage` is `Some(rect)`, commands whose bounding rect does not
//! intersect `rect` are skipped. `None` = full repaint (skip nothing).
//! Over-estimation of bounding rects is safe; under-estimation causes
//! missing pixels. `GpuCmd::bounding_rect()` always over-estimates.

use crate::canvas::{FramebufferCanvas, ViCanvas};
use crate::gpu_cmd::{GpuCmd, GpuCommandBuffer, RecordedCmd};
use crate::layout::Rect;
use ostd::display::ViSurface;

// ─── CommandExecutor ─────────────────────────────────────────────────────────

/// Executes a recorded `GpuCommandBuffer`, optionally constrained to a damage rect.
pub trait CommandExecutor {
    /// Replay all commands in `buf`, skipping those outside `damage` if set.
    fn execute(&mut self, buf: &GpuCommandBuffer, damage: Option<Rect>);
}

// ─── CpuExecutor ─────────────────────────────────────────────────────────────

/// G1 CPU executor: replays `GpuCmd`s via `FramebufferCanvas` + `ViSurface`.
///
/// Produces identical output to `FramebufferRenderer` but skips commands
/// outside the supplied damage rect, reducing CPU rasterization work.
pub struct CpuExecutor {
    surf: ViSurface,
    font: crate::font_context::FontContext,
}

impl CpuExecutor {
    pub fn new(surf: ViSurface) -> Self {
        Self { surf, font: crate::font_context::FontContext::default() }
    }

    /// Unwrap the inner `ViSurface` (e.g. for IPC cleanup after app exit).
    pub fn into_surf(self) -> ViSurface {
        self.surf
    }
}

impl CommandExecutor for CpuExecutor {
    fn execute(&mut self, buf: &GpuCommandBuffer, damage: Option<Rect>) {
        let stride = self.surf.stride() as u32;
        let (w, h) = (self.surf.width(), self.surf.height());
        let pixels = self.surf.pixels_mut();
        let mut canvas = FramebufferCanvas::new(pixels, stride, w, h);
        execute_commands(&mut canvas, &mut self.font, buf, damage);

        // G1: always damage_all; G2+ can flip only the damage rect.
        self.surf.damage_all();
    }
}

/// CPU playback shared by the surface executor and pixel-buffer regressions.
fn execute_commands(canvas: &mut FramebufferCanvas<'_>, font: &mut crate::font_context::FontContext, buf: &GpuCommandBuffer, damage: Option<Rect>) {
    if let Some(rect) = damage { canvas.clip_push(rect); }
    for RecordedCmd { cmd, bounds } in buf.recorded_slice() {
        if let Some(damage_rect) = damage {
            if let Some(b) = bounds {
                if !b.intersects(damage_rect) { continue; }
            }
        }
        match cmd {
            GpuCmd::FillRect { rect, color } => canvas.fill_rect(*rect, *color),
            GpuCmd::DrawLine { a, b, color } => canvas.draw_line(*a, *b, *color),
            GpuCmd::DrawText { pos, text, color, size_px } => {
                canvas.draw_text_scaled(*pos, text, *size_px, *color, &mut font.atlas);
            }
            GpuCmd::DrawImage { dest, pixels, src_stride } => canvas.draw_image(*dest, pixels, *src_stride),
            GpuCmd::DrawTextShort { pos, bytes, len, color, size_px } => {
                let text = core::str::from_utf8(&bytes[..*len as usize]).expect("recorded text is valid UTF-8");
                canvas.draw_text_scaled(*pos, text, *size_px, *color, &mut font.atlas);
            }
            GpuCmd::ClipPush { rect } => canvas.clip_push(*rect),
            GpuCmd::ClipPop => canvas.clip_pop(),
        }
    }
    if damage.is_some() { canvas.clip_pop(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Color;
    use crate::gpu_canvas::GpuCanvas;
    use crate::layout::Point;

    #[test]
    fn recorded_cpu_text_preserves_fractional_sizes_and_clipping() {
        let mut font = crate::font_context::FontContext::default();
        let long_text = "W".repeat(130);
        for text in ["Wiế", long_text.as_str()] {
            for px in [16.5, 0.5] {
                let mut commands = GpuCommandBuffer::new();
                {
                    let mut recorder = GpuCanvas::new(&mut commands, 160, 50);
                    recorder.clip_push(Rect::new(3.0, 0.0, 80.0, 50.0));
                    recorder.draw_text_scaled(Point::new(1.0, 1.0), text, px, Color::WHITE, &mut font.atlas);
                    recorder.clip_pop();
                }
                let mut recorded = alloc::vec![0; 160 * 50 * 4];
                let mut direct = recorded.clone();
                let mut playback = FramebufferCanvas::new(&mut recorded, 640, 160, 50);
                execute_commands(&mut playback, &mut font, &commands, None);
                let mut expected = FramebufferCanvas::new(&mut direct, 640, 160, 50);
                expected.clip_push(Rect::new(3.0, 0.0, 80.0, 50.0));
                expected.draw_text_scaled(Point::new(1.0, 1.0), text, px, Color::WHITE, &mut font.atlas);
                assert_eq!(recorded, direct, "recorded playback differs at {px}px");
            }
        }
    }
}
