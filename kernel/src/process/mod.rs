pub mod syscall;
pub mod task;
pub mod scheduler;
pub mod drivers;
pub mod ipc_test;

#[cfg(test)]
mod tests;

use scheduler::Scheduler;
use log::info;
use crate::sync::Spinlock;
use crate::prelude::*;

// Global Scheduler Instance
pub(crate) static SCHEDULER: Spinlock<Option<Scheduler>> = Spinlock::new(None);

// Global Tick Counter
static TICKS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

// Helper context to save the initial boot/kernel state during first task switch
static mut BOOT_CONTEXT: crate::arch::context::Context = crate::arch::context::Context {
    ra: 0, sp: 0, s0: 0, s1: 0, s2: 0, s3: 0, s4: 0, s5: 0, s6: 0, s7: 0, s8: 0, s9: 0, s10: 0, s11: 0, mepc: 0, mstatus: 0, gp: 0, tp: 0,
};

// Trampoline for Thread Spawning
#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    ".section .text\n",
    ".global thread_trampoline\n",
    ".align 4\n",
    "thread_trampoline:\n",
    "    csrsi mstatus, 0x8\n",
    "    mv a0, s0\n",
    "    jr s1\n"
);

extern "C" {
    pub fn thread_trampoline();
}

pub fn get_kernel_gp_tp() -> (usize, usize) {
    let gp: usize;
    let tp: usize;
    unsafe {
        #[cfg(target_arch = "riscv64")]
        {
            core::arch::asm!("mv {0}, gp", out(reg) gp);
            core::arch::asm!("mv {0}, tp", out(reg) tp);
        }
        #[cfg(not(target_arch = "riscv64"))]
        { gp = 0; tp = 0; }
    }
    (gp, tp)
}

pub fn system_ticks() -> usize {
    TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn tick() {
    TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

pub fn init() {
    info!("Process: Initializing Scheduler...");
    let mut sched_guard = SCHEDULER.lock();
    
    // SAFETY: Use ptr::write to overwrite the Spinlock guard's data WITHOUT dropping the old value.
    // This prevents "Freed node aliases existing hole" panic on soft reboot (where .data persists but Heap is reset).
    unsafe {
        core::ptr::write(&mut *sched_guard, Some(Scheduler::new()));
    }
    
    drivers::init();

    unsafe {
        ostd::syscall::register_trap_handler(crate::process::syscall::handle_software_trap);
    }
    
    if let Some(s) = sched_guard.as_mut() {
        let id = s.spawn("init", alloc::vec::Vec::new());
        if let Some(task) = s.tasks.get_mut(&id) {
            let entry = vios_shell::app_main as *const () as usize;
            let (gp, tp) = get_kernel_gp_tp();
            task.context.ra = entry;
            task.context.mepc = entry;
            task.context.mstatus = 0x1800; // MPP=M-mode
            task.context.gp = gp;
            task.context.tp = tp;
            info!("Process: Linked Task 'init' (ID {}) to vios_shell::app_main (0x{:X})", id, entry);
        }
    }
}

/// Core scheduling logic: picks next task and performs switch OUTSIDE of the lock.
pub fn yield_cpu() {
    let switch_info = if let Some(sched) = SCHEDULER.lock().as_mut() {
        sched.pick_next()
    } else {
        None
    };

    if let Some((curr, next)) = switch_info {
        unsafe {
            use crate::arch::context::Context;
            
            let final_curr = if curr.is_null() {
                &mut BOOT_CONTEXT as *mut _
            } else {
                curr
            };
            
            #[cfg(target_arch = "riscv64")]
            {
                Context::switch(final_curr, next);
            }
            
            // NOTE: Code sau Context::switch sẽ KHÔNG chạy vì nó có noreturn!
            // Khi task được switch về, nó sẽ tiếp tục từ nơi nó đã yield.
        }
    }
}

pub fn spawn(name: &str, allowed_drivers: alloc::vec::Vec<usize>) -> usize {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        sched.spawn(name, allowed_drivers)
    } else {
        0
    }
}


pub fn spawn_kernel_task(entry: fn()) -> usize {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        let id = sched.spawn("kernel_task", alloc::vec::Vec::new());
        if let Some(task) = sched.tasks.get_mut(&id) {
            let entry_addr = entry as *const () as usize;
            let (gp, tp) = get_kernel_gp_tp();
            task.context.ra = entry_addr;
            task.context.mepc = entry_addr;
            task.context.mstatus = 0x1800; // MPP=M-mode
            task.context.gp = gp;
            task.context.tp = tp;
            // Kernel tasks share the same page table (or no page table in M-mode usually, 
            // but if we use satp we need to copy kernel mapping. 
            // Assuming no_std / bare metal M-mode for now without paging or shared kernel mapping).
            info!("Process: Spawned kernel task ID {} at 0x{:X}", id, entry_addr);
        }
        id
    } else {
        0
    }
}

