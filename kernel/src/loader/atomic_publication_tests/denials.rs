use super::baseline::with_populated_baseline;
use super::snapshot::{
    denied_with_full_rollback, denied_with_governed_rollback, denied_with_logged_denial,
    platform_singleton_denial,
    denied_without_frame_allocator,
};

const NO_AUDIT_RECORDS: usize = 0;
const LOGGED_DENIAL_AUDIT_RECORDS: usize = 1;
const GOVERNED_DENIAL_AUDIT_RECORDS: usize = 2;
const PLATFORM_SINGLETON_AUDIT_RECORDS: usize = 4;

fn record(case: &'static str, failures: &mut usize, contract: impl FnOnce() -> bool) {
    if contract() {
        log::info!("ATOMIC_PUBLICATION_{}: PASS", case);
    } else {
        *failures += 1;
        log::error!("ATOMIC_PUBLICATION_{}: FAIL", case);
    }
}

pub(super) fn run() -> usize {
    let mut failures = 0;
    record("AP-00", &mut failures, || {
        with_populated_baseline("AP-00", LOGGED_DENIAL_AUDIT_RECORDS, || {
            denied_with_logged_denial("AP-00", || {
                super::super::spawn_gated(
                    &[],
                    "/bin/atomic-malformed",
                    super::super::SpawnRequest::governed_boot(),
                )
                .map(|_| ())
            })
        })
    });
    for case in ["AP-01", "AP-02", "AP-03", "AP-04", "AP-RF", "AP-RM", "AP-RW"] {
        record(case, &mut failures, || {
            with_populated_baseline(case, NO_AUDIT_RECORDS, || {
                denied_with_full_rollback(case, || {
                    crate::task::prepare_elf_task(
                        crate::INIT_ELF,
                        "atomic-denial",
                        types::CellId(0),
                        alloc::vec::Vec::new(),
                    )
                    .map(|_| ())
                })
            })
        });
    }
    record("AP-FA", &mut failures, || {
        with_populated_baseline("AP-FA", NO_AUDIT_RECORDS, || {
            denied_without_frame_allocator(|| {
                crate::task::prepare_elf_task(
                    crate::INIT_ELF, "atomic-no-allocator", types::CellId(0),
                    alloc::vec::Vec::new(),
                ).map(|_| ())
            })
        })
    });
    record("AP-05", &mut failures, || {
        with_populated_baseline(
            "AP-05",
            PLATFORM_SINGLETON_AUDIT_RECORDS,
            platform_singleton_denial,
        )
    });
    record("AP-06", &mut failures, || {
        with_populated_baseline("AP-06", GOVERNED_DENIAL_AUDIT_RECORDS, || {
            denied_with_governed_rollback("AP-06", 2, || {
                super::spawn_governed_platform(super::super::SpawnRequest::governed_boot())
                    .map(|_| ())
            })
        })
    });
    for case in ["AP-07", "AP-08"] {
        record(case, &mut failures, || {
            with_populated_baseline(case, NO_AUDIT_RECORDS, || {
                denied_with_full_rollback(case, || {
                    super::super::spawn_trusted_init(crate::INIT_ELF).map(|_| ())
                })
            })
        });
    }
    record("AP-09", &mut failures, || {
        with_populated_baseline("AP-09", LOGGED_DENIAL_AUDIT_RECORDS, || {
            denied_with_governed_rollback("AP-09", 1, || {
                super::super::spawn_from_path(
                    "/bin/vfs",
                    super::super::SpawnRequest::governed_boot(),
                )
                .map(|_| ())
            })
        })
    });
    record("AP-10", &mut failures, || {
        with_populated_baseline("AP-10", GOVERNED_DENIAL_AUDIT_RECORDS, || {
            denied_with_governed_rollback("AP-10", 2, || {
                let mut request = super::super::SpawnRequest::governed_boot();
                request.priority = u8::MAX;
                super::spawn_governed_platform(request).map(|_| ())
            })
        })
    });
    record("AP-11", &mut failures, || {
        with_populated_baseline("AP-11", GOVERNED_DENIAL_AUDIT_RECORDS, || {
            denied_with_governed_rollback("AP-11", 2, || {
                let mut request = super::super::SpawnRequest::governed_boot();
                request.replacement =
                    Some(crate::cell::hotswap::ReplacementReservation::invalid_for_test());
                super::spawn_governed_platform(request).map(|_| ())
            })
        })
    });
    record("AP-PM", &mut failures, measurement_reservation_contract);
    record("AP-PR", &mut failures, ready_reservation_contract);
    record("AP-PT", &mut failures, task_table_reservation_contract);
    record("AP-PA", &mut failures, authority_reservation_contract);
    record("AP-PH", &mut failures, session_header_contract);
    record("AP-PL", &mut failures, session_ledger_contract);
    record("AP-PU", &mut failures, session_unbound_receipt_contract);
    record("AP-PS", &mut failures, session_owner_contract);
    record("AP-PC", &mut failures, measurement_creator_share_contract);
    record("AP-PF", &mut failures, ready_rollback_fifo_contract);
    record("AP-PK", &mut failures, session_cancel_contract);
    record("AP-PB", &mut failures, session_close_scope_contract);
    record("AP-PE", &mut failures, session_reclaim_contract);
    record("AP-PI", &mut failures, session_issuer_replacement_contract);
    record("AP-PQ", &mut failures, session_query_child_contract);
    record("AP-PG", &mut failures, session_frontend_receipt_contract);
    record("AP-PX", &mut failures, serial_raw_relay_contract);
    record("AP-PZ", &mut failures, crate::task::syscall::test_raw_shutdown_gate);
    failures
}

/// The measurement-log promise is charged before publication, never stolen by
/// ordinary launches, consumed exactly once by commit without allocating, and
/// released without touching the aggregate when dropped.
fn measurement_reservation_contract() -> bool {
    use crate::measurement_log as log;
    const HASH_A: [u8; 32] = [0xA1; 32];
    const HASH_B: [u8; 32] = [0xB2; 32];
    const CREATOR: log::Creator = (u64::MAX - 1, 0x5E55_2000);
    let (len0, _, promises0, maximum) = log::test_shape();
    if promises0 != 0 || len0 + 4 >= maximum {
        // Too close to the documented maximum to exercise exhaustion safely.
        return true;
    }
    let aggregate0 = log::aggregate();
    let Ok(dropped) = log::PublicationReservation::reserve(CREATOR) else { return false };
    let (_, capacity, promises, _) = log::test_shape();
    if promises != 1 || capacity < len0 + 1 { return false; }
    drop(dropped);
    if log::test_shape().2 != 0 || log::entry_count() != len0 || log::aggregate() != aggregate0 {
        return false;
    }
    let (Ok(first), Ok(second)) = (log::PublicationReservation::reserve(CREATOR), log::PublicationReservation::reserve(CREATOR))
    else { return false };
    // An ordinary launch appends around the promises and keeps their capacity.
    log::commit_staged(0x7ff0_0001, alloc::string::String::from("/atomic/ordinary"), HASH_A);
    let (len1, capacity1, promises1, _) = log::test_shape();
    if len1 != len0 + 1 || promises1 != 2 || capacity1 < len1 + promises1 { return false; }
    let aggregate1 = log::aggregate();
    first.commit(0x7ff0_0002, alloc::string::String::from("/atomic/reserved"), HASH_B);
    let (len2, capacity2, promises2, _) = log::test_shape();
    if len2 != len1 + 1 || promises2 != 1 || capacity2 != capacity1 || log::aggregate() == aggregate1 {
        return false;
    }
    drop(second);
    if log::test_shape().2 != 0 { return false; }
    // At the documented maximum, preparation is refused and ordinary launches
    // cannot steal a promised slot (their aggregate still advances).
    let mut held = alloc::vec::Vec::new();
    for n in 0..8u64 {
        // Each creator is bounded by its own share, so filling the log takes several.
        let creator = (u64::MAX - 100 - n, 0x5E55_2000);
        while let Ok(reservation) = log::PublicationReservation::reserve(creator) {
            held.push(reservation);
        }
        let (len, _, promises, _) = log::test_shape();
        if len + promises == maximum { break; }
    }
    let (len3, _, promises3, _) = log::test_shape();
    let exhausted = len3 + promises3 == maximum && promises3 == held.len();
    let aggregate3 = log::aggregate();
    log::commit_staged(0x7ff0_0003, alloc::string::String::from("/atomic/refused"), HASH_A);
    let stolen = log::entry_count() != len3 || log::test_shape().2 != promises3;
    let advanced = log::aggregate() != aggregate3;
    drop(held);
    exhausted && !stolen && advanced && log::test_shape().2 == 0
}

