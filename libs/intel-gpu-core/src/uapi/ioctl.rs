//! DRM and Linux Xe ioctl definitions and request decoding.
//!
//! Upstream reference: `include/uapi/drm/drm.h`, `include/uapi/drm/xe_drm.h` and
//! `include/uapi/asm-generic/ioctl.h` (MIT / GPL-2.0 WITH Linux-syscall-note).
//! Struct sizes are the LP64 layouts every Cellos target uses.

pub const DRM_IOCTL_BASE: u32 = 0x64; // 'd'
pub const DRM_COMMAND_BASE: u32 = 0x40;

// ── Linux ioctl encoding: nr 7:0, type 15:8, size 29:16, dir 31:30 ─────────────

pub const IOC_WRITE: u32 = 1;
pub const IOC_READ: u32 = 2;

#[inline]
pub const fn ioctl_nr(cmd: u32) -> u32 {
    cmd & 0xFF
}

#[inline]
pub const fn ioctl_type(cmd: u32) -> u32 {
    (cmd >> 8) & 0xFF
}

#[inline]
pub const fn ioctl_size(cmd: u32) -> usize {
    ((cmd >> 16) & 0x3FFF) as usize
}

#[inline]
pub const fn ioctl_dir(cmd: u32) -> u32 {
    cmd >> 30
}

/// `DRM_IOWR(nr, struct)` for a struct of `size` bytes.
pub const fn drm_iowr(nr: u32, size: usize) -> u32 {
    ((IOC_READ | IOC_WRITE) << 30) | ((size as u32) << 16) | (DRM_IOCTL_BASE << 8) | nr
}

/// `DRM_IOW(nr, struct)` for a struct of `size` bytes.
pub const fn drm_iow(nr: u32, size: usize) -> u32 {
    (IOC_WRITE << 30) | ((size as u32) << 16) | (DRM_IOCTL_BASE << 8) | nr
}

// ── DRM Core ioctl numbers ───────────────────────────────────────────────────

pub const DRM_IOCTL_NR_VERSION: u32 = 0x00;
pub const DRM_IOCTL_NR_GET_CAP: u32 = 0x0C;
pub const DRM_IOCTL_NR_GEM_CLOSE: u32 = 0x09;

pub const DRM_IOCTL_NR_SYNCOBJ_CREATE: u32 = 0xBF;
pub const DRM_IOCTL_NR_SYNCOBJ_DESTROY: u32 = 0xC0;
pub const DRM_IOCTL_NR_SYNCOBJ_WAIT: u32 = 0xC3;
pub const DRM_IOCTL_NR_SYNCOBJ_RESET: u32 = 0xC4;
pub const DRM_IOCTL_NR_SYNCOBJ_SIGNAL: u32 = 0xC5;
pub const DRM_IOCTL_NR_SYNCOBJ_TIMELINE_WAIT: u32 = 0xCA;
pub const DRM_IOCTL_NR_SYNCOBJ_QUERY: u32 = 0xCB;
pub const DRM_IOCTL_NR_SYNCOBJ_TRANSFER: u32 = 0xCC;
pub const DRM_IOCTL_NR_SYNCOBJ_TIMELINE_SIGNAL: u32 = 0xCD;

// ── Xe Driver Specific ioctl numbers ─────────────────────────────────────────

pub const DRM_XE_DEVICE_QUERY: u32 = DRM_COMMAND_BASE; // 0x40
pub const DRM_XE_GEM_CREATE: u32 = DRM_COMMAND_BASE + 0x01; // 0x41
pub const DRM_XE_GEM_MMAP_OFFSET: u32 = DRM_COMMAND_BASE + 0x02; // 0x42
pub const DRM_XE_VM_CREATE: u32 = DRM_COMMAND_BASE + 0x03; // 0x43
pub const DRM_XE_VM_DESTROY: u32 = DRM_COMMAND_BASE + 0x04; // 0x44
pub const DRM_XE_VM_BIND: u32 = DRM_COMMAND_BASE + 0x05; // 0x45
pub const DRM_XE_EXEC_QUEUE_CREATE: u32 = DRM_COMMAND_BASE + 0x06; // 0x46
pub const DRM_XE_EXEC_QUEUE_DESTROY: u32 = DRM_COMMAND_BASE + 0x07; // 0x47
pub const DRM_XE_EXEC_QUEUE_GET_PROPERTY: u32 = DRM_COMMAND_BASE + 0x08; // 0x48
pub const DRM_XE_EXEC: u32 = DRM_COMMAND_BASE + 0x09; // 0x49
pub const DRM_XE_VM_GET_PROPERTY: u32 = DRM_COMMAND_BASE + 0x0F; // 0x4F