pub fn spawn_with_arg(name: &str, allowed_drivers: alloc::vec::Vec<usize>, entry: usize, arg: usize) -> usize {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        sched.spawn_thread(name, allowed_drivers, entry, arg)
    } else {
        0
    }
}

pub fn current_task_id() -> usize {
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        sched.current_task_id.unwrap_or(0) 
    } else {
        0
    }
}

pub fn has_ready_tasks() -> bool {
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        sched.has_ready_tasks()
    } else {
        false
    }
}

// Helper to resolve path relative to CWD
fn resolve_path(cwd: &str, path: &str) -> alloc::string::String {
    if path.starts_with('/') {
        alloc::string::String::from(path)
    } else {
        use crate::fs::pathbuf::PathBuf;
        let mut p = PathBuf::from(cwd);
        // We need a proper join implementation that handles ".."
        // For now, simple concatenation or using PathBuf join if available
        p.push(path);
        alloc::string::String::from(p.as_str())
    }
}

#[allow(clippy::result_unit_err)]
pub fn file_open(path: &str) -> Result<usize, ()> {
    let mut full_path = alloc::string::String::from(path);
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            full_path = resolve_path(&task.cwd, path);
        }
    }

    if let Some(fs) = crate::fs::ROOT_FS.lock().as_ref() {
        if crate::fs::block_on(fs.lookup(&full_path)).is_ok() {
            if let Some(sched) = SCHEDULER.lock().as_mut() {
                if let Some(task) = sched.current_task_mut() {
                    let fd = task.open_files.len() + 3;
                    task.open_files.insert(fd, full_path);
                    return Ok(fd);
                }
            }
        }
    }
    Err(())
}

pub fn file_chdir(path: &str) -> Result<usize, ()> {
     let mut full_path = alloc::string::String::from(path);
     if let Some(sched) = SCHEDULER.lock().as_ref() {
        if let Some(task) = sched.current_task_ref() {
            full_path = resolve_path(&task.cwd, path);
        }
     }
    
     if let Some(fs) = crate::fs::ROOT_FS.lock().as_ref() {
        // Verify it exists and is a directory
        if let Ok(node) = crate::fs::block_on(fs.lookup(&full_path)) {
            // Need to check if directory but Inode trait usually has metadata
            // For now assume if lookup works, we can cd? 
            // Better: get metadata.
             let metadata = crate::fs::block_on(node.getattr());
             if let Ok(attr) = metadata {
                 if attr.file_type == crate::fs::FileType::Directory {
                      if let Some(sched) = SCHEDULER.lock().as_mut() {
                          if let Some(task) = sched.current_task_mut() {
                              task.cwd = full_path;
                              return Ok(0);
                          }
                      }
                 }
             }
        }
     }
     Err(())
}

pub fn file_getcwd(buf: &mut [u8]) -> Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        if let Some(task) = sched.current_task_ref() {
            let cwd = task.cwd.as_bytes();
            if cwd.len() > buf.len() { return Err(()); }
            buf[..cwd.len()].copy_from_slice(cwd);
            return Ok(cwd.len());
        }
    }
    Err(())
}

pub fn file_read(fd: usize, buf: &mut [u8]) -> usize {
    if fd == 0 {
        if buf.is_empty() { return 0; }
        // Blocking Read from Stdin
        loop {
            // Need to unlock console quickly to avoid blocking interrupts
            let byte = {
                 crate::process::drivers::console_drv::CONSOLE.lock().read_byte()
            };

            if let Some(b) = byte {
                buf[0] = b;
                return 1;
            }
            yield_cpu();
        }
    }

    let path = if let Some(sched) = SCHEDULER.lock().as_ref() {
        sched.current_task_id.and_then(|id| sched.tasks.get(&id)).and_then(|t| t.open_files.get(&fd).cloned())
    } else { None };

    if let Some(p) = path {
        if let Some(fs) = crate::fs::ROOT_FS.lock().as_ref() {
             if let Ok(node) = crate::fs::block_on(fs.lookup(&p)) {
                 return crate::fs::block_on(node.read_at(0, buf)).unwrap_or(0);
             }
        }
    }
    0
}

