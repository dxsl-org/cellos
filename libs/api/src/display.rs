// SPDX-License-Identifier: MPL-2.0

//! Display / compositor API types.
//!
//! Cells create surfaces via `CreateSurface` IPC to the compositor, receive a
//! `SurfaceCap`, and write pixel data + damage rectangles via subsequent IPC.
//! The compositor blends all live surfaces and flushes the result to the VirtIO
//! GPU via the `GpuFlush` kernel syscall.

use crate::cap::CapId;

// ─── Geometry ─────────────────────────────────────────────────────────────────

/// Axis-aligned rectangle in screen coordinates.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// Return the area in pixels.
    pub fn area(&self) -> u32 {
        self.w.saturating_mul(self.h)
    }

    /// Return true if `other` overlaps this rect.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w as i32
            && self.x + self.w as i32 > other.x
            && self.y < other.y + other.h as i32
            && self.y + self.h as i32 > other.y
    }

    /// Return the union of two rects (smallest rect containing both).
    pub fn union(&self, other: &Rect) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let x2 = (self.x + self.w as i32).max(other.x + other.w as i32);
        let y2 = (self.y + self.h as i32).max(other.y + other.h as i32);
        Rect { x, y, w: (x2 - x) as u32, h: (y2 - y) as u32 }
    }
}

// ─── Pixel format ─────────────────────────────────────────────────────────────

/// Pixel layout for surface data.
///
/// `Bgra8888` matches the VirtIO GPU native format (avoids a per-pixel swap).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// B, G, R, A — 4 bytes per pixel, native VirtIO GPU format.
    Bgra8888 = 0,
    /// R, G, B, A — 4 bytes per pixel.
    Rgba8888 = 1,
}

impl PixelFormat {
    /// Bytes per pixel for this format.
    pub const fn bpp(self) -> u32 {
        4 // all current formats are 4 bytes per pixel
    }
}

// ─── Surface capability ───────────────────────────────────────────────────────

/// An opaque handle to a compositor surface (backed by a kernel capability).
///
/// Obtained from `CreateSurface` IPC.  Single-owner: moving `SurfaceCap`
/// transfers ownership.  Dropping without calling `destroy()` leaks the
/// compositor resource until the owning cell exits.
#[must_use = "dropping a SurfaceCap without destroy() leaks the surface until the cell exits"]
#[repr(transparent)]
pub struct SurfaceCap(pub CapId);

impl SurfaceCap {
    /// Create from a raw capability ID.
    pub fn from_cap(id: CapId) -> Self { Self(id) }

    /// Return the underlying capability ID.
    pub fn cap_id(&self) -> CapId { self.0 }
}

// ─── Compositor IPC opcodes ───────────────────────────────────────────────────

/// Opcodes for messages from cells to the compositor cell.
pub mod compositor_ops {
    /// Request a new surface of `(w: u32, h: u32)` pixels.
    /// Payload: `[w: u32 LE, h: u32 LE]`  Reply: CapId (u64 LE)
    pub const CREATE_SURFACE: u8  = 0x01;
    /// Write pixels into a surface.
    /// Payload: `[cap: u64, x: i32, y: i32, w: u32, h: u32, pixel_data: [u8]]`
    pub const WRITE_PIXELS: u8    = 0x02;
    /// Mark a rect of a surface as damaged (needs redraw).
    /// Payload: `[cap: u64, Rect: 16 bytes]`
    pub const DAMAGE_SURFACE: u8  = 0x03;
    /// Move a surface to a new screen position.
    /// Payload: `[cap: u64, x: i32, y: i32]`
    pub const MOVE_SURFACE: u8    = 0x04;
    /// Raise a surface to the top of the z-order.
    /// Payload: `[cap: u64]`
    pub const RAISE_SURFACE: u8   = 0x05;
    /// Destroy a surface and release its capability.
    /// Payload: `[cap: u64]`  Reply: `[0x00]`
    pub const DESTROY_SURFACE: u8 = 0x06;
    /// Query screen dimensions.
    /// Payload: empty  Reply: `[w: u32, h: u32]`
    pub const GET_SCREEN_SIZE: u8 = 0x10;
    /// Dump raw framebuffer (debug only).
    /// Reply: pixel data of whole screen
    pub const DUMP_FB: u8         = 0xFE;
}

// ─── GPU flush IPC opcode (compositor → kernel GPU driver) ────────────────────

/// Compositor IPC endpoint (conventionally cell 2 in default boot).
pub const COMPOSITOR_ENDPOINT: usize = 5; // init=1, vfs=2, config=3, net=4, compositor=5

/// Screen resolution used when VirtIO GPU is unavailable.
pub const FALLBACK_WIDTH:  u32 = 1024;
pub const FALLBACK_HEIGHT: u32 = 768;
