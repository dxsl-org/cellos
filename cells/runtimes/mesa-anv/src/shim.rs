//! DRM transport shim for Mesa `anv` running on Cellos (ADR-0033).
//!
//! Intercepts `open("/dev/dri/renderD128")`, `ioctl`, `mmap`, and `close`.
//! Routes requests to the underlying [`XeSession`] and [`Syncobjs`].

use crate::ioctl_defs::*;
use crate::query::handle_device_query;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use intel_gpu_core::backend::{BindOp, XeError, XeSession, XE_GEM_CPU_CACHING_WB};
use intel_gpu_core::syncobj::{FenceId, Syncobjs, WaitState, WAIT_ALL};

pub struct DrmFile {
    pub session: XeSession,
    pub syncobjs: Syncobjs,
}

impl DrmFile {
    pub fn new() -> Self {
        Self {
            session: XeSession::new(),
            syncobjs: Syncobjs::new(),
        }
    }
}

pub struct DrmShim {
    next_fd: i32,
    pub files: BTreeMap<i32, DrmFile>,
}

impl DrmShim {
    pub fn new() -> Self {
        Self {
            next_fd: 10, // Start above standard stdio descriptors
            files: BTreeMap::new(),
        }
    }

    pub fn open(&mut self, path: &str) -> Result<i32, i32> {
        if path != "/dev/dri/renderD128" && path != "renderD128" {
            return Err(ENOENT);
        }

        let fd = self.next_fd;
        self.next_fd += 1;
        self.files.insert(fd, DrmFile::new());
        Ok(fd)
    }

    pub fn close(&mut self, fd: i32) -> Result<(), i32> {
        self.files.remove(&fd).ok_or(EBADF)?;
        Ok(())
    }

