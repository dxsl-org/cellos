//! App-side display helpers: `ViSurface` for Grant-backed compositor surfaces.
//!
//! ## Usage
//! ```
//! let comp_tid = wait_for_compositor();
//! let mut surf = ViSurface::create(comp_tid, 640, 480, PixelFormat::Bgra8888)?;
//! let px = surf.pixels_mut();
//! // draw into px ...
//! surf.damage_all();
//! ```

extern crate alloc;

use api::display::{compositor_ops, AttachGrant, DamageNotify, PixelFormat, Rect};
use api::syscall::service;
use types::{ViError, ViResult};

use crate::syscall::{
    sys_grant_register, sys_grant_share, sys_grant_slice, sys_grant_unregister, sys_lookup_service,
    sys_recv, sys_send, SyscallResult,
};

// ─── Service lookup ───────────────────────────────────────────────────────────

/// Block until the compositor service is registered and return its TID.
pub fn wait_for_compositor() -> usize {
    loop {
        if let Some(tid) = sys_lookup_service(service::COMPOSITOR) {
            return tid;
        }
        crate::task::yield_now();
    }
}

// ─── ViSurface ────────────────────────────────────────────────────────────────

/// A compositor surface backed by a Grant buffer the app cell owns directly.
///
/// The app writes pixels into `pixels_mut()` and calls `damage()` / `damage_all()`
/// to tell the compositor which regions need to be re-blended.  No pixel data
/// crosses an IPC boundary — only a 24-byte `DamageNotify` is sent per dirty region.
///
/// ## Lifecycle
/// `ViSurface::create` → write pixels → `damage` → … → `drop` (auto-cleans up).
///
/// `ViSurface` is `!Send`: the Grant pointer must stay on the cell's task.
pub struct ViSurface {
    comp_tid: usize,
    cap: u32,
    reg_id: usize,
    ptr: *mut u8,
    width: u32,
    height: u32,
    fmt: PixelFormat,
    /// Makes `ViSurface` !Send on stable Rust — the raw pointer must stay on its origin task.
    _not_send: core::marker::PhantomData<*mut ()>,
}

impl ViSurface {
    /// Create a new surface of `(width × height)` pixels, attaching it to the
    /// running compositor at `comp_tid`.
    ///
    /// Allocates a persistent Grant buffer, shares it read-only with the compositor,
    /// and sends `CREATE_SURFACE` + `ATTACH_GRANT` IPC.
    ///
    /// # Errors
    /// - `OutOfMemory` if `sys_grant_register` fails.
    /// - `IO` if the compositor rejects `ATTACH_GRANT` (e.g. too many surfaces).
    pub fn create(comp_tid: usize, width: u32, height: u32, fmt: PixelFormat) -> ViResult<Self> {
        let size = (width * height * fmt.bpp()) as usize;

        // 1. Allocate a persistent physical Grant buffer (lives until we call unregister).
        let reg_id = sys_grant_register(size).ok_or(ViError::OutOfMemory)?;

        // 2. Share read-only with compositor so it can read our pixels.
        sys_grant_share(reg_id, comp_tid, 0 /* ReadOnly */);

        // 3. Get our own write pointer into the Grant.
        let ptr = sys_grant_slice(reg_id).ok_or_else(|| {
            sys_grant_unregister(reg_id);
            ViError::IO
        })?;

        // 4. Ask compositor to create a surface slot → get cap.
        let cap = ipc_create_surface(comp_tid, width, height).inspect_err(|_e| {
            sys_grant_unregister(reg_id);
        })?;

        // 5. Tell compositor to attach our Grant to that slot.
        ipc_attach_grant(comp_tid, cap, reg_id, width, height, fmt).inspect_err(|_e| {
            let _ = ipc_destroy_surface(comp_tid, cap);
            sys_grant_unregister(reg_id);
        })?;

        Ok(Self {
            comp_tid,
            cap,
            reg_id,
            ptr,
            width,
            height,
            fmt,
            _not_send: core::marker::PhantomData,
        })
    }

