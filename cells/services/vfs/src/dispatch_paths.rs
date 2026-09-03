//! Serving path-string addressed operations.

use api::ipc::{VfsRequest, VfsResponse};

use crate::caller::Caller;
use crate::manager::VfsManager;
use crate::paths::{unlink_file, write_file, ERR_DENIED, ERR_IO, ERR_QUOTA};

pub(crate) fn handle_path_request(
    vfs: &mut VfsManager,
    caller: Caller,
    req: &VfsRequest<'_>,
) -> Option<VfsResponse<'static>> {
    let resp = match *req {
        VfsRequest::GetFile(p) => {
            if !vfs.access.can_read(caller, p) {
                return Some(VfsResponse::Err(ERR_DENIED));
            }
            if let Some((ptr, len)) = vfs.get_file_ptr(p) {
                VfsResponse::DataPtr {
                    ptr: ptr as u64,
                    len: len as u64,
                }
            } else {
                VfsResponse::Err(ERR_IO)
            }
        }

        VfsRequest::Stat(p) => {
            if !vfs.access.can_read(caller, p) {
                return Some(VfsResponse::Err(ERR_DENIED));
            }
            match vfs.stat(p) {
                Some((size, is_dir)) => VfsResponse::Stat { size, is_dir },
                None => VfsResponse::Err(ERR_IO),
            }
        }

        VfsRequest::Write { path, content } => write_file(vfs, caller, path, content),

        VfsRequest::Append { path, content } => {
            if crate::access::is_guest_disk_path(path) || !vfs.access.can_write(caller, path) {
                return Some(VfsResponse::Err(ERR_DENIED));
            }
            let append_len = content.len() as u64;
            if !vfs.quota.can_charge(caller.cell, append_len) {
                return Some(VfsResponse::Err(ERR_QUOTA));
            }
            if vfs.append(path, content) {
                let _ = vfs.quota.charge(caller.cell, append_len);
                vfs.quota.record_writer(path, caller.cell);
                VfsResponse::Ok
            } else {
                VfsResponse::Err(ERR_IO)
            }
        }

        VfsRequest::Mkdir(p) => {
            if !vfs.access.can_write(caller, p) {
                VfsResponse::Err(ERR_DENIED)
            } else if vfs.mkdir(p) {
                VfsResponse::Ok
            } else {
                VfsResponse::Err(ERR_IO)
            }
        }

        VfsRequest::Rmdir(p) => {
            if !vfs.access.can_remove_dir(caller, p) {
                return Some(VfsResponse::Err(ERR_DENIED));
            }
            if vfs.rmdir(p) {
                VfsResponse::Ok
            } else {
                VfsResponse::Err(ERR_IO)
            }
        }

        VfsRequest::Unlink(p) => unlink_file(vfs, caller, p),

        VfsRequest::RmdirRecursive(p) => {
            if !vfs.access.can_remove_tree(caller, p) {
                return Some(VfsResponse::Err(ERR_DENIED));
            }
            let files = crate::subtree::files_under(vfs, p, 32);
            if vfs.rmdir_recursive(p) {
                for (path, size) in files {
                    vfs.quota.release_path(&path, size);
                }
                VfsResponse::Ok
            } else {
                VfsResponse::Err(ERR_IO)
            }
        }

        _ => return None,
    };
    Some(resp)
}
