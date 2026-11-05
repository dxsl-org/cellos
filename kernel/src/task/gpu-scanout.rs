//! `GpuFlush` / `GpuCursor` (300/301): the registered compositor's scanout path.
//!
//! The pixel source is a caller pointer. Before the GPU Driver Cell is told to
//! read it, every page of the exact range it will read is checked readable
//! through the caller's copy view (so a kernel or unmapped address is refused,
//! not painted or faulted on). The kernel framebuffer fallback copies through
//! the same view in bounded chunks and never dereferences the user pointer.

use super::copy_glue::TaskCopyView;
use super::syscall::SyscallError;
use core::sync::atomic::{AtomicBool, Ordering};

const PAGE_SIZE: usize = 4096;
/// One 64×64 BGRA8888 sprite, the only size `VirtIOGpu::setup_cursor` takes.
const CURSOR_SPRITE_LEN: usize = 64 * 64 * 4;
/// Kernel staging chunk for the framebuffer fallback (stack, not heap).
const STAGING_CHUNK: usize = 4096;

/// GPU Driver Cell opcodes after the AppContext `[0xAC, 0x00]` envelope.
const OP_FLUSH: u8 = 0x10;
const OP_CUR_SET: u8 = 0x11;
const OP_CUR_MOVE: u8 = 0x12;

/// `xy`/`wh` packed as `(hi << 16) | lo`, 16 bits each, exactly as `ostd` packs.
fn unpack(packed: usize) -> (usize, usize) {
    ((packed >> 16) & 0xFFFF, packed & 0xFFFF)
}

/// Scanout belongs to the live compositor registration: its provider task or a
/// thread of the same Cell incarnation. Anyone else is refused.
fn require_compositor(caller_id: usize) -> Result<(), SyscallError> {
    let Some((tid, cell_id, generation)) =
        crate::cell::service_registry::lookup_bound(api::syscall::service::COMPOSITOR)
    else {
        return Err(SyscallError::PermissionDenied);
    };
    let same_cell = || {
        super::SCHEDULER
            .lock()
            .as_ref()
            .and_then(|sched| sched.tasks.get(&caller_id))
            .is_some_and(|task| task.cell_id.0 == cell_id && task.cell_generation == generation)
    };
    if caller_id == tid || same_cell() {
        Ok(())
    } else {
        Err(SyscallError::PermissionDenied)
    }
}

/// Every page of `[ptr, ptr + len)` is readable through `view`.
fn probe_readable(view: &TaskCopyView, ptr: usize, len: usize) -> Result<(), SyscallError> {
    let end = ptr.checked_add(len).ok_or(SyscallError::InvalidInput)?;
    if ptr == 0 || len == 0 {
        return Err(SyscallError::InvalidInput);
    }
    let mut byte = [0u8; 1];
    let mut at = ptr;
    while at < end {
        view.read_into(at, &mut byte)
            .map_err(|_| SyscallError::InvalidInput)?;
        at = (at & !(PAGE_SIZE - 1)) + PAGE_SIZE;
    }
    Ok(())
}

fn gpu_cell() -> usize {
    crate::task::drivers::driver_cell::GPU_DRIVER_CELL.load(Ordering::Acquire)
}

fn warn_once(flag: &AtomicBool, message: &str) {
    if !flag.swap(true, Ordering::Relaxed) {
        log::warn!("{}", message);
    }
}

