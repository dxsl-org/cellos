//! Host tests for the retirement verdict table.

use super::*;

/// Empty the table. Callers hold `crate::TEST_STATE_LOCK`.
pub(crate) fn reset() {
    let mut table = TABLE.lock();
    table.slots = [None; MAX_RECORDS];
    table.next_seq = 0;
}

fn guard() -> impl Drop {
    let g = crate::TEST_STATE_LOCK.lock();
    reset();
    g
}

fn owner(n: u64) -> CellOwner {
    CellOwner::new(0x40 + n, 7, 1000 + n)
}

fn state(record: &ViRetirementV1) -> Option<RetirementState> {
    record.retirement_state()
}

#[test]
fn classification_matrix() {
    let clean = RetirementTally { member_threads: 2, grants_reclaimed: 3, ..Default::default() };
    assert_eq!(classify(&clean), (RetirementState::Contained, 0));

    let borrow = RetirementTally { writable_borrows: 1, ..Default::default() };
    assert_eq!(classify(&borrow), (RetirementState::Contained, reason::WRITABLE_BORROW));

    let dma = RetirementTally { dma_deferred: 1, ..Default::default() };
    assert_eq!(classify(&dma), (RetirementState::Quarantined, reason::DMA_DEFERRED));

    for tracked in [
        RetirementTally { quarantined_pages: 4, ..Default::default() },
        RetirementTally { pins_quarantined: 1, ..Default::default() },
        RetirementTally { vfs_leases_pending: 1, ..Default::default() },
    ] {
        assert_eq!(classify(&tracked), (RetirementState::Quarantined, reason::TRACKED_QUARANTINE));
    }

    let shm = RetirementTally { shm_frames_released: 1, dma_deferred: 1, ..Default::default() };
    assert_eq!(
        classify(&shm),
        (RetirementState::Unproven, reason::SHM_UNTRACKED | reason::DMA_DEFERRED)
    );

    let leak = RetirementTally { leaked_pages: 2, pins_quarantined: 1, ..Default::default() };
    assert_eq!(
        classify(&leak),
        (RetirementState::Unproven, reason::QUARANTINE_OVERFLOW | reason::TRACKED_QUARANTINE)
    );
}

#[test]
fn record_is_pending_until_closed_then_final() {
    let _g = guard();
    let o = owner(1);
    assert!(open(o, &[50]));
    let pending = query(50, false, o.root_tid as usize).unwrap();
    assert_eq!(state(&pending), Some(RetirementState::Pending));
    assert_eq!((pending.cell_id, pending.generation), (o.cell_id, o.generation));

    let tally = RetirementTally {
        member_threads: 3,
        remote_hart_mask: 0b10,
        writable_borrows: 1,
        shm_frames_released: 2,
        ..Default::default()
    };
    close(o, tally);
    let done = query(50, false, o.root_tid as usize).unwrap();
    assert_eq!(state(&done), Some(RetirementState::Unproven));
    assert_eq!(done.reasons, reason::SHM_UNTRACKED | reason::WRITABLE_BORROW);
    assert_eq!((done.member_threads, done.remote_hart_mask), (3, 0b10));
    assert_eq!(done.shm_frames_released, 2);
    assert_eq!(ViRetirementV1::decode(&done.encode()), Ok(done));
}

#[test]
fn only_watchers_or_supervisors_may_read() {
    let _g = guard();
    let o = owner(2);
    assert!(open(o, &[50, 51]));
    close(o, RetirementTally::default());
    let root = o.root_tid as usize;
    assert!(query(51, false, root).is_ok(), "watcher");
    assert!(query(99, true, root).is_ok(), "supervisor");
    assert_eq!(query(99, false, root), Err(QueryError::Denied));
    assert_eq!(query(50, false, root + 1), Err(QueryError::NotFound));
}

#[test]
fn watchers_beyond_the_bound_are_not_admitted() {
    let _g = guard();
    let o = owner(3);
    let many: [usize; MAX_WATCHERS + 1] = core::array::from_fn(|i| 200 + i);
    assert!(open(o, &many));
    let root = o.root_tid as usize;
    assert!(query(200 + MAX_WATCHERS - 1, false, root).is_ok());
    assert_eq!(query(200 + MAX_WATCHERS, false, root), Err(QueryError::Denied));
}

#[test]
fn pending_records_are_never_evicted() {
    let _g = guard();
    for n in 0..MAX_RECORDS as u64 {
        assert!(open(owner(n), &[50]));
    }
    assert!(!open(owner(500), &[50]), "every slot pending: not recorded");
    assert_eq!(query(50, false, owner(500).root_tid as usize), Err(QueryError::NotFound));
    for n in 0..MAX_RECORDS as u64 {
        assert!(query(50, false, owner(n).root_tid as usize).is_ok());
    }
}

#[test]
fn full_table_evicts_the_oldest_final_record() {
    let _g = guard();
    for n in 0..MAX_RECORDS as u64 {
        assert!(open(owner(n), &[50]));
    }
    close(owner(9), RetirementTally::default());
    close(owner(4), RetirementTally::default());
    assert!(open(owner(500), &[50]));
    assert_eq!(query(50, false, owner(9).root_tid as usize), Err(QueryError::NotFound));
    assert!(query(50, false, owner(4).root_tid as usize).is_ok(), "closed later, kept");
    assert!(open(owner(501), &[50]));
    assert_eq!(query(50, false, owner(4).root_tid as usize), Err(QueryError::NotFound));
    assert!(!open(owner(502), &[50]), "only pending records remain");
}

#[test]
fn closing_an_unknown_owner_is_a_no_op() {
    let _g = guard();
    let o = owner(5);
    close(o, RetirementTally { leaked_pages: 1, ..Default::default() });
    assert_eq!(query(50, true, o.root_tid as usize), Err(QueryError::NotFound));

    assert!(open(o, &[50]));
    close(o, RetirementTally::default());
    close(o, RetirementTally { leaked_pages: 1, ..Default::default() });
    let record = query(50, false, o.root_tid as usize).unwrap();
    assert_eq!(state(&record), Some(RetirementState::Contained), "final record is not rewritten");
}
