//! Opens the persistent guest block image through the VFS capability API.

pub type PersistentDisk = (usize, api::vfs_file_handles::ViVfsFileHandle, u64);

pub fn open() -> Option<PersistentDisk> {
    let vfs_tid = ostd::syscall::sys_lookup_service(api::syscall::service::VFS)?;
    let mut response = [0u8; api::ipc::IPC_BUF_SIZE];
    let mut request = [0u8; api::ipc::IPC_BUF_SIZE];
    let root_request = api::ipc::VfsRequest::OpenRootDir { path: "/" };
    let api::ipc::VfsResponse::DirHandle(root) =
        ostd::ipc::service_call_typed(vfs_tid, &root_request, &mut request, &mut response).ok()?
    else {
        return None;
    };
    let mnt_request = api::ipc::VfsRequest::OpenDir {
        dir: root,
        name: "mnt",
    };
    let api::ipc::VfsResponse::DirHandle(mnt) =
        ostd::ipc::service_call_typed(vfs_tid, &mnt_request, &mut request, &mut response).ok()?
    else {
        return None;
    };
    let sd_request = api::ipc::VfsRequest::OpenDir {
        dir: mnt,
        name: "sd",
    };
    let api::ipc::VfsResponse::DirHandle(sd) =
        ostd::ipc::service_call_typed(vfs_tid, &sd_request, &mut request, &mut response).ok()?
    else {
        return None;
    };
    let file_request = api::ipc::VfsRequest::OpenFileAt {
        dir: sd,
        name: "guest_disk.img",
    };
    let api::ipc::VfsResponse::FileHandle(file) =
        ostd::ipc::service_call_typed(vfs_tid, &file_request, &mut request, &mut response).ok()?
    else {
        return None;
    };
    let stat = api::ipc::VfsRequest::Stat("/mnt/sd/guest_disk.img");
    match ostd::ipc::service_call_typed(vfs_tid, &stat, &mut request, &mut response).ok()? {
        api::ipc::VfsResponse::Stat {
            size,
            is_dir: false,
        } => Some((vfs_tid, file, size)),
        _ => None,
    }
}
