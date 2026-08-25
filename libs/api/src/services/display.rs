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

/// Input behavior assigned when a surface is created.
///
/// `Background` is compositor-enforced: it remains visible but cannot be a
/// pointer target, capture target, or keyboard-focus owner.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRole {
    Interactive = 0,
    Background = 1,
}

impl SurfaceRole {
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Background,
            _ => Self::Interactive,
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

// ─── Window lifecycle IPC messages ───────────────────────────────────────────

/// Maximum number of UTF-8 bytes in a window title.
pub const MAX_TITLE_BYTES: usize = 64;

/// Exact wire size of [`SetTitle`].
pub const SET_TITLE_WIRE_SIZE: usize = 72;
/// Exact wire size of [`ConfigureAck`].
pub const CONFIGURE_ACK_WIRE_SIZE: usize = 12;
/// Exact wire size of [`CloseResponse`].
pub const CLOSE_RESPONSE_WIRE_SIZE: usize = 12;
/// Exact wire size of [`SurfaceStateRequest`].
pub const SURFACE_STATE_REQUEST_WIRE_SIZE: usize = 8;
/// Exact wire size of [`DetachReplacedGrant`].
pub const DETACH_REPLACED_GRANT_WIRE_SIZE: usize = 16;
/// Exact wire size of [`WindowConfigure`].
pub const WINDOW_CONFIGURE_WIRE_SIZE: usize = 28;
/// Exact wire size of [`WindowCloseRequest`].
pub const WINDOW_CLOSE_REQUEST_WIRE_SIZE: usize = 12;
/// Exact wire size of [`WindowStateChanged`].
pub const WINDOW_STATE_CHANGED_WIRE_SIZE: usize = 12;

/// A failure while validating a fixed display-protocol frame.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayProtocolError {
    /// The frame was not the exact documented fixed size.
    InvalidLength = 1,
    /// The frame opcode did not identify the expected message type.
    InvalidOpcode = 2,
    /// A reserved byte was nonzero.
    NonZeroReserved = 3,
    /// An enum or boolean discriminant was not part of the protocol.
    InvalidDiscriminant = 4,
    /// A title length exceeded [`MAX_TITLE_BYTES`].
    TitleTooLong = 5,
    /// A title's declared bytes were not valid UTF-8.
    InvalidUtf8 = 6,
    /// Unused bytes in a fixed-width title were nonzero.
    NonZeroTitlePadding = 7,
}

/// Reason a compositor asks its owner to configure a window.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigureKind {
    /// The compositor proposes a resize.
    Resize = 0,
    /// The compositor proposes the maximized content rectangle.
    Maximize = 1,
    /// The compositor proposes the restored content rectangle.
    Restore = 2,
}

impl ConfigureKind {
    fn decode(value: u8) -> Result<Self, DisplayProtocolError> {
        match value {
            0 => Ok(Self::Resize),
            1 => Ok(Self::Maximize),
            2 => Ok(Self::Restore),
            _ => Err(DisplayProtocolError::InvalidDiscriminant),
        }
    }
}

/// Compositor-managed state of a window.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// The window is visible at its normal geometry.
    Normal = 0,
    /// The window is hidden while remaining owned by its client.
    Minimized = 1,
    /// The window occupies the compositor-proposed maximum geometry.
    Maximized = 2,
    /// The compositor is awaiting the owner's close response.
    Closing = 3,
}

impl WindowState {
    fn decode(value: u8) -> Result<Self, DisplayProtocolError> {
        match value {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Minimized),
            2 => Ok(Self::Maximized),
            3 => Ok(Self::Closing),
            _ => Err(DisplayProtocolError::InvalidDiscriminant),
        }
    }
}

/// Owner decision for a compositor close request.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseResponseAction {
    /// Keep the window open.
    Reject = 0,
    /// Permit the compositor to close the window.
    Accept = 1,
}

