//! Unit tests for bounded copied IPC wire message.

use super::ipc_wire::*;

#[test]
fn wire_message_bounds() {
    let header = IpcWireHeader {
        sender_tid: 1,
        sender_cell_id: 10,
        sender_generation: 1,
        delivery_id: 7,
        async_op: 0,
    };
    let small_payload = [0xabu8; 128];
    let msg =
        IpcWireMessage::try_new(header, &small_payload).expect("should allocate small payload");
    assert_eq!(msg.len(), 128);
    assert_eq!(msg.as_slice(), &small_payload[..]);
    assert_eq!(msg.header.sender_tid, 1);
    assert_eq!(msg.header.sender_cell_id, 10);
    assert_eq!(msg.header.sender_generation, 1);
    assert_eq!(msg.header.delivery_id, 7);

    let empty = IpcWireMessage::try_new(header, &[]).expect("should allow empty payload");
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);

    // Bounded max payload
    let max_payload = alloc::vec![0xcd; MAX_IPC_WIRE_PAYLOAD];
    let msg_max = IpcWireMessage::try_new(header, &max_payload).expect("should allow max payload");
    assert_eq!(msg_max.len(), MAX_IPC_WIRE_PAYLOAD);

    // Exceeding payload must fail
    let overlarge = alloc::vec![0xef; MAX_IPC_WIRE_PAYLOAD + 1];
    assert!(IpcWireMessage::try_new(header, &overlarge).is_err());
}

// ---- OSD-3: a blocking send never targets a tombstone ---------------------

mod tombstone_target {
    use super::super::scheduler::Scheduler;
    use super::super::tcb::{Task, TaskState};
    use super::super::{ipc_send_kernel, ipc_target_is_tombstone, IpcSendError, SCHEDULER};
    use crate::sync::SpinlockGuard;
    use types::CellId;

    const CALLER: usize = 52_001;
    const TARGET: usize = 52_002;

    /// Serialises on the shared test state and swaps a private scheduler into
    /// the global slot for the duration of one send.
    struct SchedulerStateGuard {
        _guard: SpinlockGuard<'static, ()>,
        saved: Option<Scheduler>,
    }

    impl SchedulerStateGuard {
        fn new() -> Self {
            let guard = crate::TEST_STATE_LOCK.lock();
            let saved = SCHEDULER.lock().take();
            Self {
                _guard: guard,
                saved,
            }
        }

        fn set(&mut self, scheduler: Scheduler) {
            *SCHEDULER.lock() = Some(scheduler);
        }
    }

    impl Drop for SchedulerStateGuard {
        fn drop(&mut self) {
            *SCHEDULER.lock() = self.saved.take();
        }
    }

    fn task(tid: usize, state: TaskState) -> Task {
        let mut task = Task::new(
            tid,
            CellId(60 + tid as u64 % 7),
            "osd3",
            alloc::vec::Vec::new(),
        );
        task.state = state;
        task
    }

    fn recv() -> TaskState {
        TaskState::Recv {
            mask: 0,
            buf_ptr: 0x0200_0000,
            buf_len: 64,
            deadline: None,
        }
    }

    #[test]
    fn only_retiring_and_terminated_are_tombstones() {
        for state in [TaskState::Retiring, TaskState::Terminated] {
            assert!(
                ipc_target_is_tombstone(&task(TARGET, state.clone())),
                "{state:?}"
            );
        }
        for state in [
            TaskState::Ready,
            TaskState::Running,
            recv(),
            TaskState::Sending {
                target: CALLER,
                delivery_id: 1,
            },
            TaskState::Frozen { swap_id: 1 },
        ] {
            assert!(
                !ipc_target_is_tombstone(&task(TARGET, state.clone())),
                "{state:?}"
            );
        }
    }

    /// Caller `Ready`, target in `state`: the result of one kernel send, the
    /// caller's state afterwards and the number of messages left in the target mailbox.
    fn send_to(state: TaskState) -> (Result<usize, IpcSendError>, TaskState, usize) {
        let mut guard = SchedulerStateGuard::new();
        let mut sched = Scheduler::new();
        sched.tasks.insert(
            CALLER,
            alloc::boxed::Box::new(task(CALLER, TaskState::Ready)),
        );
        sched
            .tasks
            .insert(TARGET, alloc::boxed::Box::new(task(TARGET, state)));
        guard.set(sched);
        let result = ipc_send_kernel(CALLER, TARGET, b"reply");
        let sched = SCHEDULER.lock();
        let sched = sched.as_ref().expect("scheduler");
        let caller_state = sched.tasks[&CALLER].state.clone();
        let queued = sched.tasks[&TARGET].pending_msgs.len();
        (result, caller_state, queued)
    }

    #[test]
    fn send_to_a_tombstone_fails_without_parking_or_queueing() {
        for state in [TaskState::Retiring, TaskState::Terminated] {
            let (result, caller, queued) = send_to(state.clone());
            assert_eq!(result, Err(IpcSendError::TargetGone), "{state:?}");
            assert_eq!(caller, TaskState::Ready, "{state:?}: caller must not park");
            assert_eq!(queued, 0, "{state:?}: nothing may reach the dead mailbox");
        }
    }

    #[test]
    fn send_to_a_live_receiver_is_unchanged() {
        let (result, caller, queued) = send_to(recv());
        assert_eq!(result, Ok(0));
        assert_eq!(caller, TaskState::Ready);
        assert_eq!(queued, 1);
    }
}
