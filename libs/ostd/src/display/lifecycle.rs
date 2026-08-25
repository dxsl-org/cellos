//! Managed-surface lifecycle requests, Grant replacement, and cleanup.

use api::display::{compositor_ops, SetTitle, SurfaceStateRequest, WindowConfigure};
use types::{ViError, ViResult};

use crate::syscall::{
    sys_grant_register, sys_grant_share, sys_grant_slice, sys_grant_unregister, sys_send,
};

use super::ipc::{self, AttachGrantResult};
use super::surface::ViSurface;

impl ViSurface {
    /// Set this surface's UTF-8 `title` for compositor-owned decoration.
    ///
    /// # Errors
    /// Returns `InvalidInput` when `title` exceeds the protocol bound and `IO`
    /// when the request cannot be sent.
    pub fn set_title(&self, title: &str) -> ViResult<()> {
        let request = SetTitle::new(self.cap, title).map_err(|_| ViError::InvalidInput)?;
        let frame = request.encode().map_err(|_| ViError::InvalidInput)?;
        ipc::send_lifecycle_request(self.comp_tid, &frame)
    }

    /// Request compositor minimization of this managed surface.
    ///
    /// # Errors
    /// Returns `IO` when the request cannot be sent.
    pub fn minimize(&self) -> ViResult<()> {
        self.request_state(compositor_ops::MINIMIZE)
    }

    /// Request compositor maximization of this managed surface.
    ///
    /// # Errors
    /// Returns `IO` when the request cannot be sent.
    pub fn maximize(&self) -> ViResult<()> {
        self.request_state(compositor_ops::MAXIMIZE)
    }

    /// Request restoration from the current managed minimized or maximized state.
    ///
    /// # Errors
    /// Returns `IO` when the request cannot be sent.
    pub fn restore(&self) -> ViResult<()> {
        self.request_state(compositor_ops::RESTORE)
    }

    /// Acknowledge the compositor's nonzero configuration `serial`.
    ///
    /// # Errors
    /// Returns `IO` when the acknowledgement cannot be sent or is rejected.
    pub fn acknowledge_configure(&self, serial: u32) -> ViResult<()> {
        ipc::configure_ack(self.comp_tid, self.cap, serial)
    }

    /// Apply `configure` by staging a replacement Grant and acknowledging its serial.
    ///
    /// On successful acknowledgement this swaps the local dimensions and pixels.
    /// An explicit attachment rejection releases the new Grant; an ambiguous IPC
    /// failure retains it for `Drop` so the compositor never reads freed pages.
    ///
    /// # Errors
    /// Returns `InvalidArgument` for a mismatched cap, invalid geometry/serial,
    /// or unresolved staged/retired Grant; `OutOfMemory` when registration fails;
    /// and `IO` for mapping or compositor transaction failures.
    pub fn apply_configure(&mut self, configure: WindowConfigure) -> ViResult<()> {
        if configure.cap != self.cap
            || configure.serial == 0
            || configure.rect.w == 0
            || configure.rect.h == 0
            || self.staged_reg_id.is_some()
            || self.retired_reg_id.is_some()
        {
            return Err(ViError::InvalidArgument);
        }
        let new_size = ipc::surface_byte_len(configure.rect.w, configure.rect.h, self.fmt)?;
        let new_reg_id = sys_grant_register(new_size).ok_or(ViError::OutOfMemory)?;
        sys_grant_share(new_reg_id, self.comp_tid, 0 /* ReadOnly */);
        let new_ptr = match sys_grant_slice(new_reg_id) {
            Some(pointer) => pointer,
            None => {
                sys_grant_unregister(new_reg_id);
                return Err(ViError::IO);
            }
        };
        // An uncertain transport result retains the new Grant until Drop.
        self.staged_reg_id = Some(new_reg_id);
        match ipc::stage_grant(
            self.comp_tid,
            self.cap,
            new_reg_id,
            configure.rect.w,
            configure.rect.h,
            self.fmt,
        ) {
            AttachGrantResult::Attached => {}
            AttachGrantResult::Rejected => {
                self.staged_reg_id = None;
                sys_grant_unregister(new_reg_id);
                return Err(ViError::IO);
            }
            AttachGrantResult::AmbiguousFailure => return Err(ViError::IO),
        }
        if ipc::configure_ack(self.comp_tid, self.cap, configure.serial).is_err() {
            if ipc::detach_grant(self.comp_tid, self.cap).is_ok() {
                self.staged_reg_id = None;
                sys_grant_unregister(new_reg_id);
            }
            return Err(ViError::IO);
        }

        let old_reg_id = self.reg_id;
        self.reg_id = new_reg_id;
        self.retired_reg_id = Some(old_reg_id);
        self.ptr = new_ptr;
        self.staged_reg_id = None;
        self.width = configure.rect.w;
        self.height = configure.rect.h;
        self.detach_retired_grant();
        Ok(())
    }

    /// Respond to close-request `serial`; `accept` selects acceptance or rejection.
    ///
    /// # Errors
    /// Returns `InvalidInput` for an invalid wire frame and `IO` when delivery fails.
    pub fn respond_close(&self, serial: u32, accept: bool) -> ViResult<()> {
        let frame = api::display::CloseResponse::new(self.cap, serial, accept)
            .encode()
            .map_err(|_| ViError::InvalidInput)?;
        ipc::send_lifecycle_request(self.comp_tid, &frame)
    }

    fn request_state(&self, opcode: u8) -> ViResult<()> {
        let request =
            SurfaceStateRequest::new(self.cap, opcode).map_err(|_| ViError::InvalidInput)?;
        let frame = request.encode().map_err(|_| ViError::InvalidInput)?;
        ipc::send_lifecycle_request(self.comp_tid, &frame)
    }

    fn detach_retired_grant(&mut self) {
        let Some(reg_id) = self.retired_reg_id else {
            return;
        };
        if ipc::detach_replaced_grant(self.comp_tid, self.cap, reg_id).is_ok() {
            sys_grant_unregister(reg_id);
            self.retired_reg_id = None;
        }
    }

    /// Destroy this surface now; equivalent to consuming it through `Drop`.
    pub fn destroy(self) {
        drop(self);
    }
}

impl Drop for ViSurface {
    fn drop(&mut self) {
        let mut detach = [0u8; 9];
        detach[0] = compositor_ops::DETACH_GRANT;
        detach[1..9].copy_from_slice(&(self.cap as u64).to_le_bytes());
        sys_send(self.comp_tid, &detach);
        let _ = ipc::receive_status(self.comp_tid, 0x01);
        let _ = ipc::destroy_surface(self.comp_tid, self.cap);
        sys_grant_unregister(self.reg_id);
        if let Some(reg_id) = self.staged_reg_id {
            sys_grant_unregister(reg_id);
        }
        if let Some(reg_id) = self.retired_reg_id {
            sys_grant_unregister(reg_id);
        }
    }
}
