// SPDX-License-Identifier: MIT
//! `ViSurfaceRenderer` — software `ViRenderer` backed by an in-process pixel buffer.
//!
//! # G1 implementation
//! Allocates a `Vec<u8>` pixel buffer on the heap and wraps it with
//! `FramebufferCanvas` on each `render()` call (stack-confined borrow, no unsafe).
//! The canvas is passed to the draw closure directly — no copy, no extra allocation.
//!
//! Presenting (flushing to the compositor or GPU) is a **no-op** in this G1 renderer.
//! Callers that need real display output should use `FramebufferRenderer` (wraps a
//! `ViSurface` grant buffer) from `libs/viui/src/renderer.rs`.
//!
//! # When to use this vs `FramebufferRenderer`
//! - **Tests / headless** — no compositor available; use `ViSurfaceRenderer`.
//! - **ViOS compositor surface** — compositor is up, Grant API is ready; use
//!   `FramebufferRenderer::new(ViSurface::create(...))`.
//!
//! # TODO (tracked)
//! Replace no-op present with compositor Grant surface flush when the Grant-based
//! compositor redesign (`.agents/260607-1854-compositor-grant-surfaces/`) ships.
//! At that point `ViSurfaceRenderer` may be retired entirely in favour of
//! `FramebufferRenderer`, or kept as a headless fallback.

extern crate alloc;
use alloc::{vec, vec::Vec};

use crate::canvas::{FramebufferCanvas, ViCanvas};
use crate::layout::Rect;
use crate::renderer::ViRenderer;

// ─── ViSurfaceRenderer ───────────────────────────────────────────────────────

/// In-process software renderer with a heap-allocated pixel buffer.
///
/// BGRA8888 pixel layout — byte order: B G R A at ascending addresses (matches
/// the ViCell compositor wire format and VirtIO GPU display).
///
/// Stride = `width * 4` bytes (no padding — pixels are packed).
pub struct ViSurfaceRenderer {
    /// BGRA8888 pixel buffer owned by this renderer.
    /// Length = width * height * 4 bytes.
    pixels: Vec<u8>,
    width:  u32,
    height: u32,
}

impl ViSurfaceRenderer {
    /// Allocate a zero-cleared pixel buffer of `(width × height)` pixels.
    ///
    /// Minimum viable size: 1×1. Requesting 0 for either dimension is allowed
    /// but will cause all draw operations to be clipped away.
    pub fn new(width: u32, height: u32) -> Self {
        let byte_count = (width as usize) * (height as usize) * 4;
        Self {
            pixels: vec![0u8; byte_count],
            width,
            height,
        }
    }

    /// Read-only view of the current pixel buffer contents.
    ///
    /// Useful for testing (compare rendered output against expected bytes) and
    /// for manually presenting to a GPU/compositor without going through `render()`.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Stride in bytes per row (= `width * 4` for packed BGRA8888).
    pub fn stride(&self) -> u32 {
        self.width * 4
    }
}

impl ViRenderer for ViSurfaceRenderer {
    /// Run the draw closure against the internal pixel buffer, then discard
    /// the canvas (no-op present).
    ///
    /// `damage` is accepted but ignored — the full buffer is always repainted.
    /// This matches the G1 `FramebufferRenderer` contract (damage advisory only).
    fn render(&mut self, _damage: Option<Rect>, draw: &mut dyn FnMut(&mut dyn ViCanvas)) {
        // Create the canvas here on the stack so its 'fb lifetime is confined to
        // this call frame — no self-referential struct, no unsafe, no extra alloc.
        let stride = self.stride();
        let mut canvas = FramebufferCanvas::new(&mut self.pixels, stride, self.width, self.height);
        draw(&mut canvas);
        // TODO: present dirty region to compositor via Grant IPC when
        // `.agents/260607-1854-compositor-grant-surfaces/` plan is merged.
        // For now this is a no-op: pixels stay in `self.pixels` and callers
        // can read them via `pixels()` for testing or manual flush.
    }

    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
