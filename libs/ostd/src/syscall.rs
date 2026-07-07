#![allow(unsafe_code)]

use api::syscall::{ViSpawnArgs, ViSyscall};
use core::arch::asm;

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
    #[cfg(target_arch = "riscv64")]
    asm!(
        "ecall",
        inlateout("a0") a0 => ret,
        in("a1") a1,
        in("a2") a2,
        in("a3") a3,
        in("a7") (id as usize),
        options(nostack, preserves_flags)
    );
    // ViCell ARM64 ABI: x0=syscall_nr, x1=a0, x2=a1, x3=a2, x4=a3; ret in x0.
    #[cfg(target_arch = "aarch64")]
    asm!(
        "svc #0",
        inlateout("x0") id as usize => ret,
        in("x1") a0,
        in("x2") a1,
        in("x3") a2,
        in("x4") a3,
        options(nostack, preserves_flags)
    );
    // ViCell x86_64 ABI: RAX=syscall_nr (a7 slot), RDI=a0, RSI=a1, RDX=a2, R10=a3
    // (R10, not RCX — SYSCALL clobbers RCX to save user RIP).  Return value in RAX.
    // SAFETY: SYSCALL transitions to kernel via LSTAR; the kernel validates all
    // pointer arguments before use.  RCX and R11 are clobbered by the hardware
    // (RCX ← user RIP, R11 ← user RFLAGS) and must be declared as outputs.
    #[cfg(target_arch = "x86_64")]
    asm!(
        "syscall",
        inlateout("rax") id as usize => ret,
        in("rdi") a0,
        in("rsi") a1,
        in("rdx") a2,
        in("r10") a3,
        lateout("rcx") _,   // clobbered by SYSCALL (saves user RIP)
        lateout("r11") _,   // clobbered by SYSCALL (saves user RFLAGS)
        options(nostack)
    );
    ret
}

