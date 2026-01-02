#![allow(unsafe_code)]

#[derive(Debug, Copy, Clone)]
pub enum SyscallResult {
    Ok(usize),
    Err(SyscallError),
}

use thiserror::Error;

#[derive(Debug, Copy, Clone, Error)]
pub enum SyscallError {
    #[error("Invalid Driver ID given")]
    InvalidDriverId,
    #[error("Invalid Command ID given")]
    InvalidCommand,
    #[error("Buffer provided is too small")]
    BufferTooSmall,
    #[error("Permission Denied")]
    PermissionDenied,
    #[error("File Not Found")]
    FileNotFound,
    #[error("Try Again (Futex)")]
    TryAgain,
    #[error("Unknown Error")]
    Unknown,
}

pub type TrapHandler = fn(usize, usize, usize, usize, usize) -> isize;

pub static mut SYSCALL_TRAP: Option<TrapHandler> = None;

/// Called by Kernel to register the Trap Handler
pub unsafe fn register_trap_handler(handler: TrapHandler) {
    SYSCALL_TRAP = Some(handler);
}

// --- Hubris ABI Wrappers ---

pub fn sys_send(target: usize, msg: &[u8]) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 0 = Send
             let ret = trap(0, target, msg.as_ptr() as usize, msg.len(), 0);
             SyscallResult::Ok(ret as usize)
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_recv(mask: usize, buf: &mut [u8]) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 1 = Recv
             let ret = trap(1, mask, buf.as_mut_ptr() as usize, buf.len(), 0);
             SyscallResult::Ok(ret as usize)
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_try_recv(mask: usize, buf: &mut [u8]) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 7 = TryRecv
             let ret = trap(7, mask, buf.as_mut_ptr() as usize, buf.len(), 0);
             SyscallResult::Ok(ret as usize)
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_reply(target_unused: usize, result: usize) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 2 = Reply
             let ret = trap(2, target_unused, result, 0, 0);
             SyscallResult::Ok(ret as usize)
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

// --- Secure Lease System API ---

