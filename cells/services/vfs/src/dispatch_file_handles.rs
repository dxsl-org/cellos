use api::dir_handles::ViDirHandle;
use api::ipc::{VfsResponse, IPC_BUF_SIZE};
use api::vfs_file_handles::ViVfsFileHandle;

use crate::caller::Caller;
use crate::dispatch_dirs::{resolve, MAX_INLINE_PAYLOAD};
use crate::file_handles::FileHandleError;
use crate::manager::VfsManager;
use crate::paths::{ERR_DENIED, ERR_HANDLE, ERR_IO, ERR_QUOTA};

pub fn open_file<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    name: &str,
) -> VfsResponse<'a> {
    if !caller.may_own_state() {
        return VfsResponse::Err(ERR_DENIED);
    }
    let path = match resolve(vfs, caller, dir, name) {
        Ok(path) => path,
        Err(code) => return VfsResponse::Err(code),
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    match vfs.stat(&path) {
        Some((_, false)) => {}
        Some((_, true)) | None => return VfsResponse::Err(ERR_IO),
    }
    match vfs.files.insert(caller, &path, dir.0) {
        Ok(file) => VfsResponse::FileHandle(file),
        Err(err) => VfsResponse::Err(file_err(err)),
    }
}

pub fn read_file<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    file: ViVfsFileHandle,
    offset: u64,
    max: u32,
    resp_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> VfsResponse<'a> {
    let path = match vfs.files.begin_sync_read(caller, file) {
        Ok(path) => path,
        Err(err) => return VfsResponse::Err(file_err(err)),
    };
    if !vfs.access.can_read(caller, &path) {
        let _ = vfs.files.finish_sync_read(caller, file);
        return VfsResponse::Err(ERR_DENIED);
    }
    let data = vfs.read_to_vec(&path);
    if vfs.stat(&path).is_none() {
        let _ = vfs.files.finish_sync_read(caller, file);
        return VfsResponse::Err(ERR_IO);
    }
    let start = match usize::try_from(offset) {
        Ok(start) => start,
        Err(_) => {
            let _ = vfs.files.finish_sync_read(caller, file);
            return VfsResponse::Err(ERR_IO);
        }
    };
    if max == 0 || start >= data.len() {
        let _ = vfs.files.finish_sync_read(caller, file);
        return VfsResponse::Data(&[]);
    }
    let n = usize::try_from(max)
        .unwrap_or(MAX_INLINE_PAYLOAD)
        .min(MAX_INLINE_PAYLOAD)
        .min(data.len() - start);
    resp_buf[..n].copy_from_slice(&data[start..start + n]);
    let _ = vfs.files.finish_sync_read(caller, file);
    VfsResponse::Data(&resp_buf[..n])
}

pub fn close_file<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    file: ViVfsFileHandle,
) -> VfsResponse<'a> {
    if vfs.files.close(caller, file) {
        VfsResponse::Ok
    } else {
        VfsResponse::Err(ERR_HANDLE)
    }
}

fn file_err(err: FileHandleError) -> u8 {
    match err {
        FileHandleError::UnknownHandle => ERR_HANDLE,
        FileHandleError::TooManyHandles | FileHandleError::Exhausted => ERR_QUOTA,
    }
}
