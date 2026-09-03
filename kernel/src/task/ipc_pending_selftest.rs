//! Boot self-test for receiver-owned IPC delivery.
//!
//! Runs after scheduler initialization and before secondary harts start. Each
//! producer receives a synthetic `Recv` task whose buffer pointer is deliberately
//! invalid: success proves the producer queued owned bytes without touching the
//! foreign buffer. Teardown removes every synthetic task and ready-queue entry.

use super::completion_selftest::{insert, queue, remove};
use super::tcb::{Task, TaskState, HOTSWAP_MSG_QUEUE_DEPTH, INPUT_EVENT_QUEUE_DEPTH};
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
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

fn prepare_completion_wait_receiver() {
    insert(SENDER, TEST_CELL);
    insert(RECEIVER, TEST_CELL + 1);
    if let Some(task) = super::SCHEDULER
        .lock()
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&RECEIVER))
    {
        task.state = TaskState::WaitCompletion {
            source: api::completion::source::NET_RX,
            deadline: None,
        };
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

fn verify_completion_wait_wake(expected: &[u8], ready_before: usize, sender_blocks: bool) -> bool {
    let guard = super::SCHEDULER.lock();
    let Some(sched) = guard.as_ref() else {
        return fail("scheduler missing after completion-wait IPC wake");
    };
    let Some(receiver) = sched.tasks.get(&RECEIVER) else {
        return fail("completion-wait receiver disappeared");
    };
    let sender_generation = sched
        .tasks
        .get(&SENDER)
        .map(|task| task.cell_generation)
        .unwrap_or(0);
    let receiver_ok = receiver.state == TaskState::Ready
        && matches!(
            receiver.pending_msgs.as_slice(),
            [msg]
                if msg.sender_tid == SENDER
                    && msg.payload() == expected
                    && msg.wire_header().is_some_and(|header| {
                        header.sender_tid == SENDER
                            && header.sender_cell_id == TEST_CELL
                            && header.sender_generation == sender_generation
                            && header.delivery_id != 0
                    })
        );
    let sender_ok = sched.tasks.get(&SENDER).is_some_and(|sender| {
        if sender_blocks {
            matches!(
                sender.state,
                TaskState::Sending { target, .. } if target == RECEIVER
            )
        } else {
            sender.state == TaskState::Ready
        }
    });
    receiver_ok && sender_ok && super::hart_local::ready::total_ready_count() == ready_before + 1
}

/// The state publication is the handoff boundary: a producer arriving before
/// `yield_cpu` or after the switch observes the same state and must perform the
/// same single ready transition.
fn all_producers_wake_net_rx_completion_wait() -> bool {
    let payload = b"completion-ipc";
    let mut ok = true;

    prepare_completion_wait_receiver();
    let ready_before = super::hart_local::ready::total_ready_count();
    if super::ipc_send(SENDER, RECEIVER, payload.as_ptr() as usize, payload.len()) != Ok(1)
        || !verify_completion_wait_wake(payload, ready_before, true)
    {
        ok = fail("ipc_send did not wake NET_RX wait while preserving blocking send");
    }
    reset();

    prepare_completion_wait_receiver();
    let ready_before = super::hart_local::ready::total_ready_count();
    if super::ipc_post_nonblock(SENDER, RECEIVER, payload).is_err()
        || !verify_completion_wait_wake(payload, ready_before, false)
    {
        ok = fail("ipc_post_nonblock did not wake NET_RX completion wait");
    }
    reset();

    prepare_completion_wait_receiver();
    let ready_before = super::hart_local::ready::total_ready_count();
    let saved_input = super::drivers::driver_cell::INPUT_CELL_TID.load(Ordering::Acquire);
    super::drivers::driver_cell::INPUT_CELL_TID.store(SENDER, Ordering::Release);
    let sent =
        super::ipc_try_send(SENDER, RECEIVER, payload.as_ptr() as usize, payload.len()).is_ok();
    super::drivers::driver_cell::INPUT_CELL_TID.store(saved_input, Ordering::Release);
    if !sent || !verify_completion_wait_wake(payload, ready_before, false) {
        ok = fail("trusted ipc_try_send did not wake NET_RX completion wait");
    }
    reset();

    prepare_completion_wait_receiver();
    let ready_before = super::hart_local::ready::total_ready_count();
    let saved_input = super::drivers::driver_cell::INPUT_CELL_TID.load(Ordering::Acquire);
    super::drivers::driver_cell::INPUT_CELL_TID.store(0, Ordering::Release);
    let rejected =
        super::ipc_try_send(SENDER, RECEIVER, payload.as_ptr() as usize, payload.len()).is_err();
    super::drivers::driver_cell::INPUT_CELL_TID.store(saved_input, Ordering::Release);
    let unchanged = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&RECEIVER))
            .is_some_and(|task| {
                matches!(
                    task.state,
                    TaskState::WaitCompletion {
                        source: api::completion::source::NET_RX,
                        ..
                    }
                ) && task.pending_msgs.is_empty()
            })
    };
    if !rejected || !unchanged || super::hart_local::ready::total_ready_count() != ready_before {
        ok = fail("untrusted ipc_try_send broadened completion-wait admission");
    }
    reset();
    ok
}

