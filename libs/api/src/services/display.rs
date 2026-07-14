// SPDX-License-Identifier: MPL-2.0

//! Display / compositor API types.
//!
//! ## Surface lifecycle (Grant model)
//!
//! 1. App calls `CREATE_SURFACE` → receives a `cap: u32` handle.
//! 2. App allocates a persistent Grant buffer (`sys_grant_register`), shares it
//!    read-only with the compositor (`sys_grant_share(perm=0)`).
//! 3. App sends `ATTACH_GRANT` (24 bytes) — compositor maps the buffer.
//! 4. App writes pixels directly into the Grant buffer (zero IPC for pixel data).
//! 5. App sends `DAMAGE_NOTIFY` (24 bytes) to signal dirty regions.
//! 6. On close: App sends `DETACH_GRANT` then `DESTROY_SURFACE`; calls
//!    `sys_grant_unregister` to release physical pages.
//!
//! The legacy `WRITE_PIXELS` path (compositor owns pixel storage) is preserved
//! for backward compatibility but deprecated.

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
        Rect {
            x,
            y,
            w: (x2 - x) as u32,
            h: (y2 - y) as u32,
        }
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

    /// Decode from the wire byte used in `AttachGrant`.
    pub const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Rgba8888,
            _ => Self::Bgra8888,
        }
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
    pub fn from_cap(id: CapId) -> Self {
        Self(id)
    }

    /// Return the underlying capability ID.
    pub fn cap_id(&self) -> CapId {
        self.0
    }
}

// ─── Grant-model IPC messages ─────────────────────────────────────────────────

/// Notify the compositor that a region of a Grant-backed surface is dirty.
///
/// Fire-and-forget (no reply).  Total wire size: 24 bytes.
///
/// `cap` is the surface handle returned by `CREATE_SURFACE`.
/// `rect` is in surface-local coordinates.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DamageNotify {
    /// Must equal `compositor_ops::DAMAGE_NOTIFY` (0x07).
    pub opcode: u8,
    pub _pad: [u8; 3],
    /// Surface cap (lower 32 bits; fits current cap space).
    pub cap: u32,
    /// Dirty region in surface-local coordinates.
    pub rect: Rect,
}

/// Attach an app-owned Grant buffer to a compositor surface.
///
/// The app must have already called `sys_grant_share(reg_id, comp_tid, 0 /* ReadOnly */)`.
/// Compositor replies with `[0x01]` on success, `[0x00]` on failure.
/// Total wire size: 24 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AttachGrant {
    /// Must equal `compositor_ops::ATTACH_GRANT` (0x08).
    pub opcode: u8,
    /// `PixelFormat` byte (0 = Bgra8888, 1 = Rgba8888).
    pub fmt: u8,
    pub _pad: [u8; 2],
    /// Surface cap.
    pub cap: u32,
    /// Grant register ID (`sys_grant_register` return value = physical base addr).
    pub reg_id: u64,
    /// Surface pixel width.
    pub width: u32,
    /// Surface pixel height.
    pub height: u32,
}

// Compile-time size assertions — these structs are sent over fixed IPC buffers.
const _: () = assert!(core::mem::size_of::<DamageNotify>() == 24);
const _: () = assert!(core::mem::size_of::<AttachGrant>() == 24);

impl DamageNotify {
    /// Encode into a 24-byte LE buffer for IPC.
    pub fn encode(&self) -> [u8; 24] {
        let mut b = [0u8; 24];
        b[0] = self.opcode;
        // b[1..4] = _pad (zero)
        b[4..8].copy_from_slice(&self.cap.to_le_bytes());
        b[8..12].copy_from_slice(&self.rect.x.to_le_bytes());
        b[12..16].copy_from_slice(&self.rect.y.to_le_bytes());
        b[16..20].copy_from_slice(&self.rect.w.to_le_bytes());
        b[20..24].copy_from_slice(&self.rect.h.to_le_bytes());
        b
    }

    /// Decode from a raw 24-byte LE IPC buffer.
    pub fn decode(b: &[u8; 24]) -> Self {
        Self {
            opcode: b[0],
            _pad: [0; 3],
            cap: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            rect: Rect {
                x: i32::from_le_bytes([b[8], b[9], b[10], b[11]]),
                y: i32::from_le_bytes([b[12], b[13], b[14], b[15]]),
                w: u32::from_le_bytes([b[16], b[17], b[18], b[19]]),
                h: u32::from_le_bytes([b[20], b[21], b[22], b[23]]),
            },
        }
    }
}

