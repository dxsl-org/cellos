//! Host unit tests for the Mesa `anv` DRM transport shim.

use crate::ioctl_defs::*;
use crate::query::*;
use crate::shim::DrmShim;

#[test]
fn shim_open_and_close() {
    let mut shim = DrmShim::new();
    let fd = shim.open("/dev/dri/renderD128").unwrap();
    assert!(fd >= 10);

    // Invalid path rejected with -ENOENT
    assert_eq!(shim.open("/dev/dri/card0"), Err(ENOENT));

    // Close
    assert_eq!(shim.close(fd), Ok(()));
    // Repeated close returns -EBADF
    assert_eq!(shim.close(fd), Err(EBADF));
}

#[test]
fn shim_ioctl_version() {
    let mut shim = DrmShim::new();
    let fd = shim.open("/dev/dri/renderD128").unwrap();

    let mut arg = [0u8; 40];
    assert_eq!(shim.ioctl(fd, DRM_IOCTL_NR_VERSION, &mut arg), Ok(()));
    let major = i32::from_le_bytes([arg[0], arg[1], arg[2], arg[3]]);
    assert_eq!(major, 1);
}

#[test]
fn shim_ioctl_get_cap_timeline() {
    let mut shim = DrmShim::new();
    let fd = shim.open("/dev/dri/renderD128").unwrap();

    let mut arg = [0u8; 16];
    arg[0..8].copy_from_slice(&DRM_CAP_SYNCOBJ_TIMELINE.to_le_bytes());
    assert_eq!(shim.ioctl(fd, DRM_IOCTL_NR_GET_CAP, &mut arg), Ok(()));
    let val = u64::from_le_bytes([
        arg[8], arg[9], arg[10], arg[11], arg[12], arg[13], arg[14], arg[15],
    ]);
    assert_eq!(val, 1);
}

#[test]
fn shim_ioctl_device_query() {
    let mut shim = DrmShim::new();
    let fd = shim.open("/dev/dri/renderD128").unwrap();

    // Query ENGINES (id 0) with size 0: probes required size
    let mut arg = [0u8; 24];
    arg[8..12].copy_from_slice(&DRM_XE_DEVICE_QUERY_ENGINES.to_le_bytes());
    assert_eq!(shim.ioctl(fd, DRM_XE_DEVICE_QUERY, &mut arg), Ok(()));
    let size = u32::from_le_bytes([arg[12], arg[13], arg[14], arg[15]]);
    assert!(size > 0);
}

#[test]
fn shim_ioctl_gem_lifecycle() {
    let mut shim = DrmShim::new();
    let fd = shim.open("/dev/dri/renderD128").unwrap();

    // Create GEM: size = 8192
    let mut create_arg = [0u8; 24];
    create_arg[8..16].copy_from_slice(&8192u64.to_le_bytes());
    assert_eq!(shim.ioctl(fd, DRM_XE_GEM_CREATE, &mut create_arg), Ok(()));
    let handle = u32::from_le_bytes([create_arg[4], create_arg[5], create_arg[6], create_arg[7]]);
    assert_eq!(handle, 1);

    // Get mmap offset
    let mut mmap_arg = [0u8; 16];
    mmap_arg[4..8].copy_from_slice(&handle.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_XE_GEM_MMAP_OFFSET, &mut mmap_arg),
        Ok(())
    );
    let offset = u64::from_le_bytes([
        mmap_arg[8],
        mmap_arg[9],
        mmap_arg[10],
        mmap_arg[11],
        mmap_arg[12],
        mmap_arg[13],
        mmap_arg[14],
        mmap_arg[15],
    ]);
    assert_eq!(offset, (1u64) << 32);

    // Close GEM handle
    let mut close_arg = [0u8; 8];
    close_arg[0..4].copy_from_slice(&handle.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_NR_GEM_CLOSE, &mut close_arg),
        Ok(())
    );
}

#[test]
fn shim_ioctl_vm_and_queue_lifecycle() {
    let mut shim = DrmShim::new();
    let fd = shim.open("/dev/dri/renderD128").unwrap();

    // Create VM
    let mut vm_arg = [0u8; 16];
    assert_eq!(shim.ioctl(fd, DRM_XE_VM_CREATE, &mut vm_arg), Ok(()));
    let vm_id = u32::from_le_bytes([vm_arg[0], vm_arg[1], vm_arg[2], vm_arg[3]]);
    assert_eq!(vm_id, 1);

    // Create Exec Queue
    let mut q_arg = [0u8; 16];
    q_arg[4..8].copy_from_slice(&vm_id.to_le_bytes());
    assert_eq!(shim.ioctl(fd, DRM_XE_EXEC_QUEUE_CREATE, &mut q_arg), Ok(()));
    let q_id = u32::from_le_bytes([q_arg[0], q_arg[1], q_arg[2], q_arg[3]]);
    assert_eq!(q_id, 1);

    // Get Exec Queue property (ban)
    let mut prop_arg = [0u8; 16];
    prop_arg[4..8].copy_from_slice(&q_id.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_XE_EXEC_QUEUE_GET_PROPERTY, &mut prop_arg),
        Ok(())
    );
    let ban = u64::from_le_bytes([
        prop_arg[8],
        prop_arg[9],
        prop_arg[10],
        prop_arg[11],
        prop_arg[12],
        prop_arg[13],
        prop_arg[14],
        prop_arg[15],
    ]);
    assert_eq!(ban, 0); // Not banned

    // Exec (empty batch = idle barrier)
    let mut exec_arg = [0u8; 16];
    exec_arg[0..4].copy_from_slice(&q_id.to_le_bytes());
    assert_eq!(shim.ioctl(fd, DRM_XE_EXEC, &mut exec_arg), Ok(()));

    // Destroy Queue
    let mut q_destroy_arg = [0u8; 8];
    q_destroy_arg[0..4].copy_from_slice(&q_id.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_XE_EXEC_QUEUE_DESTROY, &mut q_destroy_arg),
        Ok(())
    );

    // Destroy VM
    let mut vm_destroy_arg = [0u8; 8];
    vm_destroy_arg[0..4].copy_from_slice(&vm_id.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_XE_VM_DESTROY, &mut vm_destroy_arg),
        Ok(())
    );
}

#[test]
fn shim_ioctl_syncobj_lifecycle() {
    let mut shim = DrmShim::new();
    let fd = shim.open("/dev/dri/renderD128").unwrap();

    // Create syncobj
    let mut sync_arg = [0u8; 8];
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_NR_SYNCOBJ_CREATE, &mut sync_arg),
        Ok(())
    );
    let handle = u32::from_le_bytes([sync_arg[0], sync_arg[1], sync_arg[2], sync_arg[3]]);
    assert_eq!(handle, 1);

    // Destroy syncobj
    let mut destroy_arg = [0u8; 8];
    destroy_arg[0..4].copy_from_slice(&handle.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_NR_SYNCOBJ_DESTROY, &mut destroy_arg),
        Ok(())
    );
}

#[test]
fn shim_unknown_ioctl_returns_einval() {
    let mut shim = DrmShim::new();
    let fd = shim.open("/dev/dri/renderD128").unwrap();

    let mut arg = [0u8; 8];
    assert_eq!(shim.ioctl(fd, 0xFF, &mut arg), Err(EINVAL));
}