/// Grants a lease (access rights) to a target task for a specific memory buffer.
/// Returns Ok(lease_id) on success.
pub fn sys_lend(target: usize, buf: &[u8], flags: u32) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 6 = Lend
             // Args: target, ptr, len, flags
             let ret = trap(6, target, buf.as_ptr() as usize, buf.len(), flags as usize);
             if ret >= 0 {
                 SyscallResult::Ok(ret as usize)
             } else {
                 SyscallResult::Err(SyscallError::PermissionDenied)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

/// Borrows memory from a Lease (Read).
/// Copies FROM the Lease Buffer (at offset) TO dest_buf.
pub fn sys_borrow_read(lease_id: usize, offset: usize, dest_buf: &mut [u8]) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 4 = BorrowRead
             // Args: lease_id, offset, dst=dest_buf.ptr, len=dest_buf.len
             let ret = trap(4, lease_id, offset, dest_buf.as_mut_ptr() as usize, dest_buf.len());
             if ret >= 0 {
                  SyscallResult::Ok(ret as usize)
             } else {
                  SyscallResult::Err(SyscallError::PermissionDenied)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

/// Borrows memory from a Lease (Write).
/// Copies FROM src_buf TO the Lease Buffer (at offset).
pub fn sys_borrow_write(lease_id: usize, offset: usize, src_buf: &[u8]) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 5 = BorrowWrite
             // Args: lease_id, offset, src=src_buf.ptr, len=src_buf.len
             let ret = trap(5, lease_id, offset, src_buf.as_ptr() as usize, src_buf.len());
             if ret >= 0 {
                  SyscallResult::Ok(ret as usize)
             } else {
                  SyscallResult::Err(SyscallError::PermissionDenied)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_yield() {
    // NOTE: We use direct function call instead of ecall instruction because:
    // - ViOS runs entirely in M-mode (Machine mode)
    // - ecall from M-mode is ILLEGAL in RISC-V (causes Exception 0x2)
    // - ecall is only valid from U-mode or S-mode to trap into higher privilege
    // - For M-mode only kernels, we must use direct function calls
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             trap(104, 0, 0, 0, 0);
        }
    }
}

pub fn sys_set_timer(deadline: usize) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 3 = SetTimer
             let ret = trap(3, deadline, 0, 0, 0);
             SyscallResult::Ok(ret as usize)
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_spawn(entry: usize, arg: usize) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 8 = Spawn
             let ret = trap(8, entry, arg, 0, 0);
             if ret > 0 {
                 SyscallResult::Ok(ret as usize)
             } else {
                 SyscallResult::Err(SyscallError::Unknown)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_futex_wait(addr: &core::sync::atomic::AtomicU32, val: u32) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 9 = FutexWait
             let ret = trap(9, addr as *const _ as usize, val as usize, 0, 0);
             if ret == 0 {
                 SyscallResult::Ok(0)
             } else {
                 SyscallResult::Err(SyscallError::TryAgain)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_futex_wake(addr: &core::sync::atomic::AtomicU32, count: usize) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 10 = FutexWake
             let ret = trap(10, addr as *const _ as usize, count, 0, 0);
             if ret >= 0 {
                 SyscallResult::Ok(ret as usize)
             } else {
                 SyscallResult::Err(SyscallError::Unknown)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_grant(target: usize, ptr: usize, len: usize, flags: usize) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 12 = Grant
             let ret = trap(12, target, ptr, len, flags);
             if ret >= 0 {
                 SyscallResult::Ok(ret as usize)
             } else {
                 SyscallResult::Err(SyscallError::PermissionDenied)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_map(grant_id: usize) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 13 = Map
             let ret = trap(13, grant_id, 0, 0, 0);
             if ret >= 0 {
                 SyscallResult::Ok(ret as usize)
             } else {
                 SyscallResult::Err(SyscallError::PermissionDenied)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

// --- Legacy Wrappers ---

pub fn sys_service_lookup(name: &str) -> Option<usize> {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 100 = ServiceLookup
             let ret = trap(100, name.as_ptr() as usize, name.len(), 0, 0);
             if ret >= 0 {
                 return Some(ret as usize);
             }
        }
    }
    None
}

pub fn sys_open(path: &str) -> Result<usize, SyscallError> {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 101 = Open
             let ret = trap(101, path.as_ptr() as usize, path.len(), 0, 0);
             if ret >= 0 {
                 Ok(ret as usize)
             } else {
                 Err(SyscallError::FileNotFound)
             }
        } else {
            Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_read(fd: usize, buffer: &mut [u8]) -> Result<usize, SyscallError> {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 102 = Read
             let ret = trap(102, fd, buffer.as_mut_ptr() as usize, buffer.len(), 0);
             if ret >= 0 {
                 Ok(ret as usize)
             } else {
                 Err(SyscallError::PermissionDenied)
             }
        } else {
            Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_close(fd: usize) {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 103 = Close
             trap(103, fd, 0, 0, 0);
        }
    }
}

pub fn sys_log(msg: &str) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 11 = Log
             let max_len = 1024; // Limit kernel burden
             let len = core::cmp::min(msg.len(), max_len);
             let ret = trap(11, msg.as_ptr() as usize, len, 0, 0);
             SyscallResult::Ok(ret as usize)
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_chdir(path: &str) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 107 = ChDir
             let ret = trap(107, path.as_ptr() as usize, path.len(), 0, 0);
             if ret == 0 {
                 SyscallResult::Ok(0)
             } else {
                 SyscallResult::Err(SyscallError::FileNotFound)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_getcwd(buf: &mut [u8]) -> SyscallResult {
    unsafe {
        if let Some(trap) = SYSCALL_TRAP {
             // 108 = GetCwd
             let ret = trap(108, buf.as_mut_ptr() as usize, buf.len(), 0, 0);
             if ret >= 0 {
                 SyscallResult::Ok(ret as usize)
             } else {
                 SyscallResult::Err(SyscallError::BufferTooSmall)
             }
        } else {
             SyscallResult::Err(SyscallError::Unknown)
        }
    }
}
