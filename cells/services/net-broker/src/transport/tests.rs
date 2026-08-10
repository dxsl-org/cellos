extern crate alloc;

use super::*;
use alloc::vec;

#[test]
fn load_vfs_key_accepts_exact_32_bytes() {
    let key = load_vfs_key("/etc/cellos/cluster.key", |path, max_bytes| {
        assert_eq!(path, "/etc/cellos/cluster.key");
        assert_eq!(max_bytes, K1_READ_MAX_BYTES);
        Ok((0u8..32).collect())
    })
    .expect("key");

    assert_eq!(key, core::array::from_fn(|i| i as u8));
}

#[test]
fn load_vfs_key_rejects_short_file() {
    let err = load_vfs_key("/etc/cellos/cluster.key", |_, _| Ok(vec![5; 31])).unwrap_err();
    assert_eq!(err, ViError::IO);
}

#[test]
fn load_vfs_key_preserves_first_32_bytes_and_surfaces_oversize_failure() {
    let key = load_vfs_key("/etc/cellos/cluster.key", |_, _| Ok(vec![9; 48])).expect("key");
    assert_eq!(key, [9; 32]);

    let err =
        load_vfs_key("/etc/cellos/cluster.key", |_, _| Err(ViError::OutOfMemory)).unwrap_err();
    assert_eq!(err, ViError::OutOfMemory);
}