impl CloseResponseAction {
    fn decode(value: u8) -> Result<Self, DisplayProtocolError> {
        match value {
            0 => Ok(Self::Reject),
            1 => Ok(Self::Accept),
            _ => Err(DisplayProtocolError::InvalidDiscriminant),
        }
    }
}

/// Fixed 72-byte owner request that replaces a surface title.
///
/// LE layout: `opcode:u8, title_len:u8, reserved:[u8;2], cap:u32,
/// title:[u8;64]`. `title_len` names the number of UTF-8 bytes, not characters.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetTitle {
    /// Must be [`compositor_ops::SET_TITLE`].
    pub opcode: u8,
    /// Number of valid UTF-8 bytes in `title`.
    pub title_len: u8,
    /// Reserved; must be zero on the wire.
    pub _pad: [u8; 2],
    /// Surface capability owned by the sender.
    pub cap: u32,
    /// Zero-padded UTF-8 title bytes.
    pub title: [u8; MAX_TITLE_BYTES],
}

impl SetTitle {
    /// Construct a title request for `cap` from UTF-8 `title`.
    ///
    /// # Returns
    /// A zero-padded fixed-width request containing `title`'s bytes.
    ///
    /// # Errors
    /// Returns [`DisplayProtocolError::TitleTooLong`] when `title` exceeds
    /// [`MAX_TITLE_BYTES`] bytes.
    pub fn new(cap: u32, title: &str) -> Result<Self, DisplayProtocolError> {
        if title.len() > MAX_TITLE_BYTES {
            return Err(DisplayProtocolError::TitleTooLong);
        }
        let mut bytes = [0; MAX_TITLE_BYTES];
        bytes[..title.len()].copy_from_slice(title.as_bytes());
        Ok(Self {
            opcode: compositor_ops::SET_TITLE,
            title_len: title.len() as u8,
            _pad: [0; 2],
            cap,
            title: bytes,
        })
    }

    /// Return the validated UTF-8 title.
    ///
    /// # Errors
    /// Returns the applicable protocol error if callers modified this public
    /// frame after construction.
    pub fn title_str(&self) -> Result<&str, DisplayProtocolError> {
        self.validate()?;
        core::str::from_utf8(&self.title[..self.title_len as usize])
            .map_err(|_| DisplayProtocolError::InvalidUtf8)
    }

    /// Encode this request into its exact 72-byte little-endian wire frame.
    ///
    /// # Errors
    /// Returns a protocol error when a public field contains an invalid opcode,
    /// reserved byte, title length, title encoding, or title padding.
    pub fn encode(&self) -> Result<[u8; 72], DisplayProtocolError> {
        self.validate()?;
        let mut bytes = [0; 72];
        bytes[0] = self.opcode;
        bytes[1] = self.title_len;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..72].copy_from_slice(&self.title);
        Ok(bytes)
    }

    /// Decode an exact 72-byte little-endian title request.
    ///
    /// # Errors
    /// Returns a protocol error for a wrong length or opcode, nonzero reserved
    /// or title-padding bytes, an oversized title, or invalid UTF-8.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        if bytes.len() != 72 {
            return Err(DisplayProtocolError::InvalidLength);
        }
        let mut title = [0; MAX_TITLE_BYTES];
        title.copy_from_slice(&bytes[8..72]);
        let frame = Self {
            opcode: bytes[0],
            title_len: bytes[1],
            _pad: [bytes[2], bytes[3]],
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            title,
        };
        frame.validate()?;
        Ok(frame)
    }

    fn validate(&self) -> Result<(), DisplayProtocolError> {
        if self.opcode != compositor_ops::SET_TITLE {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if self._pad != [0; 2] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        let length = self.title_len as usize;
        if length > MAX_TITLE_BYTES {
            return Err(DisplayProtocolError::TitleTooLong);
        }
        if self.title[length..].iter().any(|byte| *byte != 0) {
            return Err(DisplayProtocolError::NonZeroTitlePadding);
        }
        core::str::from_utf8(&self.title[..length])
            .map_err(|_| DisplayProtocolError::InvalidUtf8)?;
        Ok(())
    }
}

