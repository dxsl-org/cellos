//! Serving the handle-addressed operations.
//!
//! Each arm resolves a caller-held directory plus one component into an absolute
//! path and then reuses the same backend and quota code the path-string
//! operations use. The difference is upstream of all of it: the caller could not
//! have named a path outside its handle, so there is no path decision left to
//! make here.
//!
//! The access table still runs on the resolved path. It is defence in depth now
//! rather than the primary control — the handle already decided *which*
//! directory — but it remains the statement of what the system permits anywhere,
//! and a handle to a read-only mount must not become a write capability just
//! because a handle was issued for it.
//!
//! Settling what a cell inherited, before any of this runs, lives in
//! [`crate::dir_admission`].

use alloc::string::String;
use api::dir_handles::ViDirHandle;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};

use crate::caller::Caller;
use crate::dirs::DirError;
use crate::dispatch_file_handles::{close_file, open_file, read_file};
use crate::manager::VfsManager;
use crate::paths::{unlink_file, write_file, ERR_DENIED, ERR_HANDLE, ERR_IO, ERR_QUOTA};

/// Largest payload an inline reply carries, leaving room for the postcard
/// envelope inside the IPC frame. A caller that needs more compares against
/// `StatAt` and uses the grant path.
pub(crate) const MAX_INLINE_PAYLOAD: usize = IPC_BUF_SIZE - 96;

/// Serve one handle-addressed request.
pub fn handle<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    req: &VfsRequest<'_>,
    resp_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> VfsResponse<'a> {
    match *req {
        VfsRequest::OpenRootDir { path } => open_root(vfs, caller, path),
        VfsRequest::OpenDir { dir, name } => open_dir(vfs, caller, dir, name),
        VfsRequest::ReadAt { dir, name } => read_at(vfs, caller, dir, name, resp_buf),
        VfsRequest::OpenFileAt { dir, name } => open_file(vfs, caller, dir, name),
        VfsRequest::ReadFileHandle { file, offset, max } => {
            read_file(vfs, caller, file, offset, max, resp_buf)
        }
        VfsRequest::CloseFile { file } => close_file(vfs, caller, file),
        VfsRequest::WriteAt { dir, name, content } => match resolve(vfs, caller, dir, name) {
            Ok(path) => write_file(vfs, caller, &path, content),
            Err(code) => VfsResponse::Err(code),
        },
        VfsRequest::StatAt { dir, name } => match resolve(vfs, caller, dir, name) {
            Ok(path) => stat_path(vfs, caller, &path),
            Err(code) => VfsResponse::Err(code),
        },
        VfsRequest::ListAt { dir } => list_at(vfs, caller, dir, resp_buf),
        VfsRequest::UnlinkAt { dir, name } => match resolve(vfs, caller, dir, name) {
            Ok(path) => unlink_file(vfs, caller, &path),
            Err(code) => VfsResponse::Err(code),
        },
        VfsRequest::CloseDir { dir } => match vfs.dirs.revoke(caller, dir) {
            Ok(outcome) => {
                let _ = vfs.files.revoke_by_parent_dirs(&outcome.revoked_ids);
                VfsResponse::Ok
            }
            Err(e) => VfsResponse::Err(dir_err(e)),
        },
        VfsRequest::SealPaths => {
            if vfs.dirs.seal(caller) {
                VfsResponse::Ok
            } else {
                VfsResponse::Err(ERR_DENIED)
            }
        }
        // `handle` is only reached from the directory arms of the dispatch
        // match; anything else is a routing mistake, not a caller error.
        _ => VfsResponse::Err(0xFF),
    }
}

/// Map a refusal from the handle table onto the wire.
fn dir_err(err: DirError) -> u8 {
    match err {
        // Unknown and not-yours are the same answer, so sweeping handle values
        // reveals nothing about what other cells hold.
        DirError::UnknownHandle => ERR_HANDLE,
        // The caller tried to express something a handle cannot express.
        DirError::BadName(_) => ERR_DENIED,
        DirError::TooManyHandles => ERR_QUOTA,
    }
}

pub(crate) fn resolve(
    vfs: &VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    name: &str,
) -> Result<String, u8> {
    vfs.dirs.resolve(caller, dir, name).map_err(dir_err)
}

fn open_root<'a>(vfs: &mut VfsManager, caller: Caller, path: &str) -> VfsResponse<'a> {
    // Checked as raw bytes, before anything is joined or cleaned up: this is the
    // one operation left that takes a path, so it is the one place a traversal
    // could still enter the handle graph.
    let Ok(checked) = api::dir_name::validate_dir_path(path.as_bytes()) else {
        return VfsResponse::Err(ERR_DENIED);
    };
    if !vfs.access.can_read(caller, checked) {
        return VfsResponse::Err(ERR_DENIED);
    }
    // A handle to something that is not a directory would resolve names against
    // a file, which no backend defines.
    match vfs.stat(checked) {
        Some((_, true)) => {}
        _ => return VfsResponse::Err(ERR_IO),
    }
    match vfs.dirs.open_root(caller, checked) {
        Ok(handle) => VfsResponse::DirHandle(handle),
        Err(e) => VfsResponse::Err(dir_err(e)),
    }
}

fn open_dir<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    name: &str,
) -> VfsResponse<'a> {
    let path = match resolve(vfs, caller, dir, name) {
        Ok(p) => p,
        Err(code) => return VfsResponse::Err(code),
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    match vfs.stat(&path) {
        Some((_, true)) => {}
        _ if vfs.is_mount_ancestor(&path) => {}
        _ => return VfsResponse::Err(ERR_IO),
    }
    match vfs.dirs.insert(caller, path, Some(dir.0)) {
        Ok(handle) => VfsResponse::DirHandle(handle),
        Err(e) => VfsResponse::Err(dir_err(e)),
    }
}

fn read_at<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    name: &str,
    resp_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> VfsResponse<'a> {
    let path = match resolve(vfs, caller, dir, name) {
        Ok(p) => p,
        Err(code) => return VfsResponse::Err(code),
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    // Absent and empty must not look alike: an empty reply for a missing file
    // would make a caller act on data that does not exist.
    if vfs.stat(&path).is_none() {
        return VfsResponse::Err(ERR_IO);
    }
    let data = vfs.read_to_vec(&path);
    let n = data.len().min(MAX_INLINE_PAYLOAD);
    resp_buf[..n].copy_from_slice(&data[..n]);
    VfsResponse::Data(&resp_buf[..n])
}

fn stat_path<'a>(vfs: &VfsManager, caller: Caller, path: &str) -> VfsResponse<'a> {
    if !vfs.access.can_read(caller, path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    match vfs.stat(path) {
        Some((size, is_dir)) => VfsResponse::Stat { size, is_dir },
        None => VfsResponse::Err(ERR_IO),
    }
}

fn list_at<'a>(
    vfs: &VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    resp_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> VfsResponse<'a> {
    let Some(path) = vfs.dirs.dir_path(caller, dir).map(String::from) else {
        return VfsResponse::Err(ERR_HANDLE);
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    let n = vfs.list_dir(&path, &mut resp_buf[..MAX_INLINE_PAYLOAD]);
    VfsResponse::Data(&resp_buf[..n])
}
