//! The kernel's file-descriptor and working-directory layer over the embedded
//! BootFS (VIFS1): path resolution against a task's cwd, the per-task open-file
//! table and the stdio descriptors. A Spec 15 exception: its target is the VFS
//! Cell, which already owns every mounted filesystem except BootFS.
// Result<_, ()> signatures predate a kernel error type, as in `task` (where
// this layer lived until Phase 09).
#![allow(clippy::result_unit_err)]

use crate::task::{current_task_id, SCHEDULER};
use alloc::string::String;

fn append_path_components(output: &mut String, path: &str) {
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if output.len() > 1 {
                    let slash = output.rfind('/').unwrap_or(0);
                    output.truncate(slash.max(1));
                }
            }
            component => {
                if output.len() > 1 {
                    output.push('/');
                }
                output.push_str(component);
            }
        }
    }
}

/// Lexically canonicalize `path` against `cwd` without allocating components.
///
/// The caller reserves the maximum possible output size once, so appending
/// cannot trigger additional allocator calls.
pub(crate) fn resolve_path(cwd: &str, path: &str) -> core::result::Result<String, ()> {
    if path.is_empty() {
        return Err(());
    }

    let capacity = if path.starts_with('/') {
        path.len()
    } else {
        cwd.len()
            .checked_add(path.len())
            .and_then(|len| len.checked_add(1))
            .ok_or(())?
    };
    let mut output = String::new();
    output.try_reserve_exact(capacity).map_err(|_| ())?;
    output.push('/');
    if !path.starts_with('/') {
        append_path_components(&mut output, cwd);
    }
    append_path_components(&mut output, path);
    Ok(output)
}

struct ResolvedTaskPath {
    path: String,
    #[cfg(feature = "p01-trace")]
    owner: crate::p01_measurement::Owner,
}

fn resolve_task_path(caller_id: usize, path: &str) -> core::result::Result<ResolvedTaskPath, ()> {
    let scheduler = SCHEDULER.lock();
    let task = scheduler
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id));
    let task = task.ok_or(())?;
    let path = if path.starts_with('/') {
        resolve_path("/", path)?
    } else {
        resolve_path(&task.cwd, path)?
    };
    Ok(ResolvedTaskPath {
        path,
        #[cfg(feature = "p01-trace")]
        owner: crate::p01_resources::owner(task),
    })
}

// --- File System Syscall Handlers ---

pub fn file_open(caller_id: usize, path: &str) -> core::result::Result<usize, ()> {
    let resolved = resolve_task_path(caller_id, path)?;

    use crate::fs::VIFS1;
    #[cfg(feature = "p01-trace")]
    let heap_owner = resolved.owner;
    let file = {
        let fs_lock = VIFS1.lock();
        let fs = fs_lock.as_ref().ok_or(())?;
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            Some(heap_owner),
            crate::memory::heap::p01_owner::TASK_IO,
        );
        fs.open(&resolved.path, abi::fs::OpenMode::Read)
            .map_err(|_| ())?
    };

    let mut scheduler = SCHEDULER.lock();
    let task = scheduler
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&caller_id))
        .ok_or(())?;
    let fd = task.open_files.keys().max().map(|fd| fd + 1).unwrap_or(3);
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        Some(crate::p01_resources::owner(task)),
        crate::memory::heap::p01_owner::TASK_FIELDS,
    );
    task.open_files
        .insert(fd, crate::task::tcb::FileHandle::new(file));
    Ok(fd)
}

pub fn file_read(fd: usize, buf: &mut [u8]) -> usize {
    if fd == 0 {
        if let Some(count) = crate::ipc::cell_session::stdio_read(current_task_id(), buf) {
            return count;
        }
        // Stdin (Keyboard)
        if buf.is_empty() {
            return 0;
        }

        let mut cons = crate::dev::console_drv::CONSOLE.lock();
        cons.poll();
        let b = cons.read_byte();
        if let Some(byte) = b {
            buf[0] = byte;
            return 1;
        }
        return 0;
    }

    // File Read — synchronous, under the SCHEDULER lock: the VIFS1 ramdisk is
    // synchronous, and a blocking reader (e.g. DOOM's WAD load) needs the real
    // byte count, never a placeholder 0.
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            if let Some(handle) = task.open_files.get_mut(&fd) {
                return handle.read(buf).unwrap_or(0);
            }
        }
    }
    0
}

pub fn file_write(fd: usize, buf: &[u8]) -> usize {
    if fd == 1 || fd == 2 {
        if let Some(count) = crate::ipc::cell_session::stdio_write(current_task_id(), fd, buf) {
            return count;
        }
        // Stdout/Stderr
        if let Ok(s) = core::str::from_utf8(buf) {
            crate::task::print_user_log(s, crate::task::current_task_id());
            return buf.len();
        }
        return 0;
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

pub fn file_readdir(fd: usize, buf: &mut [u8]) -> core::result::Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            if let Some(handle) = task.open_files.get_mut(&fd) {
                match handle.read_dir() {
                    Ok(Some(entry)) => {
                        // Serialize DirEntry to buf
                        // Entry size is 64 + 1 + 8 + padding = 73+ ? sizeof(DirEntry)
                        // types::DirEntry is repr(C).
                        // We copy bytes directly.
                        let ptr = &entry as *const _ as *const u8;
                        let size = core::mem::size_of::<types::DirEntry>();
                        if buf.len() < size {
                            return Err(());
                        }

                        unsafe {
                            core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), size);
                        }
                        return Ok(size);
                    }
                    Ok(None) => return Ok(0), // EOF
                    Err(_) => return Err(()),
                }
            }
        }
    }
    Err(())
}

