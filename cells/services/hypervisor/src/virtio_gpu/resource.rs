//! VirtIO-GPU host-side resource table and scanout Grant copy path.

extern crate alloc;

use super::command;
use alloc::{collections::BTreeMap, vec::Vec};

mod control;
mod render;

const BYTES_PER_PIXEL: usize = 4;
const MAX_RESOURCES: usize = 64;
const MAX_BACKING_ENTRIES: usize = 128;
const SCANOUT_ID: u32 = 0;

pub enum ResourceError {
    InvalidResourceId,
    InvalidParameter,
    OutOfMemory,
}

struct Resource {
    width: u32,
    height: u32,
    format: u32,
    backing: Vec<command::MemEntry>,
}

struct ScanoutGrant {
    reg_id: usize,
    ptr: *mut u8,
    len: usize,
    width: u32,
    height: u32,
}

struct CursorState {
    resource_id: u32,
    x: u32,
    y: u32,
    hot_x: u32,
    hot_y: u32,
}

pub struct ResourceTable {
    resources: BTreeMap<u32, Resource>,
    scanout_resource_id: Option<u32>,
    scanout: Option<ScanoutGrant>,
    scanout_dimensions: Option<(u32, u32)>,
    cursor: Option<CursorState>,
}

fn copy_from_backing(
    vm_id: usize,
    backing: &[command::MemEntry],
    mut offset: u64,
    dst: &mut [u8],
) -> Result<(), ResourceError> {
    let mut copied = 0usize;
    for entry in backing {
        let entry_len = entry.length as u64;
        if offset >= entry_len {
            offset -= entry_len;
            continue;
        }
        let start = entry
            .addr
            .checked_add(offset)
            .ok_or(ResourceError::InvalidParameter)?;
        let available = (entry_len - offset) as usize;
        let count = available.min(dst.len() - copied);
        let got = crate::vmm::read_guest_memory(vm_id, start, &mut dst[copied..copied + count]);
        if got != count || got == usize::MAX {
            return Err(ResourceError::InvalidParameter);
        }
        copied += count;
        if copied == dst.len() {
            return Ok(());
        }
        offset = 0;
    }
    Err(ResourceError::InvalidParameter)
}

fn validate_rect(rect: command::Rect, width: u32, height: u32) -> Result<(), ResourceError> {
    let x2 = rect
        .x
        .checked_add(rect.width)
        .ok_or(ResourceError::InvalidParameter)?;
    let y2 = rect
        .y
        .checked_add(rect.height)
        .ok_or(ResourceError::InvalidParameter)?;
    if x2 > width || y2 > height {
        return Err(ResourceError::InvalidParameter);
    }
    Ok(())
}

fn rect_covers_resource(rect: command::Rect, width: u32, height: u32) -> bool {
    rect.x == 0 && rect.y == 0 && rect.width == width && rect.height == height
}

fn full_rect(width: u32, height: u32) -> command::Rect {
    command::Rect {
        x: 0,
        y: 0,
        width,
        height,
    }
}

fn pixel_len(width: u32, height: u32) -> Option<usize> {
    row_bytes(width)?.checked_mul(height as usize)
}

fn row_bytes(width: u32) -> Option<usize> {
    (width as usize).checked_mul(BYTES_PER_PIXEL)
}

fn blend_cursor(
    scanout: &mut ScanoutGrant,
    pixels: &[u8],
    width: u32,
    height: u32,
    cursor: &CursorState,
) {
    let origin_x = cursor.x as i64 - cursor.hot_x as i64;
    let origin_y = cursor.y as i64 - cursor.hot_y as i64;
    for row in 0..height {
        let dst_y = origin_y + row as i64;
        if dst_y < 0 || dst_y >= scanout.height as i64 {
            continue;
        }
        for col in 0..width {
            let dst_x = origin_x + col as i64;
            if dst_x < 0 || dst_x >= scanout.width as i64 {
                continue;
            }
            let src_off = ((row as usize * width as usize) + col as usize) * 4;
            let dst_off = ((dst_y as usize * scanout.width as usize) + dst_x as usize) * 4;
            if src_off + 4 > pixels.len() || dst_off + 4 > scanout.len {
                continue;
            }
            let alpha = pixels[src_off + 3] as u32;
            if alpha == 0 {
                continue;
            }
            // SAFETY: dst_off + 4 <= scanout.len bounds this pointer within the Grant.
            let dst = unsafe { core::slice::from_raw_parts_mut(scanout.ptr.add(dst_off), 4) };
            for channel in 0..3 {
                let src = pixels[src_off + channel] as u32;
                let old = dst[channel] as u32;
                dst[channel] = ((src * alpha + old * (255 - alpha)) / 255) as u8;
            }
            dst[3] = 255;
        }
    }
}