// ── Full request codes the shim serves (direction and struct size included) ──

pub const DRM_IOCTL_VERSION: u32 = drm_iowr(DRM_IOCTL_NR_VERSION, 64);
pub const DRM_IOCTL_GET_CAP: u32 = drm_iowr(DRM_IOCTL_NR_GET_CAP, 16);
pub const DRM_IOCTL_GEM_CLOSE: u32 = drm_iow(DRM_IOCTL_NR_GEM_CLOSE, 8);
pub const DRM_IOCTL_SYNCOBJ_CREATE: u32 = drm_iowr(DRM_IOCTL_NR_SYNCOBJ_CREATE, 8);
pub const DRM_IOCTL_SYNCOBJ_DESTROY: u32 = drm_iowr(DRM_IOCTL_NR_SYNCOBJ_DESTROY, 8);
pub const DRM_IOCTL_XE_DEVICE_QUERY: u32 = drm_iowr(DRM_XE_DEVICE_QUERY, 40);
pub const DRM_IOCTL_XE_GEM_CREATE: u32 = drm_iowr(DRM_XE_GEM_CREATE, 56);
pub const DRM_IOCTL_XE_GEM_MMAP_OFFSET: u32 = drm_iowr(DRM_XE_GEM_MMAP_OFFSET, 40);
pub const DRM_IOCTL_XE_VM_CREATE: u32 = drm_iowr(DRM_XE_VM_CREATE, 32);
pub const DRM_IOCTL_XE_VM_DESTROY: u32 = drm_iow(DRM_XE_VM_DESTROY, 24);
pub const DRM_IOCTL_XE_VM_BIND: u32 = drm_iow(DRM_XE_VM_BIND, VM_BIND_SIZE);
pub const DRM_IOCTL_XE_EXEC_QUEUE_CREATE: u32 = drm_iowr(DRM_XE_EXEC_QUEUE_CREATE, 48);
pub const DRM_IOCTL_XE_EXEC_QUEUE_DESTROY: u32 = drm_iow(DRM_XE_EXEC_QUEUE_DESTROY, 24);
pub const DRM_IOCTL_XE_EXEC_QUEUE_GET_PROPERTY: u32 = drm_iowr(DRM_XE_EXEC_QUEUE_GET_PROPERTY, 40);
pub const DRM_IOCTL_XE_EXEC: u32 = drm_iow(DRM_XE_EXEC, 56);

/// `sizeof(struct drm_xe_engine_class_instance)`, the element `EXEC_QUEUE_CREATE.instances`
/// points at.
pub const EXEC_QUEUE_INSTANCE_SIZE: usize = 8;
/// `sizeof(struct drm_xe_vm_bind)`.
pub const VM_BIND_SIZE: usize = 136;
/// `sizeof(struct drm_xe_vm_bind_op)`, the element `drm_xe_vm_bind.vector_of_binds`
/// points at when `num_binds > 1`.
pub const VM_BIND_OP_SIZE: usize = 80;
/// `sizeof(struct drm_xe_sync)`, the element `syncs` points at in `VM_BIND` and `EXEC`.
pub const XE_SYNC_SIZE: usize = 48;

// ── DRM Caps ─────────────────────────────────────────────────────────────────

pub const DRM_CAP_SYNCOBJ: u64 = 0x13;
pub const DRM_CAP_SYNCOBJ_TIMELINE: u64 = 0x14;

// ── Linux standard error numbers returned to Mesa ────────────────────────────

pub const EINVAL: i32 = 22;
pub const ENOENT: i32 = 2;
pub const ENOMEM: i32 = 12;
pub const EPERM: i32 = 1;
pub const EOPNOTSUPP: i32 = 95;
pub const ECANCELED: i32 = 125;
pub const ETIME: i32 = 62;
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const EIO: i32 = 5;
pub const ENODEV: i32 = 19;
pub const EMFILE: i32 = 24;
pub const ENFILE: i32 = 23;
pub const EAGAIN: i32 = 11;
