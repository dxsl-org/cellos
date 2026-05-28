//! Unit tests for Scheduler and Task Logic
//!
//! Intended to be run manually or via a custom runner as kernel is no_std binary.

#![allow(dead_code)]

use crate::task::scheduler::Scheduler;
use crate::task::tcb::{Task, TaskState};
use alloc::vec::Vec;
use types::CellId;

/// Manual Test Runner
pub fn run_scheduler_tests() {
    log::info!("Running Scheduler Tests...");
    test_scheduler_task_table();
    test_task_state_transitions();
    // test_ipc_* tests require more context, skipping for now
    test_reply_value_storage();
    test_current_caller_tracking();
    test_lease_attributes();
    test_round_robin_scheduling(); // New test
    test_scheduler_current_task();
    test_multiple_tasks_ready_queue();
    log::info!("Scheduler Tests Passed!");
}

fn test_scheduler_task_table() {
    let mut sched = Scheduler::new();

    // Spawn a task
    let id = sched.spawn("test-task", CellId(0), Vec::new());

    // Verify task exists in table
    assert!(sched.tasks.contains_key(&id));
    assert_eq!(sched.tasks.get(&id).unwrap().name, "test-task");

    // Verify task is in ready queue
    assert_eq!(sched.ready_queue.len(), 1);
}

fn test_task_state_transitions() {
    // Note: Task::new expects allowed_drivers
    let mut task = Task::new(1, CellId(0), "test", Vec::new());

    // Initial state
    assert_eq!(task.state, TaskState::Ready);

    // Transition to Sending
    task.state = TaskState::Sending {
        target: 2,
        msg_ptr: 0x1000,
        msg_len: 64,
    };

    match task.state {
        TaskState::Sending { target, .. } => assert_eq!(target, 2),
        _ => panic!("Expected Sending state"),
    }

    // Transition to Recv
    task.state = TaskState::Recv {
        mask: 0,
        buf_ptr: 0x2000,
        buf_len: 128,
    };

    match task.state {
        TaskState::Recv { buf_len, .. } => assert_eq!(buf_len, 128),
        _ => panic!("Expected Recv state"),
    }
}

fn test_reply_value_storage() {
    let mut task = Task::new(1, CellId(0), "test", Vec::new());

    // Initially no reply value
    assert_eq!(task.reply_value, None);

    // Set reply value
    task.reply_value = Some(0xDEADBEEF);

    // Verify
    assert_eq!(task.reply_value, Some(0xDEADBEEF));
}

fn test_current_caller_tracking() {
    let mut task = Task::new(1, CellId(0), "server", Vec::new());

    // Initially no caller
    assert_eq!(task.current_caller, None);

    // Client 5 sends to us
    task.current_caller = Some(5);

    // Verify
    assert_eq!(task.current_caller, Some(5));

    // After reply, clear
    task.current_caller = None;
    assert_eq!(task.current_caller, None);
}

fn test_lease_attributes() {
    use crate::task::tcb::LeaseAttributes;

    let read_only = LeaseAttributes::READ;
    // let write_only = LeaseAttributes::WRITE; // Not used
    let read_write = LeaseAttributes(LeaseAttributes::READ.0 | LeaseAttributes::WRITE.0);

    // Test contains
    assert!(read_write.contains(LeaseAttributes::READ));
    assert!(read_write.contains(LeaseAttributes::WRITE));
    assert!(!read_only.contains(LeaseAttributes::WRITE));
}

fn test_round_robin_scheduling() {
    let mut sched = Scheduler::new();

    let id1 = sched.spawn("task1", CellId(0), Vec::new());
    let id2 = sched.spawn("task2", CellId(0), Vec::new());

    // pick_next -> should be task1
    sched.pick_next(); // Selects task1
    assert_eq!(sched.current_task_id, Some(id1));

    // pick_next -> should be task2 (Round Robin)
    // Note: pick_next() logic:
    // 1. If current (task1) is Running, force it to Ready locally?
    // In pick_next: "Decide if current task needs to yield".
    // "if task.state == TaskState::Running { task.state = TaskState::Ready; push_back }"
    // But spawn() sets state to Ready.
    // First pick_next sets task1 to Running.
    // Second pick_next sees task1 is Running. Moves it to Ready Queue end. Pop task2.
    sched.pick_next();
    assert_eq!(sched.current_task_id, Some(id2));

    // pick_next -> should be task1 again
    sched.pick_next();
    assert_eq!(sched.current_task_id, Some(id1));
}

fn test_scheduler_current_task() {
    let mut sched = Scheduler::new();

    // Initially no current task
    assert_eq!(sched.current_task_id, None);

    // Spawn and schedule
    let id = sched.spawn("test", CellId(0), Vec::new());
    
    // Sched::pick_next would be called by yield/interrupt.
    // We simulate it here.
    let _ = sched.pick_next();

    // Now should have current task (if pick_next selected it)
    assert_eq!(sched.current_task_id, Some(id));

    // Verify we can access it
    let task = sched.current_task_ref();
    assert!(task.is_some());
    assert_eq!(task.unwrap().id, id);
}

fn test_multiple_tasks_ready_queue() {
    let mut sched = Scheduler::new();

    // Spawn 3 tasks
    let id1 = sched.spawn("task1", CellId(0), Vec::new());
    let id2 = sched.spawn("task2", CellId(0), Vec::new());
    let id3 = sched.spawn("task3", CellId(0), Vec::new());

    // All should be in ready queue
    assert_eq!(sched.ready_queue.len(), 3);

    // All should be in task table
    assert!(sched.tasks.contains_key(&id1));
    assert!(sched.tasks.contains_key(&id2));
    assert!(sched.tasks.contains_key(&id3));
}
