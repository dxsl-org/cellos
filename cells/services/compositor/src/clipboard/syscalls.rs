//! The clipboard's [`Kernel`] calls on CellOS.

use ostd::syscall::{
    sys_grant_copy_rows_owned_by, sys_grant_register, sys_grant_share, sys_try_send, GrantRows,
    SyscallResult,
};
use wire::display::CLIPBOARD_MAX;

use super::Kernel;

/// The running kernel, as the compositor's serving thread (which owns the
/// clipboard Grant).
pub struct Syscalls;

/// One row of `len` bytes from the start of a Grant.
const fn row(len: usize) -> GrantRows {
    GrantRows {
        src_offset: 0,
        src_stride: len,
        dst_offset: 0,
        dst_stride: len,
        row_len: len,
        rows: 1,
    }
}

impl Kernel for Syscalls {
    fn copy_from(&mut self, grant: usize, owner: usize, dst: &mut [u8]) -> bool {
        sys_grant_copy_rows_owned_by(grant, owner, row(dst.len()), dst).is_some()
    }

    fn register(&mut self) -> Option<usize> {
        sys_grant_register(CLIPBOARD_MAX)
    }

    fn write(&mut self, grant: usize, text: &[u8]) -> bool {
        ostd::grant::with_shared_bytes_mut(grant, |bytes| {
            bytes
                .get_mut(..text.len())
                .map(|dst| dst.copy_from_slice(text))
        })
        .flatten()
        .is_some()
    }

    fn share(&mut self, grant: usize, tid: usize) -> bool {
        sys_grant_share(grant, tid, 0)
    }

    fn send(&mut self, tid: usize, frame: &[u8]) -> bool {
        matches!(sys_try_send(tid, frame), SyscallResult::Ok(0))
    }
}