/// A promised ready slot is pinned to its destination hart, survives ordinary
/// growth and stealing, commits without growing the queue, and leaves no queue
/// key behind when dropped.
fn ready_reservation_contract() -> bool {
    use crate::task::hart_local::{ready, HART_LOCALS};
    const BASE: usize = 91_000;
    if crate::task::smp::is_rt_hart_online() || ready::total_ready_count() != 0 {
        return true;
    }
    let normal = api::TaskPriority::Normal as u8;
    let cleanup = |tids: &[usize]| {
        for tid in tids {
            ready::remove_from_all(*tid);
        }
    };
    let Ok(reservation) = ready::PublicationReservation::reserve(normal) else { return false };
    let (hart, priority) = reservation.destination();
    if hart >= crate::task::smp::MAX_HARTS { return false; }
    let covers = |promises: usize| {
        let (len, capacity, publication, rollback) = ready::test_shape(hart, priority);
        publication == promises && rollback == 0 && capacity >= len + publication + rollback
    };
    if !covers(1) { return false; }
    let mut pushed = alloc::vec::Vec::new();
    for n in 0..40 {
        ready::push_on_hart(hart, BASE + n, priority);
        pushed.push(BASE + n);
        if !covers(1) {
            drop(reservation);
            cleanup(&pushed);
            return false;
        }
    }
    let (len_before, capacity_before, _, _) = ready::test_shape(hart, priority);
    reservation.commit(BASE + 99);
    pushed.push(BASE + 99);
    let (len_after, capacity_after, publication, _) = ready::test_shape(hart, priority);
    let tail = HART_LOCALS[hart].ready.lock().get(&priority).and_then(|queue| queue.back().copied());
    cleanup(&pushed);
    if len_after != len_before + 1 || capacity_after != capacity_before || publication != 0
        || tail != Some(BASE + 99) || ready::total_ready_count() != 0 {
        return false;
    }
    // Dropping a reservation releases the promise and any key it created.
    let before = HART_LOCALS[hart].ready.lock().clone();
    let Ok(transient) = ready::PublicationReservation::reserve(5) else { return false };
    let (transient_hart, transient_priority) = transient.destination();
    let promised = ready::test_shape(transient_hart, transient_priority).2;
    drop(transient);
    if promised != 1 || ready::test_shape(transient_hart, transient_priority).2 != 0
        || transient_hart != hart || *HART_LOCALS[hart].ready.lock() != before {
        return false;
    }
    // Stealing into the promised bucket keeps headroom for the promise, moves
    // exactly (stealable / 2).max(1) in victim order, and leaves the rest queued.
    let Ok(held) = ready::PublicationReservation::reserve(normal) else { return false };
    let victim = 1 - hart;
    let mut victims = alloc::vec::Vec::new();
    for n in 0..8 {
        ready::push_on_hart(victim, BASE + 200 + n, normal);
        victims.push(BASE + 200 + n);
    }
    ready::steal_from_busiest(hart);
    let moved: alloc::vec::Vec<usize> = HART_LOCALS[hart].ready.lock().get(&normal)
        .map(|queue| queue.iter().copied().collect()).unwrap_or_default();
    let remaining = HART_LOCALS[victim].ready.lock().get(&normal).map_or(0, |queue| queue.len());
    let (len, capacity, publication, _) = ready::test_shape(hart, normal);
    let shape_ok = publication == 1 && capacity >= len + publication;
    let order_ok = moved == [BASE + 200, BASE + 201, BASE + 202, BASE + 203] && remaining == 4;
    let (_, capacity_before_commit, _, _) = ready::test_shape(hart, normal);
    held.commit(BASE + 300);
    victims.push(BASE + 300);
    let grew = ready::test_shape(hart, normal).1 != capacity_before_commit;
    cleanup(&victims);
    shape_ok && order_ok && !grew && ready::total_ready_count() == 0
}

/// Reserved task-table capacity keeps a prepared task invisible, survives
/// ordinary insertions, and publishes without growing the table.
fn task_table_reservation_contract() -> bool {
    const FIRST: usize = 77_001;
    const SECOND: usize = 77_002;
    let task = |tid: usize| alloc::boxed::Box::new(crate::task::Task::new(
        tid, types::CellId(0), "reservation", alloc::vec::Vec::new()));
    let mut guard = crate::task::SCHEDULER.lock();
    let Some(sched) = guard.as_mut() else { return false };
    let (rows0, _, reservations0) = sched.tasks.test_shape();
    if reservations0 != 0 || sched.tasks.contains_key(&FIRST) || sched.tasks.contains_key(&SECOND) {
        return false;
    }
    if sched.tasks.reserve_publication().is_err() { return false; }
    let invisible = !sched.tasks.contains_key(&SECOND);
    sched.tasks.insert(FIRST, task(FIRST));
    let (rows1, capacity1, reservations1) = sched.tasks.test_shape();
    let covered = rows1 == rows0 + 1 && reservations1 == 1 && capacity1 >= rows1 + reservations1;
    sched.tasks.publish_reserved(SECOND, task(SECOND));
    let (rows2, capacity2, reservations2) = sched.tasks.test_shape();
    let published = rows2 == rows0 + 2 && reservations2 == 0 && capacity2 == capacity1
        && sched.tasks.contains_key(&SECOND);
    sched.tasks.remove(&FIRST);
    sched.tasks.remove(&SECOND);
    let reserved = sched.tasks.reserve_publication().is_ok();
    sched.tasks.abort_publication();
    let (rows3, _, reservations3) = sched.tasks.test_shape();
    invisible && covered && published && reserved && rows3 == rows0 && reservations3 == 0
}

/// A prepared CSpace authority is addressable only after commit, commits without
/// growing the table, and an aborted reservation releases its promise.
fn authority_reservation_contract() -> bool {
    use crate::cell::cspace::{retire_cell_authority, AuthorityReservation, CELL_AUTHORITIES};
    let cell = types::CellId(62);
    let generation = 0x5EA1_0001u64;
    let key = (cell.0, generation);
    let (rows0, _, reservations0) = CELL_AUTHORITIES.lock().test_shape();
    if reservations0 != 0 || CELL_AUTHORITIES.lock().get_mut(&key).is_some() { return false; }
    let Ok(aborted) = AuthorityReservation::reserve(cell, generation, crate::task::cap::CapSet::EMPTY)
    else { return false };
    let addressable = CELL_AUTHORITIES.lock().get_mut(&key).is_some();
    let promised = CELL_AUTHORITIES.lock().test_shape().2;
    let hidden = !addressable && promised == 1;
    drop(aborted);
    let (rows1, _, reservations1) = CELL_AUTHORITIES.lock().test_shape();
    let Ok(reservation) = AuthorityReservation::reserve(cell, generation, crate::task::cap::CapSet::EMPTY)
    else { return false };
    let (_, capacity_before, _) = CELL_AUTHORITIES.lock().test_shape();
    reservation.commit();
    let (rows2, capacity_after, reservations2) = CELL_AUTHORITIES.lock().test_shape();
    let published = CELL_AUTHORITIES.lock().get_mut(&key).is_some();
    retire_cell_authority(cell, generation);
    hidden && rows1 == rows0 && reservations1 == 0
        && rows2 == rows0 + 1 && capacity_after == capacity_before && reservations2 == 0 && published
}

