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

    // ── Unmap invalidation order ─────────────────────────────────────────────
    //
    // A page that is being unmapped must be invalidated before its frame can
    // return to the allocator: a hart that still holds the translation may read
    // the leaf and walk the table chain. `unmap_private_page` released the owned
    // leaf and pruned its tables without invalidating anything.
    let mut unmap_order_detail = (false, false, false, false, false);
    let unmap_order = {
        let mut builder = AddressSpaceBuilder::new();
        let built = builder
            .map_user_page(PRIVATE_PAGE, MappingKind::Private, flags())
            .and_then(|()| builder.build());
        match built {
            Ok(space) => {
                // Count this space's own frames, not unrelated allocations
                // made by other tasks while the remote ack is outstanding.
                let before = space.frames.lock().len();
                let tag = space.asid();
                let remote_acks_before: Vec<(usize, usize)> = crate::task::smp::online_harts()
                    .filter(|hart| *hart != crate::task::hart_local::current_hart_id())
                    .map(|hart| (hart, crate::task::smp::tlb_flush_complete_epoch(hart)))
                    .collect();
                crate::memory::tlb_shootdown::begin_test_flush_observation();
                let unmapped = space.unmap_private_page(PRIVATE_PAGE).is_ok();
                let tagged = crate::memory::tlb_shootdown::test_tag_flush_observed(tag);
                crate::memory::tlb_shootdown::finish_test_flush_observation();
                let remote_acked = remote_acks_before.iter().all(|(hart, before)| {
                    crate::task::smp::tlb_flush_complete_epoch(*hart) > *before
                });
                let released = space.frames.lock().len() < before;
                let not_quarantined = quarantined_frame_count() == 0;
                unmap_order_detail = (unmapped, tagged, remote_acked, released, not_quarantined);
                unmapped && tagged && remote_acked && released && not_quarantined
            }
            Err(_) => false,
        }
    };
    if unmap_order {
        // Remote acknowledgement evidence, same shape as the lease fixture: on a
        // two-hart run the release above waited for the other hart's own flush.
        let me = crate::task::hart_local::current_hart_id();
        let remotes: alloc::vec::Vec<usize> = crate::task::smp::online_harts()
            .filter(|hart| *hart != me)
            .collect();
        log::info!(
            "[aspace] unmap release confirmed: remote_harts={} quarantined={}",
            remotes.len(),
            quarantined_frame_count()
        );
        log::info!("S22-RV64-UNMAP-ORDER: PASS");
    } else {
        log::error!(
            "S22-RV64-UNMAP-ORDER: FAIL unmapped={} tagged={} remote_acked={} released={} not_quarantined={}",
            unmap_order_detail.0,
            unmap_order_detail.1,
            unmap_order_detail.2,
            unmap_order_detail.3,
            unmap_order_detail.4
        );
    }

    // The final grant leaf detaches table frames too. Its backing page remains
    // externally owned; only acknowledged tag invalidation permits reclaiming
    // the tables. An unowned "private" leaf must fail without losing its ledger.
    let grant_last_leaf = (|| {
        let backing = allocate_owned_frame().ok()?;
        let space = AddressSpaceBuilder::new().build().ok()?;
        space
            .map_grant_page(ABI_PAGE, backing.physical_address(), flags())
            .ok()?;
        let before = space.table_frames.lock().len();
        let tag = space.asid();
        crate::memory::tlb_shootdown::begin_test_flush_observation();
        let unmapped = space.unmap_grant_page(ABI_PAGE).is_ok();
        let tagged = crate::memory::tlb_shootdown::test_tag_flush_observed(tag);
        crate::memory::tlb_shootdown::finish_test_flush_observation();
        Some(
            before > 0
                && unmapped
                && tagged
                && space.table_frames.lock().len() < before
                && space.page_proof_for(ABI_PAGE).is_none()
                && quarantined_frame_count() == 0,
        )
    })()
    .unwrap_or(false);
    let unowned_error_restores_ledger = (|| {
        let backing = allocate_owned_frame().ok()?;
        let mut builder = AddressSpaceBuilder::new();
        builder
            .map_existing_user_page(
                ABI_PAGE,
                backing.physical_address(),
                MappingKind::Private,
                flags(),
            )
            .ok()?;
        let space = builder.build().ok()?;
        Some(
            space.unmap_private_page(ABI_PAGE) == Err(AddressSpaceError::NotFound)
                && space.page_proof_for(ABI_PAGE).is_some()
                && space.table_frames.lock().len() > 0,
        )
    })()
    .unwrap_or(false);
    if grant_last_leaf && unowned_error_restores_ledger {
        log::info!("S22-RV64-ROOT-UNMAP: PASS");
    } else {
        log::error!(
            "S22-RV64-ROOT-UNMAP: FAIL grant_last_leaf={} error_ledger={}",
            grant_last_leaf,
            unowned_error_restores_ledger
        );
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

    // Releasing a tag now waits for every online hart to confirm its own
    // invalidation, so the recycle below is also the witness that the wait
    // completed: if an ack never arrived the slot is retained (fail closed) and
    // the released value does not come back.
    let me = crate::task::hart_local::current_hart_id();
    let remote_acks_before: alloc::vec::Vec<(usize, usize)> = crate::task::smp::online_harts()
        .filter(|hart| *hart != me)
        .map(|hart| (hart, crate::task::smp::tlb_flush_complete_epoch(hart)))
        .collect();

    let released = live.pop();
    let released_value = released.as_ref().map(|lease| lease.value);
    drop(released);
    let remote_ack_advanced = remote_acks_before
        .iter()
        .all(|(hart, before)| crate::task::smp::tlb_flush_complete_epoch(*hart) > *before);
    let recycled = AsidLease::acquire(2).is_some_and(|again| {
        Some(again.value) == released_value && live_tag_owner(again.value) == Some(2)
    });
    drop(live);

    if exhausted
        && distinct_live
        && width_ok
        && owner_tracked
        && refused_at_exhaustion
        && remote_ack_advanced
        && recycled
    {
        // Detail first, terminal last and unadorned: the lane anchors its
        // pattern to the end of the line.
        log::info!(
            "[asid] lease release confirmed: remote_acks={} harts={}",
            remote_acks_before.len(),
            crate::task::smp::online_hart_count()
        );
        log::info!("S22-RV64-ASID-LEASE: PASS");
    } else {
        log::error!(
            "S22-RV64-ASID-LEASE: FAIL exhausted={exhausted} distinct={distinct_live} \
             width={width_ok} owner={owner_tracked} refused_at_exhaustion={refused_at_exhaustion} \
             remote_ack_advanced={remote_ack_advanced} recycled={recycled}"
        );
    }

    // ── Deferred release of unconfirmed frames ────────────────────────────────
    //
    // A release path must not wait for an acknowledgement that may never arrive.
    // This is the unconfirmed window (`set_test_withhold_tag_ack`): the tag's
    // invalidation is reported unconfirmed, so `unmap_private_page` has to retain
    // its frames and record the tag while returning its error. Proven here:
    //
    //   * the frames are retained, not freed and not quarantined;
    //   * the queue entry is pending and the tag reads unconfirmed;
    //   * the reaper cannot release anything while the window lasts;
    //   * once the acknowledgement resumes, the reaper releases exactly those
    //     frames — no more, no fewer — and the tag reads confirmed again.
    //
    // The seam reports the invalidation unconfirmed rather than a real remote hart
    // stalling: the deferred branch must be reachable on a one-hart boot, where no
    // remote exists to stall at all.
    let mut deferred_detail = (0usize, 0usize, 0usize, false, false, false, false, false);
    let deferred_release = (|| {
        use crate::memory::deferred_release as queue;
        let mut builder = AddressSpaceBuilder::new();
        let space = builder
            .map_user_page(PRIVATE_PAGE, MappingKind::Private, flags())
            .and_then(|()| builder.build())
            .ok()?;
        let tag = space.asid();
        let frames_before = used_frames()?;
        let depth_before = queue::deferred_release_depth();
        let queued_frames_before = queue::deferred_release_frames();
        let attempts_before = queue::deferred_release_attempts();

        crate::memory::tlb_shootdown::set_test_withhold_tag_ack(true);
        let unconfirmed_error = space.unmap_private_page(PRIVATE_PAGE)
            == Err(AddressSpaceError::InvalidationUnacknowledged);
        let depth_after = queue::deferred_release_depth();
        let retained = queue::deferred_release_frames() - queued_frames_before;
        let queued = depth_after == depth_before + 1 && retained > 0;
        let unconfirmed = queue::tag_invalidation_unconfirmed(tag);
        let retained_not_freed = used_frames() == Some(frames_before);
        let not_quarantined = quarantined_frame_count() == 0;

        // The reaper may not release it while the acknowledgement is missing. Two
        // full reissue cycles: the reissue branch must not confirm the tag either.
        for _ in 0..64 {
            queue::reap_deferred_releases();
        }
        let still_pending = queue::deferred_release_depth() == depth_after;
        let attempted = queue::deferred_release_attempts() > attempts_before;
        let reaper_released_nothing = used_frames() == Some(frames_before);

        // Acknowledgement resumes.
        crate::memory::tlb_shootdown::set_test_withhold_tag_ack(false);
        let mut released = false;
        let deadline = hal::common::timer::read_mtime() + 40 * hal::common::timer::TICKS_PER_10MS;
        while hal::common::timer::read_mtime() < deadline {
            queue::reap_deferred_releases();
            if queue::deferred_release_depth() == depth_before {
                released = true;
                break;
            }
            let slice = hal::common::timer::read_mtime() + hal::common::timer::TICKS_PER_10MS;
            while hal::common::timer::read_mtime() < slice {
                core::hint::spin_loop();
            }
        }
        let released_exactly =
            used_frames().and_then(|now| frames_before.checked_sub(now)) == Some(retained);
        let tag_confirmed = !queue::tag_invalidation_unconfirmed(tag);
        let quiet = quarantined_frame_count() == 0
            && queue::deferred_release_abandoned() == 0
            && queue::deferred_release_abandoned_frames() == 0
            && queue::leaked_stack_frames() == 0;
        deferred_detail = (
            retained,
            depth_after,
            queue::deferred_release_attempts() - attempts_before,
            unconfirmed_error,
            queued && unconfirmed,
            still_pending && attempted && reaper_released_nothing,
            released && released_exactly && tag_confirmed,
            quiet,
        );
        Some(
            unconfirmed_error
                && queued
                && unconfirmed
                && retained_not_freed
                && not_quarantined
                && still_pending
                && attempted
                && reaper_released_nothing
                && released
                && released_exactly
                && tag_confirmed
                && quiet,
        )
    })()
    .unwrap_or(false);
    if deferred_release {
        log::info!(
            "[aspace] deferred release confirmed: retained={} frames depth={} attempts={} released=true quarantined={}",
            deferred_detail.0,
            deferred_detail.1,
            deferred_detail.2,
            quarantined_frame_count()
        );
        log::info!("S22-RV64-DEFERRED-RELEASE: PASS");
    } else {
        log::error!(
            "S22-RV64-DEFERRED-RELEASE: FAIL retained={} depth={} attempts={} withheld_error={} \
             queued={} pending_through_window={} released={} quiet={}",
            deferred_detail.0,
            deferred_detail.1,
            deferred_detail.2,
            deferred_detail.3,
            deferred_detail.4,
            deferred_detail.5,
            deferred_detail.6,
            deferred_detail.7
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
