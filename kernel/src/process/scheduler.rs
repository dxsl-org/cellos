use alloc::collections::{BTreeMap, VecDeque};
use alloc::boxed::Box;
use log::{info, warn};
use super::task::{Task, TaskState};
use crate::prelude::*;

/// Round-Robin Scheduler with Central Task Table (Hubris-like)
pub struct Scheduler {
    pub tasks: BTreeMap<usize, Box<Task>>,
    pub ready_queue: VecDeque<usize>,
    pub current_task_id: Option<usize>,
    pub next_task_id: usize,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
            ready_queue: VecDeque::new(),
            current_task_id: None,
            next_task_id: 1,
        }
    }

    pub fn spawn(&mut self, name: &str, allowed_drivers: alloc::vec::Vec<usize>) -> usize {
        let mut task = Box::new(Task::new(self.next_task_id, name, allowed_drivers));
        task.state = TaskState::Ready;
        let id = task.id;
        
        const STACK_SIZE: usize = 131072; // 128KB
        unsafe {
            use alloc::alloc::{alloc, Layout};
            let layout = Layout::from_size_align_unchecked(STACK_SIZE, 16);
            let stack_ptr = alloc(layout);
            
            if !stack_ptr.is_null() {
                let stack_base = stack_ptr as usize;
                let stack_top = stack_base + STACK_SIZE;
                
                // Get entry point address
                let entry = task_entry_point as *const () as usize;
                
                // Get kernel gp/tp for task initialization
                let (gp, tp) = crate::process::get_kernel_gp_tp();
                
                // Initialize context with proper entry point
                task.context.sp = stack_top;
                task.context.ra = entry;
                task.context.mepc = entry;
                task.context.mstatus = 0x1800; // MPP=M-mode
                task.context.gp = gp;
                task.context.tp = tp;
                
                task.stack_base = Some(stack_base);
                info!("Task '{}' (ID {}): Stack 0x{:X}-0x{:X}, Entry 0x{:X}", 
                    name, id, stack_base, stack_top, entry);
            } else {
                warn!("Failed to allocate stack for task {}", id);
            }
        }
        
        self.tasks.insert(id, task);
        self.ready_queue.push_back(id);
        self.next_task_id += 1;
        id
    }

    pub fn spawn_thread(&mut self, name: &str, allowed_drivers: alloc::vec::Vec<usize>, entry: usize, arg: usize) -> usize {
        let mut task = Box::new(Task::new(self.next_task_id, name, allowed_drivers));
        task.state = TaskState::Ready;
        let id = task.id;
        
        const STACK_SIZE: usize = 131072; // 128KB
        unsafe {
            use alloc::alloc::{alloc, Layout};
            let layout = Layout::from_size_align_unchecked(STACK_SIZE, 16);
            let stack_ptr = alloc(layout);
            
            if !stack_ptr.is_null() {
                let stack_base = stack_ptr as usize;
                let stack_top = stack_base + STACK_SIZE;
                
                // Get kernel gp/tp for task initialization
                let (gp, tp) = crate::process::get_kernel_gp_tp();
                
                 // Initialize context with trampoline
                let trampoline = crate::process::thread_trampoline as usize;

                task.context.sp = stack_top;
                task.context.ra = trampoline; // Jump to Trampoline
                task.context.mepc = trampoline;
                
                task.context.s0 = arg;   // Argument in Saved Reg 0
                task.context.s1 = entry; // Real Entry in Saved Reg 1

                task.context.mstatus = 0x1800; // MPP=M-mode
                task.context.gp = gp;
                task.context.tp = tp;
                
                task.stack_base = Some(stack_base);
                info!("Thread '{}' (ID {}): Stack 0x{:X}-0x{:X}, Entry 0x{:X}, Arg 0x{:X}", 
                    name, id, stack_base, stack_top, entry, arg);
            } else {
                warn!("Failed to allocate stack for thread {}", id);
            }
        }
        
        self.tasks.insert(id, task);
        self.ready_queue.push_back(id);
        info!("Thread spawned: ID {} added to ready_queue (queue len: {})", id, self.ready_queue.len());
        self.next_task_id += 1;
        id
    }

    /// Picks the next task to run and returns pointers for context switch.
    /// Returns: Option<(current_context_ptr, next_context_ptr)>
    pub fn pick_next(&mut self) -> Option<(*mut crate::arch::context::Context, *const crate::arch::context::Context)> {
        let now = crate::process::system_ticks();

        // 1. Wake up sleeping tasks
        let mut waking_tasks = VecDeque::new();
        for (id, task) in self.tasks.iter_mut() {
            let mut should_wake = false;
            if let TaskState::Sleeping { until } = &task.state {
                if now >= *until {
                    should_wake = true;
                }
            }
            if should_wake {
                task.state = TaskState::Ready;
                waking_tasks.push_back(*id);
            }
        }
        for id in waking_tasks {
            self.ready_queue.push_back(id);
        }

        // 2. Decide if current task needs to yield
        let current_id = self.current_task_id;
        if let Some(cid) = current_id {
            if let Some(task) = self.tasks.get_mut(&cid) {
                if task.state == TaskState::Running {
                    task.state = TaskState::Ready;
                    self.ready_queue.push_back(cid);
                }
            }
        }

        // 3. Get next task
        let next_id = self.ready_queue.pop_front();
        
        if let Some(nid) = next_id {
            if let Some(next_task) = self.tasks.get_mut(&nid) {
                next_task.state = TaskState::Running;
            }

            if Some(nid) == current_id {
                self.current_task_id = Some(nid);
                return None; // No switch needed
            }

            unsafe {
                let tasks_map_ptr = &mut self.tasks as *mut BTreeMap<usize, Box<Task>>;
                let next_ctx = (*tasks_map_ptr).get_mut(&nid).map(|t| &t.context as *const _);
                self.current_task_id = Some(nid);

                if let Some(cid) = current_id {
                    let curr_ctx = (*tasks_map_ptr).get_mut(&cid).map(|t| &mut t.context as *mut _);
                    if let (Some(c), Some(n)) = (curr_ctx, next_ctx) {
                        return Some((c, n));
                    }
                } else {
                    // First switch
                    if let Some(n) = next_ctx {
                        return Some((core::ptr::null_mut(), n));
                    }
                }
            }
        } else {
            self.current_task_id = None;
        }

        None
    }

    pub fn current_task_mut(&mut self) -> Option<&mut Task> {
        self.current_task_id.and_then(|id| self.tasks.get_mut(&id).map(|b| &mut **b))
    }
    
    pub fn current_task_ref(&self) -> Option<&Task> {
        self.current_task_id.and_then(|id| self.tasks.get(&id).map(|b| &**b))
    }
    
    pub fn has_ready_tasks(&self) -> bool {
        !self.ready_queue.is_empty()
    }
}

/// Default entry point for kernel tasks
#[no_mangle]
extern "C" fn task_entry_point() {
    unsafe {
         crate::process::SCHEDULER.force_unlock();
         crate::arch::trap::init();
         // Enable Interrupts MANUALLY now that we're safe and stack is clean
         crate::arch::trap::enable_interrupts();
    }
    info!("Task started!");
    loop {
        for _ in 0..10_000_000 { core::hint::spin_loop(); }
        info!("Task tick (ID: {})...", crate::process::current_task_id());
        crate::process::yield_cpu();
    }
}