    /// Direct mutable access to the pixel buffer.
    ///
    /// The app writes directly here; the compositor reads it via a read-only Grant.
    /// After writing, call `damage()` or `damage_all()` to trigger a repaint.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        let len = (self.width * self.height * self.fmt.bpp()) as usize;
        // SAFETY: ptr is our own registered Grant buffer (sys_grant_register).
        // We hold &mut self so no other code can call pixels_mut concurrently.
        // The compositor holds ReadOnly access — the kernel blocks any compositor write.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, len) }
    }

    /// Stride in bytes (width × bytes-per-pixel).
    pub fn stride(&self) -> usize {
        self.width as usize * self.fmt.bpp() as usize
    }

    /// Surface width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Surface height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Signal a dirty region to the compositor (fire-and-forget, 24-byte IPC).
    ///
    /// The compositor will re-blend this rect on the next render tick.
    pub fn damage(&self, rect: Rect) {
        let msg = DamageNotify {
            opcode: compositor_ops::DAMAGE_NOTIFY,
            _pad: [0; 3],
            cap: self.cap,
            rect,
        };
        sys_send(self.comp_tid, &msg.encode());
    }

    /// Signal the entire surface as dirty.
    pub fn damage_all(&self) {
        self.damage(Rect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        });
    }

    /// Move this surface to a new screen position.
    pub fn move_to(&self, x: i32, y: i32) {
        let mut buf = [0u8; 13];
        buf[0] = compositor_ops::MOVE_SURFACE;
        buf[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        buf[9..13].copy_from_slice(&x.to_le_bytes());
        // y needs 4 more bytes — extend to 17
        let mut buf17 = [0u8; 17];
        buf17[..13].copy_from_slice(&buf);
        buf17[13..17].copy_from_slice(&y.to_le_bytes());
        sys_send(self.comp_tid, &buf17);
    }

    /// Raise this surface to the top of the z-order.
    pub fn raise(&self) {
        let mut buf = [0u8; 9];
        buf[0] = compositor_ops::RAISE_SURFACE;
        buf[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        sys_send(self.comp_tid, &buf);
    }

    /// Explicitly destroy the surface (also called by `Drop`).
    pub fn destroy(self) {
        drop(self);
    }
}

impl Drop for ViSurface {
    fn drop(&mut self) {
        // 1. Detach grant — compositor stops reading from our buffer.
        let mut detach = [0u8; 9];
        detach[0] = compositor_ops::DETACH_GRANT;
        detach[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        sys_send(self.comp_tid, &detach);
        // Drain reply (compositor sends [0x01] on success).
        let mut resp = [0u8; 8];
        let _ = sys_recv(0, &mut resp);

        // 2. Destroy surface slot in compositor.
        let _ = ipc_destroy_surface(self.comp_tid, self.cap);

        // 3. Release physical Grant pages.
        sys_grant_unregister(self.reg_id);
    }
}

// ─── Private IPC helpers ──────────────────────────────────────────────────────

/// Send `CREATE_SURFACE` and return the cap (u32).
fn ipc_create_surface(comp_tid: usize, w: u32, h: u32) -> ViResult<u32> {
    let mut req = [0u8; 9];
    req[0] = compositor_ops::CREATE_SURFACE;
    req[1..5].copy_from_slice(&w.to_le_bytes());
    req[5..9].copy_from_slice(&h.to_le_bytes());
    sys_send(comp_tid, &req);

    let mut resp = [0u8; 8];
    match sys_recv(0, &mut resp) {
        SyscallResult::Ok(_) => {
            let cap = u32::from_le_bytes([resp[0], resp[1], resp[2], resp[3]]);
            if cap == 0 {
                Err(ViError::IO)
            } else {
                Ok(cap)
            }
        }
        _ => Err(ViError::IO),
    }
}

/// Send `ATTACH_GRANT` and verify the compositor accepted it.
fn ipc_attach_grant(
    comp_tid: usize,
    cap: u32,
    reg_id: usize,
    w: u32,
    h: u32,
    fmt: PixelFormat,
) -> ViResult<()> {
    let ag = AttachGrant {
        opcode: compositor_ops::ATTACH_GRANT,
        fmt: fmt as u8,
        _pad: [0; 2],
        cap,
        reg_id: reg_id as u64,
        width: w,
        height: h,
    };
    sys_send(comp_tid, &ag.encode());

    let mut resp = [0u8; 8];
    match sys_recv(0, &mut resp) {
        SyscallResult::Ok(_) if resp[0] == 0x01 => Ok(()),
        _ => Err(ViError::IO),
    }
}

/// Send `DESTROY_SURFACE` (best-effort; errors silently ignored on Drop path).
fn ipc_destroy_surface(comp_tid: usize, cap: u32) -> ViResult<()> {
    let mut req = [0u8; 9];
    req[0] = compositor_ops::DESTROY_SURFACE;
    req[1..9].copy_from_slice(&(cap as u64).to_le_bytes());
    sys_send(comp_tid, &req);

    let mut resp = [0u8; 8];
    match sys_recv(0, &mut resp) {
        SyscallResult::Ok(_) => Ok(()),
        _ => Err(ViError::IO),
    }
}
