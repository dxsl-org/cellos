//! Serving path-string addressed operations.

use wire::ipc::{VfsRequest, VfsResponse};
use wire::proto::vfs::VfsError;

use crate::caller::Caller;
use crate::manager::VfsManager;
use crate::paths::{unlink_file, write_file};

pub(crate) fn handle_path_request(
    vfs: &mut VfsManager,
    caller: Caller,
    req: &VfsRequest<'_>,
) -> Option<VfsResponse<'static>> {
    let resp = match *req {
        VfsRequest::Stat(p) => {
            if !vfs.access.can_read(caller, p) {
                return Some(VfsResponse::Err(VfsError::Denied as u8));
            }
            match vfs.stat(p) {
                Ok(Some((size, is_dir))) => VfsResponse::Stat { size, is_dir },
                Ok(None) | Err(_) => VfsResponse::Err(VfsError::Io as u8),
            }
        }

        VfsRequest::Write { path, content } => write_file(vfs, caller, path, content),

        VfsRequest::Append { path, content } => {
            if crate::access::is_guest_disk_path(path) || !vfs.access.can_write(caller, path) {
                return Some(VfsResponse::Err(VfsError::Denied as u8));
            }
            let _lease = if path == "/srv" || path.starts_with("/srv/") {
                let Ok(k) = crate::namespace::NamespaceKey::parse(path) else {
                    return Some(VfsResponse::Err(VfsError::Denied as u8));
                };
                let Ok(l) = vfs.ledger.acquire_transient(&k) else {
                    return Some(VfsResponse::Err(VfsError::Denied as u8));
                };
                Some(l)
            } else {
                None
            };
            let append_len = content.len() as u64;
            if !vfs
                .quota
                .can_charge(caller.cell, vfs.quota.append_cost(path, append_len))
            {
                return Some(VfsResponse::Err(VfsError::Quota as u8));
            }
            if vfs.append(path, content).is_ok() {
                vfs.quota.commit_append(caller.cell, path, append_len);
                VfsResponse::Ok
            } else {
                VfsResponse::Err(VfsError::Io as u8)
            }
        }

        VfsRequest::Mkdir(p) => crate::dir_ops::mkdir(vfs, caller, p),

        VfsRequest::Rmdir(p) => crate::dir_ops::rmdir(vfs, caller, p),

        VfsRequest::Unlink(p) => unlink_file(vfs, caller, p),

        VfsRequest::RmdirRecursive(p) => {
            // `/srv` Rmdir/Unlink each hold an exclusive namespace reservation
            // on their key; a whole subtree has no such reservation, so `/srv`
            // entries are removed one at a time instead.
            if !vfs.access.can_remove_tree(caller, p)
                || *p == *"/"
                || p == "/srv"
                || p.starts_with("/srv/")
                || vfs.is_mount_point(p)
                || vfs.is_mount_ancestor(p)
            {
                return Some(VfsResponse::Err(VfsError::Denied as u8));
            }
            let files = crate::subtree::files_under(vfs, p, 32);
            if vfs.rmdir_recursive(p).is_ok() {
                for (path, size) in files {
                    vfs.quota.release_path(&path, size);
                }
                vfs.quota.release_tree(p);
                VfsResponse::Ok
            } else {
                VfsResponse::Err(VfsError::Io as u8)
            }
        }

        VfsRequest::Rename { old, new } => crate::dir_ops::rename(vfs, caller, old, new),

        _ => return None,
    };
    Some(resp)
}