pub(super) fn flush(
    caller_id: usize,
    data_ptr: usize,
    data_len: usize,
    xy: usize,
    wh: usize,
) -> Result<usize, SyscallError> {
    require_compositor(caller_id)?;
    let (x, y) = unpack(xy);
    let (w, h) = unpack(wh);
    // The u32 bound is the GPU Cell's wire field for the byte count.
    let needed = w
        .checked_mul(h)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&bytes| bytes != 0 && bytes <= data_len && bytes <= u32::MAX as usize)
        .ok_or(SyscallError::InvalidInput)?;
    // Every source offset below is `< needed`, so this one check covers them all.
    data_ptr
        .checked_add(needed)
        .ok_or(SyscallError::InvalidInput)?;
    let view = super::syscall::caller_copy_view_for(caller_id)?;

    let gpu_cell = gpu_cell();
    if gpu_cell != 0 {
        probe_readable(&view, data_ptr, needed)?;
        let mut msg = [0u8; 23];
        msg[..3].copy_from_slice(&[0xAC, 0x00, OP_FLUSH]);
        msg[3..7].copy_from_slice(&(xy as u32).to_le_bytes());
        msg[7..11].copy_from_slice(&(wh as u32).to_le_bytes());
        msg[11..19].copy_from_slice(&(data_ptr as u64).to_le_bytes());
        msg[19..23].copy_from_slice(&(needed as u32).to_le_bytes());
        let _ = super::ipc_post_nonblock(0, gpu_cell, &msg);
        return Ok(0);
    }

    let Some((fb_base, fb_size, fb_w, fb_h, fb_pitch)) =
        crate::resource_registry::display_framebuffer_info()
    else {
        static WARNED: AtomicBool = AtomicBool::new(false);
        warn_once(
            &WARNED,
            "[gpu_flush] GPU Driver Cell not registered — software fallback active",
        );
        return Err(SyscallError::Unknown);
    };
    let visible_w = w.min((fb_w as usize).saturating_sub(x));
    let visible_h = h.min((fb_h as usize).saturating_sub(y));
    let line_bytes = visible_w * 4;
    #[cfg(target_arch = "x86_64")]
    let fb = crate::memory::frame::phys_to_virt(fb_base) as *mut u8;
    #[cfg(not(target_arch = "x86_64"))]
    let fb = fb_base as *mut u8;
    let mut staging = [0u8; STAGING_CHUNK];
    for row in 0..visible_h {
        let dst = (y + row)
            .checked_mul(fb_pitch)
            .and_then(|offset| offset.checked_add(x * 4))
            .filter(|offset| {
                offset
                    .checked_add(line_bytes)
                    .is_some_and(|end| end <= fb_size)
            })
            .ok_or(SyscallError::InvalidInput)?;
        let src = data_ptr + row * w * 4;
        let mut done = 0;
        while done < line_bytes {
            let chunk = &mut staging[..(line_bytes - done).min(STAGING_CHUNK)];
            view.read_into(src + done, chunk)
                .map_err(|_| SyscallError::InvalidInput)?;
            // SAFETY: `dst + line_bytes <= fb_size` was checked above, and the
            // active display framebuffer is a kernel-mapped, non-RAM window of
            // `fb_size` bytes at `fb` registered through `RegisterDisplayFramebuffer`.
            unsafe {
                core::ptr::copy_nonoverlapping(chunk.as_ptr(), fb.add(dst + done), chunk.len());
            }
            done += chunk.len();
        }
    }
    Ok(0)
}

pub(super) fn cursor(
    caller_id: usize,
    op: usize,
    data_ptr: usize,
    xy: usize,
    hot: usize,
) -> Result<usize, SyscallError> {
    require_compositor(caller_id)?;
    let gpu_cell = gpu_cell();
    if gpu_cell == 0 {
        static WARNED: AtomicBool = AtomicBool::new(false);
        warn_once(&WARNED, "[gpu_cursor] GPU Driver Cell not registered");
        return Err(SyscallError::Unknown);
    }
    match op {
        0 => {
            let view = super::syscall::caller_copy_view_for(caller_id)?;
            probe_readable(&view, data_ptr, CURSOR_SPRITE_LEN)?;
            let mut msg = [0u8; 19];
            msg[..3].copy_from_slice(&[0xAC, 0x00, OP_CUR_SET]);
            msg[3..11].copy_from_slice(&(data_ptr as u64).to_le_bytes());
            msg[11..15].copy_from_slice(&(xy as u32).to_le_bytes());
            msg[15..19].copy_from_slice(&(hot as u32).to_le_bytes());
            let _ = super::ipc_post_nonblock(0, gpu_cell, &msg);
        }
        1 => {
            let mut msg = [0u8; 7];
            msg[..3].copy_from_slice(&[0xAC, 0x00, OP_CUR_MOVE]);
            msg[3..7].copy_from_slice(&(xy as u32).to_le_bytes());
            let _ = super::ipc_post_nonblock(0, gpu_cell, &msg);
        }
        _ => return Err(SyscallError::InvalidInput),
    }
    Ok(0)
}

#[cfg(test)]
#[path = "gpu-scanout-tests.rs"]
mod tests;
