//! IPC System Calls (Inspired by Tock OS)
//!
//! This module defines the interface between "Cells/Silos" and the Kernel.
//! See [docs/architecture/03-driver-strategy.md] for the full rationale.


use super::task::TaskState;
use crate::prelude::*;


/// Result of a System Call
pub type SyscallResult = Result<usize, SyscallError>;

#[derive(Debug, Copy, Clone)]
pub enum SyscallError {
    InvalidDriverId,
    InvalidCommand,
    BufferTooSmall,
    PermissionDenied,
    FileNotFound,
    TryAgain,
    Unknown,
}

/// The Fundamental Verbs of ViOS IPC (Hubris ABI + Lease System)
#[derive(Debug, Copy, Clone)]
pub enum Syscall {
    /// 0: Send (Blocking Message Send)
    Send { target: usize, msg_ptr: usize, msg_len: usize },
    /// 1: Recv (Blocking Message Receive)
    Recv { mask: usize, buf_ptr: usize, buf_len: usize },
    /// 2: Reply (Unblocking Reply to Caller)
    Reply { caller: usize, result: usize },
    /// 3: SetTimer (Wake up after ticks)
    SetTimer { deadline: usize },
    /// 4: BorrowRead (Copy from Lease to Caller)
    BorrowRead { lease_id: usize, offset: usize, ptr: usize, len: usize },
    /// 5: BorrowWrite (Copy from Caller to Lease)
    BorrowWrite { lease_id: usize, offset: usize, ptr: usize, len: usize },
    /// 6: Lend (Create a Lease for Target Task)
    Lend { target: usize, ptr: usize, len: usize, flags: usize },
    /// 7: TryRecv (Non-blocking Receive)
    TryRecv { mask: usize, buf_ptr: usize, buf_len: usize },
    /// 8: Spawn (Create new Task/Thread) - Returns Task ID
    Spawn { entry: usize, arg: usize },
    /// 9: FutexWait (Wait for value at address)
    FutexWait { addr: usize, val: u32 },
    /// 10: FutexWake (Wake up waiting tasks)
    FutexWake { addr: usize, count: usize },
    /// 11: Log (Debug Print)
    Log { msg_ptr: usize, msg_len: usize },
    /// 12: Grant (Zero Copy)
    Grant { target: usize, ptr: usize, len: usize, flags: usize },
    /// 13: Map (Zero Copy)
    Map { grant_id: usize },
    
    // --- Legacy / Compatibility Layer ---
    /// 100: Service Lookup (Find driver ID by name)
    ServiceLookup { name_ptr: usize, name_len: usize },
    /// 101: Open (Path -> FD)
    Open { path_ptr: usize, path_len: usize },
    /// 102: Read (FD, Buffer -> Bytes Read)
    Read { fd: usize, buf_ptr: usize, buf_len: usize },
    /// 103: Close (FD)
    Close { fd: usize },
    /// 105: ReadDir (Read Directory Entries)
    ReadDir { fd: usize, buf_ptr: usize, buf_len: usize },
    /// 106: FStat (Get File Info)
    FStat { fd: usize, stat_ptr: usize },
    /// 107: ChDir (Change Directory)
    ChDir { path_ptr: usize, path_len: usize },
    /// 108: GetCwd (Get Current Directory)
    GetCwd { buf_ptr: usize, buf_len: usize },
    /// 104: Yield (Legacy)
    Yield,
}

