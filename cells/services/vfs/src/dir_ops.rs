//! Directory lifecycle and rename on a resolved absolute path.
//!
//! Shared by the path-string requests (`Mkdir`, `Rmdir`, `Rename`) and their
//! handle-addressed forms (`MkdirAt`, `RmdirAt`, `RenameAt`). The two addressing
//! models differ only in how the path is obtained; keeping one body per
//! operation is what guarantees a sealed cell meets exactly the access,
//! namespace-reservation and quota rules an unsealed one does.

use wire::ipc::VfsResponse;
use wire::proto::vfs::VfsError;

use crate::caller::Caller;
use crate::manager::VfsManager;

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
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    let Ok(_reservation) = reserve_srv(vfs, path) else {
        return VfsResponse::Err(VfsError::Denied as u8);
    };
    // A new directory costs the entry charge; `mkdir -p` over an existing one
    // (FAT) creates nothing and costs nothing.
    let creates = !matches!(vfs.stat(path), Ok(Some(_)));
    if creates && !vfs.quota.can_charge(caller.cell, vfs.quota.entry_charge()) {
        return VfsResponse::Err(VfsError::Quota as u8);
    }
    if vfs.mkdir(path).is_ok() {
        if creates {
            vfs.quota.commit_entry(caller.cell, path);
        }
        VfsResponse::Ok
    } else {
        VfsResponse::Err(VfsError::Io as u8)
    }
}

pub(crate) fn rmdir(vfs: &mut VfsManager, caller: Caller, path: &str) -> VfsResponse<'static> {
    if !vfs.access.can_remove_dir(caller, path)
        || path == "/"
        || vfs.is_mount_point(path)
        || vfs.is_mount_ancestor(path)
    {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    let Ok(_reservation) = reserve_srv(vfs, path) else {
        return VfsResponse::Err(VfsError::Denied as u8);
    };
    if vfs.rmdir(path).is_ok() {
        vfs.quota.release_path(path, 0);
        VfsResponse::Ok
    } else {
        VfsResponse::Err(VfsError::Io as u8)
    }
}

/// Where a rename may happen. Both names under `/srv/`: atomic, never
/// replacing (CellosFS). Both under `/mnt/disk/`: POSIX, replacing (FAT,
/// `backend_fat::rename`), and on one volume — two volumes answer
/// `Unsupported`. Anything else, including one name in each area, is refused.
/// Both names need the caller's mutate right and the access rules' write.
pub(crate) fn rename(
    vfs: &mut VfsManager,
    caller: Caller,
    old: &str,
    new: &str,
) -> VfsResponse<'static> {
    if !caller.may_mutate() {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.access.can_write(caller, old) || !vfs.access.can_write(caller, new) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    let under = |area: &str| old.starts_with(area) && new.starts_with(area);
    if under("/srv/") {
        rename_srv(vfs, old, new)
    } else if under(DISK_AREA) {
        rename_on_disk(vfs, old, new)
    } else {
        VfsResponse::Err(VfsError::Denied as u8)
    }
}

/// Secondary disks' FAT volumes, one mounted at each `/mnt/disk/<device>`.
const DISK_AREA: &str = "/mnt/disk/";

fn rename_srv(vfs: &mut VfsManager, old: &str, new: &str) -> VfsResponse<'static> {
    let Ok(old_key) = crate::namespace::NamespaceKey::parse(old) else {
        return VfsResponse::Err(VfsError::Denied as u8);
    };
    let Ok(new_key) = crate::namespace::NamespaceKey::parse(new) else {
        return VfsResponse::Err(VfsError::Denied as u8);
    };
    if !vfs.healthy(old) || !vfs.healthy(new) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    if old_key == new_key {
        return match vfs.stat(old) {
            Ok(Some((_, false))) => VfsResponse::Ok,
            _ => VfsResponse::Err(VfsError::Io as u8),
        };
    }
    let Ok(reservation) = vfs.ledger.reserve_two(&old_key, &new_key) else {
        return VfsResponse::Err(VfsError::Denied as u8);
    };
    let ok = vfs.rename_no_replace(old, new).is_ok();
    drop(reservation);
    if !ok {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    vfs.quota.move_tree(old, new);
    VfsResponse::Ok
}

/// The volume roots themselves never move. A replaced entry's charge goes back
/// to whoever was charged for it; the moved records follow the new name.
fn rename_on_disk(vfs: &mut VfsManager, old: &str, new: &str) -> VfsResponse<'static> {
    if [old, new]
        .iter()
        .any(|path| vfs.is_mount_point(path) || vfs.is_mount_ancestor(path))
    {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.healthy(old) || !vfs.healthy(new) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let replaced = if crate::paths::may_alias(old, new) {
        None
    } else {
        match vfs.stat(new) {
            Ok(Some((size, is_dir))) => Some(if is_dir { 0 } else { size }),
            _ => None,
        }
    };
    if let Err(error) = vfs.rename_replace(old, new) {
        return VfsResponse::Err(error.wire());
    }
    if let Some(size) = replaced {
        vfs.quota.release_path(new, size);
    }
    vfs.quota.move_tree(old, new);
    VfsResponse::Ok
}