    pub fn ioctl(&mut self, fd: i32, request: u32, arg: &mut [u8]) -> Result<(), i32> {
        let file = self.files.get_mut(&fd).ok_or(EBADF)?;
        let nr = ioctl_nr(request);

        match nr {
            DRM_IOCTL_NR_VERSION => {
                // struct drm_version: name, date, desc
                // Write driver name "xe" into name buffer if non-null
                if arg.len() >= 40 {
                    // name_len at offset 8, name ptr at 16
                    // For host simulation, we set the name length and copy string
                    arg[0..4].copy_from_slice(&1i32.to_le_bytes()); // version_major = 1
                    arg[4..8].copy_from_slice(&0i32.to_le_bytes()); // version_minor = 0
                }
                Ok(())
            }
            DRM_IOCTL_NR_GET_CAP => {
                // struct drm_get_cap: capability (u64), value (u64)
                if arg.len() < 16 {
                    return Err(EINVAL);
                }
                let cap = u64::from_le_bytes([
                    arg[0], arg[1], arg[2], arg[3], arg[4], arg[5], arg[6], arg[7],
                ]);
                if cap == DRM_CAP_SYNCOBJ_TIMELINE {
                    arg[8..16].copy_from_slice(&1u64.to_le_bytes());
                    Ok(())
                } else {
                    Err(EINVAL)
                }
            }
            DRM_XE_DEVICE_QUERY => {
                // struct drm_xe_device_query: extensions (u64), query (u32), size (u32), data (u64)
                if arg.len() < 24 {
                    return Err(EINVAL);
                }
                let query_id = u32::from_le_bytes([arg[8], arg[9], arg[10], arg[11]]);
                let mut size = u32::from_le_bytes([arg[12], arg[13], arg[14], arg[15]]);

                handle_device_query(query_id, &mut size, None)?;
                arg[12..16].copy_from_slice(&size.to_le_bytes());
                Ok(())
            }
            DRM_XE_GEM_CREATE => {
                // struct drm_xe_gem_create: vm_id (u32), handle (u32), size (u64), cpu_caching (u16), ...
                if arg.len() < 24 {
                    return Err(EINVAL);
                }
                let vm_id = u32::from_le_bytes([arg[0], arg[1], arg[2], arg[3]]);
                let size = u64::from_le_bytes([
                    arg[8], arg[9], arg[10], arg[11], arg[12], arg[13], arg[14], arg[15],
                ]) as usize;

                let handle = file
                    .session
                    .gem_create(size, 0, vm_id, XE_GEM_CPU_CACHING_WB)
                    .map_err(xe_to_errno)?;

                arg[4..8].copy_from_slice(&handle.to_le_bytes());
                Ok(())
            }
            DRM_XE_GEM_MMAP_OFFSET => {
                if arg.len() < 16 {
                    return Err(EINVAL);
                }
                let handle = u32::from_le_bytes([arg[4], arg[5], arg[6], arg[7]]);
                let offset = file.session.gem_mmap_offset(handle).map_err(xe_to_errno)?;
                arg[8..16].copy_from_slice(&offset.to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_NR_GEM_CLOSE => {
                if arg.len() < 4 {
                    return Err(EINVAL);
                }
                let handle = u32::from_le_bytes([arg[0], arg[1], arg[2], arg[3]]);
                file.session.gem_close(handle).map_err(xe_to_errno)?;
                Ok(())
            }
            DRM_XE_VM_CREATE => {
                if arg.len() < 8 {
                    return Err(EINVAL);
                }
                let flags = u32::from_le_bytes([arg[4], arg[5], arg[6], arg[7]]);
                let vm_id = file.session.vm_create(flags).map_err(xe_to_errno)?;
                arg[0..4].copy_from_slice(&vm_id.to_le_bytes());
                Ok(())
            }
            DRM_XE_VM_DESTROY => {
                if arg.len() < 4 {
                    return Err(EINVAL);
                }
                let vm_id = u32::from_le_bytes([arg[0], arg[1], arg[2], arg[3]]);
                file.session.vm_destroy(vm_id).map_err(xe_to_errno)?;
                Ok(())
            }
            DRM_XE_EXEC_QUEUE_CREATE => {
                if arg.len() < 16 {
                    return Err(EINVAL);
                }
                let vm_id = u32::from_le_bytes([arg[4], arg[5], arg[6], arg[7]]);
                let q_id = file
                    .session
                    .exec_queue_create(vm_id, 0, 1) // RENDER, NORMAL
                    .map_err(xe_to_errno)?;
                arg[0..4].copy_from_slice(&q_id.to_le_bytes());
                Ok(())
            }
            DRM_XE_EXEC_QUEUE_DESTROY => {
                if arg.len() < 4 {
                    return Err(EINVAL);
                }
                let q_id = u32::from_le_bytes([arg[0], arg[1], arg[2], arg[3]]);
                file.session.exec_queue_destroy(q_id).map_err(xe_to_errno)?;
                Ok(())
            }
            DRM_XE_EXEC_QUEUE_GET_PROPERTY => {
                if arg.len() < 16 {
                    return Err(EINVAL);
                }
                let q_id = u32::from_le_bytes([arg[4], arg[5], arg[6], arg[7]]);
                let ban = file
                    .session
                    .exec_queue_get_property_ban(q_id)
                    .map_err(xe_to_errno)?;
                arg[8..16].copy_from_slice(&(ban as u64).to_le_bytes());
                Ok(())
            }
            DRM_XE_EXEC => {
                if arg.len() < 16 {
                    return Err(EINVAL);
                }
                let q_id = u32::from_le_bytes([arg[0], arg[1], arg[2], arg[3]]);
                let _job_id = file.session.exec(q_id, 0, 0, None).map_err(xe_to_errno)?;
                Ok(())
            }
            DRM_IOCTL_NR_SYNCOBJ_CREATE => {
                if arg.len() < 8 {
                    return Err(EINVAL);
                }
                let flags = u32::from_le_bytes([arg[4], arg[5], arg[6], arg[7]]);
                let handle = file.syncobjs.create(flags).map_err(|_| EINVAL)?;
                arg[0..4].copy_from_slice(&handle.to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_NR_SYNCOBJ_DESTROY => {
                if arg.len() < 4 {
                    return Err(EINVAL);
                }
                let handle = u32::from_le_bytes([arg[0], arg[1], arg[2], arg[3]]);
                file.syncobjs.destroy(handle).map_err(|_| ENOENT)?;
                Ok(())
            }
            _ => Err(EINVAL),
        }
    }
}

fn xe_to_errno(err: XeError) -> i32 {
    match err {
        XeError::Invalid => EINVAL,
        XeError::NoEntry => ENOENT,
        XeError::OutOfMemory => ENOMEM,
        XeError::Canceled => ECANCELED,
        XeError::PermissionDenied => EPERM,
        XeError::NotSupported => EOPNOTSUPP,
        XeError::TryAgain => 11, // EAGAIN
    }
}
