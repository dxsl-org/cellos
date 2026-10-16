//! Self-test for read-only capability enforcement on files.
//!
//! Confirms that capabilities created via OpenCap only possess FILE_READ permissions
//! and that WriteCap is denied.

use crate::cell::cspace::{register_cell_authority, ControlStatus, Principal, ResourceKind, CELL_AUTHORITIES};
use crate::task::cap::CapSet;
use api::cap::CapPerms;
use types::CellId;

pub fn run() {
    let cell = CellId(63);
    let gen = 1u64;
    register_cell_authority(cell, gen, CapSet::EMPTY);
    let mut auths = CELL_AUTHORITIES.lock();
    let auth = auths.get_mut(&(cell.0, gen)).expect("authority registered");
    let ticket = auth.allocate_slot(
        Principal::Cell { id: cell, generation: gen },
        Principal::Kernel,
        ResourceKind::BootFSFile,
        CapPerms::FILE_READ.0,
        None,
        None,
    ).expect("file capability slot allocated");

    // Read and Seek should pass permission check
    let slot = auth.verify_ticket(ticket).expect("ticket verified");
    assert!(CapPerms(slot.rights).has(CapPerms::READ));
    assert!(CapPerms(slot.rights).has(CapPerms::SEEK));

    // Write permission must be denied
    assert!(!CapPerms(slot.rights).has(CapPerms::WRITE));

    // park_file with WRITE must fail with Denied
    assert_eq!(
        auth.park_file(ticket, CapPerms::WRITE.0).err(),
        Some(ControlStatus::Denied)
    );

    auth.close_ticket(ticket).expect("ticket closed");
}
