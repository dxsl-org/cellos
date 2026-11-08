//! DRM and Linux Xe ioctl definitions and request decoding.
//!
//! Upstream reference: `include/uapi/drm/drm.h` and `include/uapi/drm/xe_drm.h` (MIT).

pub const DRM_IOCTL_BASE: u32 = 0x64; // 'd'
pub const DRM_COMMAND_BASE: u32 = 0x40;

// ── Linux ioctl decoding macros ───────────────────────────────────────────────

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

pub const DRM_XE_DEVICE_QUERY: u32 = DRM_COMMAND_BASE + 0x00; // 0x40
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

// ── DRM Caps ─────────────────────────────────────────────────────────────────

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
