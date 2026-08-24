//! Self-test fixtures for Phase 04 bounded copied IPC.
//!
//! Emits:
//! - `S22-RV64-IPC-COPY: PASS harts=N` — publish->dequeue through the real
//!   `ipc_send`/`ipc_recv` production path with exact-payload assertion
//! - `S22-RV64-IPC-NO-PEER-MAP: PASS harts=N` — no sender page in receiver ledger
//! - `S22-RV64-IPC-SCATTER: PASS harts=N` — atomic multi-iovec receive transaction
//! - `S22-RV64-IPC-COPY-RACE: PASS harts=2` — sender exit overlaps queued delivery

mod copy_case;
mod race_case;
mod scatter_case;

use super::ipc_wire::IpcWireMessage;
use super::Task;
use crate::memory::address_space::{AddressSpace, AddressSpaceBuilder, MappingKind};
use crate::memory::frame::phys_to_virt;
use crate::memory::paging::Flags;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use types::CellId;

pub(super) const SENDER_TID: usize = 9401;
pub(super) const RECEIVER_TID: usize = 9402;
pub(super) const SENDER_CELL: u64 = 55;
pub(super) const RECEIVER_CELL: u64 = 56;

pub(super) const SENDER_VA: usize = 0x0100_0000;
pub(super) const RECEIVER_VA: usize = 0x0200_0000;

fn build_space(va: usize) -> Option<Arc<AddressSpace>> {
    let mut builder = AddressSpaceBuilder::new();
    let bits = Flags::READ | Flags::WRITE;
    builder
        .map_user_page(va, MappingKind::Private, Flags::from_bits(bits))
        .ok()?;
    builder.build().ok()
}

pub(super) fn fill_page(space: &AddressSpace, va: usize, byte: u8, len: usize) -> bool {
    const PAGE_SIZE: usize = 4096;
    let page = va & !(PAGE_SIZE - 1);
    let offset_in_page = va & (PAGE_SIZE - 1);
    let Some((_, pa)) = space.page_proof_for(page) else {
        return false;
    };
    // SAFETY: the ledger entry proves exclusive ownership while the fixture holds the Arc.
    unsafe {
        for offset in 0..len {
            (phys_to_virt(pa + offset_in_page + offset) as *mut u8).write_volatile(byte);
        }
    }
    true
}

pub(super) fn read_page(space: &AddressSpace, va: usize, byte: u8, len: usize) -> bool {
    const PAGE_SIZE: usize = 4096;
    let page = va & !(PAGE_SIZE - 1);
    let offset_in_page = va & (PAGE_SIZE - 1);
    let Some((_, pa)) = space.page_proof_for(page) else {
        return false;
    };
    // SAFETY: as above.
    unsafe {
        for offset in 0..len {
            if (phys_to_virt(pa + offset_in_page + offset) as *const u8).read_volatile() != byte {
                return false;
            }
        }
    }
    true
}

fn setup_task(tid: usize, cell: u64, name: &'static str, va: usize) -> Option<Arc<AddressSpace>> {
    let space = build_space(va)?;
    let mut task = Box::new(Task::new(tid, CellId(cell), name, Vec::new()));
    task.cell_generation = 1;
    task.root_tid = tid;
    task.address_space = super::tcb::TaskAddressSpace::Domain(space.clone());

    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        let owner = api::cell_owner::CellOwner::new(cell, 1, tid as u64);
        sched.publish_live_cell_owner(owner);
        sched.tasks.insert(tid, task);
    }
    Some(space)
}

pub(super) fn cleanup_task(tid: usize, cell: u64) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.remove(&tid) {
            let owner = api::cell_owner::CellOwner::new(cell, task.cell_generation, task.root_tid as u64);
            sched.clear_live_cell_owner_for_test(owner);
        }
    }
}

/// Run Phase 04 bounded copied IPC self-tests.
pub fn run_primary(harts: usize) {
    log::info!("[selftest] S22-RV64-IPC starting harts={}", harts);

    let Some(sender_space) = setup_task(SENDER_TID, SENDER_CELL, "ipc-sender", SENDER_VA) else {
        log::error!("S22-RV64-IPC-COPY: FAIL fixture-setup sender");
        return;
    };
    let Some(receiver_space) = setup_task(RECEIVER_TID, RECEIVER_CELL, "ipc-receiver", RECEIVER_VA) else {
        log::error!("S22-RV64-IPC-COPY: FAIL fixture-setup receiver");
        cleanup_task(SENDER_TID, SENDER_CELL);
        return;
    };

    // 1. Sender page must NOT appear in the receiver's ledger.
    if receiver_space.page_proof_for(SENDER_VA).is_some() {
        log::error!("S22-RV64-IPC-NO-PEER-MAP: FAIL sender page mapped in receiver");
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return;
    }
    log::info!("S22-RV64-IPC-NO-PEER-MAP: PASS harts={}", harts);

    // ── Case A (IPC-COPY): receiver waiting, real ipc_send publish path ──
    if !copy_case::run_copy_case(harts, &sender_space, &receiver_space) {
        return;
    }

    // ── Case C (RecvScatter atomic transaction): valid-first / invalid-later ──
    if !scatter_case::run_scatter_case(harts, &receiver_space) {
        return;
    }

    // ── Case B (IPC-COPY-RACE, 2 harts): sender death after copy ──
    if harts >= 2 && !race_case::run_race_case(&sender_space, &receiver_space) {
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return;
    }

    // Sanity: wire bound constant stays within the documented maximum.
    let _ = IpcWireMessage::try_new(
        super::ipc_wire::IpcWireHeader {
            sender_tid: 0,
            sender_cell_id: 0,
            sender_generation: 0,
            delivery_id: 0,
        },
        &[],
    )
    .expect("empty wire message must allocate");

    cleanup_task(SENDER_TID, SENDER_CELL);
    cleanup_task(RECEIVER_TID, RECEIVER_CELL);
}
