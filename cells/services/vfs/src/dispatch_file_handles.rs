use abi::dir_handles::ViDirHandle;
use wire::ipc::{VfsResponse, IPC_BUF_SIZE};
use wire::proto::vfs::VfsError;
use wire::vfs_file_handles::ViVfsFileHandle;

use crate::caller::Caller;
use crate::dispatch_dirs::{resolve, MAX_INLINE_PAYLOAD};
use crate::file_handles::FileHandleError;
use crate::fs_error::FsError;
use crate::manager::VfsManager;

pub fn open_file<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    name: &str,
) -> VfsResponse<'a> {
    if !caller.may_own_state() {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    let path = match resolve(vfs, caller, dir, name) {
        Ok(path) => path,
        Err(code) => return VfsResponse::Err(code),
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    match vfs.stat(&path) {
        Ok(Some((_, false))) => {}
        Ok(Some((_, true)) | None) | Err(_) => return VfsResponse::Err(VfsError::Io as u8),
    }
    let lease = if path == "/srv" || path.starts_with("/srv/") {
        let Ok(k) = crate::namespace::NamespaceKey::parse(&path) else {
            return VfsResponse::Err(VfsError::Denied as u8);
        };
        let Ok(l) = vfs.ledger.acquire_service_handle(&k) else {
            return VfsResponse::Err(VfsError::Denied as u8);
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
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    // Read only the requested range. This used to load the whole file into a fresh `Vec` and slice
    // it, once per request: a client reading in 4 KiB chunks paid `file_size` of copying and one
    // file-sized allocation per chunk. At 103 KB that was invisible; at 1.18 MB it drove a cell
    // read from the AI inference service into a multi-minute stall (`read_at` is the ranged
    // backend operation the whole time).
    let Ok(Some((size, is_dir))) = vfs.stat(&path) else {
        let _ = vfs.files.finish_sync_read(caller, file);
        return VfsResponse::Err(VfsError::Io as u8);
    };
    if is_dir {
        let _ = vfs.files.finish_sync_read(caller, file);
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let start = match usize::try_from(offset) {
        Ok(start) => start,
        Err(_) => {
            let _ = vfs.files.finish_sync_read(caller, file);
            return VfsResponse::Err(VfsError::Io as u8);
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
    if !vfs.healthy(&path) {
        let _ = vfs.files.finish_sync_read(caller, file);
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let mut read = match read {
        Ok(read) => read,
        Err(FsError::Unsupported) => 0,
        // A device failure is an error, never a short chunk.
        Err(_) => {
            let _ = vfs.files.finish_sync_read(caller, file);
            return VfsResponse::Err(VfsError::Io as u8);
        }
    };
    if read == 0 && n > 0 {
        // A backend that does not implement ranged reads returns nothing. Falling back to the
        // whole-file copy keeps such a backend correct (at its old cost) instead of silently
        // truncating every chunked read; `BootFsProxy`, `BinOverlay`, `FatBackend`, `CellosFs` and
        // `RamFsBackend` all implement `read_at` today, so this path is for future ones.
        let data = vfs.read_to_vec(&path);
        if !vfs.healthy(&path) {
            let _ = vfs.files.finish_sync_read(caller, file);
            return VfsResponse::Err(VfsError::Io as u8);
        }
        let Ok(data) = data else {
            let _ = vfs.files.finish_sync_read(caller, file);
            return VfsResponse::Err(VfsError::Io as u8);
        };
        if offset < data.len() as u64 {
            let start = offset as usize;
            let take = n.min(data.len() - start);
            resp_buf[..take].copy_from_slice(&data[start..start + take]);
            read = take;
        }
    }
    let _ = vfs.files.finish_sync_read(caller, file);
    VfsResponse::Data(&resp_buf[..read])
}

/// `FileHandleState`: re-authorizes the handle's path like a read, then reports
/// the current size and whether a mutation reached the path since `OpenFileAt`.
/// A path that is no longer a regular file is `VfsError::Io`.
pub fn file_state<'a>(vfs: &VfsManager, caller: Caller, file: ViVfsFileHandle) -> VfsResponse<'a> {
    let Some((path, modified)) = vfs.files.state_of(caller, file) else {
        return VfsResponse::Err(VfsError::Handle as u8);
    };
    if !vfs.access.can_read(caller, path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.healthy(path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    match vfs.stat(path) {
        Ok(Some((size, false))) => VfsResponse::FileState { size, modified },
        _ => VfsResponse::Err(VfsError::Io as u8),
    }
}

pub fn close_file<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    file: ViVfsFileHandle,
) -> VfsResponse<'a> {
    if vfs.files.close(caller, file) {
        VfsResponse::Ok
    } else {
        VfsResponse::Err(VfsError::Handle as u8)
    }
}

fn file_err(err: FileHandleError) -> u8 {
    match err {
        FileHandleError::UnknownHandle => VfsError::Handle as u8,
        FileHandleError::TooManyHandles | FileHandleError::Exhausted => VfsError::Quota as u8,
    }
}