fn ready_receiver_is_not_enqueued_twice() -> bool {
    prepare_completion_wait_receiver();
    let ready_before = super::hart_local::ready::total_ready_count();
    let first = super::ipc_post_nonblock(SENDER, RECEIVER, b"first");
    let second = super::ipc_post_nonblock(SENDER, RECEIVER, b"second");
    let unchanged_ready_depth = super::hart_local::ready::total_ready_count() == ready_before + 1;
    let messages_preserved = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&RECEIVER))
            .is_some_and(|task| {
                matches!(
                    task.pending_msgs.as_slice(),
                    [first, second]
                        if first.payload() == b"first" && second.payload() == b"second"
                )
            })
    };
    reset();
    first.is_ok() && second.is_ok() && unchanged_ready_depth && messages_preserved
        || fail("IPC queued a receiver that was already Ready more than once")
}

fn timer_completion_wait_remains_deadline_only() -> bool {
    insert(SENDER, TEST_CELL);
    insert(RECEIVER, TEST_CELL + 1);
    let ready_before = super::hart_local::ready::total_ready_count();
    let queued_before_park = super::ipc_post_nonblock(SENDER, RECEIVER, b"before-timer-park");
    let decision = {
        let mut guard = super::SCHEDULER.lock();
        guard
            .as_mut()
            .and_then(|sched| sched.tasks.get_mut(&RECEIVER))
            .map(|task| {
                super::completion_wait::publish_wait_state_locked(
                    task,
                    RECEIVER,
                    api::completion::source::TIMER,
                    Some(u64::MAX),
                )
            })
    };
    let posted_while_waiting = super::ipc_post_nonblock(SENDER, RECEIVER, b"timer-ipc");
    let unchanged = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&RECEIVER))
            .is_some_and(|task| {
                matches!(
                    task.state,
                    TaskState::WaitCompletion {
                        source: api::completion::source::TIMER,
                        ..
                    }
                ) && matches!(
                    task.pending_msgs.as_slice(),
                    [before, during]
                        if before.payload() == b"before-timer-park"
                            && during.payload() == b"timer-ipc"
                )
            })
    };
    let ready_unchanged = super::hart_local::ready::total_ready_count() == ready_before;
    reset();
    queued_before_park.is_ok()
        && decision == Some(super::completion_wait::CompletionParkDecision::Parked)
        && posted_while_waiting.is_ok()
        && unchanged
        && ready_unchanged
        || fail("IPC interrupted a TIMER completion wait")
}

fn fill_completion_wait_mailbox(depth: usize) -> bool {
    prepare_completion_wait_receiver();
    let mut guard = super::SCHEDULER.lock();
    let Some(task) = guard
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&RECEIVER))
    else {
        return false;
    };
    for _ in 0..depth {
        if super::queue_pending_msg(task, SENDER, b"x", depth).is_err() {
            return false;
        }
    }
    true
}

fn completion_wait_mailbox_is_unchanged(depth: usize, ready_before: usize) -> bool {
    let guard = super::SCHEDULER.lock();
    let Some(sched) = guard.as_ref() else {
        return false;
    };
    sched.tasks.get(&RECEIVER).is_some_and(|task| {
        matches!(
            task.state,
            TaskState::WaitCompletion {
                source: api::completion::source::NET_RX,
                ..
            }
        ) && task.pending_msgs.len() == depth
    }) && sched
        .tasks
        .get(&SENDER)
        .is_some_and(|task| task.state == TaskState::Ready)
        && super::hart_local::ready::total_ready_count() == ready_before
}

