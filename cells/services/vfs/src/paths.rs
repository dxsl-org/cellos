//! Operations still addressed by an absolute path, and the codes they answer with.
//!
//! Write and delete live here rather than in either dispatch module because both
//! addressing models reach them: a `Write { path }` and a `WriteAt { dir, name }`
//! that resolved to the same path must charge the same cell the same bytes. Two
//! copies of a net-delta calculation are two chances to get that wrong.

use crate::caller::Caller;
use crate::manager::VfsManager;

/// `types::ViError::PermissionDenied` as the wire code.
pub(crate) const ERR_DENIED: u8 = 3;
/// The path does not exist, or the backend refused the operation.
pub(crate) const ERR_IO: u8 = 1;
/// Quota exceeded.
pub(crate) const ERR_QUOTA: u8 = 2;
/// Stale, unknown, or not-this-caller's handle.
pub(crate) const ERR_HANDLE: u8 = 4;

/// Authorize, charge and perform a write to an absolute path.
///
/// Shared by the path-string and handle-addressed write operations so the quota
/// accounting cannot drift between them: two copies of a net-delta calculation
/// is two chances to charge the wrong cell.
///
/// Overwriting charges the delta, not the full new size — otherwise repeated
/// overwrites inflate usage. The delta is only a delta for the cell that was
/// charged for the old contents: if another cell wrote them, this caller gets
/// nothing back and must afford the whole new size.
pub(crate) fn write_file<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    path: &str,
    content: &[u8],
) -> api::ipc::VfsResponse<'a> {
    if crate::access::is_guest_disk_path(path) || !vfs.access.can_write(caller, path) {
        return api::ipc::VfsResponse::Err(ERR_DENIED);
    }
    let _lease = if path == "/srv" || path.starts_with("/srv/") {
        let Ok(k) = crate::namespace::NamespaceKey::parse(path) else {
            return api::ipc::VfsResponse::Err(ERR_DENIED);
        };
        let Ok(l) = vfs.ledger.acquire_transient(&k) else {
            return api::ipc::VfsResponse::Err(ERR_DENIED);
        };
        Some(l)
    } else {
        None
    };
    let old_size = vfs.file_size(path);
    let new_size = content.len() as u64;
    let refunded_to_caller = if vfs.quota.writer_of(path) == Some(caller.cell) {
        old_size
    } else {
        0
    };
    let net_charge = new_size.saturating_sub(refunded_to_caller);
    if net_charge > 0 && !vfs.quota.can_charge(caller.cell, net_charge) {
        return api::ipc::VfsResponse::Err(ERR_QUOTA);
    }
    if vfs.write(path, content) {
        // Release the old contents to whoever was charged for them, then charge
        // the new contents to this caller.
        vfs.quota.release_path(path, old_size);
        let _ = vfs.quota.charge(caller.cell, new_size);
        vfs.quota.set_writer(path, caller.cell);
        api::ipc::VfsResponse::Ok
    } else {
        api::ipc::VfsResponse::Err(ERR_IO)
    }
}

/// Authorize and perform a delete of an absolute path, refunding its bytes.
///
/// Authorizes BEFORE `file_size`: an unauthorized caller must learn nothing, not
/// even whether the file exists or how large it is. Credit goes to the cell that
/// was charged, never the cell that asked — otherwise deleting another cell's
/// file mints quota for the deleter and leaves the writer charged for bytes that
/// are gone.
pub(crate) fn unlink_file<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    path: &str,
) -> api::ipc::VfsResponse<'a> {
    if crate::access::is_guest_disk_path(path) || !vfs.access.can_write(caller, path) {
        return api::ipc::VfsResponse::Err(ERR_DENIED);
    }
    let _res = if path == "/srv" || path.starts_with("/srv/") {
        let Ok(k) = crate::namespace::NamespaceKey::parse(path) else {
            return api::ipc::VfsResponse::Err(ERR_DENIED);
        };
        let Ok(r) = vfs.ledger.reserve_one(&k) else {
            return api::ipc::VfsResponse::Err(ERR_DENIED);
        };
        Some(r)
    } else {
        None
    };
    let file_size = vfs.file_size(path);
    if vfs.unlink(path) {
        vfs.quota.release_path(path, file_size);
        api::ipc::VfsResponse::Ok
    } else {
        api::ipc::VfsResponse::Err(ERR_IO)
    }
}
