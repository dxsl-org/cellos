//! QEMU-only transaction probes for the private-root substrate.

use super::*;
use crate::memory::frame::FRAME_ALLOCATOR;

const PRIVATE_PAGE: usize = 0x4000;
const ABI_PAGE: usize = 0x5000;

fn flags() -> Flags {
    Flags::from_bits(Flags::READ | Flags::WRITE)
}
fn supervisor_flags() -> Flags {
    Flags::from_bits(Flags::VALID | Flags::READ | Flags::EXECUTE)
}
fn used_frames() -> Option<usize> {
    FRAME_ALLOCATOR
        .lock()
        .as_ref()
        .map(|frames| frames.used_frames())
}

pub(crate) fn run_primary() {
    let Some(before) = used_frames() else {
        log::error!("S22-RV64-ASPACE: FAIL — frame allocator unavailable");
        return;
    };
    let kernel_root = *crate::memory::paging::KERNEL_ROOT.lock();

    fail_allocation_after(1);
    let mut builder = AddressSpaceBuilder::new();
    let _ = builder.map_user_page(PRIVATE_PAGE, MappingKind::Private, flags());
    let allocation_rollback = builder.build().is_err() && used_frames() == Some(before);
    fail_allocation_after(usize::MAX);

    fail_next_map();
    let mut builder = AddressSpaceBuilder::new();
    let _ = builder.map_user_page(PRIVATE_PAGE, MappingKind::Private, flags());
    let map_rollback = builder.build().is_err() && used_frames() == Some(before);

    let private_mapping = {
        let mut builder = AddressSpaceBuilder::new();
        let supervisor_ok = match SupervisorMapping::identity_page(PAGE_SIZE, supervisor_flags()) {
            Ok(mapping) => {
                builder.allow_supervisor(mapping);
                true
            }
            Err(_) => false,
        };
        let request_ok = builder
            .map_user_page(PRIVATE_PAGE, MappingKind::Private, flags())
            .is_ok();
        match builder.build() {
            Ok(space) => {
                supervisor_ok
                    && request_ok
                    && space.ledger().len() == 1
                    && space.ledger()[0].kind == MappingKind::Private
                    && kernel_root
                        .map(|root| root != space.root_ppn() * PAGE_SIZE)
                        .unwrap_or(true)
            }
            Err(_) => false,
        }
    };
    let tracked_lifecycle = match AddressSpaceBuilder::new().build() {
        Ok(space) => match space.acquire_copy_reader() {
            Ok(reader) => {
                let reader_tracked = space.copy_reader_count() == 1;
                let hart_tracked =
                    space.set_current_hart(1, true).is_ok() && space.current_harts() == 1 << 1;
                space.retire();
                let new_leases_denied =
                    matches!(space.acquire_copy_reader(), Err(AddressSpaceError::Dying))
                        && matches!(
                            space.set_current_hart(2, true),
                            Err(AddressSpaceError::Dying)
                        );
                let hart_cleared =
                    space.set_current_hart(1, false).is_ok() && space.current_harts() == 0;
                drop(reader);
                reader_tracked
                    && hart_tracked
                    && new_leases_denied
                    && hart_cleared
                    && space.copy_reader_count() == 0
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    let mut builder = AddressSpaceBuilder::new();
    let write_execute_denied = builder
        .map_user_page(
            ABI_PAGE,
            MappingKind::ImmutableImage,
            Flags::from_bits(Flags::READ | Flags::WRITE | Flags::EXECUTE),
        )
        .is_err();
    let global_root_unchanged =
        *crate::memory::paging::KERNEL_ROOT.lock() == kernel_root && used_frames() == Some(before);
    if allocation_rollback
        && map_rollback
        && private_mapping
        && tracked_lifecycle
        && write_execute_denied
        && global_root_unchanged
    {
        log::info!("S22-RV64-ASPACE: PASS");
    } else {
        log::error!("S22-RV64-ASPACE: FAIL");
    }

    // ── ASID lease contract ──────────────────────────────────────────────────
    //
    // Two live roots must never carry the same architectural tag, and a released
    // tag must return only after its holder invalidated it — so exhaustion has to
    // refuse rather than reissue a live value. Claim the whole pool, prove the
    // values are distinct and inside the architectural width, then release one and
    // prove exactly that value comes back.
    let mut live: Vec<AsidLease> = Vec::new();
    while let Some(lease) = AsidLease::acquire(1) {
        live.push(lease);
    }
    let exhausted = live.len() == MAX_LIVE_ASIDS;
    let mut values: Vec<usize> = live.iter().map(|lease| lease.value).collect();
    values.sort_unstable();
    values.dedup();
    let distinct_live = values.len() == live.len();
    let width_ok = live
        .iter()
        .all(|lease| lease.value >= 1 && lease.value < 1usize << asid_width());
    let owner_tracked = live
        .iter()
        .all(|lease| live_tag_owner(lease.value) == Some(1));
    let refused_at_exhaustion = AsidLease::acquire(1).is_none();

    let released = live.pop();
    let released_value = released.as_ref().map(|lease| lease.value);
    drop(released);
    let recycled = AsidLease::acquire(2).is_some_and(|again| {
        Some(again.value) == released_value && live_tag_owner(again.value) == Some(2)
    });
    drop(live);

    if exhausted && distinct_live && width_ok && owner_tracked && refused_at_exhaustion && recycled {
        log::info!("S22-RV64-ASID-LEASE: PASS");
    } else {
        log::error!(
            "S22-RV64-ASID-LEASE: FAIL exhausted={exhausted} distinct={distinct_live} \
             width={width_ok} owner={owner_tracked} refused_at_exhaustion={refused_at_exhaustion} \
             recycled={recycled}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_mapping_rejects_kernel_and_write_execute_pages() {
        assert_eq!(
            validate_user_mapping(USER_LIMIT, flags()),
            Err(AddressSpaceError::InvalidMapping)
        );
        assert_eq!(
            validate_user_mapping(
                PRIVATE_PAGE,
                Flags::from_bits(Flags::WRITE | Flags::EXECUTE)
            ),
            Err(AddressSpaceError::WriteExecute)
        );
    }
}
