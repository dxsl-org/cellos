//! Settling what a cell inherited before anything it sent is served.
//!
//! The kernel states provenance — which cell named a set of handle values for
//! this one — and nothing more. It cannot tell whether the spawner was entitled
//! to them, because a cell acquires handles from this service after it starts
//! and only this service knows what it holds. So the record is pulled here, once
//! per cell generation, and checked against the table by
//! [`crate::dirs::DirTable::bind_inherited`].
//!
//! Ordering matters more than it looks. Both answers this produces — what the
//! cell holds, and whether it may still name a path — have to be in place before
//! the cell's first request, or a cell whose spawner placed it in the capability
//! world would get one path-string operation in ahead of the seal.

use api::dir_attestation::DIR_ATTESTATION_LEN;

use crate::caller::Caller;
use crate::dirs::bind::BindOutcome;
use crate::dirs::lifecycle::Contact;
use crate::manager::VfsManager;

/// Settle what the kernel says about `caller`. Idempotent per cell generation.
pub fn admit(vfs: &mut VfsManager, caller: Caller) {
    if vfs.dirs.on_contact(caller) != Contact::NeedsAttestation {
        return;
    }
    // Marked before the query, not after: one attempt per cell generation either
    // way, so a cell cannot make the service re-query by failing repeatedly.
    vfs.dirs.mark_attested(caller);
    let mut buf = [0u8; DIR_ATTESTATION_LEN];
    let Some(record) = ostd::syscall::sys_query_dir_handles(caller.cell.0, &mut buf) else {
        return;
    };
    match vfs.dirs.bind_inherited(caller, &record) {
        BindOutcome::NothingNamed => {}
        // The record could not be attributed to this caller, so there is no set
        // to act on. The cell keeps the authority an unmigrated cell has; it
        // gains nothing.
        BindOutcome::NotThisCell => {
            ostd::io::println("[vfs] discarding a provenance record for a different cell");
        }
        BindOutcome::Bound(_) => {
            vfs.dirs.seal(caller);
        }
        BindOutcome::Refused => {
            vfs.dirs.seal(caller);
            ostd::io::println(
                "[vfs] refused an inherited handle set: the spawner did not hold all of it",
            );
        }
    }
}
