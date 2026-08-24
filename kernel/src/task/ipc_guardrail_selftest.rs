//! Boot-time guards for IPC behaviors that generic reactor work must preserve.
//!
//! `RecvScatter` currently parks through the ordinary `TaskState::Recv` shape,
//! even though its temporary receive buffer cannot outlive the syscall frame.
//! These tests freeze the safe boundary: producers leave owned mailbox bytes,
//! do not write the stale pointer, and do not silently route this path to the CQ.

use super::completion_selftest::{insert, remove};
use super::tcb::TaskState;

const SENDER: usize = 9341;
const RECEIVER: usize = 9342;
const DEAD_PEER: usize = 9343;
const TEST_CELL: u64 = 9441;
const INVALID_RECV_PTR: usize = usize::MAX - 128;

fn fail(reason: &str) -> bool {
    log::error!("[selftest] IPC-GUARDRAILS: FAIL — {}", reason);
    false
}

fn recv_scatter_stays_mailbox_isolated() -> bool {
    insert(SENDER, TEST_CELL);
    insert(RECEIVER, TEST_CELL + 1);
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(receiver) = sched.tasks.get_mut(&RECEIVER) {
            receiver.state = TaskState::Recv {
                mask: SENDER,
                buf_ptr: INVALID_RECV_PTR,
                buf_len: 16,
                deadline: None,
            };
        }
    }

    let payload = b"scatter-guard";
    let sent = super::ipc_send(SENDER, RECEIVER, payload.as_ptr() as usize, payload.len()) == Ok(0);
    let isolated = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&RECEIVER))
            .is_some_and(|receiver| {
                receiver.state == TaskState::Ready
                    && receiver.completion.is_none()
                    && matches!(
                        receiver.pending_msgs.as_slice(),
                        [msg]
                            if msg.sender_tid == SENDER
                                && msg.payload() == payload
                    )
            })
    };

    remove(SENDER);
    remove(RECEIVER);
    sent && isolated
        || fail("RecvScatter-shaped receive wrote outside the mailbox or reached the CQ")
}

fn dead_peer_unblocks_sender() -> bool {
    insert(SENDER, TEST_CELL);
    insert(DEAD_PEER, TEST_CELL + 1);
    let (setup, ready_with_error, requeued, reaped) = {
        let mut guard = super::SCHEDULER.lock();
        if let Some(sched) = guard.as_mut() {
            let setup = if let Some(sender) = sched.tasks.get_mut(&SENDER) {
                sender.state = TaskState::Sending {
                    target: DEAD_PEER,
                    delivery_id: super::next_delivery_id(),
                };
                true
            } else {
                false
            };
            if setup {
                sched.exit_task(DEAD_PEER, usize::MAX);
            }
            let ready_with_error = sched.tasks.get(&SENDER).is_some_and(|sender| {
                sender.state == TaskState::Ready
                    && sender.reply_value == Some(usize::MAX)
                    && sender.trap_frame.regs[10] == usize::MAX as _
            });
            let requeued = super::hart_local::HART_LOCALS.iter().any(|local| {
                local
                    .ready
                    .lock()
                    .values()
                    .any(|queue| queue.contains(&SENDER))
            });
            (
                setup,
                ready_with_error,
                requeued,
                sched.take_reapable_zombies(),
            )
        } else {
            (false, false, false, alloc::vec::Vec::new())
        }
    };

    drop(reaped);
    super::hart_local::ready::remove_from_all(SENDER);
    insert(RECEIVER, TEST_CELL + 2);
    let payload = [0u8; 1];
    let followup = super::ipc_send(SENDER, RECEIVER, payload.as_ptr() as usize, 1) == Ok(1)
        && super::SCHEDULER
            .lock()
            .as_ref()
            .and_then(|sched| sched.tasks.get(&SENDER))
            .is_some_and(|sender| {
                sender.reply_value.is_none()
                    && matches!(sender.state, TaskState::Sending { target, .. } if target == RECEIVER)
            });
    remove(SENDER);
    remove(RECEIVER);
    remove(DEAD_PEER);
    setup && ready_with_error && requeued && followup
        || fail("dead-peer error wake or the next send's result reset was incorrect")
}

pub fn self_test() -> bool {
    let ok = recv_scatter_stays_mailbox_isolated() & dead_peer_unblocks_sender();
    if ok {
        log::info!("[selftest] IPC-GUARDRAILS: PASS (dead-peer bounded, RecvScatter isolated)");
    }
    ok
}
