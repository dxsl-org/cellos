// SPDX-License-Identifier: MIT
//! `GpuCanvas` — `ViCanvas` implementation that records draw commands.
//!
//! Instead of rasterizing immediately, every draw call is captured as a
//! `GpuCmd` in the supplied `GpuCommandBuffer`. The executor then replays the
//! buffer — optionally filtering by damage rect — via CPU or hardware GPU.
//!
//! # Clip behaviour
//! `fill_rect` and `clip_push/pop` are handled here; the clipped rect is
//! stored in the command. `draw_image` records the original dest and delegates
//! clipping to the executor (avoids src-offset arithmetic in the recorder).

use crate::canvas::{Color, TextStyle, ViCanvas};
use crate::gpu_cmd::{GpuCmd, GpuCommandBuffer};
use crate::layout::{Point, Rect};
use alloc::{string::String, vec::Vec};

const CLIP_DEPTH: usize = 16;

// ─── GpuCanvas ───────────────────────────────────────────────────────────────

/// Recording canvas: captures draw calls as `GpuCmd`s.
///
/// `'buf` ties the canvas to the `GpuCommandBuffer` it records into; the
/// buffer outlives the canvas and is consumed by the executor after painting.
pub struct GpuCanvas<'buf> {
    buf: &'buf mut GpuCommandBuffer,
    width: u32,
    height: u32,
    clip_stack: [Rect; CLIP_DEPTH],
    clip_depth: usize,
    /// Empty scratch buffer returned by `pixels_mut()`.
    /// `pixels_mut()` is only called by the v1 PaintCx path; the v2 ViNode
    /// paint path never calls it, so an empty slice is safe.
    scratch: Vec<u8>,
}

impl<'buf> GpuCanvas<'buf> {
    pub fn new(buf: &'buf mut GpuCommandBuffer, width: u32, height: u32) -> Self {
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            w: width as f32,
            h: height as f32,
        };
        let mut clip_stack = [Rect::default(); CLIP_DEPTH];
        clip_stack[0] = bounds;
        Self {
            buf,
            width,
            height,
            clip_stack,
            clip_depth: 0,
            scratch: Vec::new(),
        }
    }

    fn active_clip(&self) -> Rect {
        self.clip_stack[self.clip_depth]
    }

    fn record_text(&mut self, pos: Point, text: &str, size_px: f32, color: Color) {
        if text.len() <= 127 {
            let mut bytes = [0u8; 128];
            bytes[..text.len()].copy_from_slice(text.as_bytes());
            self.buf.push(GpuCmd::DrawTextShort { pos, bytes, len: text.len() as u8, color, size_px });
        } else {
            self.buf.push(GpuCmd::DrawText { pos, text: String::from(text), color, size_px });
        }
    }
}

// ─── ViCanvas impl ───────────────────────────────────────────────────────────

impl<'buf> ViCanvas for GpuCanvas<'buf> {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let clip = self.active_clip();
        if let Some(clipped) = rect.intersect(&clip) {
            self.buf.push(GpuCmd::FillRect {
                rect: clipped,
                color,
            });
        }
    }

    fn draw_line(&mut self, a: Point, b: Point, color: Color) {
        // Line endpoints are not clipped here; FramebufferCanvas::draw_line
        // has per-pixel bounds checking via put_pixel.
        self.buf.push(GpuCmd::DrawLine { a, b, color });
    }

    fn draw_text(&mut self, pos: Point, text: &str, style: TextStyle) {
        let px = if style.size_px == 0 { 16.0 } else { style.size_px as f32 };
        self.record_text(pos, text, px, style.color);
    }

    /// Records scalable intent only; this recorder does not rasterize glyphs.
    fn draw_text_scaled(&mut self, pos: Point, text: &str, px: f32, color: Color, atlas: &mut ostd::font_atlas::GlyphAtlas) {
        self.record_text(pos, text, px, color);
        // Pad measured advances for glyph bearings/overhangs; unknown bounds
        // on legacy raw text commands are never incorrectly damage-culled.
        self.buf.set_last_bounds(Some(Rect::new(pos.x - px, pos.y - px, atlas.measure(text, px) + px * 2.0, atlas.line_height(px) + px * 2.0)));
    }

    fn draw_image(&mut self, dest: Rect, pixels: &[u8], src_stride: u32) {
        // Record unclipped dest — executor applies clip during playback.
        // This avoids source-pixel-offset arithmetic in the recorder.
        self.buf.push(GpuCmd::DrawImage {
            dest,
            pixels: pixels.to_vec(),
            src_stride,
        });
    }

    fn clip_push(&mut self, rect: Rect) {
        if self.clip_depth + 1 < CLIP_DEPTH {
            let parent = self.active_clip();
            self.clip_depth += 1;
            self.clip_stack[self.clip_depth] = rect.intersect(&parent).unwrap_or(Rect::ZERO);
            self.buf.push(GpuCmd::ClipPush { rect });
        }
    }

    fn clip_pop(&mut self) {
        if self.clip_depth > 0 {
            self.clip_depth -= 1;
            self.buf.push(GpuCmd::ClipPop);
        }
    }

    fn clip_rect(&self) -> Option<Rect> {
        Some(self.active_clip())
    }

    /// Returns an empty scratch slice — v2 ViNode paint path never calls this.
    fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.scratch
    }
    fn stride(&self) -> u32 {
        self.width * 4
    }
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
}