pub fn file_close(fd: usize) {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            task.open_files.remove(&fd);
        }
    }
}

pub fn file_readdir(_fd: usize, _buf: &mut [u8]) -> Result<usize, ()> {
    // Requires keeping DirStream state in Task?
    // Current Task struct only maps FD -> Path String.
    // To support readdir, we need to open the directory and keep the iterator.
    // For now, this is complex to add without refactoring Task::open_files to hold generic objects.
    // We defer implementation or implement a simpler one-shot (read all).
    Err(()) 
}

pub fn file_fstat(_fd: usize, _stat_ptr: usize) -> Result<usize, ()> {
    // Need to define Stat struct layout shared with OSTD
    Err(())
}

use log::warn;
use crate::process::task::{TaskState, LeaseAttributes};

pub fn ipc_lend(_lender_id: usize, target_id: usize, ptr: usize, len: usize, flags: u32) -> Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(target_task) = sched.tasks.get_mut(&target_id) {
            let lease_id = target_task.add_lease(ptr, len, LeaseAttributes(flags));
            return Ok(lease_id);
        }
    }
    Err(())
}

pub fn ipc_send(caller_id: usize, target_id: usize, msg_ptr: usize, msg_len: usize) -> Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if !sched.tasks.contains_key(&target_id) {
            warn!("IPC: Target Task {} not found!", target_id);
            return Err(());
        }

        let target_ready = if let Some(target) = sched.tasks.get(&target_id) {
             match target.state {
                 TaskState::Recv { mask: _, buf_ptr, buf_len } => Some((buf_ptr, buf_len)),
                 _ => None
             }
        } else { None };

        if let Some((dest_ptr, dest_len)) = target_ready {
            let app_src = msg_ptr as *const u8;
            let app_dst = dest_ptr as *mut u8;
            let copy_len = core::cmp::min(msg_len, dest_len);
            unsafe { core::ptr::copy_nonoverlapping(app_src, app_dst, copy_len); }
            
            if let Some(target) = sched.tasks.get_mut(&target_id) {
                target.state = TaskState::Ready;
                target.current_caller = Some(caller_id); 
                sched.ready_queue.push_back(target_id);
            }
            
            if let Some(caller) = sched.tasks.get_mut(&caller_id) {
                 caller.state = TaskState::Sending { target: target_id, msg_ptr, msg_len }; 
            }
            return Ok(0);
        } else {
             if let Some(caller) = sched.tasks.get_mut(&caller_id) {
                 caller.state = TaskState::Sending { target: target_id, msg_ptr, msg_len };
             }
             return Ok(1);
        }
    }
    Err(())
}

pub fn ipc_recv(caller_id: usize, mask: usize, buf_ptr: usize, buf_len: usize) -> Result<usize, ()> {
     if let Some(sched) = SCHEDULER.lock().as_mut() {
         let mut found_sender = None;
         for (tid, task) in sched.tasks.iter() {
             if let TaskState::Sending { target, msg_ptr, msg_len } = task.state {
                 if target == caller_id {
                     found_sender = Some((*tid, msg_ptr, msg_len));
                     break;
                 }
             }
         }
         
         if let Some((sender_id, src_ptr, src_len)) = found_sender {
            let app_src = src_ptr as *const u8;
            let app_dst = buf_ptr as *mut u8;
            let copy_len = core::cmp::min(src_len, buf_len);
            unsafe { core::ptr::copy_nonoverlapping(app_src, app_dst, copy_len); }
            
            if let Some(caller) = sched.tasks.get_mut(&caller_id) {
                caller.current_caller = Some(sender_id);
            }
            return Ok(sender_id);
         } else {
             if let Some(caller) = sched.tasks.get_mut(&caller_id) {
                 caller.state = TaskState::Recv { mask, buf_ptr, buf_len };
             }
             return Ok(0);
         }
     }
     Err(())
}

pub fn ipc_try_recv(caller_id: usize, _mask: usize, buf_ptr: usize, buf_len: usize) -> Result<usize, ()> {
     if let Some(sched) = SCHEDULER.lock().as_mut() {
         let mut found_sender = None;
         for (tid, task) in sched.tasks.iter() {
             if let TaskState::Sending { target, msg_ptr, msg_len } = task.state {
                 if target == caller_id {
                     found_sender = Some((*tid, msg_ptr, msg_len));
                     break;
                 }
             }
         }
         
         if let Some((sender_id, src_ptr, src_len)) = found_sender {
            let app_src = src_ptr as *const u8;
            let app_dst = buf_ptr as *mut u8;
            let copy_len = core::cmp::min(src_len, buf_len);
            unsafe { core::ptr::copy_nonoverlapping(app_src, app_dst, copy_len); }
            
            if let Some(caller) = sched.tasks.get_mut(&caller_id) {
                caller.current_caller = Some(sender_id);
            }
            return Ok(sender_id);
         } else {
             return Ok(0);
         }
     }
     Err(())
}

