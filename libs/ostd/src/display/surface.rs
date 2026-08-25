//! Grant-backed compositor surface allocation and immediate drawing operations.

use api::display::{compositor_ops, DamageNotify, PixelFormat, Rect, SurfaceRole};
use types::{ViError, ViResult};

use crate::syscall::{
    sys_grant_register, sys_grant_share, sys_grant_slice, sys_grant_unregister, sys_send,
};

use super::ipc;

/// A compositor surface backed by a Grant buffer the app cell owns directly.
///
/// The app writes pixels into [`Self::pixels_mut`] and calls [`Self::damage`] or
/// [`Self::damage_all`] to request re-blending. `ViSurface` is `!Send`: its
/// Grant pointer must remain on the cell's task.
pub struct ViSurface {
    pub(super) comp_tid: usize,
    pub(super) cap: u32,
    pub(super) reg_id: usize,
    pub(super) retired_reg_id: Option<usize>,
    pub(super) staged_reg_id: Option<usize>,
    pub(super) ptr: *mut u8,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) fmt: PixelFormat,
    _not_send: core::marker::PhantomData<*mut ()>,
}

impl ViSurface {
    /// Create an interactive `width` × `height` BGRA surface owned by `comp_tid`.
    ///
    /// Returns a Grant-backed surface whose pixels are writable by this task.
    ///
    /// # Errors
    /// Returns `OutOfMemory` when Grant registration fails and `IO` when Grant
    /// mapping or compositor surface setup fails.
    pub fn create(comp_tid: usize, width: u32, height: u32, fmt: PixelFormat) -> ViResult<Self> {
        Self::create_with_role(comp_tid, width, height, fmt, SurfaceRole::Interactive)
    }

    /// Create a non-focusable `width` × `height` background surface for `comp_tid`.
    ///
    /// # Errors
    /// Returns `OutOfMemory` when Grant registration fails and `IO` when Grant
    /// mapping or compositor surface setup fails.
    pub fn create_background(
        comp_tid: usize,
        width: u32,
        height: u32,
        fmt: PixelFormat,
    ) -> ViResult<Self> {
        Self::create_with_role(comp_tid, width, height, fmt, SurfaceRole::Background)
    }

    fn create_with_role(
        comp_tid: usize,
        width: u32,
        height: u32,
        fmt: PixelFormat,
        role: SurfaceRole,
    ) -> ViResult<Self> {
        let size = (width * height * fmt.bpp()) as usize;
        let reg_id = sys_grant_register(size).ok_or(ViError::OutOfMemory)?;
        sys_grant_share(reg_id, comp_tid, 0 /* ReadOnly */);
        let ptr = sys_grant_slice(reg_id).ok_or_else(|| {
            sys_grant_unregister(reg_id);
            ViError::IO
        })?;
        let cap = ipc::create_surface(comp_tid, width, height, role).inspect_err(|_| {
            sys_grant_unregister(reg_id);
        })?;
        ipc::attach_grant(comp_tid, cap, reg_id, width, height, fmt).inspect_err(|_| {
            let _ = ipc::destroy_surface(comp_tid, cap);
            sys_grant_unregister(reg_id);
        })?;
        Ok(Self {
            comp_tid,
            cap,
            reg_id,
            retired_reg_id: None,
            staged_reg_id: None,
            ptr,
            width,
            height,
            fmt,
            _not_send: core::marker::PhantomData,
        })
    }

    /// Return mutable pixels for the current Grant-backed surface dimensions.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        let len = (self.width * self.height * self.fmt.bpp()) as usize;
        // SAFETY: this is our registered Grant; `&mut self` prevents concurrent use.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, len) }
    }

    /// Return the current row stride in bytes.
    pub fn stride(&self) -> usize {
        self.width as usize * self.fmt.bpp() as usize
    }

    /// Return the current surface width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Return the current surface height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Return the capability that identifies this surface in lifecycle events.
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// Mark the surface-local `rect` dirty for a later compositor blend.
    pub fn damage(&self, rect: Rect) {
        let msg = DamageNotify {
            opcode: compositor_ops::DAMAGE_NOTIFY,
            _pad: [0; 3],
            cap: self.cap,
            rect,
        };
        sys_send(self.comp_tid, &msg.encode());
    }

    /// Mark every current surface pixel dirty for a later compositor blend.
    pub fn damage_all(&self) {
        self.damage(Rect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        });
    }

    /// Move the surface's content origin to screen coordinates (`x`, `y`).
    pub fn move_to(&self, x: i32, y: i32) {
        let mut buf = [0u8; 13];
        buf[0] = compositor_ops::MOVE_SURFACE;
        buf[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        buf[9..13].copy_from_slice(&x.to_le_bytes());
        let mut buf17 = [0u8; 17];
        buf17[..13].copy_from_slice(&buf);
        buf17[13..17].copy_from_slice(&y.to_le_bytes());
        sys_send(self.comp_tid, &buf17);
    }

    /// Raise this surface above every other compositor surface.
    pub fn raise(&self) {
        let mut buf = [0u8; 9];
        buf[0] = compositor_ops::RAISE_SURFACE;
        buf[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        sys_send(self.comp_tid, &buf);
    }
}
