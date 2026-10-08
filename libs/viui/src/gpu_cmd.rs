// SPDX-License-Identifier: MIT
//! GPU draw command types and command buffer recorder.
//!
//! `GpuCmd` encodes one draw operation without executing it. Commands are
//! accumulated into `GpuCommandBuffer` during a paint pass and replayed by
//! a `CommandExecutor` — allowing damage-rect filtering and future hardware
//! GPU execution without changing widget code.

use crate::canvas::Color;
use crate::layout::{Point, Rect};
use alloc::{string::String, vec::Vec};

// ─── GpuCmd ──────────────────────────────────────────────────────────────────

/// A single recorded draw operation.
#[derive(Debug)]
pub enum GpuCmd {
    /// Fill a pre-clipped solid rectangle.
    FillRect { rect: Rect, color: Color },
    /// Draw a line from `a` to `b` (Bresenham, handled by executor).
    DrawLine { a: Point, b: Point, color: Color },
    /// Record Inter text at its exact scalable pixel size.
    DrawText {
        pos: Point,
        text: String,
        color: Color,
        size_px: f32,
    },
    /// Blit raw BGRA pixels. Destination may extend beyond current clip —
    /// executor applies clipping during playback.
    DrawImage {
        dest: Rect,
        pixels: Vec<u8>,
        src_stride: u32,
    },
    /// Zero-alloc path for text ≤ 127 bytes (covers all typical single-line UI strings).
    /// Bytes are always valid UTF-8 — written from `&str` in `GpuCanvas::draw_text`.
    DrawTextShort {
        pos: Point,
        bytes: [u8; 128],
        len: u8,
        color: Color,
        size_px: f32,
    },
    /// Clip state is replayed so recorded text observes the paint damage clip.
    ClipPush { rect: Rect },
    ClipPop,
}

impl GpuCmd {
    /// Conservative bounding rect used for damage-rect filtering.
    ///
    /// Returns `None` only if the command is inherently unclippable; callers
    /// that see `None` must always execute the command.
    pub fn bounding_rect(&self) -> Option<Rect> {
        match self {
            GpuCmd::FillRect { rect, .. } => Some(*rect),
            GpuCmd::DrawLine { a, b, .. } => {
                let x = a.x.min(b.x);
                let y = a.y.min(b.y);
                let x2 = a.x.max(b.x);
                let y2 = a.y.max(b.y);
                // Min 1px so a horizontal/vertical line still has area.
                Some(Rect {
                    x,
                    y,
                    w: (x2 - x).max(1.0),
                    h: (y2 - y).max(1.0),
                })
            }
            GpuCmd::DrawText { .. } => None,
            GpuCmd::DrawImage { dest, .. } => Some(*dest),
            GpuCmd::DrawTextShort { .. } => None,
            GpuCmd::ClipPush { .. } | GpuCmd::ClipPop => None,
        }
    }
}

// ─── RecordedCmd ─────────────────────────────────────────────────────────────

/// A `GpuCmd` with its bounding rect pre-computed at record time.
///
/// `bounds` mirrors `GpuCmd::bounding_rect()` but is evaluated once in
/// `GpuCommandBuffer::push()` so the executor reads a stored field instead
/// of recomputing it every frame.
pub struct RecordedCmd {
    pub cmd: GpuCmd,
    pub bounds: Option<Rect>,
}

// ─── GpuCommandBuffer ────────────────────────────────────────────────────────

/// Ordered list of draw commands recorded during one paint pass.
pub struct GpuCommandBuffer {
    cmds: Vec<RecordedCmd>,
}

impl GpuCommandBuffer {
    pub fn new() -> Self {
        Self { cmds: Vec::new() }
    }

    /// Append a command, pre-computing its bounding rect once at record time.
    pub fn push(&mut self, cmd: GpuCmd) {
        let bounds = cmd.bounding_rect();
        self.cmds.push(RecordedCmd { cmd, bounds });
    }

    /// Supply bounds measured by the recording font instead of guessing advances.
    pub fn set_last_bounds(&mut self, bounds: Option<Rect>) {
        if let Some(last) = self.cmds.last_mut() { last.bounds = bounds; }
    }

    /// Iterate recorded commands with pre-computed bounds.
    pub fn recorded_slice(&self) -> &[RecordedCmd] {
        &self.cmds
    }

    pub fn len(&self) -> usize {
        self.cmds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }

    /// Clear all commands while retaining the Vec's heap allocation for the next frame.
    pub fn clear(&mut self) {
        self.cmds.clear();
    }
}

impl Default for GpuCommandBuffer {
    fn default() -> Self {
        Self::new()
    }
}
