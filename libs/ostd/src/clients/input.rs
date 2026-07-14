// SPDX-License-Identifier: MPL-2.0

//! Input service client — keyboard / mouse focus registration.

use super::vierr_from_code;
use crate::service::InputRef;
use crate::{ViError, ViResult};
use api::ipc::{InputRequest, InputResponse, IPC_BUF_SIZE};

/// Ergonomic client for the input service.
///
/// Wraps [`InputRef`] and provides focus registration helpers.
pub struct InputClient {
    svc: InputRef,
}

impl InputClient {
    /// Create a new unresolved client. Resolution is lazy (first call).
    pub fn new() -> Self {
        Self {
            svc: InputRef::new(),
        }
    }

    /// Register the calling cell as the keyboard/mouse focus recipient.
    ///
    /// Returns `true` on success. May return `false` on a boot race (the input
    /// service may not be registered yet) — retry after a `sys_yield`.
    pub fn request_focus(&mut self) -> bool {
        // Use the ostd::input helper which handles the TID lookup internally.
        crate::input::request_focus()
    }

    /// Query which cell currently has focus. Returns the TID (0 = none).
    pub fn get_focus(&mut self) -> ViResult<u32> {
        let req = InputRequest::GetFocus;
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<InputRequest, InputResponse>(&req, &mut resp_buf)?
        {
            InputResponse::Focus(tid) => Ok(tid),
            InputResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    /// Unregister the calling cell from receiving input events.
    pub fn clear_focus(&mut self, cell_tid: u32) -> ViResult<()> {
        let req = InputRequest::ClearFocus { cell_tid };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .svc
            .call::<InputRequest, InputResponse>(&req, &mut resp_buf)?
        {
            InputResponse::Ok => Ok(()),
            InputResponse::Err(code) => Err(vierr_from_code(code)),
            _ => Err(ViError::IO),
        }
    }
}

impl Default for InputClient {
    fn default() -> Self {
        Self::new()
    }
}
