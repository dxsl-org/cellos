use super::path::FileReadPlan;
use super::wire::{vfs_err_from_code, MAX_READ_CHUNK};
use crate::service::VfsRef;
use crate::{ViError, ViResult};
use alloc::vec::Vec;
use api::dir_handles::ViDirHandle;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::vfs_file_handles::ViVfsFileHandle;

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

pub(super) struct ReadSession<'a, T> {
    ops: &'a mut T,
    dirs: Vec<ViDirHandle>,
    file: Option<ViVfsFileHandle>,
}

impl<'a, T: VfsReadOps> ReadSession<'a, T> {
    pub(super) fn new(ops: &'a mut T) -> Self {
        Self {
            ops,
            dirs: Vec::new(),
            file: None,
        }
    }

    pub(super) fn read(&mut self, plan: &FileReadPlan<'_>, max_bytes: usize) -> ViResult<Vec<u8>> {
        let root = self.open_root("/")?;
        let dir = plan
            .parents
            .iter()
            .try_fold(root, |current, name| self.open_dir(current, name))?;
        self.file = Some(self.open_file(dir, plan.file_name)?);
        self.read_chunks(max_bytes)
    }

    pub(super) fn cleanup(&mut self) -> ViResult<()> {
        let file_result = self.close_file();
        let dir_result = self.close_dirs();
        file_result.and(dir_result)
    }

    fn open_root(&mut self, path: &str) -> ViResult<ViDirHandle> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .ops
            .call(&VfsRequest::OpenRootDir { path }, &mut resp_buf)?
        {
            VfsResponse::DirHandle(handle) => {
                self.dirs.push(handle);
                Ok(handle)
            }
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    fn open_dir(&mut self, dir: ViDirHandle, name: &str) -> ViResult<ViDirHandle> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .ops
            .call(&VfsRequest::OpenDir { dir, name }, &mut resp_buf)?
        {
            VfsResponse::DirHandle(handle) => {
                self.dirs.push(handle);
                Ok(handle)
            }
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    fn open_file(&mut self, dir: ViDirHandle, name: &str) -> ViResult<ViVfsFileHandle> {
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .ops
            .call(&VfsRequest::OpenFileAt { dir, name }, &mut resp_buf)?
        {
            VfsResponse::FileHandle(handle) => Ok(handle),
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    fn read_chunks(&mut self, max_bytes: usize) -> ViResult<Vec<u8>> {
        let file = self.file.ok_or(ViError::IO)?;
        let mut bytes = Vec::with_capacity(max_bytes.min(MAX_READ_CHUNK as usize));
        let mut offset = 0u64;
        loop {
            let remaining = max_bytes.saturating_sub(bytes.len());
            let request_max = remaining.min(MAX_READ_CHUNK as usize).max(1) as u32;
            let mut resp_buf = [0u8; IPC_BUF_SIZE];
            match self.ops.call(
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

    fn close_file(&mut self) -> ViResult<()> {
        let Some(file) = self.file.take() else {
            return Ok(());
        };
        let mut resp_buf = [0u8; IPC_BUF_SIZE];
        match self
            .ops
            .call(&VfsRequest::CloseFile { file }, &mut resp_buf)?
        {
            VfsResponse::Ok => Ok(()),
            VfsResponse::Err(code) => Err(vfs_err_from_code(code)),
            _ => Err(ViError::IO),
        }
    }

    fn close_dirs(&mut self) -> ViResult<()> {
        let mut first_err = None;
        while let Some(dir) = self.dirs.pop() {
            let mut resp_buf = [0u8; IPC_BUF_SIZE];
            let close = match self.ops.call(&VfsRequest::CloseDir { dir }, &mut resp_buf) {
                Ok(VfsResponse::Ok) => Ok(()),
                Ok(VfsResponse::Err(code)) => Err(vfs_err_from_code(code)),
                Ok(_) => Err(ViError::IO),
                Err(err) => Err(err),
            };
            if first_err.is_none() {
                first_err = close.err();
            }
        }
        first_err.map_or(Ok(()), Err)
    }
}
