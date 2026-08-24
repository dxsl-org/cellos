//! Boot self-test for receiver-owned IPC delivery.
//!
//! Runs after scheduler initialization and before secondary harts start. Each
//! producer receives a synthetic `Recv` task whose buffer pointer is deliberately
//! invalid: success proves the producer queued owned bytes without touching the
//! foreign buffer. Teardown removes every synthetic task and ready-queue entry.

use super::completion_selftest::{insert, remove};
use super::tcb::{Task, TaskState, HOTSWAP_MSG_QUEUE_DEPTH};
use alloc::vec::Vec;
use types::CellId;

const SENDER: usize = 9321;
const RECEIVER: usize = 9322;
const TEST_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 3) as u64;
const QUOTA_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 1) as u64;
const INVALID_RECV_PTR: usize = usize::MAX - 64;

fn fail(reason: &str) -> bool {
    log::error!("[selftest] IPC-PENDING: FAIL — {}", reason);
    false
}

fn prepare_receiver(mask: usize) {
    insert(SENDER, TEST_CELL);
    insert(RECEIVER, TEST_CELL + 1);
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&RECEIVER) {
            task.state = TaskState::Recv {
                mask,
                buf_ptr: INVALID_RECV_PTR,
                buf_len: 16,
                deadline: None,
            };
        }
    }
}

fn verify_delivery(expected: &[u8]) -> bool {
    let guard = super::SCHEDULER.lock();
    let Some(task) = guard.as_ref().and_then(|sched| sched.tasks.get(&RECEIVER)) else {
        return fail("synthetic receiver disappeared");
    };
    if task.state != TaskState::Ready {
        return fail("delivery did not wake the receiver");
    }
    match task.pending_msgs.as_slice() {
        [msg] if msg.sender_tid == SENDER && msg.payload() == expected => true,
        _ => fail("delivery did not leave exactly one owned mailbox message"),
    }
}

fn reset() {
    remove(SENDER);
    remove(RECEIVER);
}

fn all_producers_defer_foreign_writes() -> bool {
    // RecvScatter leaves the same TaskState::Recv shape behind, but its temporary
    // buffer is already gone. INVALID_RECV_PTR models that stale pointer: exercising
    // every producer here proves none of them dereferences either Recv variant's
    // retained destination outside the receiver's own syscall context.
    let payload = b"owned-ipc";
    let mut ok = true;

    prepare_receiver(SENDER);
    if super::ipc_send(SENDER, RECEIVER, payload.as_ptr() as usize, payload.len()) != Ok(0)
        || !verify_delivery(payload)
    {
        ok = fail("ipc_send did not defer matched Recv delivery");
    }
    reset();

    prepare_receiver(usize::MAX);
    if super::ipc_post_nonblock(SENDER, RECEIVER, payload).is_err() || !verify_delivery(payload) {
        ok = fail("ipc_post_nonblock did not defer matched Recv delivery");
    }
    reset();

    prepare_receiver(SENDER);
    if super::ipc_try_send(SENDER, RECEIVER, payload.as_ptr() as usize, payload.len()).is_err()
        || !verify_delivery(payload)
    {
        ok = fail("ipc_try_send did not defer matched Recv delivery");
    }
    reset();
    ok
}

fn full_mailbox_refuses_without_wake() -> bool {
    prepare_receiver(SENDER);
    let fill_ok = {
        let mut guard = super::SCHEDULER.lock();
        let task = guard
            .as_mut()
            .and_then(|sched| sched.tasks.get_mut(&RECEIVER))
            .expect("synthetic receiver exists");
        let mut ok = true;
        for _ in 0..HOTSWAP_MSG_QUEUE_DEPTH {
            if super::queue_pending_msg(task, SENDER, b"x", HOTSWAP_MSG_QUEUE_DEPTH).is_err() {
                ok = false;
                break;
            }
        }
        ok
    };
    if !fill_ok {
        reset();
        return fail("mailbox refused an entry below its depth");
    }

    let refused = super::ipc_post_nonblock(SENDER, RECEIVER, b"overflow").is_err();
    let unchanged = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&RECEIVER))
            .is_some_and(|task| {
                matches!(task.state, TaskState::Recv { .. })
                    && task.current_caller.is_none()
                    && task.pending_msgs.len() == HOTSWAP_MSG_QUEUE_DEPTH
            })
    };
    reset();
    refused && unchanged || fail("full mailbox changed receiver state or accepted overflow")
}

fn quota_failure_is_fallible() -> bool {
    let mut task = Task::new(9330, CellId(QUOTA_CELL), "ipc-quota-selftest", Vec::new());
    crate::memory::cell_quota::register(CellId(QUOTA_CELL), 0);
    let inline_ok =
        super::queue_pending_msg(&mut task, SENDER, b"x", HOTSWAP_MSG_QUEUE_DEPTH).is_ok();
    if inline_ok {
        drop(task.pending_msgs.remove(0));
    }
    let heap_backed_payload = [0x5a; 33];
    let result = super::queue_pending_msg(
        &mut task,
        SENDER,
        &heap_backed_payload,
        HOTSWAP_MSG_QUEUE_DEPTH,
    );
    crate::memory::cell_quota::deregister(CellId(QUOTA_CELL));

    if inline_ok && result.is_err() && task.pending_msgs.is_empty() {
        true
    } else {
        fail("inline delivery allocated or heap quota failure mutated the mailbox")
    }
}

