//! Opens the persistent guest block image through the VFS capability API.

pub type PersistentDisk = (usize, api::vfs_file_handles::ViVfsFileHandle, u64);

/// Which file under `/mnt/sd` backs the guest's only disk, and whether the
/// guest may write it.
#[derive(Clone, Copy)]
pub struct DiskSpec {
    pub name: &'static str,
    pub read_only: bool,
}

/// The writable policy-owned disk of the console and Ubuntu profiles.
#[cfg_attr(feature = "volatile-disk", allow(dead_code))]
pub const GUEST_DISK: DiskSpec = DiskSpec {
    name: "guest_disk.img",
    read_only: false,
};

// VFS cold-start can remain in nested block-driver work for several seconds on
// shared TCG runners. The complete admission-plus-reply exchange stays finite.
const VFS_CALL_TIMEOUT_TICKS: u64 = 1_000;

pub fn open(spec: DiskSpec) -> Result<Option<PersistentDisk>, ()> {
    let Some(vfs_tid) = ostd::syscall::sys_lookup_service(api::syscall::service::VFS) else {
        return Ok(None);
    };
    let mut poisoned = false;
    open_for(vfs_tid, spec, &mut poisoned).map(Some).ok_or(())
}

fn close_partial(vfs_tid: usize, root: api::dir_handles::ViDirHandle, poisoned: &mut bool) {
    let request = api::ipc::VfsRequest::CloseDir { dir: root };
    let mut send_buffer = [0u8; api::ipc::IPC_BUF_SIZE];
    let mut response_buffer = [0u8; api::ipc::IPC_BUF_SIZE];
    let result: Result<api::ipc::VfsResponse<'_>, _> = ostd::ipc::service_call_typed_bounded(
        vfs_tid,
        &request,
        &mut send_buffer,
        &mut response_buffer,
        VFS_CALL_TIMEOUT_TICKS,
    );
    *poisoned |= matches!(
        result,
        Err(ostd::ipc::IpcError::Recv
            | ostd::ipc::IpcError::PeerGone
            | ostd::ipc::IpcError::Indeterminate)
    );
}

/// Open the policy-owned guest disk through one resolved VFS generation.
///
/// Requests admit until an end-to-end deadline while VFS waits on a nested
/// block-driver reply. A partial open returns `None`; callers retain persistent
/// mode and reconnect through service discovery after an uncertain receive.
pub fn open_for(vfs_tid: usize, spec: DiskSpec, poisoned: &mut bool) -> Option<PersistentDisk> {
    let mut response = [0u8; api::ipc::IPC_BUF_SIZE];
    let mut request = [0u8; api::ipc::IPC_BUF_SIZE];
    let mut cleanup_root = None;
    macro_rules! call {
        ($step:literal, $request:expr) => {{
            response.fill(0);
            match ostd::ipc::service_call_typed_bounded(
                vfs_tid,
                $request,
                &mut request,
                &mut response,
                VFS_CALL_TIMEOUT_TICKS,
            ) {
                Ok(value) => value,
                Err(error) => {
                    ostd::io::println(&alloc::format!(
                        "[hv-disk] VFS {} failed: {:?}",
                        $step,
                        error
                    ));
                    let uncertain = matches!(
                        error,
                        ostd::ipc::IpcError::Recv
                            | ostd::ipc::IpcError::PeerGone
                            | ostd::ipc::IpcError::Indeterminate
                    );
                    *poisoned |= uncertain;
                    if !uncertain {
                        if let Some(root) = cleanup_root {
                            close_partial(vfs_tid, root, poisoned);
                        }
                    }
                    return None;
                }
            }
        }};
    }
    let root_request = api::ipc::VfsRequest::OpenRootDir { path: "/" };
    let root = match call!("open-root", &root_request) {
        api::ipc::VfsResponse::DirHandle(root) => root,
        other => {
            ostd::io::println(&alloc::format!(
                "[hv-disk] open-root returned unexpected response: {:?}",
                other
            ));
            return None;
        }
    };
    cleanup_root = Some(root);
    let mnt_request = api::ipc::VfsRequest::OpenDir {
        dir: root,
        name: "mnt",
    };
    let mnt = match call!("open-mnt", &mnt_request) {
        api::ipc::VfsResponse::DirHandle(mnt) => mnt,
        other => {
            ostd::io::println(&alloc::format!(
                "[hv-disk] open-mnt returned unexpected response: {:?}",
                other
            ));
            close_partial(vfs_tid, root, poisoned);
            return None;
        }
    };
    let sd_request = api::ipc::VfsRequest::OpenDir {
        dir: mnt,
        name: "sd",
    };
    let sd = match call!("open-sd", &sd_request) {
        api::ipc::VfsResponse::DirHandle(sd) => sd,
        other => {
            ostd::io::println(&alloc::format!(
                "[hv-disk] open-sd returned unexpected response: {:?}",
                other
            ));
            close_partial(vfs_tid, root, poisoned);
            return None;
        }
    };
    let file_request = api::ipc::VfsRequest::OpenFileAt {
        dir: sd,
        name: spec.name,
    };
    let file = match call!("open-file", &file_request) {
        api::ipc::VfsResponse::FileHandle(file) => file,
        other => {
            ostd::io::println(&alloc::format!(
                "[hv-disk] open-file returned unexpected response: {:?}",
                other
            ));
            close_partial(vfs_tid, root, poisoned);
            return None;
        }
    };
    let path = alloc::format!("/mnt/sd/{}", spec.name);
    let stat = api::ipc::VfsRequest::Stat(&path);
    match call!("stat", &stat) {
        api::ipc::VfsResponse::Stat {
            size,
            is_dir: false,
        } => Some((vfs_tid, file, size)),
        other => {
            ostd::io::println(&alloc::format!(
                "[hv-disk] stat returned unexpected response: {:?}",
                other
            ));
            close_partial(vfs_tid, root, poisoned);
            None
        }
    }
}