/// The real transport entry rejects a zero correlation ID as a header-only
/// syntax error (echoing zero), and denies a nonzero request from a task that is
/// not a live Cell before any owner state is consulted.
fn session_header_contract() -> bool {
    use api::cell_session::{ResultHeaderV1, Status, HEADER_LEN};
    let mut request = [0u8; HEADER_LEN];
    request[0..4].copy_from_slice(&1u32.to_le_bytes());
    request[4..6].copy_from_slice(&13u16.to_le_bytes());
    request[8..12].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    let mut result = [0u8; 64];
    let len = crate::task::cell_session::dispatch(usize::MAX, &request, &mut result);
    let zero_id = len == HEADER_LEN
        && ResultHeaderV1::decode(&result[..len])
            .is_ok_and(|h| h.status == Status::Malformed && h.request_id == 0 && h.selector == 13);
    request[16..24].copy_from_slice(&7u64.to_le_bytes());
    let len = crate::task::cell_session::dispatch(usize::MAX, &request, &mut result);
    let denied = len == HEADER_LEN
        && ResultHeaderV1::decode(&result[..len])
            .is_ok_and(|h| h.status == Status::Denied && h.request_id == 7 && h.selector == 13);
    zero_id && denied
}

fn session_call(tid: usize, selector: u16, id: u64, body: &[u8])
    -> (api::cell_session::Status, alloc::vec::Vec<u8>) {
    let mut request = alloc::vec::Vec::new();
    request.extend_from_slice(&1u32.to_le_bytes());
    request.extend_from_slice(&selector.to_le_bytes());
    request.extend_from_slice(&0u16.to_le_bytes());
    request.extend_from_slice(&((api::cell_session::HEADER_LEN + body.len()) as u32).to_le_bytes());
    request.extend_from_slice(&0u32.to_le_bytes());
    request.extend_from_slice(&id.to_le_bytes());
    request.extend_from_slice(body);
    let mut result = [0u8; 1536];
    let len = crate::task::cell_session::dispatch(tid, &request, &mut result);
    // Mirror the syscall layer: the reply was copied out, so it is observed.
    crate::task::cell_session::delivered(tid, &request);
    match api::cell_session::ResultHeaderV1::decode(&result[..len]) {
        Ok(header) => (header.status, result[api::cell_session::HEADER_LEN..len].to_vec()),
        Err(status) => (status, alloc::vec::Vec::new()),
    }
}

/// Real `dispatch` against two live synthetic Cells: retained-outcome replay and
/// request-ID fence, exact-holder checks, partial IO that keeps WouldBlock, EOF
/// and Cancelled distinct, acknowledgement, and subtree cancellation.
fn session_owner_contract() -> bool {
    use api::cell_session::{ObjectRefV1 as Ref, Status};
    use crate::task::thread_cap_selftest::{insert, mk_task, remove};
    const OWNER_TID: usize = 92_001;
    const FOREIGN_TID: usize = 92_002;
    const GENERATION: u64 = 0x5E55_1001;
    let mut cells = [0u64; 2];
    {
        let guard = crate::task::SCHEDULER.lock();
        let Some(sched) = guard.as_ref() else { return false };
        let mut found = 0;
        for raw in (1..crate::memory::cell_quota::MAX_CELLS).rev() {
            if found < 2 && sched.cell_owner_slot_is_empty(types::CellId(raw as u64)) {
                cells[found] = raw as u64;
                found += 1;
            }
        }
        if found < 2 { return false; }
    }
    for (tid, cell) in [(OWNER_TID, cells[0]), (FOREIGN_TID, cells[1])] {
        let mut task = mk_task(tid, cell);
        task.cell_generation = GENERATION;
        insert(task);
    }
    let mut ok = true;
    let refs = |bytes: &[u8]| Ref::from_bytes(bytes).unwrap_or(Ref::ZERO);
    // Root operation creation: zero is the correlation scope; a repeated ID
    // replays its exact outcome and mints nothing.
    let (status, first) = session_call(OWNER_TID, 1, 1, &Ref::ZERO.to_bytes());
    let operation = refs(&first);
    ok &= status == Status::Ok && operation != Ref::ZERO;
    let (status, replay) = session_call(OWNER_TID, 1, 1, &Ref::ZERO.to_bytes());
    ok &= status == Status::Ok && replay == first;
    let (status, second) = session_call(OWNER_TID, 1, 2, &Ref::ZERO.to_bytes());
    ok &= status == Status::Ok && refs(&second).id == operation.id + 1;
    let (status, _) = session_call(OWNER_TID, 1, 5, &Ref::ZERO.to_bytes());
    ok &= status == Status::UnknownRequest;
    // Stream creation, with a changed body for a used ID refused.
    let mut create = alloc::vec::Vec::new();
    create.extend_from_slice(&operation.to_bytes());
    create.extend_from_slice(&64u32.to_le_bytes());
    create.extend_from_slice(&0u32.to_le_bytes());
    let (status, streams) = session_call(OWNER_TID, 2, 1, &create);
    let (reader, writer) = (refs(streams.get(..16).unwrap_or(&[])), refs(streams.get(16..32).unwrap_or(&[])));
    ok &= status == Status::Ok && reader != Ref::ZERO && writer != Ref::ZERO;
    let mut changed = create.clone();
    changed[16..20].copy_from_slice(&128u32.to_le_bytes());
    ok &= session_call(OWNER_TID, 2, 1, &changed).0 == Status::Malformed;
    let io = |endpoint: Ref, count: u32, deadline: u64, bytes: &[u8]| {
        let mut body = alloc::vec::Vec::new();
        body.extend_from_slice(&endpoint.to_bytes());
        body.extend_from_slice(&count.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&deadline.to_le_bytes());
        body.extend_from_slice(bytes);
        body
    };
    // Partial progress is reported, never replayed.
    let (status, wrote) = session_call(OWNER_TID, 6, 2, &io(writer, 5, 0, b"hello"));
    ok &= status == Status::Ok && wrote.get(..4) == Some(&5u32.to_le_bytes()[..]);
    let (status, read) = session_call(OWNER_TID, 5, 3, &io(reader, 16, 0, &[]));
    ok &= status == Status::Ok && read.get(..4) == Some(&5u32.to_le_bytes()[..]) && read.get(8..13) == Some(&b"hello"[..]);
    // Nonblocking empty read is WouldBlock; EOF is a distinct status later.
    ok &= session_call(OWNER_TID, 5, 4, &io(reader, 16, 0, &[])).0 == Status::WouldBlock;
    // Direction is part of the right: writing through the read end is refused.
    ok &= session_call(OWNER_TID, 6, 5, &io(reader, 1, 0, b"x")).0 == Status::WrongDirection;
    // A foreign Cell holds nothing: its own ledger, exact-holder denial.
    ok &= session_call(FOREIGN_TID, 5, 1, &io(reader, 16, 0, &[])).0 == Status::WrongOwner;
    ok &= session_call(OWNER_TID, 7, 6, &writer.to_bytes()).0 == Status::Ok;
    ok &= session_call(OWNER_TID, 5, 7, &io(reader, 16, 0, &[])).0 == Status::Eof;
    // QueryRequest returns the original outcome of a pending-looking read.
    let mut query = alloc::vec::Vec::new();
    query.extend_from_slice(&operation.to_bytes());
    query.extend_from_slice(&4u64.to_le_bytes());
    ok &= session_call(OWNER_TID, 28, 100, &query).0 == Status::WouldBlock;
    // Acknowledgement retires outcomes and advances the fence: a retired ID is
    // never new again.
    let mut ack = alloc::vec::Vec::new();
    ack.extend_from_slice(&operation.to_bytes());
    ack.extend_from_slice(&7u64.to_le_bytes());
    ok &= session_call(OWNER_TID, 25, 101, &ack).0 == Status::Ok;
    // (The acknowledged operation reclaimed the closed writer record, so the
    // retired-ID probe goes through the still-open reader.)
    ok &= session_call(OWNER_TID, 5, 2, &io(reader, 16, 0, &[])).0 == Status::UnknownRequest;
    // Cancellation revokes the holder subtree: the typed state is Cancelled.
    let mut cancel = alloc::vec::Vec::new();
    cancel.extend_from_slice(&operation.to_bytes());
    cancel.extend_from_slice(&2u32.to_le_bytes());
    cancel.extend_from_slice(&0u32.to_le_bytes());
    ok &= session_call(OWNER_TID, 8, 8, &cancel).0 == Status::Ok;
    ok &= session_call(OWNER_TID, 5, 9, &io(reader, 16, 0, &[])).0 == Status::Cancelled;
    let (status, state) = session_call(OWNER_TID, 9, 102, &operation.to_bytes());
    ok &= status == Status::Ok && state.get(..4) == Some(&1u32.to_le_bytes()[..]);
    for (tid, cell) in [(OWNER_TID, cells[0]), (FOREIGN_TID, cells[1])] {
        remove(tid);
        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
            crate::task::cell_session::retired(sched, api::cell_owner::CellOwner::new(cell, GENERATION, tid as u64));
        }
    }
    ok
}