fn full_completion_wait_mailbox_never_wakes() -> bool {
    let payload = b"overflow";
    let mut ok = true;

    if !fill_completion_wait_mailbox(HOTSWAP_MSG_QUEUE_DEPTH) {
        reset();
        return fail("could not fill Send completion-wait mailbox");
    }
    let ready_before = super::hart_local::ready::total_ready_count();
    let refused = super::ipc_send(SENDER, RECEIVER, payload.as_ptr() as usize, payload.len())
        == Err(super::IpcSendError::Backpressure);
    if !refused || !completion_wait_mailbox_is_unchanged(HOTSWAP_MSG_QUEUE_DEPTH, ready_before) {
        ok = fail("full mailbox Send mutated or woke completion waiter");
    }
    reset();

    if !fill_completion_wait_mailbox(HOTSWAP_MSG_QUEUE_DEPTH) {
        reset();
        return fail("could not fill post completion-wait mailbox");
    }
    let ready_before = super::hart_local::ready::total_ready_count();
    let refused = super::ipc_post_nonblock(SENDER, RECEIVER, payload).is_err();
    if !refused || !completion_wait_mailbox_is_unchanged(HOTSWAP_MSG_QUEUE_DEPTH, ready_before) {
        ok = fail("full mailbox post mutated or woke completion waiter");
    }
    reset();

    if !fill_completion_wait_mailbox(INPUT_EVENT_QUEUE_DEPTH) {
        reset();
        return fail("could not fill TrySend completion-wait mailbox");
    }
    let ready_before = super::hart_local::ready::total_ready_count();
    let saved_input = super::drivers::driver_cell::INPUT_CELL_TID.load(Ordering::Acquire);
    super::drivers::driver_cell::INPUT_CELL_TID.store(SENDER, Ordering::Release);
    let refused =
        super::ipc_try_send(SENDER, RECEIVER, payload.as_ptr() as usize, payload.len()).is_err();
    super::drivers::driver_cell::INPUT_CELL_TID.store(saved_input, Ordering::Release);
    if !refused || !completion_wait_mailbox_is_unchanged(INPUT_EVENT_QUEUE_DEPTH, ready_before) {
        ok = fail("full mailbox TrySend mutated or woke completion waiter");
    }
    reset();
    ok
}
#[cfg(target_arch = "riscv64")]
fn completion_wait_publication_arms_existing_handoff() -> bool {
    insert(RECEIVER, TEST_CELL + 1);
    let hart = super::hart_local::current_hart_id();
    let saved_current = super::hart_local::ready::current_task_id_for(hart);
    let saved_outgoing = super::hart_local::ready::outgoing_context_save_task_id_for(hart);
    super::hart_local::ready::set_current_task_id(hart, RECEIVER);
    let decision = {
        let mut guard = super::SCHEDULER.lock();
        guard
            .as_mut()
            .and_then(|sched| sched.tasks.get_mut(&RECEIVER))
            .map(|task| {
                super::completion_wait::publish_wait_state_locked(
                    task,
                    RECEIVER,
                    api::completion::source::NET_RX,
                    None,
                )
            })
    };
    let armed = super::hart_local::ready::outgoing_context_save_task_id_for(hart) == RECEIVER;
    super::hart_local::ready::set_current_task_id(hart, saved_current);
    super::hart_local::ready::begin_outgoing_context_save(hart, saved_outgoing);
    reset();
    decision == Some(super::completion_wait::CompletionParkDecision::Parked) && armed
        || fail("completion-wait publication did not arm the existing SMP handoff")
}

#[cfg(not(target_arch = "riscv64"))]
fn completion_wait_publication_arms_existing_handoff() -> bool {
    true
}

