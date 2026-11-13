//! Case A: publish->dequeue through production ipc_send/ipc_recv with exact payload assertion.

use super::{
    cleanup_task, fill_page, read_page, RECEIVER_CELL, RECEIVER_TID, RECEIVER_VA, SENDER_CELL,
    SENDER_TID, SENDER_VA,
};
use crate::memory::address_space::AddressSpace;
use crate::task::tcb::TaskState;
use crate::task::tcb::WaitKind;

pub(super) fn run_copy_case(
    harts: usize,
    sender_space: &AddressSpace,
    receiver_space: &AddressSpace,
) -> bool {
    const COPY_LEN: usize = 256;
    if !fill_page(sender_space, SENDER_VA, 0x5a, COPY_LEN) {
        log::error!("S22-RV64-IPC-COPY: FAIL fixture-fill");
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return false;
    }
    // Receiver parked in Recv so ipc_send takes the wake path.
    if let Some(sched) = crate::task::SCHEDULER.lock().as_mut() {
        if let Some(receiver) = sched.tasks.get_mut(&RECEIVER_TID) {
            receiver.state = TaskState::Blocked {
                wait: WaitKind::Recv {
                    mask: 0,
                    buf_ptr: RECEIVER_VA,
                    buf_len: COPY_LEN,
                },
                deadline: None,
            };
        }
    }
    // The receiver is a synthetic endpoint (`Task::new`: zeroed Context, no
    // kernel stack). The Recv wake below queues it on this hart; an idle peer
    // hart would steal it and switch onto sp=0. Reserve it against stealing
    // across the wake, then drop the queue entry before releasing.
    let hart = crate::sched::hart_local::current_hart_id();
    if !crate::sched::hart_local::ready::reserve_test_dispatch_on_hart(hart, RECEIVER_TID) {
        log::error!("S22-RV64-IPC-COPY: FAIL dispatch-reservation busy");
        cleanup_task(SENDER_TID, SENDER_CELL);
        cleanup_task(RECEIVER_TID, RECEIVER_CELL);
        return false;
    }
    let send_res = crate::task::ipc_send(SENDER_TID, RECEIVER_TID, SENDER_VA, COPY_LEN);
    if let Some(_sched) = crate::task::SCHEDULER.lock().as_mut() {
        crate::sched::hart_local::ready::remove_from_all(RECEIVER_TID);
    }
    crate::sched::hart_local::ready::release_test_dispatch_on_hart(hart, RECEIVER_TID);
    let recv_res = crate::task::ipc_recv(RECEIVER_TID, 0, RECEIVER_VA, COPY_LEN);
    let payload_ok = read_page(receiver_space, RECEIVER_VA, 0x5a, COPY_LEN);
    let sender_ready = crate::task::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched
            .tasks
            .get(&SENDER_TID)
            .is_some_and(|t| t.state == TaskState::Ready)
    });
    match (send_res, recv_res) {
        (Ok(0), Ok(sender)) if sender == SENDER_TID && payload_ok && sender_ready => {
            log::info!("S22-RV64-IPC-COPY: PASS harts={}", harts);
            true
        }
        (send, recv) => {
            log::error!(
                "S22-RV64-IPC-COPY: FAIL send={:?} recv={:?} payload_ok={} sender_ready={}",
                send,
                recv,
                payload_ok,
                sender_ready
            );
            cleanup_task(SENDER_TID, SENDER_CELL);
            cleanup_task(RECEIVER_TID, RECEIVER_CELL);
            false
        }
    }
}