/// Fixed 12-byte owner acknowledgement of a compositor configuration proposal.
///
/// LE layout: `opcode:u8, reserved:[u8;3], cap:u32, serial:u32`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigureAck {
    /// Must be [`compositor_ops::CONFIGURE_ACK`].
    pub opcode: u8,
    /// Reserved; must be zero on the wire.
    pub _pad: [u8; 3],
    /// Surface capability being acknowledged.
    pub cap: u32,
    /// Configuration serial supplied by [`WindowConfigure`].
    pub serial: u32,
}

/// Fixed 12-byte owner response to a compositor close request.
///
/// LE layout: `opcode:u8, accept:u8, reserved:[u8;2], cap:u32, serial:u32`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseResponse {
    /// Must be [`compositor_ops::CLOSE_RESPONSE`].
    pub opcode: u8,
    /// Exactly `Reject` or `Accept` on the wire.
    pub accept: CloseResponseAction,
    /// Reserved; must be zero on the wire.
    pub _pad: [u8; 2],
    /// Surface capability being closed.
    pub cap: u32,
    /// Close-request serial supplied by [`WindowCloseRequest`].
    pub serial: u32,
}

/// Fixed 8-byte owner request to change a window's compositor-managed state.
///
/// LE layout: `opcode:u8, reserved:[u8;3], cap:u32`. `opcode` is one of
/// `MINIMIZE`, `MAXIMIZE`, or `RESTORE`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceStateRequest {
    /// State-request opcode.
    pub opcode: u8,
    /// Reserved; must be zero on the wire.
    pub _pad: [u8; 3],
    /// Surface capability whose state changes.
    pub cap: u32,
}

/// Fixed 16-byte owner acknowledgement that a retired Grant may be released.
///
/// LE layout: `opcode:u8, reserved:[u8;3], cap:u32, reg_id:u64`. It names the
/// old registration only; it never detaches the newly active Grant.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetachReplacedGrant {
    /// Must be [`compositor_ops::DETACH_REPLACED_GRANT`].
    pub opcode: u8,
    /// Reserved; must be zero on the wire.
    pub _pad: [u8; 3],
    /// Surface capability whose retired Grant is acknowledged.
    pub cap: u32,
    /// Retired Grant registration identifier.
    pub reg_id: u64,
}

/// Fixed 28-byte compositor-to-owner configuration proposal.
///
/// LE layout: `opcode:u8, kind:u8, reserved:[u8;2], cap:u32, serial:u32,
/// x:i32, y:i32, w:u32, h:u32`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowConfigure {
    /// Must be [`compositor_events::WINDOW_CONFIGURE`].
    pub opcode: u8,
    /// Kind of proposed configuration.
    pub kind: ConfigureKind,
    /// Reserved; must be zero on the wire.
    pub _pad: [u8; 2],
    /// Surface capability addressed by the proposal.
    pub cap: u32,
    /// Serial the owner must acknowledge.
    pub serial: u32,
    /// Proposed content rectangle in screen coordinates.
    pub rect: Rect,
}

/// Fixed 12-byte compositor-to-owner close request.
///
/// LE layout: `opcode:u8, reserved:[u8;3], cap:u32, serial:u32`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowCloseRequest {
    /// Must be [`compositor_events::WINDOW_CLOSE_REQUEST`].
    pub opcode: u8,
    /// Reserved; must be zero on the wire.
    pub _pad: [u8; 3],
    /// Surface capability addressed by the request.
    pub cap: u32,
    /// Serial the owner must echo in its close response.
    pub serial: u32,
}