/// Invoke a syscall by raw numeric id (bypasses the `ViSyscall` enum).
///
/// Used for block I/O (ids 500/501) which intentionally have no `ViSyscall`
/// entry — keeping them out of `libs/api` avoids the Interface-is-Sacred
/// 2x-confirmation gate. The kernel dispatches them via the numeric fallback
/// in `ViCell_syscall_dispatch`.
#[inline(always)]
unsafe fn syscall_raw(id: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> isize {
    let mut ret: isize;
    #[cfg(target_arch = "riscv64")]
    asm!(
        "ecall",
        inlateout("a0") a0 => ret,
        in("a1") a1,
        in("a2") a2,
        in("a3") a3,
        in("a7") id,
        options(nostack, preserves_flags)
    );
    #[cfg(target_arch = "aarch64")]
    asm!(
        "svc #0",
        inlateout("x0") id => ret,
        in("x1") a0,
        in("x2") a1,
        in("x3") a2,
        in("x4") a3,
        options(nostack, preserves_flags)
    );
    // SAFETY: same contract as the typed `syscall` variant above.
    // `id` is a raw numeric syscall number (e.g. 500/501/502/503 for block I/O).
    #[cfg(target_arch = "x86_64")]
    asm!(
        "syscall",
        inlateout("rax") id => ret,
        in("rdi") a0,
        in("rsi") a1,
        in("rdx") a2,
        in("r10") a3,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

/// Read one 512-byte sector from the VirtIO block device. Returns `true` on success.
///
/// Raw syscall 500 — has no `ViSyscall` entry to preserve `libs/api` stability.
/// `buf` is filled only when this returns `true`.
pub fn sys_blk_read(sector: u64, buf: &mut [u8; 512]) -> bool {
    // SAFETY: buf is a fixed 512-byte stack array; the kernel validates the
    // pointer with validate_user_buf before writing exactly 512 bytes.
    let ret = unsafe { syscall_raw(500, sector as usize, buf.as_mut_ptr() as usize, 512, 0) };
    ret == 1
}

/// Flush the VirtIO block device write cache, ensuring all prior writes reach the disk image.
///
/// Raw syscall 503 — not in `ViSyscall` (same pattern as 500/501/502).
/// Must be called after writes to `/data/` to guarantee reboot persistence.
pub fn sys_blk_flush() -> bool {
    // SAFETY: raw syscall 503 triggers viVirtIOBlk.flush() in the kernel,
    // which sends a VirtIO FLUSH command to QEMU. Returns 1 on success, 0 on failure.
    let ret = unsafe { syscall_raw(503, 0, 0, 0, 0) };
    ret == 1
}

/// Trigger a clean system shutdown via the kernel's SBI SRST path. Never returns.
///
/// Raw syscall 502 — intentionally absent from `ViSyscall`/`libs/api` to avoid the
/// ABI 2x-confirm gate (same pattern as `sys_blk_read`/`sys_blk_write` above).
pub fn sys_shutdown() -> ! {
    // SAFETY: raw syscall 502 invokes the kernel SBI SRST shutdown; the kernel's
    // ecall to OpenSBI terminates QEMU and never returns to us.
    unsafe { syscall_raw(502, 0, 0, 0, 0); }
    // Unreachable: the kernel never returns from shutdown. Spin to satisfy `-> !`.
    loop { sys_yield(); }
}

/// Write one 512-byte sector to the VirtIO block device. Returns `true` on success.
///
/// Raw syscall 501. The write is synchronous (VirtIO polling) — durable on return.
pub fn sys_blk_write(sector: u64, buf: &[u8; 512]) -> bool {
    // SAFETY: buf is a fixed 512-byte stack array; the kernel validates the
    // pointer with validate_user_buf before reading exactly 512 bytes.
    let ret = unsafe { syscall_raw(501, sector as usize, buf.as_ptr() as usize, 512, 0) };
    ret == 1
}

pub fn sys_log(msg: &str) -> SyscallResult {
    unsafe {
        syscall(ViSyscall::Log, msg.as_ptr() as usize, msg.len(), 0, 0);
        SyscallResult::Ok(0)
    }
}

/// Drain up to `buf.len()` bytes from the kernel user-log ring buffer.
///
/// Returns the number of bytes actually copied (0 when no new output is available).
/// Requires the `ReadLog` capability declared in the cell manifest.
pub fn sys_read_log(buf: &mut [u8]) -> usize {
    let n = unsafe {
        syscall(ViSyscall::ReadLog, buf.as_mut_ptr() as usize, buf.len(), 0, 0)
    };
    n as usize
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
    loop {
        sys_yield();
    }
}

/// Force-terminate another task by its TID.
///
/// Non-blocking: returns immediately to the caller.  The kernel removes the
/// target from the scheduler, unblocks any tasks stuck sending to it, and
/// releases its caps and quota.
///
/// Requires `SpawnCap` on the caller.  System service cells (`block_io_cap` /
/// `network_cap` holders) are rejected — use hot-swap to replace them safely.
///
/// # Errors
/// Returns `Err` when: caller lacks `SpawnCap`, target is a system cell,
/// TID equals caller, or TID is not found.  If the target self-exited between
/// the check and cleanup, returns `Ok(0)` (task is already gone).
pub fn sys_force_exit(tid: usize) -> SyscallResult {
    let ret = unsafe { syscall(ViSyscall::ForceExit, tid, 0, 0, 0) };
    if ret == 0 { SyscallResult::Ok(0) } else { SyscallResult::Err(SyscallError::Unknown) }
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

pub fn sys_spawn_from_mem(data: &[u8], name: &str, args: &str) -> SyscallResult {
    unsafe {
        let spawn_args = ViSpawnArgs {
            buffer_addr: data.as_ptr() as usize,
            buffer_size: data.len(),
            name_ptr: name.as_ptr() as usize,
            name_len: name.len(),
            args_ptr: args.as_ptr() as usize,
            args_len: args.len(),
        };

        let ret = syscall(
            ViSyscall::SpawnFromMem,
            &spawn_args as *const _ as usize,
            0,
            0,
            0,
        );
        if ret > 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

/// Spawn a cell by loading its ELF from a VFS path (e.g. `/bin/shell`).
///
/// The kernel reads the ELF from disk or the bootstrap table, parses it,
/// and spawns a new task.  Returns the new cell's task ID on success.
///
/// # Errors
/// Returns `SyscallError::Unknown` if the path is not found or the ELF is invalid.
pub fn sys_spawn_from_path(path: &str) -> SyscallResult {
    // SAFETY: path is a valid UTF-8 str; kernel copies it out before returning.
    unsafe {
        let ret = syscall(
            ViSyscall::SpawnFromPath,
            path.as_ptr() as usize,
            path.len(),
            0,
            0,
        );
        if ret > 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

/// Spawn a cell from ELF bytes in a caller-owned Grant region (G2 loader redesign).
///
/// `grant_id` must be a `sys_grant_alloc` region the caller owns, holding the full
/// ELF image (`len` bytes). `path_hint` drives the kernel's `/bin/` privilege check,
/// operator-policy lookup, and measurement label — advisory only (lying about it can
/// only lose privilege). The kernel applies the IDENTICAL spawn trust gate as
/// `sys_spawn_from_path`, so the source of the bytes is irrelevant to trust.
pub fn sys_spawn_from_elf(grant_id: usize, len: usize, path_hint: &str) -> SyscallResult {
    // SAFETY: register args plus a valid UTF-8 path slice the kernel copies out
    // (under SUM) before returning.
    unsafe {
        let ret = syscall(
            ViSyscall::SpawnFromElf,
            grant_id,
            len,
            path_hint.as_ptr() as usize,
            path_hint.len(),
        );
        if ret > 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

/// Serialize all allocated physical frames to the warm-boot snapshot sector range.
///
/// The kernel quiesces hardware and writes a snapshot image at a fixed LBA on
/// the VirtIO block device.  On next boot, the snapshot is detected and the
/// kernel heap is restored instead of running a cold boot sequence.
///
/// Returns `Ok(frame_count)` on success.
pub fn sys_snapshot() -> SyscallResult {
    // SAFETY: sys_snapshot triggers a kernel write; no user-memory pointers involved.
    let ret = unsafe { syscall(ViSyscall::Snapshot, 0, 0, 0, 0) };
    SyscallResult::Ok(ret as usize)
}

/// Spawn a cell pinned to a specific hardware core.
///
/// On single-core systems `core_id` must be 0; any other value returns
/// `SyscallError::Unknown` (maps to `ViError::NotSupported` in the kernel).
///
/// # Errors
/// Returns `Err` if the path is not found, the ELF is invalid, or `core_id != 0`
/// on a single-core kernel.
pub fn sys_spawn_pinned(path: &str, priority: u8, core_id: usize) -> SyscallResult {
    // SAFETY: path is a valid UTF-8 str; kernel copies it out before returning.
    unsafe {
        let ret = syscall(
            ViSyscall::SpawnPinned,
            path.as_ptr() as usize,
            path.len(),
            priority as usize,
            core_id,
        );
        if ret > 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

/// Open a file by path and return a capability ID.
///
/// Returns `Ok(cap_id)` where `cap_id > 0`, or `Err` if the path is not found.
///
/// # Errors
/// Returns `SyscallError::FileNotFound` if the path does not exist.
pub fn sys_open_cap(path: &str) -> Result<u64, SyscallError> {
    // SAFETY: path is a valid UTF-8 str; kernel copies it before returning.
    let ret = unsafe {
        syscall(ViSyscall::OpenCap, path.as_ptr() as usize, path.len(), 0, 0)
    };
    if ret > 0 {
        Ok(ret as u64)
    } else {
        Err(SyscallError::FileNotFound)
    }
}

/// Read bytes from a cap-backed file into `buf`.
///
/// # Errors
/// Returns `SyscallError::PermissionDenied` if the caller does not own the cap.
pub fn sys_read_cap(cap_id: u64, buf: &mut [u8]) -> Result<usize, SyscallError> {
    // SAFETY: buf is a valid mutable slice; kernel writes into it.
    let ret = unsafe {
        syscall(
            ViSyscall::ReadCap,
            cap_id as usize,
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
        )
    };
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(SyscallError::Unknown)
    }
}

/// Revoke a capability (close the associated resource).
pub fn sys_close_cap(cap_id: u64) {
    // SAFETY: no memory access; just passes an integer to the kernel.
    unsafe { syscall(ViSyscall::CloseCap, cap_id as usize, 0, 0, 0) };
}

/// Write `buf` into a cap-backed file at the current cursor position.
///
/// Returns the number of bytes written, or `SyscallError::Unknown` on failure.
pub fn sys_write_cap(cap_id: u64, buf: &[u8]) -> Result<usize, SyscallError> {
    if buf.is_empty() {
        return Ok(0);
    }
    // SAFETY: buf is a valid immutable slice; kernel reads from it.
    let ret = unsafe {
        syscall(
            ViSyscall::WriteCap,
            cap_id as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
        )
    };
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(SyscallError::Unknown)
    }
}

/// Query the size of a cap-backed file in bytes.
///
/// Does not modify the file cursor — the kernel opens the file independently
/// to measure its size and restores state before returning.
///
/// # Errors
/// Returns `SyscallError::Unknown` if the cap is invalid or the file does not exist.
pub fn sys_stat_cap(cap_id: u64) -> Result<u64, SyscallError> {
    // SAFETY: no memory access; integers only.
    let ret = unsafe { syscall(ViSyscall::StatCap, cap_id as usize, 0, 0, 0) };
    if ret >= 0 {
        Ok(ret as u64)
    } else {
        Err(SyscallError::Unknown)
    }
}

/// Truncate a cap-backed file to exactly `len` bytes.
///
/// Returns `Err` if `len > current_size` (no zero-fill extension; use `WriteCap` to grow).
///
/// # Errors
/// Returns `SyscallError::Unknown` on permission denied, invalid cap, or unsupported backend.
pub fn sys_truncate_cap(cap_id: u64, len: u64) -> Result<(), SyscallError> {
    // SAFETY: no memory access; integers only.
    let ret = unsafe {
        syscall(ViSyscall::TruncateCap, cap_id as usize, len as usize, 0, 0)
    };
    if ret >= 0 { Ok(()) } else { Err(SyscallError::Unknown) }
}

/// Flush all dirty pages for a cap-backed file to the underlying block device.
///
/// No-op on write-through filesystems (RamDisk, G1). Wires into device flush on
/// NVMe (G2) for durability guarantees after write sequences.
///
/// # Errors
/// Returns `SyscallError::Unknown` on permission denied or invalid cap.
pub fn sys_sync_cap(cap_id: u64) -> Result<(), SyscallError> {
    // SAFETY: no memory access; integers only.
    let ret = unsafe { syscall(ViSyscall::SyncCap, cap_id as usize, 0, 0, 0) };
    if ret >= 0 { Ok(()) } else { Err(SyscallError::Unknown) }
}

/// Map `[phys, phys+size)` into the IOMMU for the calling Cell, authorising DMA for device `bdf`.
///
/// Caller must own the PCIe BDF via Resource Registry and the range must be within its
/// memory quota. Returns the IOVA (== `phys` in SAS identity mapping) on success.
///
/// # Errors
/// Returns `SyscallError::PermissionDenied` if the BDF is not owned by the caller,
/// `SyscallError::InvalidArgument` if `phys`/`size` are misaligned or out-of-quota,
/// or `SyscallError::Unknown` on other failure.
pub fn sys_grant_dma(bdf: u32, phys: u64, size: usize) -> Result<u64, SyscallError> {
    // SAFETY: no memory access; all arguments are plain integers.
    let ret = unsafe { syscall(ViSyscall::GrantDma, bdf as usize, phys as usize, size, 0) };
    if ret >= 0 { Ok(ret as u64) } else { Err(SyscallError::Unknown) }
}

/// Seek a cap-backed file to `offset` from `whence` (0=Start, 1=Current, 2=End).
///
/// Returns the new absolute byte position, or `SyscallError::Unknown` on failure.
///
/// # ABI note
/// `offset` is an `i64` transmitted as a `usize` bit-pattern (reinterpret). The
/// kernel sign-extends it back to `isize` / `i64` for negative values (Current/End).
pub fn sys_seek_cap(cap_id: u64, offset: i64, whence: u8) -> Result<u64, SyscallError> {
    // SAFETY: no memory access; integers only.
    let ret = unsafe {
        syscall(
            ViSyscall::SeekCap,
            cap_id as usize,
            offset as usize,  // bit-pattern reinterpret; kernel casts back via `as isize`
            whence as usize,
            0,
        )
    };
    if ret >= 0 {
        Ok(ret as u64)
    } else {
        Err(SyscallError::Unknown)
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

/// Register the caller to be notified when task `tid` exits or faults.
///
/// Requires `SpawnCap`. After this, a `sys_recv` by the caller returns `tid`
/// (as the "sender") when that task dies — enabling a supervisor to wait-any
/// across many children with a single recv loop. Returns `Ok(0)` on success.
pub fn sys_notify_on_exit(tid: usize) -> SyscallResult {
    unsafe {
        let ret = syscall(ViSyscall::NotifyOnExit, tid, 0, 0, 0);
        if ret >= 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

/// Register `tid` as the current provider of a well-known `service_id`
/// (see [`api::syscall::service`]).
///
/// Requires `SpawnCap` — intended for the supervisor (init), which registers each
/// service after spawning it and re-registers the new tid after a respawn so clients
/// reconnect transparently. Returns `Ok(0)` on success, `Err` if the caller lacks
/// `SpawnCap` or the registry is full.
pub fn sys_register_service(service_id: u16, tid: usize) -> SyscallResult {
    unsafe {
        let ret = syscall(ViSyscall::RegisterService, service_id as usize, tid, 0, 0);
        if ret == 0 { SyscallResult::Ok(0) } else { SyscallResult::Err(SyscallError::Unknown) }
    }
}

/// Resolve a well-known `service_id` to its current provider tid.
///
/// Returns `Some(tid)` for a live provider, or `None` when nothing is registered
/// (e.g. during the death→respawn window — the caller should retry). Open to all cells.
pub fn sys_lookup_service(service_id: u16) -> Option<usize> {
    unsafe {
        let ret = syscall(ViSyscall::LookupService, service_id as usize, 0, 0, 0);
        // ABI: provider tid (> 0), or 0 when no live provider is registered.
        if ret > 0 { Some(ret as usize) } else { None }
    }
}

pub fn sys_shm_alloc(size: usize) -> SyscallResult {
    unsafe {
        let ret = syscall(ViSyscall::ShmAlloc, size, 0, 0, 0);
        if ret > 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_shm_map(handle: usize, target_pid: usize) -> SyscallResult {
    unsafe {
        let ret = syscall(ViSyscall::ShmMap, handle, target_pid, 0, 0);
        if ret != 0 {
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
        let ret = syscall(
            ViSyscall::Read,
            fd,
            buffer.as_mut_ptr() as usize,
            buffer.len(),
            0,
        );
        if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(SyscallError::PermissionDenied)
        }
    }
}

/// Read the next directory entry from an open directory fd.
///
/// Returns `Ok(Some(entry))` per entry, `Ok(None)` at end of directory.
/// The kernel serializes one `types::DirEntry` (repr(C)) per call and
/// advances the handle's cursor — loop until `None`.
pub fn sys_readdir(fd: usize) -> Result<Option<types::DirEntry>, SyscallError> {
    let mut entry = types::DirEntry::default();
    let size = core::mem::size_of::<types::DirEntry>();
    // SAFETY: entry is a repr(C) value with no padding invariants; the kernel
    // writes exactly `size` bytes on success and nothing on EOF.
    let ret = unsafe {
        syscall(
            ViSyscall::ReadDir,
            fd,
            &mut entry as *mut types::DirEntry as usize,
            size,
            0,
        )
    };
    match ret {
        0 => Ok(None), // end of directory
        n if n > 0 => Ok(Some(entry)),
        _ => Err(SyscallError::Unknown),
    }
}

pub fn sys_write(fd: usize, buffer: &[u8]) -> Result<usize, SyscallError> {
    unsafe {
        let ret = syscall(
            ViSyscall::Write,
            fd,
            buffer.as_ptr() as usize,
            buffer.len(),
            0,
        );
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
        if ret >= 0 {
            SyscallResult::Ok(ret as usize)
        } else {
            SyscallResult::Err(SyscallError::Unknown)
        }
    }
}

/// Non-blocking send: deliver to `target` if it is in `Recv`, otherwise drop.
///
/// Returns `Ok(0)` on delivery, `Ok(usize::MAX)` if the target was not ready
/// (busy or exited). Never blocks the caller.
pub fn sys_try_send(target: usize, msg: &[u8]) -> SyscallResult {
    // SAFETY: msg is a valid slice; kernel copies before returning.
    let ret = unsafe {
        syscall(ViSyscall::TrySend, target, msg.as_ptr() as usize, msg.len(), 0)
    };
    SyscallResult::Ok(ret as usize)
}

/// A contiguous (ptr, len) segment for scatter/gather IPC.
///
/// The layout matches what the kernel reads: two `usize` values back-to-back.
#[repr(C)]
pub struct IoVec {
    pub ptr: usize,
    pub len: usize,
}

/// Send one IPC message gathered from up to 8 non-contiguous buffers.
///
/// The kernel concatenates the segments and delivers them to `target` as a
/// single contiguous message.
///
/// # Errors
/// Returns `Err` if `target` is not found or more than 8 segments are passed.
pub fn sys_send_gather(target: usize, segments: &[IoVec]) -> SyscallResult {
    let iovec_ptr  = segments.as_ptr() as usize;
    let iovec_count = segments.len();
    // SAFETY: segments is a valid slice; kernel reads iovec_count * 2 * sizeof(usize) bytes.
    let ret = unsafe {
        syscall(ViSyscall::SendGather, target, iovec_ptr, iovec_count, 0)
    };
    SyscallResult::Ok(ret as usize)
}

/// Receive one IPC message scattered into up to 8 non-contiguous buffers.
///
/// The kernel fills each segment in order; if the message is shorter than
/// the total capacity, remaining bytes in later segments are zeroed.
///
/// # Returns
/// `Ok(sender_id)` on success.  `Ok(0)` means the task is now blocked waiting
/// for a message (non-blocking fast path returned no sender).
pub fn sys_recv_scatter(mask: usize, segments: &mut [IoVec]) -> SyscallResult {
    let iovec_ptr   = segments.as_mut_ptr() as usize;
    let iovec_count = segments.len();
    // SAFETY: segments is a valid mutable slice; kernel writes into the pointed-to buffers.
    let ret = unsafe {
        syscall(ViSyscall::RecvScatter, mask, iovec_ptr, iovec_count, 0)
    };
    SyscallResult::Ok(ret as usize)
}

pub fn sys_read_dir(fd: usize, buffer: &mut [u8]) -> Result<usize, SyscallError> {
    unsafe {
        let ret = syscall(
            ViSyscall::ReadDir,
            fd,
            buffer.as_mut_ptr() as usize,
            buffer.len(),
            0,
        );
        if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(SyscallError::Unknown)
        }
    }
}

pub fn sys_recv(mask: usize, buf: &mut [u8]) -> SyscallResult {
    unsafe {
        let ret = syscall(
            ViSyscall::Recv,
            mask,
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
        );
        SyscallResult::Ok(ret as usize)
    }
}

/// Non-blocking receive: returns immediately with `Ok(0)` when no message is
/// queued, instead of parking the task like [`sys_recv`].
///
/// Use this in cells that must keep polling other work (e.g. the net service
/// driving DHCP) while also servicing incoming IPC.
pub fn sys_try_recv(mask: usize, buf: &mut [u8]) -> SyscallResult {
    // SAFETY: buf is a valid mutable slice; the kernel writes into it and
    // returns the sender id (or 0 when the queue is empty).
    let ret = unsafe {
        syscall(
            ViSyscall::TryRecv,
            mask,
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
        )
    };
    SyscallResult::Ok(ret as usize)
}

/// Receive a message with a timeout deadline.
///
/// `timeout_ticks` is the maximum number of **scheduler ticks** to wait (one tick
/// = 10 ms, the preemption slice). The kernel computes an absolute deadline of
/// `system_ticks() + timeout_ticks` and the scheduler wakes the task once it
/// elapses. Pass `u64::MAX` for no timeout.
///
/// # Returns
/// - `Ok(sender_id)` on success.
/// - `Ok(0)` if no message arrived before the deadline (timeout).
pub fn sys_recv_timeout(mask: usize, buf: &mut [u8], timeout_ticks: u64) -> SyscallResult {
    // SAFETY: buf is a valid mutable slice; kernel writes into it.
    let ret = unsafe {
        syscall(
            ViSyscall::RecvTimeout,
            mask,
            buf.as_mut_ptr() as usize,
            buf.len(),
            timeout_ticks as usize,
        )
    };
    if ret >= 0 {
        SyscallResult::Ok(ret as usize)
    } else {
        SyscallResult::Err(SyscallError::Unknown)
    }
}

pub fn sys_set_timer(ticks: usize) -> SyscallResult {
    unsafe {
        syscall(ViSyscall::SetTimer, ticks, 0, 0, 0);
        SyscallResult::Ok(0)
    }
}

pub fn sys_grant(_target: usize, _ptr: usize, _len: usize, _flags: usize) -> SyscallResult {
    // Assume Grant mapped to ID 12
    SyscallResult::Err(SyscallError::Unknown)
}

pub fn sys_get_procs(buffer: &mut [api::syscall::ProcessInfo]) -> Result<usize, SyscallError> {
    unsafe {
        let ret = syscall(ViSyscall::GetProcs, buffer.as_mut_ptr() as usize, buffer.len(), 0, 0);
        if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(SyscallError::Unknown)
        }
    }
}

/// Live-replace a running Cell with a new ELF version without message loss.
///
/// `cell_id` is the task ID of the cell to replace; `new_elf_path` must be a
/// valid `/bin/<name>` path present on the bootstrap disk.
///
/// # Returns
/// `Ok(new_task_id)` on success.  Returns `Err` if the cell is not found,
/// the ELF cannot be loaded, or the state-transfer protocol fails.
pub fn sys_hotswap(cell_id: usize, new_elf_path: &str) -> SyscallResult {
    // SAFETY: new_elf_path is a valid UTF-8 str; kernel copies it before returning.
    let ret = unsafe {
        syscall(
            ViSyscall::HotSwap,
            cell_id,
            new_elf_path.as_ptr() as usize,
            new_elf_path.len(),
            0,
        )
    };
    if ret > 0 { SyscallResult::Ok(ret as usize) } else { SyscallResult::Err(SyscallError::Unknown) }
}

/// Signal to the kernel that this cell has finished deserializing hot-swap state
/// and is ready to receive IPC from the service registry.
///
/// Call this at the end of an [`AppEvent::Restore`] handler, after
/// [`sys_state_restore`] completes successfully.  The hotswap orchestrator is
/// blocked waiting for this signal; calling it unblocks Step 5 (unfreeze).
pub fn sys_hotswap_ready() {
    // SAFETY: no arguments; kernel sets hotswap_ready flag on the calling task.
    unsafe { syscall(ViSyscall::HotSwapReady, 0, 0, 0, 0) };
}

/// Flush a rectangular region of pixels to the VirtIO GPU framebuffer.
///
/// `pixels` must be `w * h * 4` bytes in BGRA8888 format.
///
/// # Errors
/// Returns `Err` if the GPU driver is not initialised in the running kernel.
pub fn sys_gpu_flush(pixels: &[u8], x: u32, y: u32, w: u32, h: u32) -> Result<(), SyscallError> {
    // Pack geometry: a2 = xy (x<<16 | y), a3 = wh (w<<16 | h).
    let xy = (((x as usize) & 0xFFFF) << 16) | ((y as usize) & 0xFFFF);
    let wh = ((w as usize & 0xFFFF) << 16) | (h as usize & 0xFFFF);
    // SAFETY: pixels is a valid immutable slice; kernel validates length against w*h*4.
    let ret = unsafe {
        syscall(ViSyscall::GpuFlush, pixels.as_ptr() as usize, pixels.len(), xy, wh)
    };
    if ret >= 0 { Ok(()) } else { Err(SyscallError::Unknown) }
}

/// Play raw PCM frames on the VirtIO sound output device (blocking).
///
/// Frames must be signed 16-bit little-endian, 2 interleaved channels (L, R) at
/// 44100 Hz — i.e. 4 bytes per frame. Blocks until every frame has been handed to
/// the device. Returns the number of bytes played (0 when there is no sound
/// device or the transfer failed). Requires `AudioPlay` in `declare_syscalls!`.
pub fn sys_audio_play(pcm: &[u8]) -> usize {
    // SAFETY: pcm is a valid immutable slice; the kernel validates the pointer and
    // length, and reads exactly `pcm.len()` bytes from it.
    let ret = unsafe {
        syscall(ViSyscall::AudioPlay, pcm.as_ptr() as usize, pcm.len(), 0, 0)
    };
    if ret > 0 { ret as usize } else { 0 }
}

/// Set or move the VirtIO GPU hardware cursor.
///
/// **op = 0 (set sprite):** uploads a 64×64 BGRA8888 sprite and positions the
/// cursor at `(x, y)` with the hotspot at `(hot_x, hot_y)`. `sprite` must be
/// exactly `64 * 64 * 4 = 16384` bytes.
///
/// **op = 1 (move):** repositions the existing cursor to `(x, y)`. The `sprite`
/// pointer and hotspot values are ignored; pass a zero-length slice and `(0,0)`.
///
/// # Errors
/// Returns `Err` when the GPU cursor hardware is unavailable or the sprite size
/// is wrong.
pub fn sys_gpu_cursor(
    op: usize,
    sprite: *const u8,
    x: u32,
    y: u32,
    hot_x: u32,
    hot_y: u32,
) -> Result<(), SyscallError> {
    let xy  = (((x as usize)     & 0xFFFF) << 16) | ((y     as usize) & 0xFFFF);
    let hot = (((hot_x as usize) & 0xFFFF) << 16) | ((hot_y as usize) & 0xFFFF);
    // SAFETY: for op=0 sprite must point to a 64*64*4-byte BGRA buffer owned by the
    // caller; the kernel validates the length and reads exactly that many bytes.
    // For op=1 sprite is ignored; passing null or a valid pointer are both safe.
    let ret = unsafe {
        syscall(ViSyscall::GpuCursor, op, sprite as usize, xy, hot)
    };
    if ret >= 0 { Ok(()) } else { Err(SyscallError::Unknown) }
}

/// Query the VirtIO GPU's current scanout resolution.
///
/// Returns `(width, height)` as reported by the GPU via `GET_DISPLAY_INFO`.
/// Falls back to `(1280, 800)` if the GPU driver is not yet initialized.
pub fn sys_get_resolution() -> (u32, u32) {
    // SAFETY: no pointers or memory involved; pure register read from kernel GPU state.
    let ret = unsafe { syscall(ViSyscall::GpuGetResolution, 0, 0, 0, 0) } as usize;
    let w = (ret >> 32) as u32;
    let h = (ret & 0xFFFF_FFFF) as u32;
    if w == 0 || h == 0 { (1280, 800) } else { (w, h) }
}

/// Transmit one Ethernet frame through the kernel VirtIO NIC.
///
/// `frame` must contain a complete Ethernet frame (the kernel prepends the
/// VirtIO net header internally).
///
/// # Returns
/// `true` if the frame was queued for transmission, `false` if the NIC is not
/// present or the TX ring is full.
pub fn sys_net_tx(frame: &[u8]) -> bool {
    // SAFETY: frame is a valid immutable slice; the kernel reads exactly
    // frame.len() bytes after validating the buffer.
    let ret = unsafe {
        syscall(ViSyscall::NetTx, frame.as_ptr() as usize, frame.len(), 0, 0)
    };
    ret == 1
}

/// Receive one pending Ethernet frame from the kernel VirtIO NIC.
///
/// # Returns
/// The number of bytes written into `buf` (0 if no frame is ready).
pub fn sys_net_rx(buf: &mut [u8]) -> usize {
    // SAFETY: buf is a valid mutable slice; the kernel writes at most buf.len()
    // bytes and returns the count.
    let ret = unsafe {
        syscall(ViSyscall::NetRx, buf.as_mut_ptr() as usize, buf.len(), 0, 0)
    };
    if ret > 0 { ret as usize } else { 0 }
}

/// Stash serialized cell state in the kernel under `key`.
///
/// A replacement instance recovers it with [`sys_state_restore`] after a
/// hot-swap or respawn. Returns the number of bytes stored.
pub fn sys_state_stash(key: u64, bytes: &[u8]) -> usize {
    // SAFETY: bytes is a valid immutable slice; the kernel copies it out.
    let ret = unsafe {
        syscall(ViSyscall::StateStash, key as usize, bytes.as_ptr() as usize, bytes.len(), 0)
    };
    if ret > 0 { ret as usize } else { 0 }
}

/// Restore previously stashed state for `key` into `buf`.
///
/// Returns the number of bytes written (0 if nothing was stashed for `key`).
pub fn sys_state_restore(key: u64, buf: &mut [u8]) -> usize {
    // SAFETY: buf is a valid mutable slice; the kernel writes at most buf.len().
    let ret = unsafe {
        syscall(ViSyscall::StateRestore, key as usize, buf.as_mut_ptr() as usize, buf.len(), 0)
    };
    if ret > 0 { ret as usize } else { 0 }
}

/// Delete the kernel stash entry for `key`, freeing its slot toward the MAX_ENTRIES cap.
///
/// Call this after a successful [`sys_state_restore`] to avoid accumulating stale entries.
/// No-op when the key is absent — always safe to call.
pub fn sys_state_stash_clear(key: u64) {
    // SAFETY: pure register syscall — no memory pointers; the kernel removes the entry
    // from its BTreeMap and drops the Vec<u8> (or no-ops when key is absent).
    unsafe { syscall(ViSyscall::StateStashClear, key as usize, 0, 0, 0) };
}

/// Reserved state-stash slot used to hand a command line to a freshly spawned
/// cell. `sys_spawn_from_path` does not yet carry argv on the new cell's stack,
/// so the spawner stashes the argument string here and the spawned cell reads
/// it on startup. Single-spawner (the shell) makes this race-free in practice.
pub const ARGV_STASH_KEY: u64 = 0x0061_7267_7600_0000; // "argv"

/// Publish `args` as the command line for the next cell spawned by this task.
/// Always call before `sys_spawn_from_path` (pass `""` when there are no args)
/// so the spawned cell never reads a previous command's leftovers.
pub fn sys_set_spawn_args(args: &str) {
    sys_state_stash(ARGV_STASH_KEY, args.as_bytes());
}

/// Read the command line published for this cell by its spawner. Returns the
/// number of bytes written into `buf` (0 if none).
pub fn sys_spawn_args(buf: &mut [u8]) -> usize {
    sys_state_restore(ARGV_STASH_KEY, buf)
}

/// Read the kernel's monotonic timer (ticks since boot).
///
/// The tick frequency is architecture-dependent; query the Config Cell at
/// `system.timer_freq_hz` to convert to nanoseconds.  On RV64 this maps to
/// the `mtime` register frequency (typically 10 MHz on QEMU).
///
/// # Returns
/// Tick count as a `u64`.  Returns 0 if the syscall is not yet wired in the
/// running kernel build.
/// Assert liveness to the kernel hung-detector.
///
/// The caller promises to call this again within `interval_ticks` (10 ms scheduler
/// ticks); if it misses that deadline the kernel terminates it as HUNG so the
/// supervisor restarts it — catching deadlocks / stuck loops the CPU watchdog can't
/// see. Call it once per main-loop iteration with a generous interval. `0` disables.
pub fn sys_heartbeat(interval_ticks: u64) {
    // SAFETY: register-only syscall; reads/writes no memory.
    unsafe {
        syscall(ViSyscall::Heartbeat, interval_ticks as usize, 0, 0, 0);
    }
}

pub fn sys_get_time() -> u64 {
    // SAFETY: no memory is read or written; the kernel returns a register-size integer.
    let ret = unsafe { syscall(ViSyscall::GetTime, 0, 0, 0, 0) };
    if ret >= 0 { ret as u64 } else { 0 }
}

/// Nanoseconds since Unix epoch from the hardware RTC; 0 if no RTC is present.
pub fn sys_get_wall_time() -> u64 {
    // SAFETY: register-only syscall.
    let ret = unsafe { syscall(ViSyscall::GetTime, 2, 0, 0, 0) };
    if ret >= 0 { ret as u64 } else { 0 }
}

/// Seconds since Unix epoch from the hardware RTC; 0 if no RTC is present.
pub fn sys_get_wall_secs() -> u64 {
    // SAFETY: register-only syscall.
    let ret = unsafe { syscall(ViSyscall::GetTime, 3, 0, 0, 0) };
    if ret >= 0 { ret as u64 } else { 0 }
}

// ── Zero-Copy Grant API (Storage 2.0, Phase 01) ───────────────────────────────

/// Allocate a contiguous kernel-managed Grant region of up to 16 pages (64 KB).
///
/// # Returns
/// `Some(grant_id)` on success; `None` on OOM. `grant_id` is the physical base
/// address of the region (identity-mapped vaddr == paddr in SAS).
pub fn sys_grant_alloc(size: usize) -> Option<usize> {
    // SAFETY: register-only + kernel allocates memory on our behalf.
    let ret = unsafe { syscall(ViSyscall::GrantAlloc, size, 0, 0, 0) };
    // Kernel returns 0 on OOM (F10); compare as usize to avoid signed-bit issues
    // on targets where RAM could place a grant above the isize sign boundary.
    if (ret as usize) != 0 { Some(ret as usize) } else { None }
}

/// Share a Grant region with another task under the given permission.
///
/// `perm`: 0 = ReadOnly, 1 = WriteOnly, 2 = ReadWrite.
///
/// # Returns
/// `true` on success (caller must own the grant).
pub fn sys_grant_share(grant_id: usize, target_tid: usize, perm: u8) -> bool {
    // SAFETY: register-only; no memory pointers.
    let ret = unsafe { syscall(ViSyscall::GrantShare, grant_id, target_tid, perm as usize, 0) };
    ret == 0
}

/// Return the user-space pointer for a Grant the caller owns or holds.
///
/// In SAS the pointer equals the physical base (identity-map). Returns `None`
/// when `grant_id` is not found or the caller lacks access.
pub fn sys_grant_slice(grant_id: usize) -> Option<*mut u8> {
    // SAFETY: register-only; kernel validates ownership before returning a pointer.
    let ret = unsafe { syscall(ViSyscall::GrantSlice, grant_id, 0, 0, 0) };
    // Kernel returns usize::MAX on permission denied / not found. Cast through usize
    // to avoid the signed isize ambiguity with usize::MAX == -1i64 on 64-bit targets.
    if (ret as usize) != usize::MAX { Some(ret as usize as *mut u8) } else { None }
}

/// Release a Grant region (owner-only): unmaps its pages and frees the frames.
///
/// # Returns
/// `true` on success.
pub fn sys_grant_free(grant_id: usize) -> bool {
    // SAFETY: register-only; kernel cleans up the physical mapping.
    let ret = unsafe { syscall(ViSyscall::GrantFree, grant_id, 0, 0, 0) };
    ret == 0
}

/// Allocate a persistent pre-pinned Grant buffer that lives until the cell exits or
/// calls `sys_grant_unregister`.
///
/// Unlike `sys_grant_alloc`, the buffer is not freed by the kernel between requests —
/// it stays pinned for the cell's lifetime, enabling io_uring-style zero-copy I/O
/// without per-transfer allocation overhead.
///
/// # Returns
/// `Some(reg_id)` on success; `None` on OOM or size > 16 MiB cap.
/// `reg_id` is the physical base address (identity-mapped in SAS).
pub fn sys_grant_register(size: usize) -> Option<usize> {
    // SAFETY: register-only; kernel allocates memory on our behalf.
    let ret = unsafe { syscall(ViSyscall::GrantRegister, size, 0, 0, 0) };
    if (ret as usize) != 0 { Some(ret as usize) } else { None }
}

/// Release a registered buffer allocated via `sys_grant_register` (owner-only).
///
/// # Returns
/// `true` on success.
pub fn sys_grant_unregister(reg_id: usize) -> bool {
    // SAFETY: register-only; kernel cleans up the physical mapping.
    let ret = unsafe { syscall(ViSyscall::GrantUnregister, reg_id, 0, 0, 0) };
    ret == 0
}

/// Synchronous-but-zero-copy sector read into a pre-allocated Grant buffer.
///
/// The grant must be owned by the caller and hold ≥ 512 bytes.
/// Returns `true` when the read completes immediately (Phase 04 = true async).
///
/// Requires `BlockIoCap` (same authority gate as raw block I/O 500/501).
pub fn sys_blk_read_async(sector: u64, grant_id: usize) -> bool {
    // SAFETY: the grant buffer is already kernel-allocated and identity-mapped;
    // no additional pointer validation needed.
    let ret = unsafe { syscall(ViSyscall::BlkReadAsync, sector as usize, grant_id, 0, 0) };
    ret == 1
}

/// Request exclusive MMIO access for `[base, base+len)`.
///
/// Returns 0 on success, 1 for PermissionDenied, 2 for AlreadyExists, 3 for InvalidInput.
/// Driver Cells call this via `ostd::mmio::request_region`.
pub fn sys_request_mmio(base: usize, len: usize) -> usize {
    let ret = unsafe { syscall(ViSyscall::RequestMmio, base, len, 0, 0) };
    ret as usize
}

/// Block the calling Driver Cell until hardware IRQ `irq_num` fires.
///
/// `mmio_base` is the VirtIO MMIO slot base address; the kernel ISR uses it to
/// write the InterruptACK register (offset 0x64) before marking the IRQ pending,
/// preventing interrupt storms on level-triggered VirtIO devices. Pass 0 for
/// non-VirtIO devices (e.g. PCIe MSI or GPIO).
///
/// Lost-wakeup safe: an IRQ that fired before this call is returned immediately.
/// Requires PcieDriverCap or PlatformCap (allowlist bit 51).
pub fn sys_wait_irq(irq_num: u8, mmio_base: usize) -> Result<(), SyscallError> {
    // SAFETY: kernel validates caller capability at dispatch.
    let ret = unsafe { syscall(ViSyscall::WaitIrq, irq_num as usize, mmio_base, 0, 0) };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// Register a PCIe device BAR in the kernel BAR allowlist table.
///
/// Called by the Platform Cell after scanning ECAM config space to announce each
/// discovered device BAR. Subsequent Driver Cell calls to `sys_request_mmio` are
/// validated against this table. Requires singleton PlatformCap (allowlist bit 52).
pub fn sys_register_pcie_bar(bdf: u32, base: usize, len: usize) -> Result<(), SyscallError> {
    // SAFETY: kernel validates PlatformCap at dispatch.
    let ret = unsafe { syscall(ViSyscall::RegisterPcieBar, bdf as usize, base, len, 0) };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// Register a discovered PCI device with class and BAR0 info.
///
/// Populates the kernel `PCI_DEVICES` list so `sys_find_pcie_device` queries work
/// without a kernel-side ECAM scan. Call once per discovered device after `scan_and_register`.
/// Requires singleton PlatformCap (allowlist bit 53).
///
/// * `bdf`       — `(bus << 16) | (dev << 8) | fun`
/// * `cls`       — `(class << 16) | (subclass << 8) | prog_if`
/// * `bar0_base` — physical base of BAR0, or 0 if absent
/// * `bar0_size` — probed BAR0 size in bytes, or 0
pub fn sys_register_pci_device(
    bdf: u32,
    cls: u32,
    bar0_base: usize,
    bar0_size: usize,
) -> Result<(), SyscallError> {
    // SAFETY: kernel validates PlatformCap at dispatch.
    let ret = unsafe {
        syscall(ViSyscall::RegisterPciDevice, bdf as usize, cls as usize, bar0_base, bar0_size)
    };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// Fill `buf` with VirtIO-RNG entropy (true hardware randomness).
///
/// Required for TLS key generation — mtime-seeded PRNG is cryptographically broken.
/// Caps each call at 64 bytes (one VirtIO descriptor limit); loop to fill larger buffers.
/// Returns bytes written (0 if no VirtIO-RNG device is present — do not use for keys).
///
/// Requires `GetRandom` in the cell's `declare_syscalls!` list.
pub fn sys_get_random(buf: &mut [u8]) -> usize {
    // SAFETY: buf is a valid mutable slice; the kernel validates the pointer and writes
    // exactly min(len, 64) bytes into it before returning the count.
    let ret = unsafe {
        syscall(ViSyscall::GetRandom, buf.as_mut_ptr() as usize, buf.len(), 0, 0)
    };
    if ret > 0 { ret as usize } else { 0 }
}

/// Strip capabilities from a live running cell (runtime revocation).
///
/// `cap_mask` selects which capability classes to revoke — use the constants in
/// `api::syscall::cap_mask` (e.g. `SPAWN`, `NETWORK`, `MMIO_MASK`).
///
/// # Errors
/// - `PermissionDenied` if the caller lacks `SpawnCap`.
/// - `PermissionDenied` if the target is a system cell (`block_io` / `network` holder).
/// - `InvalidCommand` if `target_tid == caller` or target does not exist.
///
/// # Panics
/// Never panics — all failures surface as `Err`.
pub fn sys_cap_revoke(target_tid: usize, cap_mask: u32) -> Result<(), SyscallError> {
    // SAFETY: pure register syscall; no raw memory pointers involved.
    let ret = unsafe { syscall(ViSyscall::CapRevoke, target_tid, cap_mask as usize, 0, 0) };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// Block until one or more bits in `mask` fire, or `timeout_ticks` 10ms ticks elapse.
///
/// `timeout_ticks = 0` blocks indefinitely.  Returns the fired event bits (> 0) on
/// wake, or 0 on timeout.  See `api::syscall::events` for bit definitions.
///
/// Requires `WaitForEvent` in the cell's `declare_syscalls!` list.
pub fn sys_wait_for_event(mask: u32, timeout_ticks: u64) -> u32 {
    // SAFETY: pure blocking syscall; no raw pointers.
    let ret = unsafe {
        syscall(
            ViSyscall::WaitForEvent,
            mask as usize,
            timeout_ticks as usize,
            (timeout_ticks >> 32) as usize,
            0,
        )
    };
    ret as u32
}

// ── Supervisor Primitives (P03) ───────────────────────────────────────────────

/// Freeze a running Cell (stop it from being scheduled).
///
/// The cell still exists; its queued IPC is preserved and delivered after a
/// [`sys_resume_cell`] call. Requires `SupervisorCap` (granted only to init and
/// the Supervisor Cell by kernel init).
///
/// Returns `Ok(())` on success; `Err` if the caller lacks `SupervisorCap` or the
/// target does not exist / is already frozen.
pub fn sys_freeze_cell(target_tid: usize) -> Result<(), SyscallError> {
    // SAFETY: pure register syscall.
    let ret = unsafe { syscall(ViSyscall::FreezeCell, target_tid, 0, 0, 0) };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// Resume a frozen Cell so it can be scheduled again.
///
/// Requires `SupervisorCap`. Idempotent on an already-running cell.
pub fn sys_resume_cell(target_tid: usize) -> Result<(), SyscallError> {
    // SAFETY: pure register syscall.
    let ret = unsafe { syscall(ViSyscall::ResumeCell, target_tid, 0, 0, 0) };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// Forcibly terminate a Cell and reclaim all its resources.
///
/// Requires `SupervisorCap`. The `exit_code` is recorded in the audit log and
/// delivered via `NotifyOnExit` to any watchers. Cannot kill tid 0 (kernel) or
/// the calling cell itself.
pub fn sys_kill_cell(target_tid: usize, exit_code: u32) -> Result<(), SyscallError> {
    // SAFETY: pure register syscall.
    let ret = unsafe { syscall(ViSyscall::KillCell, target_tid, exit_code as usize, 0, 0) };
    if ret as usize == exit_code as usize { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

// ── Driver Cell Registration (P00) ───────────────────────────────────────────

/// Register the calling cell as the active block device driver.
///
/// After this call, `sys_lookup_service(service::BLOCK_DRIVER)` returns this
/// cell's TID. VFS cells send block-I/O IPC here instead of using the kernel
/// NVMe driver.  Requires `PcieDriverCap` (`pcie_driver = true` in manifest).
pub fn sys_register_block_driver() -> Result<(), SyscallError> {
    // SAFETY: pure register syscall; kernel validates PcieDriverCap at dispatch.
    let ret = unsafe { syscall(ViSyscall::RegisterBlockDriver, 0, 0, 0, 0) };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// Register the calling cell as the active NIC driver.
///
/// Requires `PcieDriverCap` (`pcie_driver = true` in manifest).
pub fn sys_register_nic_driver() -> Result<(), SyscallError> {
    // SAFETY: pure register syscall; kernel validates PcieDriverCap at dispatch.
    let ret = unsafe { syscall(ViSyscall::RegisterNicDriver, 0, 0, 0, 0) };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// Register the calling cell as the active GPU driver.
///
/// Uses `RegisterService` with `service::GPU_DRIVER` + `tid=0` (self-registration).
/// The kernel `RegisterService` handler allows this for `PcieDriverCap` holders.
/// Requires `PcieDriverCap` (`pcie_driver = true` in manifest).
pub fn sys_register_gpu_driver() -> Result<(), SyscallError> {
    // SAFETY: RegisterService always-permitted past allowlist; kernel validates
    // PcieDriverCap + GPU_DRIVER + tid=0 at dispatch.
    let ret = unsafe {
        syscall(
            ViSyscall::RegisterService,
            api::syscall::service::GPU_DRIVER as usize,
            0, // tid=0 means "register the caller itself"
            0,
            0,
        )
    };
    if ret == 0 { Ok(()) } else { Err(SyscallError::PermissionDenied) }
}

/// PCIe device descriptor written by `sys_find_pcie_device`.
///
/// Layout is `#[repr(C)]` to match the kernel's `write_volatile` sequence:
/// `bdf(u32) + found(u32) + bar0_base(u64) + bar0_len(u64)` = 24 bytes.
#[repr(C)]
pub struct PcieDeviceInfo {
    /// PCIe Requester ID: `bus<<8 | dev<<3 | fn`. Zero means not populated.
    pub bdf:       u32,
    /// 1 if the device was found, 0 otherwise.
    pub found:     u32,
    /// BAR0 physical base address (= virtual address in SAS).
    pub bar0_base: u64,
    /// BAR0 size in bytes.  At least 0x4000 (16 KiB) for NVMe controllers.
    pub bar0_len:  u64,
}

impl PcieDeviceInfo {
    /// Returns a zeroed-out placeholder; pass `&mut info` to `sys_find_pcie_device`.
    pub const fn zeroed() -> Self {
        Self { bdf: 0, found: 0, bar0_base: 0, bar0_len: 0 }
    }
}

/// Find the first PCIe device matching `(class, subclass, prog_if)`.
///
/// On success (`Ok(true)`), `info` is populated with the BDF and BAR0 address.
/// The kernel also records BDF → caller ownership in the Resource Registry for
/// subsequent IOMMU authorization.
///
/// Requires `PcieDriverCap` (kernel-granted ZST — init grants it to Driver Cells
/// at spawn via direct TCB write, not via manifest).
///
/// Returns `Ok(false)` if no matching device is present (VirtIO fallback case).
pub fn sys_find_pcie_device(
    class: u8, subclass: u8, prog_if: u8,
    info: &mut PcieDeviceInfo,
) -> Result<bool, SyscallError> {
    // SAFETY: `info` is a valid mutable reference; kernel writes exactly 24 bytes.
    // In SAS the kernel's VA == the cell's VA, so the pointer is directly usable.
    let ret = unsafe {
        syscall(
            ViSyscall::FindPcieDevice,
            class    as usize,
            subclass as usize,
            prog_if  as usize,
            info as *mut PcieDeviceInfo as usize,
        )
    };
    match ret {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(SyscallError::PermissionDenied),
    }
}

/// Non-blocking check: has `target_tid` already called `sys_hotswap_ready()`?
///
/// Returns `Ok(true)` when the cell is ready, `Ok(false)` when not yet,
/// `Err(PermissionDenied)` when the caller lacks `SupervisorCap`, and
/// `Err(Unknown)` when `target_tid` is not a live task.
///
/// Requires `SupervisorCap` (kernel-granted — same gate as FreezeCell/ResumeCell/KillCell).
pub fn sys_query_hotswap_ready(target_tid: usize) -> Result<bool, SyscallError> {
    // SAFETY: pure register syscall; kernel reads task.hotswap_ready under the SCHEDULER lock.
    let ret = unsafe { syscall(ViSyscall::QueryHotswapReady, target_tid, 0, 0, 0) };
    match ret {
        1 => Ok(true),
        0 => Ok(false),
        _ => Err(SyscallError::PermissionDenied), // PermissionDenied or unknown tid (FileNotFound)
    }
}