fn ipc_before_park_returns_raw_zero() -> bool {
    insert(SENDER, TEST_CELL);
    insert(RECEIVER, TEST_CELL + 1);
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        super::completion::deliver_pending_wakes(sched);
    }
    let Some(completions) = queue(RECEIVER) else {
        reset();
        return fail("pre-park case could not reach completion queue");
    };
    let Some(slot) = completions.reserve() else {
        reset();
        return fail("pre-park case could not reserve completion slot");
    };
    let Some(waiter) = super::completion_wait::WaiterRegistration::new(
        &completions,
        RECEIVER,
        slot,
        api::completion::source::NET_RX,
    ) else {
        let _ = completions.release(slot);
        reset();
        return fail("pre-park case could not register waiter");
    };
    super::waker::arm_net_rx(completions.clone(), slot);

    let ready_before = super::hart_local::ready::total_ready_count();
    let posted = super::ipc_post_nonblock(SENDER, RECEIVER, b"pre-park");
    let decision = {
        let mut guard = super::SCHEDULER.lock();
        guard
            .as_mut()
            .and_then(|sched| sched.tasks.get_mut(&RECEIVER))
            .map(|task| {
                super::completion_wait::publish_wait_state_locked(
                    task,
                    RECEIVER,
                    api::completion::source::NET_RX,
                    None,
                )
            })
    };
    let mut untouched = [0xa5u8; api::completion::COMPLETION_LEN];
    let outcome = super::completion_wait::finish_net_rx_wait(
        RECEIVER,
        untouched.as_mut_ptr() as usize,
        &completions,
    );
    drop(waiter);

    let task_ok = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&RECEIVER))
            .is_some_and(|task| {
                task.state == TaskState::Ready
                    && task.completion_wait.is_none()
                    && matches!(
                        task.pending_msgs.as_slice(),
                        [msg] if msg.payload() == b"pre-park"
                    )
            })
    };
    let ok = posted.is_ok()
        && decision == Some(super::completion_wait::CompletionParkDecision::MailboxPending)
        && outcome == Ok(0)
        && untouched == [0xa5u8; api::completion::COMPLETION_LEN]
        && completions.reserved() == 0
        && completions.drainable() == 0
        && !super::completion::wakes_pending()
        && super::hart_local::ready::total_ready_count() == ready_before
        && task_ok;
    reset();
    ok || fail("IPC-before-park did not abort NET_RX wait cleanly with raw zero")
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
        && identity
            == Some(api::caller_identity::CallerIdentity {
                flags: 0,
                cell_id: TEST_CELL,
                generation: sender_generation,
                sender_tid: SENDER as u64,
            });

    // Case 2: SENDER has bit 63 set and allowlist != u64::MAX -> flags = CALLER_FLAG_VFS_MUTATE
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&SENDER) {
            task.syscall_allowlist = 1u64 << 63;
        }
        if let Some(receiver) = sched.tasks.get_mut(&RECEIVER) {
            receiver.pending_msgs.try_push(super::tcb::PendingMsg {
                sender_tid: SENDER,
                data: super::pending_mailbox::PendingMsgData::try_copy(payload, TEST_CELL as usize)
                    .expect("inline payload copy must fit"),
                wire: None,
                enqueued_tick: 0,
            }).ok();
        }
    }
    recv_buf.fill(0);
    let mutate_delivered = super::syscall::handle_syscall(
        RECEIVER,
        super::syscall::Syscall::TryRecv {
            mask: SENDER,
            buf_ptr: recv_buf.as_mut_ptr() as usize,
            buf_len: recv_buf.len(),
            attest_caller: true,
        },
    );
    let mutate_identity = api::caller_identity::CallerIdentity::from_recv_buf(&recv_buf);
    let mutate_ok = matches!(mutate_delivered, Ok(id) if id == SENDER)
        && mutate_identity
            == Some(api::caller_identity::CallerIdentity {
                flags: api::caller_identity::CALLER_FLAG_VFS_MUTATE,
                cell_id: TEST_CELL,
                generation: sender_generation,
                sender_tid: SENDER as u64,
            });

    reset();
    (attested_ok && mutate_ok)
        || fail("try_recv with attest_caller=true did not write valid CallerIdentity trailer or mutate flags")
}

/// Returns true iff IPC publication is receiver-owned, bounded and wake-safe.
pub fn self_test() -> bool {
    let ok = all_producers_defer_foreign_writes()
        & all_producers_wake_net_rx_completion_wait()
        & ready_receiver_is_not_enqueued_twice()
        & timer_completion_wait_remains_deadline_only()
        & full_completion_wait_mailbox_never_wakes()
        & completion_wait_publication_arms_existing_handoff()
        & ipc_before_park_returns_raw_zero()
        & full_mailbox_refuses_without_wake()
        & quota_failure_is_fallible()
        & heap_payload_refunds_receiver_quota()
        & death_wake_precedes_later_message()
        & pending_drain_keeps_sender_context_without_relocking()
        & try_recv_attestation_writes_identity_trailer();
    if ok {
        log::info!("[selftest] IPC-PENDING: PASS (deferred, bounded, quota-safe, completion-wake)");
    }
    ok
}