/// Fixed 12-byte compositor-to-owner state notification.
///
/// LE layout: `opcode:u8, state:u8, reserved:[u8;2], cap:u32, serial:u32`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowStateChanged {
    /// Must be [`compositor_events::WINDOW_STATE_CHANGED`].
    pub opcode: u8,
    /// Newly active compositor-managed state.
    pub state: WindowState,
    /// Reserved; must be zero on the wire.
    pub _pad: [u8; 2],
    /// Surface capability addressed by the notification.
    pub cap: u32,
    /// Monotonic compositor serial for this transition.
    pub serial: u32,
}

impl ConfigureAck {
    /// Construct an acknowledgement for `cap` and compositor `serial`.
    ///
    /// # Returns
    /// A zero-reserved fixed-width acknowledgement.
    pub const fn new(cap: u32, serial: u32) -> Self {
        Self {
            opcode: compositor_ops::CONFIGURE_ACK,
            _pad: [0; 3],
            cap,
            serial,
        }
    }
}

impl CloseResponse {
    /// Construct a close response for `cap`, request `serial`, and `accept` decision.
    ///
    /// # Returns
    /// A fixed-width response carrying [`CloseResponseAction::Accept`] when
    /// `accept` is true and [`CloseResponseAction::Reject`] otherwise.
    pub const fn new(cap: u32, serial: u32, accept: bool) -> Self {
        Self {
            opcode: compositor_ops::CLOSE_RESPONSE,
            accept: if accept {
                CloseResponseAction::Accept
            } else {
                CloseResponseAction::Reject
            },
            _pad: [0; 2],
            cap,
            serial,
        }
    }
}

impl SurfaceStateRequest {
    /// Construct a state request for `cap` using `opcode`.
    ///
    /// # Returns
    /// A zero-reserved fixed-width state request.
    ///
    /// # Errors
    /// Returns [`DisplayProtocolError::InvalidOpcode`] unless `opcode` is
    /// `MINIMIZE`, `MAXIMIZE`, or `RESTORE`.
    pub fn new(cap: u32, opcode: u8) -> Result<Self, DisplayProtocolError> {
        let frame = Self {
            opcode,
            _pad: [0; 3],
            cap,
        };
        frame.validate()?;
        Ok(frame)
    }
}

impl DetachReplacedGrant {
    /// Construct a retired-Grant acknowledgement for `cap` and `reg_id`.
    pub const fn new(cap: u32, reg_id: u64) -> Self {
        Self {
            opcode: compositor_ops::DETACH_REPLACED_GRANT,
            _pad: [0; 3],
            cap,
            reg_id,
        }
    }

    /// Encode this acknowledgement into its exact 16-byte little-endian frame.
    ///
    /// # Errors
    /// Returns a protocol error for an invalid opcode or nonzero reserved byte.
    pub fn encode(&self) -> Result<[u8; 16], DisplayProtocolError> {
        if self.opcode != compositor_ops::DETACH_REPLACED_GRANT {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if self._pad != [0; 3] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        let mut bytes = [0; 16];
        bytes[0] = self.opcode;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.reg_id.to_le_bytes());
        Ok(bytes)
    }

    /// Decode an exact 16-byte little-endian retired-Grant acknowledgement.
    ///
    /// # Errors
    /// Returns a protocol error for a wrong length or opcode or nonzero reserved byte.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        if bytes.len() != 16 {
            return Err(DisplayProtocolError::InvalidLength);
        }
        let frame = Self {
            opcode: bytes[0],
            _pad: [bytes[1], bytes[2], bytes[3]],
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            reg_id: u64::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
        };
        if frame.opcode != compositor_ops::DETACH_REPLACED_GRANT {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if frame._pad != [0; 3] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        Ok(frame)
    }
}

impl ConfigureAck {
    /// Encode this acknowledgement into its exact 12-byte little-endian frame.
    ///
    /// # Errors
    /// Returns a protocol error for an invalid opcode or nonzero reserved byte.
    pub fn encode(&self) -> Result<[u8; 12], DisplayProtocolError> {
        if self.opcode != compositor_ops::CONFIGURE_ACK {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if self._pad != [0; 3] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        let mut bytes = [0; 12];
        bytes[0] = self.opcode;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.serial.to_le_bytes());
        Ok(bytes)
    }

