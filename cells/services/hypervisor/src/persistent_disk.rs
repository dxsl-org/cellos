//! Opens the persistent guest block image through the VFS capability API.

pub type PersistentDisk = (usize, api::vfs_file_handles::ViVfsFileHandle, u64);

// VFS cold-start can remain in nested block-driver work for several seconds on
// shared TCG runners. The complete admission-plus-reply exchange stays finite.
const VFS_CALL_TIMEOUT_TICKS: u64 = 1_000;

#[cfg(not(feature = "volatile-disk"))]
pub fn open() -> Result<Option<PersistentDisk>, ()> {
    let Some(vfs_tid) = ostd::syscall::sys_lookup_service(api::syscall::service::VFS) else {
        return Ok(None);
    };
    let mut poisoned = false;
    open_for(vfs_tid, &mut poisoned).map(Some).ok_or(())
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
    *poisoned |= matches!(result, Err(ostd::ipc::IpcError::Recv));
}

/// Open the policy-owned guest disk through one resolved VFS generation.
///
/// Requests admit until an end-to-end deadline while VFS waits on a nested
/// block-driver reply. A partial open returns `None`; callers retain persistent
/// mode and reconnect through service discovery after an uncertain receive.
pub fn open_for(vfs_tid: usize, poisoned: &mut bool) -> Option<PersistentDisk> {
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
                    let uncertain = matches!(error, ostd::ipc::IpcError::Recv);
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
    let api::ipc::VfsResponse::DirHandle(root) = call!("open-root", &root_request) else {
        ostd::io::println("[hv-disk] open-root returned unexpected response");
        return None;
    };
    cleanup_root = Some(root);
    let mnt_request = api::ipc::VfsRequest::OpenDir {
        dir: root,
        name: "mnt",
    };
    let api::ipc::VfsResponse::DirHandle(mnt) = call!("open-mnt", &mnt_request) else {
        ostd::io::println("[hv-disk] open-mnt returned unexpected response");
        close_partial(vfs_tid, root, poisoned);
        return None;
    };
    let sd_request = api::ipc::VfsRequest::OpenDir {
        dir: mnt,
        name: "sd",
    };
    let api::ipc::VfsResponse::DirHandle(sd) = call!("open-sd", &sd_request) else {
        ostd::io::println("[hv-disk] open-sd returned unexpected response");
        close_partial(vfs_tid, root, poisoned);
        return None;
    };
    let file_request = api::ipc::VfsRequest::OpenFileAt {
        dir: sd,
        name: "guest_disk.img",
    };
    let api::ipc::VfsResponse::FileHandle(file) = call!("open-file", &file_request) else {
        ostd::io::println("[hv-disk] open-file returned unexpected response");
        close_partial(vfs_tid, root, poisoned);
        return None;
    };
    let stat = api::ipc::VfsRequest::Stat("/mnt/sd/guest_disk.img");
    match call!("stat", &stat) {
        api::ipc::VfsResponse::Stat {
            size,
            is_dir: false,
        } => Some((vfs_tid, file, size)),
        _ => {
            ostd::io::println("[hv-disk] stat returned unexpected response");
            close_partial(vfs_tid, root, poisoned);
            None
        }
    }
}
