extern crate alloc;

use alloc::vec::Vec;
use api::dir_handles::ViDirHandle;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::vfs_file_handles::ViVfsFileHandle;
use ostd::service::VfsRef;
use ostd::{ViError, ViResult};

const MAX_READ_CHUNK: u32 = 4000;

pub(super) trait VfsReadOps {
    fn call<'a>(
        &mut self,
        req: &VfsRequest<'_>,
        resp_buf: &'a mut [u8; IPC_BUF_SIZE],
    ) -> ViResult<VfsResponse<'a>>;
}

impl VfsReadOps for VfsRef {
    fn call<'a>(
        &mut self,
        req: &VfsRequest<'_>,
        resp_buf: &'a mut [u8; IPC_BUF_SIZE],
    ) -> ViResult<VfsResponse<'a>> {
        VfsRef::call::<VfsRequest, VfsResponse>(self, req, resp_buf)
    }
}

pub(super) fn read_file(
    ops: &mut impl VfsReadOps,
    path: &str,
    max_bytes: usize,
) -> ViResult<Vec<u8>> {
    let mut parts = path_components(path)?;
    let file_name = parts.pop().ok_or(ViError::InvalidInput)?;
    let mut dirs = Vec::new();
    let mut file = None;
    let result = (|| {
        let root = open_root(ops, "/")?;
        dirs.push(root);
        let mut dir = root;
        for part in parts {
            dir = open_dir(ops, dir, part)?;
            dirs.push(dir);
        }
        let opened = open_file(ops, dir, file_name)?;
        file = Some(opened);
        read_chunks(ops, opened, max_bytes)
    })();
    let close_file_result = close_file(ops, file);
    let close_dirs_result = close_dirs(ops, &mut dirs);
    match (result, close_file_result, close_dirs_result) {
        (Ok(bytes), Ok(()), Ok(())) => Ok(bytes),
        (Err(err), _, _) | (Ok(_), Err(err), _) | (Ok(_), Ok(()), Err(err)) => Err(err),
    }
}

fn path_components(path: &str) -> ViResult<Vec<&str>> {
    api::dir_name::validate_dir_path(path.as_bytes()).map_err(|_| ViError::InvalidInput)?;
    path[1..]
        .split('/')
        .map(|part| api::dir_name::validate_dir_component(part.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ViError::InvalidInput)
}

fn open_root(ops: &mut impl VfsReadOps, path: &str) -> ViResult<ViDirHandle> {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match ops.call(&VfsRequest::OpenRootDir { path }, &mut resp_buf)? {
        VfsResponse::DirHandle(handle) => Ok(handle),
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}

fn open_dir(ops: &mut impl VfsReadOps, dir: ViDirHandle, name: &str) -> ViResult<ViDirHandle> {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match ops.call(&VfsRequest::OpenDir { dir, name }, &mut resp_buf)? {
        VfsResponse::DirHandle(handle) => Ok(handle),
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}

fn open_file(ops: &mut impl VfsReadOps, dir: ViDirHandle, name: &str) -> ViResult<ViVfsFileHandle> {
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match ops.call(&VfsRequest::OpenFileAt { dir, name }, &mut resp_buf)? {
        VfsResponse::FileHandle(handle) => Ok(handle),
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}

fn read_chunks(
    ops: &mut impl VfsReadOps,
    file: ViVfsFileHandle,
    max_bytes: usize,
) -> ViResult<Vec<u8>> {
    let mut bytes = Vec::with_capacity(max_bytes.min(MAX_READ_CHUNK as usize));
    let mut offset = 0u64;
    loop {
        let remaining = max_bytes.saturating_sub(bytes.len());
        let request_max = remaining.min(MAX_READ_CHUNK as usize).max(1) as u32;
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match ops.call(
            &VfsRequest::ReadFileHandle {
                file,
                offset,
                max: request_max,
            },
            &mut resp_buf,
        )? {
            VfsResponse::Data(chunk) => {
                if remaining == 0 {
                    return if chunk.is_empty() {
                        Ok(bytes)
                    } else {
                        Err(ViError::OutOfMemory)
                    };
                }
                if chunk.len() > remaining {
                    return Err(ViError::OutOfMemory);
                }
                bytes.extend_from_slice(chunk);
                if chunk.len() < remaining.min(MAX_READ_CHUNK as usize) {
                    return Ok(bytes);
                }
                offset = offset
                    .checked_add(chunk.len() as u64)
                    .ok_or(ViError::OutOfMemory)?;
            }
            VfsResponse::Err(code) => return Err(vfs_err_from_code(code)),
            _ => return Err(ViError::IO),
        }
    }
}

fn close_file(ops: &mut impl VfsReadOps, file: Option<ViVfsFileHandle>) -> ViResult<()> {
    let Some(file) = file else {
        return Ok(());
    };
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match ops.call(&VfsRequest::CloseFile { file }, &mut resp_buf)? {
        VfsResponse::Ok => Ok(()),
        VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
        _ => Err(ViError::IO),
    }
}

fn close_dirs(ops: &mut impl VfsReadOps, dirs: &mut Vec<ViDirHandle>) -> ViResult<()> {
    let mut first_err = None;
    while let Some(dir) = dirs.pop() {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        let result = match ops.call(&VfsRequest::CloseDir { dir }, &mut resp_buf) {
            Ok(VfsResponse::Ok) => Ok(()),
            Ok(VfsResponse::Err(code)) => Err(vfs_err_from_code(code)),
            Ok(_) => Err(ViError::IO),
            Err(err) => Err(err),
        };
        if first_err.is_none() {
            first_err = result.err();
        }
    }
    first_err.map_or(Ok(()), Err)
}

fn vfs_err_from_code(code: u8) -> ViError {
    match code {
        1 => ViError::IO,
        2 => ViError::OutOfMemory,
        3 => ViError::PermissionDenied,
        4 => ViError::NotFound,
        _ => ViError::Unknown,
    }
}

#[cfg(test)]
#[path = "bindings_vfs_handle_read_tests.rs"]
mod tests;
