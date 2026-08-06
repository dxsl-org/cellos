use api::ipc::{VfsRequest, VfsResponse};
use ostd::syscall::{sys_grant_alloc, sys_grant_copy_to_slice, sys_grant_free, sys_grant_share};
const READ_WRITE_GRANT: u8 = 2;
const UNKNOWN_CAP_ID: u64 = 0;

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
        VfsResponse::Stat {
            size,
            is_dir: false,
        } => {
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

pub fn read_file_into_short_grant(
    path: &str,
    grant_len: usize,
) -> Result<(GrantRegion, usize), &'static str> {
    let file_len = stat_file_len(path)?;
    if grant_len == 0 || grant_len >= file_len {
        return Err("short grant length must be nonzero and smaller than file");
    }
    let grant = GrantRegion::alloc(grant_len)?;
    grant.share_rw_with_vfs()?;

    match crate::vfs_req(&VfsRequest::ReadFileGrant {
        path,
        grant: grant.id(),
        max: file_len,
    }) {
        VfsResponse::GrantDone { bytes } if bytes == grant_len => Ok((grant, bytes)),
        VfsResponse::GrantDone { .. } => Err("ReadFileGrant ignored short grant length"),
        VfsResponse::Err(_) => Err("ReadFileGrant short grant returned an error"),
        _ => Err("ReadFileGrant short grant returned an unexpected response"),
    }
}

pub fn grant_prefix_equals(grant: &GrantRegion, expected: &[u8]) -> bool {
    let mut bytes = [0u8; 32];
    if expected.len() > bytes.len() {
        return false;
    }
    match sys_grant_copy_to_slice(grant.id(), &mut bytes[..expected.len()]) {
        Some(n) if n >= expected.len() => bytes[..expected.len()] == *expected,
        _ => false,
    }
}

pub fn unknown_cap_read_grant_returns_zero(offset: u64, size: usize) -> Result<(), &'static str> {
    let grant = GrantRegion::alloc(16)?;
    grant.share_rw_with_vfs()?;

    match crate::vfs_req(&VfsRequest::ReadGrant {
        cap: UNKNOWN_CAP_ID,
        offset,
        size,
        grant: grant.id(),
    }) {
        VfsResponse::GrantDone { bytes: 0 } => Ok(()),
        VfsResponse::GrantDone { .. } => Err("ReadGrant returned nonzero bytes for unknown cap"),
        VfsResponse::Err(_) => Err("ReadGrant returned an error"),
        _ => Err("ReadGrant returned an unexpected response"),
    }
}