    /// Decode an exact 12-byte little-endian configuration acknowledgement.
    ///
    /// # Errors
    /// Returns a protocol error for a wrong length or opcode or nonzero reserved byte.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        if bytes.len() != 12 {
            return Err(DisplayProtocolError::InvalidLength);
        }
        let frame = Self {
            opcode: bytes[0],
            _pad: [bytes[1], bytes[2], bytes[3]],
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            serial: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        };
        if frame.opcode != compositor_ops::CONFIGURE_ACK {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if frame._pad != [0; 3] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        Ok(frame)
    }
}

impl CloseResponse {
    /// Encode this response into its exact 12-byte little-endian frame.
    ///
    /// # Errors
    /// Returns a protocol error for an invalid opcode, action, or reserved byte.
    pub fn encode(&self) -> Result<[u8; 12], DisplayProtocolError> {
        if self.opcode != compositor_ops::CLOSE_RESPONSE {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        CloseResponseAction::decode(self.accept as u8)?;
        if self._pad != [0; 2] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        let mut bytes = [0; 12];
        bytes[0] = self.opcode;
        bytes[1] = self.accept as u8;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.serial.to_le_bytes());
        Ok(bytes)
    }

    /// Decode an exact 12-byte little-endian close response.
    ///
    /// # Errors
    /// Returns a protocol error for a wrong length or opcode, unknown action,
    /// or nonzero reserved byte.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        if bytes.len() != 12 {
            return Err(DisplayProtocolError::InvalidLength);
        }
        let frame = Self {
            opcode: bytes[0],
            accept: CloseResponseAction::decode(bytes[1])?,
            _pad: [bytes[2], bytes[3]],
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            serial: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        };
        if frame.opcode != compositor_ops::CLOSE_RESPONSE {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if frame._pad != [0; 2] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        Ok(frame)
    }
}

impl SurfaceStateRequest {
    /// Encode this state request into its exact 8-byte little-endian frame.
    ///
    /// # Errors
    /// Returns a protocol error for an invalid opcode or nonzero reserved byte.
    pub fn encode(&self) -> Result<[u8; 8], DisplayProtocolError> {
        self.validate()?;
        let mut bytes = [0; 8];
        bytes[0] = self.opcode;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        Ok(bytes)
    }

    /// Decode an exact 8-byte little-endian state request.
    ///
    /// # Errors
    /// Returns a protocol error for a wrong length or opcode or nonzero reserved byte.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        if bytes.len() != 8 {
            return Err(DisplayProtocolError::InvalidLength);
        }
        let frame = Self {
            opcode: bytes[0],
            _pad: [bytes[1], bytes[2], bytes[3]],
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        };
        frame.validate()?;
        Ok(frame)
    }
}

impl SurfaceStateRequest {
    fn validate(&self) -> Result<(), DisplayProtocolError> {
        if !matches!(
            self.opcode,
            compositor_ops::MINIMIZE | compositor_ops::MAXIMIZE | compositor_ops::RESTORE
        ) {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if self._pad != [0; 3] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        Ok(())
    }
}

impl WindowConfigure {
    /// Encode this configure event into its exact 28-byte little-endian frame.
    ///
    /// # Errors
    /// Returns a protocol error for an invalid opcode, configure kind, or
    /// nonzero reserved byte.
    pub fn encode(&self) -> Result<[u8; 28], DisplayProtocolError> {
        self.validate()?;
        let mut bytes = [0; 28];
        bytes[0] = self.opcode;
        bytes[1] = self.kind as u8;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.serial.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.rect.x.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.rect.y.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.rect.w.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.rect.h.to_le_bytes());
        Ok(bytes)
    }