pub fn ipc_reply(caller_id: usize, result: usize) -> Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        let target_id = sched.tasks.get(&caller_id).and_then(|t| t.current_caller);
        if let Some(tid) = target_id {
            if let Some(t) = sched.tasks.get_mut(&tid) {
                t.state = TaskState::Ready;
                t.reply_value = Some(result);
                sched.ready_queue.push_back(tid);
            }
            if let Some(task) = sched.tasks.get_mut(&caller_id) {
                task.current_caller = None;
            }
            return Ok(0);
        }
    }
    Err(())
}

pub fn ipc_borrow_read(caller_id: usize, lease_id: usize, offset: usize, dst_ptr: usize, len: usize) -> Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        if let Some(task) = sched.tasks.get(&caller_id) {
            if let Some(lease) = task.get_lease(lease_id) {
                if !lease.attributes.contains(LeaseAttributes::READ) { return Err(()); }
                if offset + len > lease.len { return Err(()); }
                unsafe { core::ptr::copy_nonoverlapping((lease.ptr + offset) as *const u8, dst_ptr as *mut u8, len); }
                return Ok(len);
            }
        }
    }
    Err(())
}

pub fn ipc_borrow_write(caller_id: usize, lease_id: usize, offset: usize, src_ptr: usize, len: usize) -> Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        if let Some(task) = sched.tasks.get(&caller_id) {
            if let Some(lease) = task.get_lease(lease_id) {
                if !lease.attributes.contains(LeaseAttributes::WRITE) { return Err(()); }
                if offset + len > lease.len { return Err(()); }
                unsafe { core::ptr::copy_nonoverlapping(src_ptr as *const u8, (lease.ptr + offset) as *mut u8, len); }
                return Ok(len);
            }
        }
    }
    Err(())
}

pub fn ipc_grant(caller_id: usize, target_id: usize, ptr: usize, len: usize, flags: u32) -> Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(target) = sched.tasks.get_mut(&target_id) {
            let gid = target.add_grant(ptr, len, flags, caller_id);
            return Ok(gid);
        }
    }
    Err(())
}

pub fn ipc_map(caller_id: usize, grant_id: usize) -> Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        if let Some(task) = sched.tasks.get(&caller_id) {
            if let Some(grant) = task.get_grant(grant_id) {
                return Ok(grant.ptr);
            }
        }
    }
    Err(())
}

/// Get scheduler statistics
pub fn scheduler_stats() -> (usize, usize) {
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        (sched.tasks.len(), sched.ready_queue.len())
    } else {
        (0, 0)
    }
}

pub fn futex_wait(caller_id: usize, addr: usize, val: u32) -> Result<usize, ()> {
    // Check condition
    unsafe {
        let current_val = *(addr as *const u32);
        if current_val != val {
            return Err(()); // EAGAIN
        }
    }

    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&caller_id) {
            task.state = TaskState::FutexWait { addr };
            return Ok(0);
        }
    }
    Err(())
}

pub fn futex_wake(_caller_id: usize, addr: usize, count: usize) -> Result<usize, ()> {
    let mut woken = 0;
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        let mut to_wake = alloc::vec::Vec::new();
        
        // Scan for waiting tasks
        for (tid, task) in sched.tasks.iter() {
             // Skip self? Futex wake usually doesn't wake self (self is running).
             if let TaskState::FutexWait { addr: wa_addr } = task.state {
                 if wa_addr == addr {
                     to_wake.push(*tid);
                     if to_wake.len() >= count { break; }
                 }
             }
        }
        
        woken = to_wake.len();

        // Wake them up
        for tid in to_wake {
            if let Some(task) = sched.tasks.get_mut(&tid) {
                task.state = TaskState::Ready;
                sched.ready_queue.push_back(tid);
            }
        }
    }
    Ok(woken)
}

pub fn print_user_log(msg: &str) {
    // If msg ends with newline, trim it because info! adds one.
    // Actually, userspace println! sends newline.
    // We want "USER: " prefix.
    info!("USER: {}", msg.trim_end());
}
