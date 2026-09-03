//! Self-test for read-only capability enforcement on files.
//!
//! Confirms that capabilities created via OpenCap only possess FILE_READ permissions
//! and that WriteCap is denied.

use crate::cell::cap_registry::{CapResource, CAP_TABLE};
use api::cap::CapPerms;
use types::{CellId, ViError};

pub fn run() {
    let mut table = CAP_TABLE.lock();
    let cell = CellId(999);
    let cap = table.alloc(
        cell,
        CapResource::File { file: None },
        CapPerms::FILE_READ.0,
    );

    // Read and Seek should pass permission check
    let entry = table.get_if_owner(cap, cell).expect("cap entry exists");
    assert!(CapPerms(entry.perms).has(CapPerms::READ));
    assert!(CapPerms(entry.perms).has(CapPerms::SEEK));

    // Write permission must be denied
    assert!(!CapPerms(entry.perms).has(CapPerms::WRITE));

    // park_file with WRITE must fail with PermissionDenied
    assert_eq!(
        table.park_file(cap, cell, CapPerms::WRITE).err(),
        Some(ViError::PermissionDenied)
    );

    table.revoke(cap);
}