/// Dispatches a system call to the appropriate handler.
/// 
/// `caller_id` is the ID of the task invoking the syscall.
pub fn handle_syscall(caller_id: usize, syscall: Syscall) -> SyscallResult {
    // Info log reduced to Debug to reduce noise
    // info!("Syscall (Task {}): Dispatched {:?}", caller_id, syscall);

    match syscall {
        // --- Hubris ABI Implementation ---
        Syscall::Send { target, msg_ptr, msg_len } => {
            let res = super::ipc_send(caller_id, target, msg_ptr, msg_len);
            match res {
                Ok(0) => Ok(0),
                Ok(1) => {
                    super::yield_cpu(); // Blocked
                    if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                        return Ok(sched.tasks.get(&caller_id).and_then(|t| t.reply_value).unwrap_or(0));
                    }
                    Ok(0)
                }
                Err(_) => Err(SyscallError::InvalidCommand),
                _ => Ok(0)
            }
        }
        Syscall::Recv { mask, buf_ptr, buf_len } => {
            let res = super::ipc_recv(caller_id, mask, buf_ptr, buf_len);
            match res {
                Ok(0) => {
                    // Blocked
                    super::yield_cpu();
                    // Resume: return who sent the message
                    if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                        return Ok(sched.tasks.get(&caller_id).and_then(|t| t.current_caller).unwrap_or(0));
                    }
                    Ok(0)
                }
                Ok(id) => Ok(id), // Got message instantly
                Err(_) => Err(SyscallError::InvalidCommand)
            }
        }
        Syscall::TryRecv { mask, buf_ptr, buf_len } => {
            // Non-blocking Recv
            let res = super::ipc_try_recv(caller_id, mask, buf_ptr, buf_len);
            match res {
                Ok(id) => Ok(id), // 0 = No message, >0 = Sender ID
                Err(_) => Err(SyscallError::InvalidCommand)
            }
        }
        Syscall::Spawn { entry, arg } => {
            let drivers = alloc::vec::Vec::new();
            let name = "thread";
            let tid = super::spawn_with_arg(name, drivers, entry, arg);
            if tid > 0 {
                Ok(tid)
            } else {
                Err(SyscallError::Unknown)
            }
        }
        Syscall::FutexWait { addr, val } => {
            // Returns Ok(0) if blocked (then yield), Err(TryAgain) if val mismatch
            match super::futex_wait(caller_id, addr, val) {
                Ok(_) => {
                    super::yield_cpu(); // Block
                    Ok(0)
                },
                Err(_) => Err(SyscallError::TryAgain) 
            }
        }
        Syscall::FutexWake { addr, count } => {
            if let Ok(n) = super::futex_wake(caller_id, addr, count) {
                Ok(n)
            } else {
                Err(SyscallError::Unknown) // Should not fail typically
            }
        }
        Syscall::Log { msg_ptr, msg_len } => {
             unsafe {
                let slice = core::slice::from_raw_parts(msg_ptr as *const u8, msg_len);
                if let Ok(msg) = core::str::from_utf8(slice) {
                    // Use crate::io::_print or similar?
                    // Kernel usually has a logger. info! is from 'log' crate.
                    crate::process::print_user_log(msg);
                }
             }
             Ok(0)
        }
        Syscall::Grant { target, ptr, len, flags } => {
             super::ipc_grant(caller_id, target, ptr, len, flags as u32).map_err(|_| SyscallError::PermissionDenied)
        }
        Syscall::Map { grant_id } => {
             super::ipc_map(caller_id, grant_id).map_err(|_| SyscallError::PermissionDenied)
        }
        Syscall::Reply { caller: _, result } => {              super::ipc_reply(caller_id, result).map_err(|_| SyscallError::InvalidCommand)
        }
        
        Syscall::Lend { target, ptr, len, flags } => {
            super::ipc_lend(caller_id, target, ptr, len, flags as u32).map_err(|_| SyscallError::PermissionDenied)
        }
        
        Syscall::BorrowRead { lease_id, offset, ptr, len } => {
             super::ipc_borrow_read(caller_id, lease_id, offset, ptr, len).map_err(|_| SyscallError::PermissionDenied)
        }
        Syscall::BorrowWrite { lease_id, offset, ptr, len } => {
             super::ipc_borrow_write(caller_id, lease_id, offset, ptr, len).map_err(|_| SyscallError::PermissionDenied)
        }
        
        // --- Legacy Implementation ---
        Syscall::Yield => {
            super::yield_cpu();
            Ok(0)
        }
        Syscall::ServiceLookup { name_ptr, name_len } => {
            unsafe {
                let slice = core::slice::from_raw_parts(name_ptr as *const u8, name_len);
                if let Ok(name) = core::str::from_utf8(slice) {
                    if let Some(id) = super::drivers::resolve(name) {
                        return Ok(id);
                    }
                }
            }
            Err(SyscallError::InvalidDriverId)
        }
        Syscall::Open { path_ptr, path_len } => {
            unsafe {
                let slice = core::slice::from_raw_parts(path_ptr as *const u8, path_len);
                if let Ok(path) = core::str::from_utf8(slice) {
                    if let Ok(fd) = super::file_open(path) {
                        return Ok(fd);
                    }
                }
            }
            Err(SyscallError::FileNotFound)
        }
        Syscall::Read { fd, buf_ptr, buf_len } => {
            unsafe {
                 let slice = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_len);
                 let read_bytes = super::file_read(fd, slice);
                 Ok(read_bytes)
            }
        }
        Syscall::Close { fd } => {
            super::file_close(fd);
            Ok(0)
        }
        Syscall::ReadDir { fd, buf_ptr, buf_len } => {
            unsafe {
                 let slice = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_len);
                 super::file_readdir(fd, slice).map_err(|_| SyscallError::Unknown)
            }
        }
        Syscall::FStat { fd, stat_ptr } => {
            super::file_fstat(fd, stat_ptr).map_err(|_| SyscallError::Unknown)
        }
        Syscall::ChDir { path_ptr, path_len } => {
             unsafe {
                let slice = core::slice::from_raw_parts(path_ptr as *const u8, path_len);
                if let Ok(path) = core::str::from_utf8(slice) {
                    if super::file_chdir(path).is_ok() {
                        return Ok(0);
                    }
                }
             }
             Err(SyscallError::FileNotFound)
        }
        Syscall::GetCwd { buf_ptr, buf_len } => {
             unsafe {
                 let slice = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_len);
                 if let Ok(len) = super::file_getcwd(slice) {
                     return Ok(len);
                 }
             }
             Err(SyscallError::BufferTooSmall)
        }
        Syscall::SetTimer { deadline } => {
            // Check if deadline passed
            let now = super::system_ticks();
            let wake_at = now + deadline; 
            
            // Sleep!
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                if let Some(task) = sched.current_task_mut() {
                    task.state = TaskState::Sleeping { until: wake_at };
                }
            }
            // Yield CPU safely
            super::yield_cpu();
            Ok(0)
        }
    }
}

