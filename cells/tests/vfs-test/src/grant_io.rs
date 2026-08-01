use api::ipc::{VfsRequest, VfsResponse};
use ostd::syscall::{sys_grant_alloc, sys_grant_free, sys_grant_share};
const READ_WRITE_GRANT: u8 = 2;

pub struct GrantRegion {
    id: usize,
    len: usize,
}

impl GrantRegion {
    pub fn alloc(len: usize) -> Result<Self, &'static str> {
        if len == 0 {
            return Err("grant probe file is empty");
        }
        let id = sys_grant_alloc(len).ok_or("GrantAlloc failed")?;
        Ok(Self { id, len })
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn share_rw_with_vfs(&self) -> Result<(), &'static str> {
        if sys_grant_share(self.id, crate::vfs_tid(), READ_WRITE_GRANT) {
            Ok(())
        } else {
            Err("GrantShare to VFS failed")
        }
    }
}

impl Drop for GrantRegion {
    fn drop(&mut self) {
        let _ = sys_grant_free(self.id);
    }
}

pub fn stat_file_len(path: &str) -> Result<usize, &'static str> {
    match crate::vfs_req(&VfsRequest::Stat(path)) {
        VfsResponse::Stat { size, is_dir: false } => {
            if size > usize::MAX as u64 {
                Err("grant probe file does not fit usize")
            } else if size == 0 {
                Err("grant probe file is empty")
            } else {
                Ok(size as usize)
            }
        }
        VfsResponse::Stat { is_dir: true, .. } => Err("grant probe path is a directory"),
        _ => Err("Stat failed for the grant probe path"),
    }
}

pub fn read_file_into_grant(path: &str) -> Result<(GrantRegion, usize), &'static str> {
    let len = stat_file_len(path)?;
    let grant = GrantRegion::alloc(len)?;
    grant.share_rw_with_vfs()?;

    let bytes = match crate::vfs_req(&VfsRequest::ReadFileGrant {
        path,
        grant: grant.id(),
        max: grant.len(),
    }) {
        VfsResponse::GrantDone { bytes } if bytes > 0 => bytes,
        VfsResponse::GrantDone { .. } => return Err("ReadFileGrant copied zero bytes"),
        VfsResponse::Err(3) => return Err("ReadFileGrant was denied before sealing"),
        VfsResponse::Err(_) => return Err("ReadFileGrant returned an error"),
        _ => return Err("ReadFileGrant returned an unexpected response"),
    };

    if bytes != len {
        return Err("ReadFileGrant did not copy the full probe file");
    }

    Ok((grant, bytes))
}
