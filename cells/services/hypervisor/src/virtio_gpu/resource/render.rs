extern crate alloc;

use super::{blend_cursor, copy_from_backing, full_rect, pixel_len, row_bytes};
use super::{ResourceError, ResourceTable, ScanoutGrant};
use alloc::vec::Vec;

impl ResourceTable {
    pub fn flush_resource(
        &self,
        resource_id: u32,
        rect: super::command::Rect,
    ) -> Result<(), ResourceError> {
        self.validate_flush_rect(resource_id, rect).map(|_| ())
    }

    pub fn scanout_grant(&self) -> Option<(usize, *mut u8, usize, u32, u32)> {
        self.scanout.as_ref().map(|scanout| {
            (
                scanout.reg_id,
                scanout.ptr,
                scanout.len,
                scanout.width,
                scanout.height,
            )
        })
    }

    pub fn bound_resource(&self) -> Option<(u32, u32, u32, u32)> {
        let resource_id = self.scanout_resource_id?;
        let resource = self.resources.get(&resource_id)?;
        Some((
            resource_id,
            resource.width,
            resource.height,
            resource.format,
        ))
    }

    pub fn validate_flush_rect(
        &self,
        resource_id: u32,
        rect: super::command::Rect,
    ) -> Result<super::command::Rect, ResourceError> {
        let resource = self
            .resources
            .get(&resource_id)
            .ok_or(ResourceError::InvalidResourceId)?;
        if self.scanout_resource_id != Some(resource_id) {
            return Err(ResourceError::InvalidResourceId);
        }
        super::validate_rect(rect, resource.width, resource.height)?;
        Ok(rect)
    }

    pub fn teardown_scanout(&mut self) {
        self.scanout_resource_id = None;
        self.release_scanout();
    }

    pub(crate) fn copy_rect(
        &mut self,
        vm_id: usize,
        resource_id: u32,
        rect: super::command::Rect,
        offset: u64,
    ) -> Result<(), ResourceError> {
        let resource = self
            .resources
            .get(&resource_id)
            .ok_or(ResourceError::InvalidResourceId)?;
        let scanout = self
            .scanout
            .as_mut()
            .ok_or(ResourceError::InvalidResourceId)?;
        super::validate_rect(rect, resource.width, resource.height)?;
        if scanout.width != resource.width || scanout.height != resource.height {
            return Err(ResourceError::InvalidParameter);
        }
        let row_len = row_bytes(rect.width).ok_or(ResourceError::InvalidParameter)?;
        let dst_stride = row_bytes(resource.width).ok_or(ResourceError::InvalidParameter)?;
        let rect_height = rect.height as usize;
        let src_base = offset
            .checked_add(rect.y as u64 * dst_stride as u64)
            .and_then(|v| v.checked_add(rect.x as u64 * super::BYTES_PER_PIXEL as u64))
            .ok_or(ResourceError::InvalidParameter)?;
        let dst_base = (rect.y as usize)
            .checked_mul(dst_stride)
            .and_then(|v| v.checked_add(rect.x as usize * super::BYTES_PER_PIXEL))
            .ok_or(ResourceError::InvalidParameter)?;
        for row in 0..rect_height {
            let src_off = src_base
                .checked_add((row * dst_stride) as u64)
                .ok_or(ResourceError::InvalidParameter)?;
            let dst_off = dst_base
                .checked_add(row * dst_stride)
                .ok_or(ResourceError::InvalidParameter)?;
            let dst_end = dst_off
                .checked_add(row_len)
                .ok_or(ResourceError::InvalidParameter)?;
            if dst_end > scanout.len {
                return Err(ResourceError::InvalidParameter);
            }
            // SAFETY: `scanout.ptr` comes from `sys_grant_slice` for the registered
            // Grant owned by this VMM; `dst_off + row_len <= scanout.len` above
            // bounds the resulting slice within that Grant.
            let dst = unsafe { core::slice::from_raw_parts_mut(scanout.ptr.add(dst_off), row_len) };
            copy_from_backing(vm_id, &resource.backing, src_off, dst)?;
        }
        Ok(())
    }

    pub(crate) fn redraw_cursor(&mut self, vm_id: usize) -> Result<(), ResourceError> {
        let Some(cursor) = self.cursor.as_ref() else {
            return Ok(());
        };
        let cursor_state = super::CursorState {
            resource_id: cursor.resource_id,
            x: cursor.x,
            y: cursor.y,
            hot_x: cursor.hot_x,
            hot_y: cursor.hot_y,
        };
        let Some((scanout_id, width, height, _)) = self.bound_resource() else {
            return Ok(());
        };
        self.copy_rect(vm_id, scanout_id, full_rect(width, height), 0)?;
        let cursor_resource = self
            .resources
            .get(&cursor_state.resource_id)
            .ok_or(ResourceError::InvalidResourceId)?;
        let cursor_len = pixel_len(cursor_resource.width, cursor_resource.height)
            .ok_or(ResourceError::InvalidParameter)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(cursor_len)
            .map_err(|_| ResourceError::OutOfMemory)?;
        pixels.resize(cursor_len, 0);
        copy_from_backing(vm_id, &cursor_resource.backing, 0, &mut pixels)?;
        let scanout = self
            .scanout
            .as_mut()
            .ok_or(ResourceError::InvalidResourceId)?;
        blend_cursor(
            scanout,
            &pixels,
            cursor_resource.width,
            cursor_resource.height,
            &cursor_state,
        );
        Ok(())
    }

    pub(crate) fn ensure_scanout(&mut self, width: u32, height: u32) -> Result<(), ResourceError> {
        let len = pixel_len(width, height).ok_or(ResourceError::InvalidParameter)?;
        if matches!(&self.scanout, Some(scanout) if scanout.width == width && scanout.height == height)
        {
            return Ok(());
        }
        self.release_scanout();
        let reg_id = ostd::syscall::sys_grant_register(len).ok_or(ResourceError::OutOfMemory)?;
        let Some(ptr) = ostd::syscall::sys_grant_slice(reg_id) else {
            let _ = ostd::syscall::sys_grant_unregister(reg_id);
            return Err(ResourceError::OutOfMemory);
        };
        self.scanout = Some(ScanoutGrant {
            reg_id,
            ptr,
            len,
            width,
            height,
        });
        Ok(())
    }

    pub(crate) fn release_scanout(&mut self) {
        if let Some(scanout) = self.scanout.take() {
            let _ = ostd::syscall::sys_grant_unregister(scanout.reg_id);
        }
    }
}
