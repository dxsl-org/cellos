//! Directory lifecycle and rename on a resolved absolute path.
//!
//! Shared by the path-string requests (`Mkdir`, `Rmdir`, `Rename`) and their
//! handle-addressed forms (`MkdirAt`, `RmdirAt`, `RenameAt`). The two addressing
//! models differ only in how the path is obtained; keeping one body per
//! operation is what guarantees a sealed cell meets exactly the access,
//! namespace-reservation and quota rules an unsealed one does.

use api::ipc::VfsResponse;

use crate::caller::Caller;
use crate::manager::VfsManager;
use crate::paths::{ERR_DENIED, ERR_IO};

/// `/srv` entries carry an exclusive namespace reservation for the duration of
/// the mutation; other mounts have no ledger. `Err` means the key could not be
/// parsed or is already reserved, which the caller reports as a refusal.
fn reserve_srv(
    vfs: &VfsManager,
    path: &str,
) -> Result<Option<crate::namespace::ExclusiveReservation>, ()> {
    if path != "/srv" && !path.starts_with("/srv/") {
        return Ok(None);
    }
    let key = crate::namespace::NamespaceKey::parse(path).map_err(|_| ())?;
    vfs.ledger.reserve_one(&key).map(Some).map_err(|_| ())
}

pub(crate) fn mkdir(vfs: &mut VfsManager, caller: Caller, path: &str) -> VfsResponse<'static> {
    if !vfs.access.can_write(caller, path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    let Ok(_reservation) = reserve_srv(vfs, path) else {
        return VfsResponse::Err(ERR_DENIED);
    };
    if vfs.mkdir(path) {
        VfsResponse::Ok
    } else {
        VfsResponse::Err(ERR_IO)
    }
}

pub(crate) fn rmdir(vfs: &mut VfsManager, caller: Caller, path: &str) -> VfsResponse<'static> {
    if !vfs.access.can_remove_dir(caller, path)
        || path == "/"
        || vfs.is_mount_point(path)
        || vfs.is_mount_ancestor(path)
    {
        return VfsResponse::Err(ERR_DENIED);
    }
    let Ok(_reservation) = reserve_srv(vfs, path) else {
        return VfsResponse::Err(ERR_DENIED);
    };
    if vfs.rmdir(path) {
        VfsResponse::Ok
    } else {
        VfsResponse::Err(ERR_IO)
    }
}

pub(crate) fn rename(
    vfs: &mut VfsManager,
    caller: Caller,
    old: &str,
    new: &str,
) -> VfsResponse<'static> {
    if !caller.may_mutate() {
        return VfsResponse::Err(ERR_DENIED);
    }
    if !old.starts_with("/srv/") || !new.starts_with("/srv/") {
        return VfsResponse::Err(ERR_DENIED);
    }
    if !vfs.access.can_write(caller, old) || !vfs.access.can_write(caller, new) {
        return VfsResponse::Err(ERR_DENIED);
    }
    let Ok(old_key) = crate::namespace::NamespaceKey::parse(old) else {
        return VfsResponse::Err(ERR_DENIED);
    };
    let Ok(new_key) = crate::namespace::NamespaceKey::parse(new) else {
        return VfsResponse::Err(ERR_DENIED);
    };
    if !vfs.healthy(old) || !vfs.healthy(new) {
        return VfsResponse::Err(ERR_IO);
    }
    if old_key == new_key {
        return match vfs.stat(old) {
            Some((_, false)) => VfsResponse::Ok,
            _ => VfsResponse::Err(ERR_IO),
        };
    }
    let Ok(reservation) = vfs.ledger.reserve_two(&old_key, &new_key) else {
        return VfsResponse::Err(ERR_DENIED);
    };
    let ok = vfs.rename_no_replace(old, new);
    drop(reservation);
    if !ok {
        return VfsResponse::Err(ERR_IO);
    }
    if let Some(w) = vfs.quota.writer_of(old) {
        vfs.quota.release_path(old, 0);
        vfs.quota.set_writer(new, w);
    }
    VfsResponse::Ok
}