/// The Trap Handler Entry Point (Simulated)
/// This matches the signature expected by ostd::syscall::register_trap_handler
pub fn handle_software_trap(syscall_id: usize, a1: usize, a2: usize, a3: usize, a4: usize) -> isize {
    let syscall = match syscall_id {
        // Hubris Standard
        0 => Syscall::Send { target: a1, msg_ptr: a2, msg_len: a3 },
        1 => Syscall::Recv { mask: a1, buf_ptr: a2, buf_len: a3 },
        2 => Syscall::Reply { caller: a1, result: a2 },
        3 => Syscall::SetTimer { deadline: a1 },
        4 => Syscall::BorrowRead { lease_id: a1, offset: a2, ptr: a3, len: a4 },
        5 => Syscall::BorrowWrite { lease_id: a1, offset: a2, ptr: a3, len: a4 },
        6 => Syscall::Lend { target: a1, ptr: a2, len: a3, flags: a4 },
        7 => Syscall::TryRecv { mask: a1, buf_ptr: a2, buf_len: a3 },
        8 => Syscall::Spawn { entry: a1, arg: a2 },
        9 => Syscall::FutexWait { addr: a1, val: a2 as u32 },
        10 => Syscall::FutexWake { addr: a1, count: a2 },
        11 => Syscall::Log { msg_ptr: a1, msg_len: a2 },
        12 => Syscall::Grant { target: a1, ptr: a2, len: a3, flags: a4 },
        13 => Syscall::Map { grant_id: a1 },
        
        // Legacy Mappings (Temporary)
        100 => Syscall::ServiceLookup { name_ptr: a1, name_len: a2 },
        101 => Syscall::Open { path_ptr: a1, path_len: a2 },
        102 => Syscall::Read { fd: a1, buf_ptr: a2, buf_len: a3 },
        103 => Syscall::Close { fd: a1 },
        105 => Syscall::ReadDir { fd: a1, buf_ptr: a2, buf_len: a3 },
        106 => Syscall::FStat { fd: a1, stat_ptr: a2 },
        107 => Syscall::ChDir { path_ptr: a1, path_len: a2 },
        108 => Syscall::GetCwd { buf_ptr: a1, buf_len: a2 },
        104 => Syscall::Yield,
        
        _ => return -1,
    };

    let caller_id = super::current_task_id(); 
    match handle_syscall(caller_id, syscall) {
        Ok(val) => val as isize,
        Err(_) => -1,
    }
}
