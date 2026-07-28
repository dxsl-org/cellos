extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::ipc::{VfsRequest, VfsResponse};
use ostd::syscall::{self, SyscallResult};

pub(super) struct DirEntry {
    pub is_dir: bool,
    pub name: String,
}

pub(super) fn stat_path(path: &str) -> Result<Option<bool>, String> {
    let mut send = [0u8; api::ipc::IPC_BUF_SIZE];
    let len = api::ipc::encode(&VfsRequest::Stat(path), &mut send)
        .map_err(|_| String::from("failed to encode VFS stat request"))?
        .len();
    let mut recv = [0u8; api::ipc::IPC_BUF_SIZE];
    let raw = request_vfs(len, &send, &mut recv)?;
    Ok(
        match api::ipc::decode::<VfsResponse>(raw)
            .map_err(|_| String::from("failed to decode VFS stat response"))?
        {
            VfsResponse::Stat { is_dir, .. } => Some(is_dir),
            VfsResponse::Err(_) => None,
            _ => return Err(format!("unexpected VFS stat response for '{path}'")),
        },
    )
}

pub(super) fn list_dir(path: &str) -> Result<Vec<DirEntry>, String> {
    let mut send = [0u8; api::ipc::IPC_BUF_SIZE];
    let len = api::ipc::encode(&VfsRequest::ListDir(path), &mut send)
        .map_err(|_| String::from("failed to encode VFS list request"))?
        .len();
    let mut recv = [0u8; api::ipc::IPC_BUF_SIZE];
    let raw = request_vfs(len, &send, &mut recv)?;
    let data = match api::ipc::decode::<VfsResponse>(raw)
        .map_err(|_| String::from("failed to decode VFS list response"))?
    {
        VfsResponse::Data(data) => data,
        VfsResponse::Err(_) => return Err(format!("cannot list '{path}'")),
        _ => return Err(format!("unexpected VFS list response for '{path}'")),
    };
    if !data.is_empty() && data[data.len() - 1] != b'\n' {
        return Err(format!("truncated directory listing for '{path}'"));
    }
    let text = core::str::from_utf8(data)
        .map_err(|_| format!("invalid directory listing for '{path}'"))?;
    let mut entries = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("d:") {
            entries.push(DirEntry {
                is_dir: true,
                name: String::from(name),
            });
        } else if let Some(name) = line.strip_prefix("f:") {
            entries.push(DirEntry {
                is_dir: false,
                name: String::from(name),
            });
        } else {
            return Err(format!("malformed directory entry in '{path}'"));
        }
    }
    Ok(entries)
}

fn request_vfs<'a>(len: usize, send: &[u8], recv: &'a mut [u8]) -> Result<&'a [u8], String> {
    let vfs = loop {
        if let Some(tid) = syscall::sys_lookup_service(api::syscall::service::VFS) {
            break tid;
        }
        ostd::task::yield_now();
    };
    syscall::sys_send(vfs, &send[..len]);
    match syscall::sys_recv(vfs, recv) {
        SyscallResult::Ok(_) => Ok(recv),
        _ => Err(String::from("VFS request failed")),
    }
}

pub(super) fn join_path(dir: &str, name: &str) -> String {
    let mut full = String::from(dir);
    if !full.ends_with('/') {
        full.push('/');
    }
    full.push_str(name);
    full
}