    /// Decode an exact 28-byte little-endian configure event.
    ///
    /// # Errors
    /// Returns a protocol error for a wrong length or opcode, unknown configure
    /// kind, or nonzero reserved byte.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        if bytes.len() != 28 {
            return Err(DisplayProtocolError::InvalidLength);
        }
        let frame = Self {
            opcode: bytes[0],
            kind: ConfigureKind::decode(bytes[1])?,
            _pad: [bytes[2], bytes[3]],
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            serial: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            rect: Rect {
                x: i32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
                y: i32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
                w: u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
                h: u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            },
        };
        frame.validate()?;
        Ok(frame)
    }

    fn validate(&self) -> Result<(), DisplayProtocolError> {
        if self.opcode != compositor_events::WINDOW_CONFIGURE {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        ConfigureKind::decode(self.kind as u8)?;
        if self._pad != [0; 2] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        Ok(())
    }
}

macro_rules! impl_compositor_event {
    ($type:ident, $opcode:expr, $field:ident : $field_ty:ty, $decode_field:expr) => {
        impl $type {
            /// Encode this compositor event into its exact 12-byte little-endian frame.
            ///
            /// # Errors
            /// Returns a protocol error for an invalid opcode, discriminant, or
            /// nonzero reserved byte.
            pub fn encode(&self) -> Result<[u8; 12], DisplayProtocolError> {
                self.validate()?;
                let mut bytes = [0; 12];
                bytes[0] = self.opcode;
                bytes[1] = self.$field as u8;
                bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
                bytes[8..12].copy_from_slice(&self.serial.to_le_bytes());
                Ok(bytes)
            }

            /// Decode an exact 12-byte little-endian compositor event.
            ///
            /// # Errors
            /// Returns a protocol error for a wrong length or opcode, unknown
            /// discriminant, or nonzero reserved byte.
            pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
                if bytes.len() != 12 {
                    return Err(DisplayProtocolError::InvalidLength);
                }
                let frame = Self {
                    opcode: bytes[0],
                    $field: $decode_field(bytes[1])?,
                    _pad: [bytes[2], bytes[3]],
                    cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                    serial: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
                };
                frame.validate()?;
                Ok(frame)
            }

            fn validate(&self) -> Result<(), DisplayProtocolError> {
                if self.opcode != $opcode {
                    return Err(DisplayProtocolError::InvalidOpcode);
                }
                let _: $field_ty = $decode_field(self.$field as u8)?;
                if self._pad != [0; 2] {
                    return Err(DisplayProtocolError::NonZeroReserved);
                }
                Ok(())
            }
        }
    };
}

impl WindowCloseRequest {
    /// Encode this close request into its exact 12-byte little-endian frame.
    ///
    /// # Errors
    /// Returns a protocol error for an invalid opcode or nonzero reserved byte.
    pub fn encode(&self) -> Result<[u8; 12], DisplayProtocolError> {
        if self.opcode != compositor_events::WINDOW_CLOSE_REQUEST {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if self._pad != [0; 3] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        let mut bytes = [0; 12];
        bytes[0] = self.opcode;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.serial.to_le_bytes());
        Ok(bytes)
    }

    /// Decode an exact 12-byte little-endian close request.
    ///
    /// # Errors
    /// Returns a protocol error for a wrong length or opcode or nonzero reserved byte.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        if bytes.len() != 12 {
            return Err(DisplayProtocolError::InvalidLength);
        }
        let frame = Self {
            opcode: bytes[0],
            _pad: [bytes[1], bytes[2], bytes[3]],
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            serial: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        };
        if frame.opcode != compositor_events::WINDOW_CLOSE_REQUEST {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if frame._pad != [0; 3] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        Ok(frame)
    }
}

impl_compositor_event!(
    WindowStateChanged,
    compositor_events::WINDOW_STATE_CHANGED,
    state: WindowState,
    WindowState::decode
);