/// Gather truthful metadata for one descriptor owned by `caller_id`.
///
/// Standard descriptors are character devices with their fixed access
/// direction. VIFS descriptors are read-only in the current open contract;
/// regular-file size is queried without changing the handle cursor, while
/// directories report size zero. No caller memory is accessed here.
pub fn file_fstat(
    caller_id: usize,
    fd: usize,
) -> core::result::Result<abi::syscall::ViFstatV1, ()> {
    use abi::syscall::{
        ViFstatV1, VI_FSTAT_ACCESS_READ, VI_FSTAT_ACCESS_WRITE, VI_FSTAT_KIND_CHARACTER,
        VI_FSTAT_KIND_DIRECTORY, VI_FSTAT_KIND_REGULAR,
    };

    let mut scheduler = SCHEDULER.lock();
    let task = scheduler
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&caller_id))
        .ok_or(())?;
    let mut metadata = ViFstatV1::default();
    match fd {
        0 => {
            metadata.kind = VI_FSTAT_KIND_CHARACTER;
            metadata.access = VI_FSTAT_ACCESS_READ;
        }
        1 | 2 => {
            metadata.kind = VI_FSTAT_KIND_CHARACTER;
            metadata.access = VI_FSTAT_ACCESS_WRITE;
        }
        _ => {
            let handle = task.open_files.get_mut(&fd).ok_or(())?;
            metadata.access = VI_FSTAT_ACCESS_READ;
            if handle.is_dir() {
                metadata.kind = VI_FSTAT_KIND_DIRECTORY;
            } else {
                metadata.kind = VI_FSTAT_KIND_REGULAR;
                metadata.size = handle.size().map_err(|_| ())?;
            }
        }
    }
    Ok(metadata)
}

pub fn file_chdir(caller_id: usize, path: &str) -> core::result::Result<usize, ()> {
    let full_path = resolve_task_path(caller_id, path)?.path;

    let is_vfs_path = full_path == "/tmp"
        || full_path.starts_with("/tmp/")
        || full_path == "/data"
        || full_path.starts_with("/data/")
        || full_path == "/srv"
        || full_path.starts_with("/srv/")
        || full_path == "/mnt"
        || full_path.starts_with("/mnt/");

    let is_directory = if is_vfs_path {
        true
    } else {
        let fs_lock = crate::fs::VIFS1.lock();
        let fs = fs_lock.as_ref().ok_or(())?;
        let stat = fs.stat(&full_path).map_err(|_| ())?;
        stat.exists && stat.is_dir
    };
    if !is_directory {
        return Err(());
    }

    let mut scheduler = SCHEDULER.lock();
    let task = scheduler
        .as_mut()
        .and_then(|sched| sched.tasks.get_mut(&caller_id))
        .ok_or(())?;
    task.cwd = full_path;
    #[cfg(feature = "p01-trace")]
    if task.cwd.capacity() != 0 {
        crate::memory::heap::p01_owner::runtime_buffer(
            task.cwd.as_ptr() as usize,
            core::alloc::Layout::array::<u8>(task.cwd.capacity()).unwrap(),
            crate::p01_resources::owner(task),
            crate::memory::heap::p01_owner::TASK_FIELDS,
        );
    }
    Ok(0)
}

pub fn file_seek(fd: usize, offset: isize, whence: usize) -> core::result::Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            if let Some(handle) = task.open_files.get_mut(&fd) {
                let pos = match whence {
                    0 => abi::fs::SeekFrom::Start(offset as u64),
                    1 => abi::fs::SeekFrom::Current(offset as i64),
                    2 => abi::fs::SeekFrom::End(offset as i64),
                    _ => return Err(()), // Invalid whence
                };

                if let Ok(new_pos) = handle.seek(pos) {
                    return Ok(new_pos as usize);
                }
            }
        }
    }
    Err(())
}

pub fn file_remove(caller_id: usize, path: &str) -> core::result::Result<usize, ()> {
    let full_path = resolve_task_path(caller_id, path)?.path;

    let fs_lock = crate::fs::VIFS1.lock();
    let fs = fs_lock.as_ref().ok_or(())?;
    fs.remove(&full_path).map_err(|_| ())?;
    Ok(0)
}

pub fn file_getcwd(caller_id: usize, buf: &mut [u8]) -> core::result::Result<usize, ()> {
    let scheduler = SCHEDULER.lock();
    let task = scheduler
        .as_ref()
        .and_then(|sched| sched.tasks.get(&caller_id))
        .ok_or(())?;
    let cwd = task.cwd.as_bytes();
    if buf.len() < cwd.len() {
        return Err(());
    }
    buf[..cwd.len()].copy_from_slice(cwd);
    Ok(cwd.len())
}
