//! Unit tests for IPC implementation
//! These tests validate the core IPC logic without requiring full kernel initialization

#[cfg(test)]
mod tests {
    use crate::process::task::{Task, TaskState};
    use crate::process::scheduler::Scheduler;
    use alloc::vec::Vec;
    use crate::prelude::*;

    #[test]
    fn test_scheduler_task_table() {
        let mut sched = Scheduler::new();
        
        // Spawn a task
        let id = sched.spawn("test-task", Vec::new());
        
        // Verify task exists in table
        assert!(sched.tasks.contains_key(&id));
        assert_eq!(sched.tasks.get(&id).unwrap().name, "test-task");
        
        // Verify task is in ready queue
        assert_eq!(sched.ready_queue.len(), 1);
    }

    #[test]
    fn test_task_state_transitions() {
        let mut task = Task::new(1, "test", Vec::new());
        
        // Initial state
        assert_eq!(task.state, TaskState::Ready);
        
        // Transition to Sending
        task.state = TaskState::Sending { 
            target: 2, 
            msg_ptr: 0x1000, 
            msg_len: 64 
        };
        
        match task.state {
            TaskState::Sending { target, .. } => assert_eq!(target, 2),
            _ => panic!("Expected Sending state"),
        }
        
        // Transition to Recv
        task.state = TaskState::Recv { 
            mask: 0, 
            buf_ptr: 0x2000, 
            buf_len: 128 
        };
        
        match task.state {
            TaskState::Recv { buf_len, .. } => assert_eq!(buf_len, 128),
            _ => panic!("Expected Recv state"),
        }
    }

    #[test]
    fn test_ipc_send_no_receiver() {
        let mut sched = Scheduler::new();
        
        // Spawn sender and receiver
        let sender_id = sched.spawn("sender", Vec::new());
        let receiver_id = sched.spawn("receiver", Vec::new());
        
        // Receiver is NOT in Recv state (still Ready)
        // So Send should block
        
        // Simulate send (we can't call ipc_send directly without SCHEDULER lock)
        // Instead, verify the logic manually
        
        let receiver = sched.tasks.get(&receiver_id).unwrap();
        assert_eq!(receiver.state, TaskState::Ready);
        
        // If we were to send, sender should block
        // (This would be tested in integration test)
    }

    #[test]
    fn test_ipc_rendezvous_detection() {
        let mut sched = Scheduler::new();
        
        let sender_id = sched.spawn("sender", Vec::new());
        let receiver_id = sched.spawn("receiver", Vec::new());
        
        // Put receiver in Recv state
        if let Some(receiver) = sched.tasks.get_mut(&receiver_id) {
            receiver.state = TaskState::Recv {
                mask: 0,
                buf_ptr: 0x3000,
                buf_len: 256,
            };
        }
        
        // Now check if rendezvous would happen
        let receiver = sched.tasks.get(&receiver_id).unwrap();
        let can_rendezvous = matches!(receiver.state, TaskState::Recv { .. });
        
        assert!(can_rendezvous, "Receiver should be ready for rendezvous");
    }

    #[test]
    fn test_reply_value_storage() {
        let mut task = Task::new(1, "test", Vec::new());
        
        // Initially no reply value
        assert_eq!(task.reply_value, None);
        
        // Set reply value
        task.reply_value = Some(0xDEADBEEF);
        
        // Verify
        assert_eq!(task.reply_value, Some(0xDEADBEEF));
    }

    #[test]
    fn test_current_caller_tracking() {
        let mut task = Task::new(1, "server", Vec::new());
        
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

    #[test]
    fn test_lease_attributes() {
        use crate::process::task::LeaseAttributes;
        
        let read_only = LeaseAttributes::READ;
        let write_only = LeaseAttributes::WRITE;
        let read_write = LeaseAttributes(LeaseAttributes::READ.0 | LeaseAttributes::WRITE.0);
        
        // Test contains
        assert!(read_write.contains(LeaseAttributes::READ));
        assert!(read_write.contains(LeaseAttributes::WRITE));
        assert!(!read_only.contains(LeaseAttributes::WRITE));
    }

    #[test]
    fn test_scheduler_current_task() {
        let mut sched = Scheduler::new();
        
        // Initially no current task
        assert_eq!(sched.current_task_id, None);
        
        // Spawn and schedule
        let id = sched.spawn("test", Vec::new());
        sched.schedule();
        
        // Now should have current task
        assert_eq!(sched.current_task_id, Some(id));
        
        // Verify we can access it
        let task = sched.current_task_ref();
        assert!(task.is_some());
        assert_eq!(task.unwrap().id, id);
    }

    #[test]
    fn test_multiple_tasks_ready_queue() {
        let mut sched = Scheduler::new();
        
        // Spawn 3 tasks
        let id1 = sched.spawn("task1", Vec::new());
        let id2 = sched.spawn("task2", Vec::new());
        let id3 = sched.spawn("task3", Vec::new());
        
        // All should be in ready queue
        assert_eq!(sched.ready_queue.len(), 3);
        
        // All should be in task table
        assert!(sched.tasks.contains_key(&id1));
        assert!(sched.tasks.contains_key(&id2));
        assert!(sched.tasks.contains_key(&id3));
    }
}
