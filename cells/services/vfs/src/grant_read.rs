//! Serving grant read operations (ReadGrant and ReadFileGrant).

use api::ipc::VfsResponse;

use crate::caller::Caller;
use crate::manager::VfsManager;
use crate::paths::{ERR_DENIED, ERR_IO};

pub(crate) fn read_grant<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    cap: u64,
    offset: u64,
    size: usize,
    grant: usize,
) -> VfsResponse<'a> {
    match vfs.handles.path_of(caller, api::cap::CapId(cap)) {
        Some(path) if !vfs.access.can_read(caller, path) => {
            return VfsResponse::Err(ERR_DENIED);
        }
        _ => {}
    }
    match ostd::syscall::sys_grant_slice_with_len(grant) {
        None => VfsResponse::Err(ERR_IO),
        Some((ptr, grant_len)) => {
            let bytes = if let Some(entry) = vfs.handles.get_mut(caller, api::cap::CapId(cap)) {
                match usize::try_from(offset) {
                    Ok(offset) if offset < entry.data_len => {
                        let avail = entry.data_len - offset;
                        let n = size.min(avail).min(grant_len).min(4096);
                        if n == 0 {
                            0
                        } else if let Some(src) = entry.data_ptr.checked_add(offset) {
                            // SAFETY: `src` stays within the in-memory file image because
                            // `offset < data_len` and `n <= data_len - offset`; `ptr` is a
                            // kernel-validated grant buffer of at least `n` bytes.
                            unsafe {
                                core::ptr::copy_nonoverlapping(src as *const u8, ptr, n);
                            }
                            n
                        } else {
                            0
                        }
                    }
                    Ok(_) | Err(_) => 0,
                }
            } else {
                0
            };
            VfsResponse::GrantDone { bytes }
        }
    }
}

pub(crate) fn read_file_grant<'a>(
    vfs: &VfsManager,
    caller: Caller,
    path: &str,
    grant: usize,
    max: usize,
) -> VfsResponse<'a> {
    if !vfs.access.can_read(caller, path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    match ostd::syscall::sys_grant_slice_with_len(grant) {
        None => VfsResponse::Err(ERR_IO),
        Some((ptr, grant_len)) => {
            let data = vfs.read_to_vec(path);
            let n = data.len().min(max).min(grant_len);
            unsafe {
                core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, n);
            }
            VfsResponse::GrantDone { bytes: n }
        }
    }
}