impl AttachGrant {
    /// Encode into a 24-byte LE buffer for IPC.
    pub fn encode(&self) -> [u8; 24] {
        let mut b = [0u8; 24];
        b[0] = self.opcode;
        b[1] = self.fmt;
        // b[2..4] = _pad (zero)
        b[4..8].copy_from_slice(&self.cap.to_le_bytes());
        b[8..16].copy_from_slice(&self.reg_id.to_le_bytes());
        b[16..20].copy_from_slice(&self.width.to_le_bytes());
        b[20..24].copy_from_slice(&self.height.to_le_bytes());
        b
    }

    /// Decode from a raw 24-byte LE IPC buffer.
    pub fn decode(b: &[u8; 24]) -> Self {
        Self {
            opcode: b[0],
            fmt: b[1],
            _pad: [0; 2],
            cap: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            reg_id: u64::from_le_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]),
            width: u32::from_le_bytes([b[16], b[17], b[18], b[19]]),
            height: u32::from_le_bytes([b[20], b[21], b[22], b[23]]),
        }
    }
}

// ─── Compositor IPC opcodes ───────────────────────────────────────────────────

/// Opcodes for messages from cells to the compositor cell.
pub mod compositor_ops {
    /// Request a new surface of `(w: u32, h: u32)` pixels.
    /// Payload: `[w: u32 LE, h: u32 LE]`  Reply: cap (u32 LE, zero-padded to 8 bytes)
    pub const CREATE_SURFACE: u8 = 0x01;

    /// Write pixels into a surface (DEPRECATED — use `ATTACH_GRANT` + `DAMAGE_NOTIFY`).
    ///
    /// Kept for backward compatibility.  Compositor still handles it but new app code
    /// should use the Grant-based path instead.
    ///
    /// Payload: `[cap: u64, x: i32, y: i32, w: u32, h: u32, pixel_data: [u8]]`
    #[deprecated(
        since = "0.3.0",
        note = "Use ATTACH_GRANT + DAMAGE_NOTIFY for zero-copy pixel transfer"
    )]
    pub const WRITE_PIXELS: u8 = 0x02;

    /// Mark a rect of a legacy (Owned) surface as damaged.
    /// Payload: `[cap: u64, Rect: 16 bytes]`
    pub const DAMAGE_SURFACE: u8 = 0x03;

    /// Move a surface to a new screen position.
    /// Payload: `[cap: u64, x: i32, y: i32]`
    pub const MOVE_SURFACE: u8 = 0x04;

    /// Raise a surface to the top of the z-order.
    /// Payload: `[cap: u64]`
    pub const RAISE_SURFACE: u8 = 0x05;

    /// Destroy a surface and release its capability.
    /// Payload: `[cap: u64]`  Reply: `[0x00]`
    pub const DESTROY_SURFACE: u8 = 0x06;

    /// Notify the compositor that a region of a Grant-backed surface is dirty.
    ///
    /// Fire-and-forget (no reply).  See `DamageNotify` for the 24-byte wire format.
    pub const DAMAGE_NOTIFY: u8 = 0x07;

    /// Attach an app-owned Grant buffer to a surface slot.
    ///
    /// App must share the Grant read-only before sending this.
    /// See `AttachGrant` for the 24-byte wire format.  Reply: `[0x01]` OK / `[0x00]` fail.
    pub const ATTACH_GRANT: u8 = 0x08;

    /// Detach the Grant from a surface slot before the app frees the Grant.
    ///
    /// Compositor stops accessing the Grant pointer immediately.
    /// Payload: `[cap: u64]`  Reply: `[0x01]`
    pub const DETACH_GRANT: u8 = 0x09;

    /// Query screen dimensions.
    /// Payload: empty  Reply: `[w: u32, h: u32]`
    pub const GET_SCREEN_SIZE: u8 = 0x10;

    /// Dump raw framebuffer (debug only).
    /// Reply: pixel data of whole screen
    pub const DUMP_FB: u8 = 0xFE;
}

// ─── Misc constants ───────────────────────────────────────────────────────────

/// Compositor IPC endpoint (conventionally cell 5 in default boot).
pub const COMPOSITOR_ENDPOINT: usize = 5; // init=1, vfs=2, config=3, net=4, compositor=5

/// Screen resolution used when VirtIO GPU is unavailable.
pub const FALLBACK_WIDTH: u32 = 1024;
pub const FALLBACK_HEIGHT: u32 = 768;