const _: () = assert!(core::mem::size_of::<SetTitle>() == 72);
const _: () = assert!(core::mem::size_of::<ConfigureAck>() == 12);
const _: () = assert!(core::mem::size_of::<CloseResponse>() == 12);
const _: () = assert!(core::mem::size_of::<SurfaceStateRequest>() == 8);
const _: () = assert!(core::mem::size_of::<DetachReplacedGrant>() == 16);
const _: () = assert!(core::mem::size_of::<WindowConfigure>() == 28);
const _: () = assert!(core::mem::size_of::<WindowCloseRequest>() == 12);
const _: () = assert!(core::mem::size_of::<WindowStateChanged>() == 12);

// ─── Compositor IPC opcodes ───────────────────────────────────────────────────

/// Opcodes for messages from cells to the compositor cell.
pub mod compositor_ops {
    /// Request a new surface of `(w: u32, h: u32)` pixels and a `SurfaceRole`.
    /// Payload: `[w: u32 LE, h: u32 LE, role: u8]`; the legacy nine-byte form
    /// defaults to `SurfaceRole::Interactive`; reply: cap (u32 LE, zero-padded
    /// to 8 bytes).
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

    /// Replace the UTF-8 title of an owner surface.
    ///
    /// Payload: [`SetTitle`] (72 bytes).
    pub const SET_TITLE: u8 = 0x0A;

    /// Acknowledge the matching [`WindowConfigure`] serial.
    ///
    /// Payload: [`ConfigureAck`] (12 bytes).
    pub const CONFIGURE_ACK: u8 = 0x0B;

    /// Accept or reject the matching [`WindowCloseRequest`] serial.
    ///
    /// Payload: [`CloseResponse`] (12 bytes).
    pub const CLOSE_RESPONSE: u8 = 0x0C;

    /// Request that an owner surface be minimized.
    ///
    /// Payload: [`SurfaceStateRequest`] (8 bytes).
    pub const MINIMIZE: u8 = 0x0D;

    /// Request that an owner surface be maximized.
    ///
    /// Payload: [`SurfaceStateRequest`] (8 bytes).
    pub const MAXIMIZE: u8 = 0x0E;

    /// Request that an owner surface be restored from a managed state.
    ///
    /// Payload: [`SurfaceStateRequest`] (8 bytes).
    pub const RESTORE: u8 = 0x0F;

    /// Acknowledge that the retired Grant named by [`DetachReplacedGrant`] can
    /// be released without detaching the newly active Grant.
    ///
    /// Payload: [`DetachReplacedGrant`] (16 bytes).
    pub const DETACH_REPLACED_GRANT: u8 = 0x11;

    /// Query screen dimensions.
    /// Payload: empty  Reply: `[w: u32, h: u32]`
    pub const GET_SCREEN_SIZE: u8 = 0x10;

    /// Dump raw framebuffer (debug only).
    /// Reply: pixel data of whole screen
    pub const DUMP_FB: u8 = 0xFE;
}

/// Opcodes for compositor lifecycle events delivered to a surface owner.
pub mod compositor_events {
    /// A configuration proposal encoded as [`WindowConfigure`] (28 bytes).
    pub const WINDOW_CONFIGURE: u8 = 0xA0;
    /// A close request encoded as [`WindowCloseRequest`] (12 bytes).
    pub const WINDOW_CLOSE_REQUEST: u8 = 0xA1;
    /// A state transition encoded as [`WindowStateChanged`] (12 bytes).
    pub const WINDOW_STATE_CHANGED: u8 = 0xA2;
}

// ─── Misc constants ───────────────────────────────────────────────────────────

/// Compositor IPC endpoint (conventionally cell 5 in default boot).
pub const COMPOSITOR_ENDPOINT: usize = 5; // init=1, vfs=2, config=3, net=4, compositor=5

/// Screen resolution used when VirtIO GPU is unavailable.
pub const FALLBACK_WIDTH: u32 = 1024;
pub const FALLBACK_HEIGHT: u32 = 768;