/// Two fresh live synthetic Cells for the ledger/receipt cases.
fn two_session_cells(tids: [usize; 2], generation: u64) -> Option<[u64; 2]> {
    use crate::task::thread_cap_selftest::{insert, mk_task};
    let mut cells = [0u64; 2];
    {
        let guard = crate::task::SCHEDULER.lock();
        let sched = guard.as_ref()?;
        let mut found = 0;
        for raw in (1..crate::memory::cell_quota::MAX_CELLS).rev() {
            if found < 2 && sched.cell_owner_slot_is_empty(types::CellId(raw as u64)) {
                cells[found] = raw as u64;
                found += 1;
            }
        }
        if found < 2 { return None; }
    }
    for (tid, cell) in tids.into_iter().zip(cells) {
        let mut task = mk_task(tid, cell);
        task.cell_generation = generation;
        insert(task);
    }
    Some(cells)
}

fn retire_session_cells(tids: [usize; 2], cells: [u64; 2], generation: u64) {
    for (tid, cell) in tids.into_iter().zip(cells) {
        crate::task::thread_cap_selftest::remove(tid);
        if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
            crate::task::cell_session::retired(sched, api::cell_owner::CellOwner::new(cell, generation, tid as u64));
        }
    }
}

/// Request-ID ledgers are scoped only after the sender is authorised in an
/// existing operation: a Cell guessing sequential references (existing or not)
/// is refused without minting a ledger or consuming an outcome slot, so it can
/// never exhaust the shared table; and closing an operation reaps every ledger
/// scoped to it (ids never reuse, so no fence reopens).
fn session_ledger_contract() -> bool {
    use api::cell_session::{ObjectRefV1 as Ref, Status};
    const OWNER_TID: usize = 92_003;
    const GUESSER_TID: usize = 92_004;
    const GENERATION: u64 = 0x5E55_1002;
    let tids = [OWNER_TID, GUESSER_TID];
    let Some(cells) = two_session_cells(tids, GENERATION) else { return false };
    let count = |actor: Option<(u64, u64)>| {
        crate::task::SCHEDULER.lock().as_ref()
            .map_or(usize::MAX, |sched| crate::task::cell_session::test_hooks::ledgers(sched, actor))
    };
    let owner = Some((cells[0], GENERATION));
    let guesser = Some((cells[1], GENERATION));
    let refs = |bytes: &[u8]| Ref::from_bytes(bytes).unwrap_or(Ref::ZERO);
    let stream_body = |operation: Ref| {
        let mut body = alloc::vec::Vec::new();
        body.extend_from_slice(&operation.to_bytes());
        body.extend_from_slice(&64u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body
    };
    let mut ok = true;
    // The owner makes a real operation and a stream in it: ledgers (zero, op).
    let (status, created) = session_call(OWNER_TID, 1, 1, &Ref::ZERO.to_bytes());
    let operation = refs(&created);
    ok &= status == Status::Ok && operation != Ref::ZERO;
    let (status, streams) = session_call(OWNER_TID, 2, 1, &stream_body(operation));
    let (reader, writer) = (refs(streams.get(..16).unwrap_or(&[])), refs(streams.get(16..32).unwrap_or(&[])));
    ok &= status == Status::Ok && reader != Ref::ZERO && writer != Ref::ZERO;
    ok &= count(owner) == 2;
    let before_total = count(None);
    // The guesser walks sequential references well past the table size.
    let mut stale_only = true;
    for guess in 0..1200u64 {
        let guessed = Ref { id: operation.id + 1_000 + guess, generation: 1 };
        stale_only &= session_call(GUESSER_TID, 2, 1, &stream_body(guessed)).0 == Status::Stale;
        stale_only &= session_call(GUESSER_TID, 20, 1, &guessed.to_bytes()).0 == Status::Stale;
    }
    ok &= stale_only;
    // Guessing the owner's real operation (and endpoint) is a membership denial.
    ok &= session_call(GUESSER_TID, 2, 1, &stream_body(operation)).0 == Status::WrongOwner;
    ok &= session_call(GUESSER_TID, 1, 1, &operation.to_bytes()).0 == Status::WrongOwner;
    ok &= session_call(GUESSER_TID, 20, 1, &operation.to_bytes()).0 == Status::WrongOwner;
    ok &= session_call(GUESSER_TID, 7, 1, &reader.to_bytes()).0 == Status::WrongOwner;
    ok &= count(guesser) == 0 && count(None) <= before_total;
    // The guesser can still make its own operation; the table was never pinned.
    ok &= session_call(GUESSER_TID, 1, 1, &Ref::ZERO.to_bytes()).0 == Status::Ok;
    ok &= count(guesser) == 1;
    // Closing the owner's operation reaps its ledger, and a request naming the
    // removed operation can no longer recreate one.
    ok &= session_call(OWNER_TID, 7, 2, &reader.to_bytes()).0 == Status::Ok;
    ok &= session_call(OWNER_TID, 7, 3, &writer.to_bytes()).0 == Status::Ok;
    ok &= session_call(OWNER_TID, 20, 4, &operation.to_bytes()).0 == Status::Ok;
    ok &= count(owner) == 1;
    ok &= session_call(OWNER_TID, 2, 5, &stream_body(operation)).0 == Status::Stale;
    ok &= count(owner) == 1;
    retire_session_cells(tids, cells, GENERATION);
    ok
}

/// A child record with a receipt the preparer never bound must not commit (the
/// stale state-0 receipt would otherwise survive a running child); binding the
/// same receipt moves the refusal on to the next, unrelated precondition.
fn session_unbound_receipt_contract() -> bool {
    use api::cell_session::{ObjectRefV1 as Ref, Status};
    use crate::task::cell_session::test_hooks as hooks;
    const OWNER_TID: usize = 92_005;
    const OTHER_TID: usize = 92_006;
    const GENERATION: u64 = 0x5E55_1003;
    let tids = [OWNER_TID, OTHER_TID];
    let Some(cells) = two_session_cells(tids, GENERATION) else { return false };
    let (status, created) = session_call(OWNER_TID, 1, 1, &Ref::ZERO.to_bytes());
    let operation = Ref::from_bytes(&created).unwrap_or(Ref::ZERO);
    let mut ok = status == Status::Ok && operation != Ref::ZERO;
    // Staging and probing share one SCHEDULER hold: a maintenance pass on
    // another hart would (correctly) abort a state-0 record whose receipt
    // issuer is not the registered provider.
    let mut guard = crate::task::SCHEDULER.lock();
    if let Some(sched) = guard.as_mut() {
        if let Some(prepared) = hooks::stage_unbound_receipt(sched, cells[0], GENERATION, operation) {
            // Unbound receipt: the commit is refused before any mutation.
            ok &= hooks::commit(sched, prepared) == Some(Status::Denied);
            ok &= hooks::prepared_state(sched, prepared) == Some(0);
            // Bound receipt: that refusal is gone; the record has no parsed
            // context, so the next precondition (WouldBlock) now speaks.
            hooks::bind_receipt(sched, prepared);
            ok &= hooks::commit(sched, prepared) == Some(Status::WouldBlock);
            ok &= hooks::prepared_state(sched, prepared) == Some(0);
            hooks::discard(sched, prepared);
        } else {
            ok = false;
        }
    } else {
        ok = false;
    }
    drop(guard);
    retire_session_cells(tids, cells, GENERATION);
    ok
}

/// A context carrying the kind-6 InputFrontend grant (slot 8) commits only with
/// its own bound, unresolved receipt for that exact child and frontend: none,
/// another child's, or another frontend's receipt is `InvalidChild`, and
/// `BindOwnerReceipt` refuses another child's receipt outright.
fn session_frontend_receipt_contract() -> bool {
    use api::cell_session::{ObjectRefV1 as Ref, Status};
    use api::service_binding::ServiceBinding;
    use crate::task::cell_session::test_hooks as hooks;
    const OWNER_TID: usize = 92_017;
    const OTHER_TID: usize = 92_018;
    const GENERATION: u64 = 0x5E55_1009;
    const FRONTEND: Ref = Ref { id: u64::MAX - 6, generation: 1 };
    const FOREIGN: Ref = Ref { id: u64::MAX - 7, generation: 1 };
    let tids = [OWNER_TID, OTHER_TID];
    let Some(cells) = two_session_cells(tids, GENERATION) else { return false };
    let (status, created) = session_call(OWNER_TID, 1, 1, &Ref::ZERO.to_bytes());
    let operation = Ref::from_bytes(&created).unwrap_or(Ref::ZERO);
    let mut ok = status == Status::Ok && operation != Ref::ZERO;
    ok &= with_session_hold(|sched| {
        let a = hooks::stage_frontend_child(sched, cells[0], GENERATION, operation, 93_006_001, FRONTEND)?;
        let b = hooks::stage_frontend_child(sched, cells[0], GENERATION, operation, 93_006_002, FRONTEND)?;
        let (pa, pb) = (a.prepared_ref(), b.prepared_ref());
        let mut ok = true;
        // No receipt at all.
        ok &= hooks::commit(sched, pa) == Some(Status::InvalidChild);
        // A receipt registered for child B cannot be bound to child A.
        let rb = hooks::stage_frontend_receipt(sched, pb, b, FRONTEND)?;
        let mut bind = alloc::vec::Vec::new();
        bind.extend_from_slice(&pa.to_bytes());
        bind.extend_from_slice(&6u16.to_le_bytes());
        bind.extend_from_slice(&8u16.to_le_bytes());
        bind.extend_from_slice(&0u32.to_le_bytes());
        bind.extend_from_slice(&ServiceBinding::new(u64::MAX, u64::MAX, u64::MAX).to_bytes());
        bind.extend_from_slice(&rb.to_bytes());
        ok &= hooks::exec(sched, cells[0], GENERATION, 12, &bind).0 == Status::Denied;
        ok &= hooks::frontend_receipt(sched, pa) == Some(Ref::ZERO);
        // Even named in A's slot, B's receipt does not admit A's grant.
        hooks::force_frontend_bind(sched, pa, rb)?;
        ok &= hooks::commit(sched, pa) == Some(Status::InvalidChild);
        // A's own receipt for another frontend does not either.
        let foreign = hooks::stage_frontend_receipt(sched, pa, a, FOREIGN)?;
        hooks::force_frontend_bind(sched, pa, foreign)?;
        ok &= hooks::commit(sched, pa) == Some(Status::InvalidChild);
        // The exact receipt passes the frontend gate; the fixture has no
        // reserved publication, so the next precondition (Revoked) speaks.
        let exact = hooks::stage_frontend_receipt(sched, pa, a, FRONTEND)?;
        hooks::force_frontend_bind(sched, pa, exact)?;
        ok &= hooks::commit(sched, pa) == Some(Status::Revoked);
        ok &= hooks::prepared_state(sched, pa) == Some(0) && hooks::prepared_state(sched, pb) == Some(0);
        hooks::discard_all(sched, &[pa, pb]);
        Some(ok)
    });
    retire_session_cells(tids, cells, GENERATION);
    ok
}

type Sched = crate::task::scheduler::Scheduler;

/// Run `probe` inside one SCHEDULER hold: no maintenance pass on another hart
/// can interleave with the staged records and the production transitions.
fn with_session_hold(probe: impl FnOnce(&mut Sched) -> Option<bool>) -> bool {
    let mut guard = crate::task::SCHEDULER.lock();
    let Some(sched) = guard.as_mut() else { return false };
    probe(sched).unwrap_or(false)
}

fn create_stream_body(operation: api::cell_session::ObjectRefV1) -> alloc::vec::Vec<u8> {
    let mut body = alloc::vec::Vec::new();
    body.extend_from_slice(&operation.to_bytes());
    body.extend_from_slice(&64u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body
}

/// `n`th reference of a result payload.
fn result_ref(bytes: &[u8], n: usize) -> Option<api::cell_session::ObjectRefV1> {
    api::cell_session::ObjectRefV1::from_bytes(bytes.get(n * 16..n * 16 + 16)?).ok()
}

/// Create an operation (`parent` or zero for a root) as `(cell, generation)` through the
/// production transition.
fn stage_operation(sched: &mut Sched, cell: u64, generation: u64, parent: api::cell_session::ObjectRefV1)
    -> Option<api::cell_session::ObjectRefV1> {
    use crate::task::cell_session::test_hooks as hooks;
    let (status, bytes) = hooks::exec(sched, cell, generation, 1, &parent.to_bytes());
    if status != api::cell_session::Status::Ok { return None; }
    result_ref(&bytes, 0)
}

/// A creator never retains more than its share of the measurement log, yet a
/// long-lived creator is never locked out: past its share its own oldest
/// committed entry is evicted (the aggregate keeps advancing and no other
/// creator's or ordinary entry is touched). Only a creator whose outstanding
/// promises alone reach the share is refused, and a refusal evicts nothing.
fn measurement_creator_share_contract() -> bool {
    use crate::measurement_log as log;
    const HASH: [u8; 32] = [0xC3; 32];
    const EXTRA: usize = 10;
    let (len0, _, promises0, maximum) = log::test_shape();
    if promises0 != 0 || len0 + log::CREATOR_SHARE + 8 >= maximum {
        // Too close to the documented maximum to exercise the share safely.
        return true;
    }
    let greedy: log::Creator = (u64::MAX - 10, 0x5E55_2001);
    let other: log::Creator = (u64::MAX - 11, 0x5E55_2001);
    let label = |n: usize| alloc::format!("/atomic/share-{n}");
    // Another creator and an ordinary launch each hold one entry.
    let Ok(peer) = log::PublicationReservation::reserve(other) else { { ::log::error!("AP-PC early exit #1 entries={} shape={:?}", log::entry_count(), log::test_shape()); return false } };
    peer.commit(0x7ff0_0020, alloc::string::String::from("/atomic/share-other"), HASH);
    log::commit_staged(0x7ff0_0021, alloc::string::String::from("/atomic/share-ordinary"), HASH);
    let held_elsewhere = log::entry_count() == len0 + 2;
    // One creator keeps launching well past its share: every reserve succeeds,
    // every commit advances the aggregate, and it retains exactly its share.
    let mut advanced = true;
    for n in 0..log::CREATOR_SHARE + EXTRA {
        let Ok(reservation) = log::PublicationReservation::reserve(greedy) else { { ::log::error!("AP-PC early exit #2 entries={} shape={:?}", log::entry_count(), log::test_shape()); return false } };
        let before = log::aggregate();
        reservation.commit(0x7ff0_1000 + n, label(n), HASH);
        advanced &= log::aggregate() != before;
    }
    let retained = log::test_creator_used(greedy) == (0, log::CREATOR_SHARE)
        && log::entry_count() == len0 + 2 + log::CREATOR_SHARE;
    // The creator's OLDEST entries were the ones evicted.
    let oldest = log::test_oldest_path(greedy) == Some(label(EXTRA));
    // Nobody else's entry was evicted.
    let untouched = log::test_creator_used(other) == (0, 1)
        && log::test_oldest_path(other).as_deref() == Some("/atomic/share-other")
        && log::test_creator_used(log::NO_CREATOR).1 >= 1;
    // Outstanding promises alone reach the share: refused, with nothing evicted.
    let mut held = alloc::vec::Vec::new();
    while held.len() < log::CREATOR_SHARE {
        match log::PublicationReservation::reserve(greedy) {
            Ok(reservation) => held.push(reservation),
            Err(_) => { ::log::error!("AP-PC early exit #3 entries={} shape={:?}", log::entry_count(), log::test_shape()); return false },
        }
    }
    let len_full = log::entry_count();
    let refused = log::PublicationReservation::reserve(greedy).is_err()
        && log::entry_count() == len_full
        && log::test_creator_used(greedy) == (log::CREATOR_SHARE, 0);
    // Another creator still reserves while that creator is at its bound.
    let Ok(peer_again) = log::PublicationReservation::reserve(other) else { { ::log::error!("AP-PC early exit #4 entries={} shape={:?}", log::entry_count(), log::test_shape()); return false } };
    drop(peer_again);
    drop(held);
    let tail = log::test_creator_used(greedy).0 == 0 && log::test_shape().2 == 0
        && log::test_creator_used(other) == (0, 1);
    if !(refused && held_elsewhere && advanced && retained && oldest && untouched && tail) {
        ::log::error!("AP-PC detail: refused={refused} held_elsewhere={held_elsewhere} advanced={advanced} retained={retained} oldest={oldest} untouched={untouched} tail={tail} greedy={:?} other={:?} entries={} len0={len0}",
            log::test_creator_used(greedy), log::test_creator_used(other), log::entry_count());
    }
    refused && held_elsewhere && advanced && retained && oldest && untouched && tail
}

/// A selection vacated from the ready queue and rolled back returns to its
/// exact FIFO position even when entries ahead of it were stolen meanwhile.
fn ready_rollback_fifo_contract() -> bool {
    crate::task::hart_local::ready::test_rollback_keeps_fifo_position(91_500)
}

/// Cancellation is a flat worklist over operations indexed by id: a deep
/// operation chain is cancelled from its root with every holder revoked, a
/// consumer-loss cascade across a long producer chain reaches the first stage,
/// an unrelated operation survives, and cancelling again is a no-op.
fn session_cancel_contract() -> bool {
    use api::cell_session::{ObjectRefV1 as Ref, Status};
    use crate::task::cell_session::test_hooks as hooks;
    const CONTROLLER_TID: usize = 92_011;
    const WORKER_TID: usize = 92_012;
    const GENERATION: u64 = 0x5E55_1006;
    const NESTED: usize = 24;
    const STAGES: usize = 24;
    let tids = [CONTROLLER_TID, WORKER_TID];
    let Some(cells) = two_session_cells(tids, GENERATION) else { return false };
    let (controller, worker) = (cells[0], cells[1]);
    let ok = with_session_hold(|sched| {
        let mut ok = true;
        // Nested chain: cancelling the root reaches the leaf and its holders.
        let mut chain = alloc::vec::Vec::new();
        let mut parent = Ref::ZERO;
        for _ in 0..NESTED {
            parent = stage_operation(sched, controller, GENERATION, parent)?;
            chain.push(parent);
        }
        let (status, leaf) = hooks::exec(sched, controller, GENERATION, 2, &create_stream_body(*chain.last()?));
        ok &= status == Status::Ok;
        let (leaf_reader, leaf_writer) = (result_ref(&leaf, 0)?, result_ref(&leaf, 1)?);
        // Producer chain: stage i feeds the ring consumed in stage i + 1.
        let mut stages = alloc::vec::Vec::new();
        for _ in 0..STAGES { stages.push(stage_operation(sched, controller, GENERATION, Ref::ZERO)?); }
        let mut producers = alloc::vec::Vec::new();
        for i in 0..STAGES - 1 {
            let (status, ring) = hooks::exec(sched, controller, GENERATION, 2, &create_stream_body(stages[i + 1]));
            ok &= status == Status::Ok;
            let writer = result_ref(&ring, 1)?;
            producers.push(hooks::stage_holder(sched, writer, stages[i], worker, GENERATION)?);
            ok &= hooks::close_endpoint(sched, writer);
        }
        let bystander = stage_operation(sched, controller, GENERATION, Ref::ZERO)?;
        let (status, spare) = hooks::exec(sched, controller, GENERATION, 2, &create_stream_body(bystander));
        ok &= status == Status::Ok;
        let (spare_reader, spare_writer) = (result_ref(&spare, 0)?, result_ref(&spare, 1)?);
        // Consumer loss at the end of the producer chain cascades to its first stage.
        hooks::cancel_operation(sched, *stages.last()?);
        ok &= stages.iter().all(|stage| hooks::operation_state(sched, *stage) == Some(1));
        ok &= producers.iter().all(|p| hooks::endpoint_state(sched, *p) == Some(2));
        // The unrelated operation and the nested chain are untouched.
        ok &= hooks::operation_state(sched, bystander) == Some(0)
            && hooks::endpoint_state(sched, spare_reader) == Some(0)
            && hooks::endpoint_state(sched, spare_writer) == Some(0);
        ok &= chain.iter().all(|op| hooks::operation_state(sched, *op) == Some(0));
        // Root cancellation of the nested chain revokes every descendant holder.
        hooks::cancel_operation(sched, chain[0]);
        ok &= chain.iter().all(|op| hooks::operation_state(sched, *op) == Some(1));
        ok &= hooks::endpoint_state(sched, leaf_reader) == Some(3)
            && hooks::endpoint_state(sched, leaf_writer) == Some(3);
        // Repeating either cancellation changes nothing.
        hooks::cancel_operation(sched, chain[0]);
        hooks::cancel_operation(sched, stages[0]);
        ok &= hooks::endpoint_state(sched, leaf_reader) == Some(3)
            && hooks::operation_state(sched, bystander) == Some(0);
        hooks::cancel_operation(sched, bystander);
        ok &= hooks::endpoint_state(sched, spare_reader) == Some(3);
        Some(ok)
    });
    retire_session_cells(tids, cells, GENERATION);
    ok
}

/// Closing a session's output reader closes only the holders on that ring and
/// cancels only the committed child writing to it: the controller's own holders,
/// the other session of the same bootstrap operation and its child survive.
fn session_close_scope_contract() -> bool {
    use api::cell_session::{ObjectRefV1 as Ref, Status};
    use crate::task::cell_session::test_hooks as hooks;
    const CONTROLLER_TID: usize = 92_007;
    const CHILD_TID: usize = 92_008;
    const GENERATION: u64 = 0x5E55_1004;
    let tids = [CONTROLLER_TID, CHILD_TID];
    let Some(cells) = two_session_cells(tids, GENERATION) else { return false };
    let (controller, child) = (cells[0], cells[1]);
    let ok = with_session_hold(|sched| {
        let root = stage_operation(sched, controller, GENERATION, Ref::ZERO)?;
        let first_child = stage_operation(sched, controller, GENERATION, root)?;
        let second_child = stage_operation(sched, controller, GENERATION, root)?;
        // Two sessions of the same controller operation. Layout: 0 stdin reader,
        // 1 stdout writer, 2 stderr writer, 3 control reader, 4 output reader,
        // 5 diagnostic reader, 6 and 7 the issuer's producer peers.
        let first = hooks::stage_bootstrap(sched, controller, GENERATION, root)?;
        let second = hooks::stage_bootstrap(sched, controller, GENERATION, root)?;
        let first_out = hooks::stage_holder(sched, first[1], first_child, child, GENERATION)?;
        let second_out = hooks::stage_holder(sched, second[1], second_child, child, GENERATION)?;
        let (status, rest) = hooks::exec(sched, controller, GENERATION, 2, &create_stream_body(root));
        let (spare_reader, spare_writer) = (result_ref(&rest, 0)?, result_ref(&rest, 1)?);
        let mut ok = status == Status::Ok;
        // The frontend of the first session loses its output reader.
        ok &= hooks::close_endpoint(sched, first[4]);
        // The child writing to that ring is cut and its writer closed...
        ok &= hooks::operation_state(sched, first_child) == Some(1)
            && hooks::endpoint_state(sched, first_out) == Some(2);
        // ...and the controller's retained writer on the ring is closed, nothing more of it.
        ok &= hooks::endpoint_state(sched, first[1]) == Some(2);
        for survivor in [first[0], first[2], first[3], first[5], first[6], first[7]] {
            ok &= hooks::endpoint_state(sched, survivor) == Some(0);
        }
        // The shared operation, the sibling session with its child, and the
        // controller's unrelated stream all continue.
        ok &= hooks::operation_state(sched, root) == Some(0)
            && hooks::operation_state(sched, second_child) == Some(0)
            && hooks::endpoint_state(sched, second_out) == Some(0);
        for endpoint in second { ok &= hooks::endpoint_state(sched, endpoint) == Some(0); }
        ok &= hooks::endpoint_state(sched, spare_reader) == Some(0)
            && hooks::endpoint_state(sched, spare_writer) == Some(0);
        // Releasing the controller's operation then cancels all of it.
        hooks::cancel_operation(sched, root);
        ok &= hooks::operation_state(sched, root) == Some(1)
            && hooks::operation_state(sched, second_child) == Some(1)
            && hooks::endpoint_state(sched, second_out) == Some(3)
            && hooks::endpoint_state(sched, spare_reader) == Some(3);
        Some(ok)
    });
    retire_session_cells(tids, cells, GENERATION);
    ok
}

/// Acknowledgement reclaims every closed record that no retained outcome names,
/// even while another outcome of the same operation is still outstanding; the
/// record an outstanding outcome names stays resolvable so its replay is exact.
fn session_reclaim_contract() -> bool {
    use api::cell_session::{ObjectRefV1 as Ref, Status};
    use crate::task::cell_session::test_hooks as hooks;
    const OWNER_TID: usize = 92_009;
    const PEER_TID: usize = 92_010;
    const GENERATION: u64 = 0x5E55_1005;
    let tids = [OWNER_TID, PEER_TID];
    let Some(cells) = two_session_cells(tids, GENERATION) else { return false };
    let state = |endpoint: Ref| {
        crate::task::SCHEDULER.lock().as_ref().and_then(|sched| hooks::endpoint_state(sched, endpoint))
    };
    let (status, created) = session_call(OWNER_TID, 1, 1, &Ref::ZERO.to_bytes());
    let operation = result_ref(&created, 0).unwrap_or(Ref::ZERO);
    let mut ok = status == Status::Ok && operation != Ref::ZERO;
    let (_, first) = session_call(OWNER_TID, 2, 1, &create_stream_body(operation));
    let (_, second) = session_call(OWNER_TID, 2, 2, &create_stream_body(operation));
    let (Some(first_reader), Some(first_writer), Some(second_reader), Some(second_writer)) =
        (result_ref(&first, 0), result_ref(&first, 1), result_ref(&second, 0), result_ref(&second, 1))
    else { return false };
    ok &= session_call(OWNER_TID, 7, 3, &first_writer.to_bytes()).0 == Status::Ok;
    ok &= session_call(OWNER_TID, 7, 4, &first_reader.to_bytes()).0 == Status::Ok;
    ok &= session_call(OWNER_TID, 7, 5, &second_writer.to_bytes()).0 == Status::Ok;
    let ack = |through: u64| {
        let mut body = alloc::vec::Vec::new();
        body.extend_from_slice(&operation.to_bytes());
        body.extend_from_slice(&through.to_le_bytes());
        body
    };
    // Acknowledge requests 1..=4 only: request 5 keeps its retained outcome.
    ok &= session_call(OWNER_TID, 25, 100, &ack(4)).0 == Status::Ok;
    ok &= state(first_writer).is_none() && state(first_reader).is_none();
    ok &= state(second_writer) == Some(1) && state(second_reader) == Some(0);
    // The outstanding outcome still replays exactly (not Stale).
    ok &= session_call(OWNER_TID, 7, 5, &second_writer.to_bytes()).0 == Status::Ok;
    ok &= session_call(OWNER_TID, 25, 101, &ack(5)).0 == Status::Ok;
    ok &= state(second_writer).is_none() && state(second_reader) == Some(0);
    ok &= session_call(OWNER_TID, 8, 6, &{
        let mut cancel = operation.to_bytes().to_vec();
        cancel.extend_from_slice(&2u32.to_le_bytes());
        cancel.extend_from_slice(&0u32.to_le_bytes());
        cancel
    }).0 == Status::Ok;
    retire_session_cells(tids, cells, GENERATION);
    ok
}

/// After a committed foreground child's input issuer is replaced or dies, the
/// operation is cancelled and the issuer's producer peers are revoked; once the
/// child is terminal the dead issuer's receipt is drained instead of pinning
/// the shared receipt table.
fn session_issuer_replacement_contract() -> bool {
    use api::cell_session::{ObjectRefV1 as Ref, Status};
    use crate::task::cell_session::{self, test_hooks as hooks};
    const PREPARER_TID: usize = 92_013;
    const CHILD_TID: usize = 92_014;
    const GENERATION: u64 = 0x5E55_1007;
    const CHILD_ROOT: u64 = 93_002_000;
    let tids = [PREPARER_TID, CHILD_TID];
    let Some(cells) = two_session_cells(tids, GENERATION) else { return false };
    let (preparer, child) = (cells[0], cells[1]);
    let ok = with_session_hold(|sched| {
        let operation = stage_operation(sched, preparer, GENERATION, Ref::ZERO)?;
        let mut writers = [Ref::ZERO; 2];
        for writer in &mut writers {
            let (status, ring) = hooks::exec(sched, preparer, GENERATION, 2, &create_stream_body(operation));
            if status != Status::Ok { return None; }
            *writer = result_ref(&ring, 1)?;
        }
        let (prepared, peers) = hooks::stage_issuer_receipt(
            sched, (preparer, GENERATION), (child, GENERATION), operation, writers)?;
        let mut ok = hooks::receipts_on(sched, operation) == 1
            && peers.iter().all(|peer| hooks::endpoint_state(sched, *peer) == Some(0))
            && hooks::prepared_flags(sched, prepared) == Some((1, 0, false))
            && hooks::operation_state(sched, operation) == Some(0);
        // The recorded issuer is not the registered provider: cut the child's
        // operation, flag the kill, and revoke the issuer's peers.
        hooks::maintain(sched);
        ok &= hooks::prepared_flags(sched, prepared) == Some((1, 4, true))
            && hooks::operation_state(sched, operation) == Some(1)
            && peers.iter().all(|peer| hooks::endpoint_state(sched, *peer) == Some(3));
        // The child dies; its terminal state remains queryable by the preparer
        // while the dead issuer's receipt is drained.
        let owner = api::cell_owner::CellOwner::new(child, GENERATION, CHILD_ROOT);
        cell_session::retiring(sched, owner, usize::MAX);
        ok &= hooks::receipts_on(sched, operation) == 1;
        cell_session::retired(sched, owner);
        hooks::maintain(sched);
        ok &= hooks::receipts_on(sched, operation) == 0
            && hooks::prepared_flags(sched, prepared).map(|flags| flags.0) == Some(3);
        Some(ok)
    });
    retire_session_cells(tids, cells, GENERATION);
    ok
}

/// `QueryChild` (selector 30): only the child's preparer reads the recorded
/// state of a receipt-less committed child; a foreign principal is denied and a
/// stale generation or wrong child identity is `Stale`. The terminal record
/// (exit code, canonical sequence) stays readable after the child is reaped,
/// across acknowledgements, while the preparer's operation is open; it never
/// blocks `CloseOperation` once terminal and is released by it. A child that
/// carried only a VFS cwd receipt keeps its record after the receipt is removed.
fn session_query_child_contract() -> bool {
    use api::cell_session::{ChildRefV1, ObjectRefV1 as Ref, Status};
    use crate::task::cell_session::{self, test_hooks as hooks};
    const PREPARER_TID: usize = 92_015;
    const FOREIGN_TID: usize = 92_016;
    const GENERATION: u64 = 0x5E55_1008;
    const CHILD_ROOT: u64 = 93_003_000;
    const EXIT_CODE: usize = 7;
    let tids = [PREPARER_TID, FOREIGN_TID];
    let Some(cells) = two_session_cells(tids, GENERATION) else { return false };
    let (status, created) = session_call(PREPARER_TID, 1, 1, &Ref::ZERO.to_bytes());
    let operation = result_ref(&created, 0).unwrap_or(Ref::ZERO);
    let mut ok = status == Status::Ok && operation != Ref::ZERO;
    // The child is a principal that is neither caller (generation + 1).
    let child_generation = GENERATION + 1;
    let mut staged = None;
    ok &= with_session_hold(|sched| {
        staged = hooks::stage_receiptless_child(sched, (cells[0], GENERATION), (cells[1], child_generation), operation, CHILD_ROOT);
        Some(staged.is_some())
    });
    let Some(child) = staged else {
        retire_session_cells(tids, cells, GENERATION);
        return false;
    };
    let query = |tid: usize, id: u64, child: ChildRefV1| {
        let (status, body) = session_call(tid, 30, id, &child.to_bytes());
        let fields = (body.len() == 24).then(|| {
            let word = |at: usize| u32::from_le_bytes(body[at..at + 4].try_into().unwrap_or([0; 4]));
            (word(0), word(4), word(8) as i32, word(12), u64::from_le_bytes(body[16..24].try_into().unwrap_or([0; 8])))
        });
        (status, fields)
    };
    // Live child: state 1, no reason, no exit code, no terminal sequence.
    ok &= query(PREPARER_TID, 200, child) == (Status::Ok, Some((1, 0, 0, 0, 0)));
    // Idempotent and non-consuming.
    ok &= query(PREPARER_TID, 200, child) == (Status::Ok, Some((1, 0, 0, 0, 0)));
    // A foreign principal is denied and learns nothing.
    ok &= query(FOREIGN_TID, 201, child) == (Status::WrongOwner, None);
    // Unknown generation, and a known reference with a different identity, are stale.
    let unknown = ChildRefV1 { publication_generation: child.publication_generation + 1, ..child };
    ok &= query(PREPARER_TID, 202, unknown) == (Status::Stale, None);
    let other_identity = ChildRefV1 {
        principal: api::cell_session::PrincipalV1 { cell_id: child.principal.cell_id, generation: child.principal.generation + 1 },
        ..child
    };
    ok &= query(PREPARER_TID, 203, other_identity) == (Status::Stale, None);
    ok &= session_call(PREPARER_TID, 30, 204, &child.to_bytes()[..31]).0 == Status::Malformed;
    // A live child blocks closing the operation.
    let (_, stream) = session_call(PREPARER_TID, 2, 1, &create_stream_body(operation));
    let (Some(reader), Some(writer)) = (result_ref(&stream, 0), result_ref(&stream, 1)) else {
        retire_session_cells(tids, cells, GENERATION);
        return false;
    };
    ok &= session_call(PREPARER_TID, 7, 2, &writer.to_bytes()).0 == Status::Ok;
    ok &= session_call(PREPARER_TID, 7, 3, &reader.to_bytes()).0 == Status::Ok;
    ok &= session_call(PREPARER_TID, 20, 4, &operation.to_bytes()).0 == Status::WouldBlock;
    // The child exits: retiring (code recorded), then retired (canonical sequence).
    ok &= with_session_hold(|sched| {
        cell_session::retiring(sched, api::cell_owner::CellOwner::new(cells[1], child_generation, CHILD_ROOT), EXIT_CODE);
        Some(true)
    });
    ok &= query(PREPARER_TID, 205, child) == (Status::Ok, Some((2, 0, EXIT_CODE as i32, 0, 0)));
    ok &= with_session_hold(|sched| {
        cell_session::retired(sched, api::cell_owner::CellOwner::new(cells[1], child_generation, CHILD_ROOT));
        Some(true)
    });
    let terminal = query(PREPARER_TID, 206, child);
    ok &= matches!(terminal, (Status::Ok, Some((3, 0, 7, 0, sequence))) if sequence != 0);
    ok &= query(FOREIGN_TID, 207, child) == (Status::WrongOwner, None);
    // Acknowledgement reclaims closed records but not a receipt-less terminal
    // child: the record is still readable with the operation still open.
    let mut ack = operation.to_bytes().to_vec();
    ack.extend_from_slice(&4u64.to_le_bytes());
    ok &= session_call(PREPARER_TID, 25, 100, &ack).0 == Status::Ok;
    ok &= query(PREPARER_TID, 208, child) == terminal;
    ok &= with_session_hold(|sched| Some(hooks::prepared_exists(sched, child.prepared_ref())));
    // A child that carried a VFS cwd receipt (every shell pipeline stage with a
    // working directory): once the issuer resolves the receipt and the next
    // reclaim removes it, the preparer still reads the terminal record.
    const VFS_CHILD_ROOT: u64 = 93_004_000;
    // Staged and finished inside one hold: a live child bound to a receipt whose
    // issuer is not the registered provider would be cancelled by maintenance.
    let mut staged = None;
    ok &= with_session_hold(|sched| {
        staged = hooks::stage_vfs_receipted_child(sched, (cells[0], GENERATION), (cells[1], child_generation), operation, VFS_CHILD_ROOT);
        let owner = api::cell_owner::CellOwner::new(cells[1], child_generation, VFS_CHILD_ROOT);
        cell_session::retiring(sched, owner, EXIT_CODE);
        cell_session::retired(sched, owner);
        Some(staged.is_some())
    });
    let Some(vfs_child) = staged else {
        retire_session_cells(tids, cells, GENERATION);
        return false;
    };
    let vfs_terminal = query(PREPARER_TID, 211, vfs_child);
    ok &= matches!(vfs_terminal, (Status::Ok, Some((3, 0, 7, 0, sequence))) if sequence != 0);
    ok &= with_session_hold(|sched| { hooks::materialize_receipts(sched, operation); Some(true) });
    // A fresh request and its acknowledgement run `reclaim` on the operation.
    let (_, stream) = session_call(PREPARER_TID, 2, 5, &create_stream_body(operation));
    let (Some(reader), Some(writer)) = (result_ref(&stream, 0), result_ref(&stream, 1)) else {
        retire_session_cells(tids, cells, GENERATION);
        return false;
    };
    ok &= session_call(PREPARER_TID, 7, 6, &writer.to_bytes()).0 == Status::Ok;
    ok &= session_call(PREPARER_TID, 7, 7, &reader.to_bytes()).0 == Status::Ok;
    let mut ack = operation.to_bytes().to_vec();
    ack.extend_from_slice(&7u64.to_le_bytes());
    ok &= session_call(PREPARER_TID, 25, 101, &ack).0 == Status::Ok;
    ok &= with_session_hold(|sched| Some(hooks::receipts_on(sched, operation) == 0));
    ok &= query(PREPARER_TID, 212, vfs_child) == vfs_terminal;
    ok &= query(PREPARER_TID, 213, child) == terminal;
    ok &= with_session_hold(|sched| Some(hooks::prepared_exists(sched, vfs_child.prepared_ref())));
    // The terminal records do not pin CloseOperation, and closing releases them.
    ok &= session_call(PREPARER_TID, 20, 8, &operation.to_bytes()).0 == Status::Ok;
    ok &= with_session_hold(|sched| Some(!hooks::prepared_exists(sched, child.prepared_ref())
        && !hooks::prepared_exists(sched, vfs_child.prepared_ref())));
    ok &= query(PREPARER_TID, 209, child) == (Status::Stale, None);
    ok &= query(PREPARER_TID, 214, vfs_child) == (Status::Stale, None);
    retire_session_cells(tids, cells, GENERATION);
    ok
}

/// A physical serial byte reaches the input service exactly as received (Enter
/// stays CR, Ctrl-J stays LF), while the legacy no-input-service `sys_read(fd=0)`
/// fallback still sees CR from the UART RX ring as LF and nothing else changed.
fn serial_raw_relay_contract() -> bool {
    use crate::task::drivers::console_drv::{route_for, Route};
    (0..=u8::MAX).all(|byte| {
        // Relay: raw for every byte and every source, with the input service online.
        route_for(true, byte, true) == Route::Relay(byte)
            && route_for(true, byte, false) == Route::Relay(byte)
            // Legacy fallback: only ring-sourced CR is rewritten; all else is untouched.
            && route_for(false, byte, false) == Route::Buffer(byte)
            && route_for(false, byte, true)
                == Route::Buffer(if byte == b'\r' { b'\n' } else { byte })
    }) && route_for(true, b'\r', true) == Route::Relay(0x0d)
        && route_for(true, b'\n', true) == Route::Relay(0x0a)
        && route_for(false, b'\n', true) == Route::Buffer(0x0a)
}