fn heap_payload_refunds_receiver_quota() -> bool {
    let mut task = Task::new(9331, CellId(QUOTA_CELL), "ipc-refund-selftest", Vec::new());
    crate::memory::cell_quota::register(CellId(QUOTA_CELL), 4096);
    let payload = [0xa5; 33];
    let queued = super::queue_pending_msg(&mut task, SENDER, &payload, HOTSWAP_MSG_QUEUE_DEPTH);
    let charged = crate::memory::cell_quota::in_use(CellId(QUOTA_CELL));
    if queued.is_ok() {
        drop(task.pending_msgs.remove(0));
    }
    let refunded = crate::memory::cell_quota::in_use(CellId(QUOTA_CELL));
    crate::memory::cell_quota::deregister(CellId(QUOTA_CELL));
    queued.is_ok() && charged >= payload.len() && refunded == 0
        || fail("heap payload was not charged and refunded to the receiver")
}

fn death_wake_precedes_later_message() -> bool {
    let mut task = Task::new(9332, CellId(1), "ipc-death-selftest", Vec::new());
    task.current_caller = Some(77);
    task.pending_exit_reason = Some(9);
    let _ = super::queue_pending_msg(&mut task, SENDER, b"later", HOTSWAP_MSG_QUEUE_DEPTH);
    let first = super::syscall::take_resume_delivery(&mut task, 0);
    let second = super::syscall::take_resume_delivery(&mut task, 0);
    matches!(
        first,
        super::syscall::ResumeDelivery::Death {
            sender_tid: 77,
            reason: 9
        }
    ) && matches!(second, super::syscall::ResumeDelivery::Message(_))
        || fail("a later mailbox message displaced the death-owned wake")
}

fn pending_drain_keeps_sender_context_without_relocking() -> bool {
    insert(SENDER, TEST_CELL);
    insert(RECEIVER, TEST_CELL + 1);
    let payload = b"context";
    let mut recv_buf = [0u8; 16];
    let result = {
        let mut guard = super::SCHEDULER.lock();
        let Some(sched) = guard.as_mut() else {
            return fail("scheduler missing during IPC pending drain selftest");
        };
        let receiver = sched
            .tasks
            .get_mut(&RECEIVER)
            .expect("synthetic receiver exists");
        receiver.pending_msgs.try_push(super::tcb::PendingMsg {
            sender_tid: SENDER,
            data: super::pending_mailbox::PendingMsgData::try_copy(payload, TEST_CELL as usize)
                .expect("inline payload copy must fit"),
            wire: None,
            enqueued_tick: 0,
        })
    };
    if result.is_err() {
        reset();
        return fail("could not enqueue the synthetic pending IPC message");
    }

    let sender_generation = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&SENDER))
            .map(|task| task.cell_generation)
            .unwrap_or(0)
    };
    let delivered = super::syscall::handle_syscall(
        RECEIVER,
        super::syscall::Syscall::TryRecv {
            mask: SENDER,
            buf_ptr: recv_buf.as_mut_ptr() as usize,
            buf_len: payload.len(),
            attest_caller: false,
        },
    );
    let preserved = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&RECEIVER))
            .is_some_and(|task| {
                task.current_caller == Some(SENDER)
                    && task.current_caller_cell_id == TEST_CELL
                    && task.current_caller_cell_generation == sender_generation
                    && task.pending_msgs.is_empty()
            })
    };
    reset();
    matches!(delivered, Ok(id) if id == SENDER)
        && recv_buf[..payload.len()] == *payload
        && preserved
        || fail("pending-message drain did not preserve sender context under one scheduler lock")
}

fn try_recv_attestation_writes_identity_trailer() -> bool {
    prepare_receiver(0);
    let payload = b"attested-try-recv";
    let mut recv_buf = [0u8; 64];
    let result = {
        let mut guard = super::SCHEDULER.lock();
        let Some(sched) = guard.as_mut() else {
            return fail("scheduler missing during try_recv attestation selftest");
        };
        let receiver = sched
            .tasks
            .get_mut(&RECEIVER)
            .expect("synthetic receiver exists");
        receiver.pending_msgs.try_push(super::tcb::PendingMsg {
            sender_tid: SENDER,
            data: super::pending_mailbox::PendingMsgData::try_copy(payload, TEST_CELL as usize)
                .expect("inline payload copy must fit"),
            wire: None,
            enqueued_tick: 0,
        })
    };
    if result.is_err() {
        reset();
        return fail("could not enqueue pending message for try_recv attestation test");
    }

    let sender_generation = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&SENDER))
            .map(|task| task.cell_generation)
            .unwrap_or(0)
    };

    // Case 1: attest_caller = true
    let delivered = super::syscall::handle_syscall(
        RECEIVER,
        super::syscall::Syscall::TryRecv {
            mask: SENDER,
            buf_ptr: recv_buf.as_mut_ptr() as usize,
            buf_len: recv_buf.len(),
            attest_caller: true,
        },
    );
    let identity = api::caller_identity::CallerIdentity::from_recv_buf(&recv_buf);
    let attested_ok = matches!(delivered, Ok(id) if id == SENDER)
        && recv_buf[..payload.len()] == *payload
        && identity == Some(api::caller_identity::CallerIdentity {
            cell_id: TEST_CELL,
            generation: sender_generation,
            sender_tid: SENDER as u64,
        });

    reset();
    attested_ok || fail("try_recv with attest_caller=true did not write valid CallerIdentity trailer")
}

/// Returns true iff all IPC producers defer foreign writes and fail safely.
pub fn self_test() -> bool {
    let ok = all_producers_defer_foreign_writes()
        & full_mailbox_refuses_without_wake()
        & quota_failure_is_fallible()
        & heap_payload_refunds_receiver_quota()
        & death_wake_precedes_later_message()
        & pending_drain_keeps_sender_context_without_relocking()
        & try_recv_attestation_writes_identity_trailer();
    if ok {
        log::info!("[selftest] IPC-PENDING: PASS (deferred, bounded, quota-safe)");
    }
    ok
}
