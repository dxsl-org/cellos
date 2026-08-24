//! Case B: IPC copy race where sender exits while message is queued in receiver mailbox.

use super::{cleanup_task, fill_page, read_page, RECEIVER_CELL, RECEIVER_TID, RECEIVER_VA, SENDER_CELL, SENDER_TID, SENDER_VA};
use crate::memory::address_space::AddressSpace;
use crate::task::ipc_wire::MAX_IPC_WIRE_PAYLOAD;
use crate::task::tcb::TaskState;

pub(super) fn run_race_case(
    sender_space: &AddressSpace,
    receiver_space: &AddressSpace,
) -> bool {
    const RACE_LEN: usize = 128;
    // Bound check first: overlarge payload must not allocate or enqueue.
    if crate::task::ipc_send(SENDER_TID, RECEIVER_TID, SENDER_VA, MAX_IPC_WIRE_PAYLOAD + 1).is_ok() {
        log::error!("S22-RV64-IPC-COPY-RACE: FAIL overlarge payload accepted");
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return false;
    }
    if !fill_page(sender_space, SENDER_VA, 0x7c, RACE_LEN) {
        log::error!("S22-RV64-IPC-COPY-RACE: FAIL fixture-fill");
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return false;
    }
    // Receiver busy (Running) -> sender publishes and blocks.
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(receiver) = sched.tasks.get_mut(&RECEIVER_TID) {
            receiver.state = TaskState::Running;
        }
    }
    let blocked = crate::task::ipc_send(SENDER_TID, RECEIVER_TID, SENDER_VA, RACE_LEN);
    // Sender dies while its message sits in the receiver's kernel mailbox.
    let death = crate::task::SCHEDULER
        .lock()
        .as_mut()
        .is_some_and(|sched| {
            let sender_blocked = matches!(
                sched.tasks.get(&SENDER_TID).map(|t| &t.state),
                Some(TaskState::Sending { .. })
            );
            if sender_blocked {
                sched.exit_task(SENDER_TID, 0);
            }
            sender_blocked
        });
    let recv_after_death = crate::task::ipc_try_recv(RECEIVER_TID, 0, RECEIVER_VA, RACE_LEN);
    let race_payload_ok = read_page(receiver_space, RECEIVER_VA, 0x7c, RACE_LEN);
    match (blocked, death, recv_after_death) {
        (Ok(1), true, Ok(sender)) if sender == SENDER_TID && race_payload_ok => {
            log::info!("S22-RV64-IPC-COPY-RACE: PASS harts=2");
            true
        }
        (blocked, death, recv) => {
            log::error!(
                "S22-RV64-IPC-COPY-RACE: FAIL blocked={:?} death={} recv={:?} payload_ok={}",
                blocked,
                death,
                recv,
                race_payload_ok
            );
            false
        }
    }
}
