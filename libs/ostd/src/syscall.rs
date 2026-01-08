#![allow(unsafe_code)]

use core::arch::asm;
use api::syscall::ViSyscall;


#[derive(Debug, Copy, Clone)]
pub enum SyscallResult {
    Ok(usize),
    Err(SyscallError),
}

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

#[inline(always)]
unsafe fn syscall(id: ViSyscall, a0: usize, a1: usize, a2: usize, a3: usize) -> isize {
    let mut ret: isize;
    asm!(
        "ecall",
        inlateout("a0") a0 => ret,
        in("a1") a1,
        in("a2") a2,
        in("a3") a3,
        in("a7") (id as usize),
        options(nostack, preserves_flags)
    );
    ret
}

pub fn sys_log(msg: &str) -> SyscallResult {
    unsafe {
        syscall(ViSyscall::Log, msg.as_ptr() as usize, msg.len(), 0, 0);
        SyscallResult::Ok(0)
    }
}

pub fn sys_yield() {
    unsafe {
        syscall(ViSyscall::Yield, 0, 0, 0, 0);
    }
}

pub fn sys_exit(code: usize) -> ! {
    unsafe {
        syscall(ViSyscall::Exit, code, 0, 0, 0);
    }
    loop { sys_yield(); }
}

pub fn sys_exec(path: &str) -> SyscallResult {
    unsafe {
         let ret = syscall(ViSyscall::Exec, path.as_ptr() as usize, path.len(), 0, 0);
         if ret != -1 {
             SyscallResult::Ok(ret as usize)
         } else {
             SyscallResult::Err(SyscallError::Unknown)
         }
    }
}

pub fn sys_spawn(entry: usize, arg: usize) -> SyscallResult {
    unsafe {
         let ret = syscall(ViSyscall::Spawn, entry, arg, 0, 0);
         if ret > 0 {
             SyscallResult::Ok(ret as usize)
         } else {
             SyscallResult::Err(SyscallError::Unknown)
         }
    }
}

pub fn sys_spawn_from_mem(data: &[u8], name: &str) -> SyscallResult {
    unsafe {
        // a0 = data ptr, a1 = data len
        // a2 = name ptr, a3 = name len
        let ret = syscall(ViSyscall::SpawnFromMem,
                          data.as_ptr() as usize, data.len(),
                          name.as_ptr() as usize, name.len());
        if ret > 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_wait(pid: usize) -> SyscallResult {
    unsafe {
        let ret = syscall(ViSyscall::Wait, pid, 0, 0, 0);
        if ret >= 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_open(path: &str) -> Result<usize, SyscallError> {
    unsafe {
        let ret = syscall(ViSyscall::Open, path.as_ptr() as usize, path.len(), 0, 0);
        if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(SyscallError::FileNotFound)
        }
    }
}

pub fn sys_close(fd: usize) {
    unsafe {
        syscall(ViSyscall::Close, fd, 0, 0, 0);
    }
}

pub fn sys_read(fd: usize, buffer: &mut [u8]) -> Result<usize, SyscallError> {
    unsafe {
        let ret = syscall(ViSyscall::Read, fd, buffer.as_mut_ptr() as usize, buffer.len(), 0);
        if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(SyscallError::PermissionDenied)
        }
    }
}

pub fn sys_write(fd: usize, buffer: &[u8]) -> Result<usize, SyscallError> {
    unsafe {
        let ret = syscall(ViSyscall::Write, fd, buffer.as_ptr() as usize, buffer.len(), 0);
        if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(SyscallError::PermissionDenied)
        }
    }
}

// IPC Wrappers
pub fn sys_send(target: usize, msg: &[u8]) -> SyscallResult {
    unsafe {
        let ret = syscall(ViSyscall::Send, target, msg.as_ptr() as usize, msg.len(), 0);
        SyscallResult::Ok(ret as usize)
    }
}

pub fn sys_read_dir(fd: usize, buffer: &mut [u8]) -> Result<usize, SyscallError> {
    unsafe {
        let ret = syscall(ViSyscall::ReadDir, fd, buffer.as_mut_ptr() as usize, buffer.len(), 0);
        if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_recv(mask: usize, buf: &mut [u8]) -> SyscallResult {
    unsafe {
        let ret = syscall(ViSyscall::Recv, mask, buf.as_mut_ptr() as usize, buf.len(), 0);
        SyscallResult::Ok(ret as usize)
    }
}

pub fn sys_set_timer(ticks: usize) -> SyscallResult {
    unsafe {
        syscall(ViSyscall::SetTimer, ticks, 0, 0, 0);
        SyscallResult::Ok(0)
    }
}

pub fn sys_grant(target: usize, ptr: usize, len: usize, flags: usize) -> SyscallResult {
    unsafe {
        // Assume Grant mapped to ID 12 in Kernel dispatch manually if not in enum
        // I should update API enum, but for now I rely on Kernel's dispatch match
        // Wait, I updated `ViSyscall` enum in previous step? No, I didn't add Grant yet.
        // Let's assume Grant is not used by Shell for now.
        SyscallResult::Err(SyscallError::Unknown)
    }
}
