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
    let lease = if path == "/srv" || path.starts_with("/srv/") {
        let Ok(k) = crate::namespace::NamespaceKey::parse(&path) else {
            return VfsResponse::Err(ERR_DENIED);
        };
        let Ok(l) = vfs.ledger.acquire_service_handle(&k) else {
            return VfsResponse::Err(ERR_DENIED);
        };
        Some(l)
    } else {
        None
    };
    match vfs.files.insert_leased(caller, &path, dir.0, lease) {
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
    // Read only the requested range. This used to load the whole file into a fresh `Vec` and slice
    // it, once per request: a client reading in 4 KiB chunks paid `file_size` of copying and one
    // file-sized allocation per chunk. At 103 KB that was invisible; at 1.18 MB it drove a cell
    // read from the AI inference service into a multi-minute stall (`read_at` is the ranged
    // backend operation the whole time).
    let Some((size, is_dir)) = vfs.stat(&path) else {
        let _ = vfs.files.finish_sync_read(caller, file);
        return VfsResponse::Err(ERR_IO);
    };
    if is_dir {
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
    let size = match usize::try_from(size) {
        Ok(size) => size,
        Err(_) => usize::MAX,
    };
    if max == 0 || start >= size {
        let _ = vfs.files.finish_sync_read(caller, file);
        return VfsResponse::Data(&[]);
    }
    let n = usize::try_from(max)
        .unwrap_or(MAX_INLINE_PAYLOAD)
        .min(MAX_INLINE_PAYLOAD)
        .min(size - start);
    let read = vfs.read_at(&path, offset, &mut resp_buf[..n]);
    let _ = vfs.files.finish_sync_read(caller, file);
    VfsResponse::Data(&resp_buf[..read])
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
