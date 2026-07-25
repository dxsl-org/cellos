extern crate alloc;

use super::{
    command, full_rect, pixel_len, rect_covers_resource, Resource, ResourceError,
    ResourceTable, SCANOUT_ID, MAX_BACKING_ENTRIES, MAX_RESOURCES,
};
use alloc::vec::Vec;

impl ResourceTable {
    pub const fn new() -> Self {
        Self {
            resources: alloc::collections::BTreeMap::new(),
            scanout_resource_id: None,
            scanout: None,
            scanout_dimensions: None,
            cursor: None,
        }
    }

    pub fn reset(&mut self) {
        self.scanout_resource_id = None;
        self.cursor = None;
        self.resources.clear();
    }

    pub fn prepare_scanout(&mut self, width: u32, height: u32) -> Result<(), ResourceError> {
        if pixel_len(width, height).is_none() {
            return Err(ResourceError::InvalidParameter);
        }
        self.scanout_dimensions = Some((width, height));
        self.ensure_scanout(width, height)
    }

    pub fn create_2d(&mut self, cmd: command::ResourceCreate2d) -> Result<(), ResourceError> {
        if !super::super::rules::valid_new_resource_id(
            cmd.resource_id,
            self.resources.contains_key(&cmd.resource_id),
        ) {
            return Err(ResourceError::InvalidResourceId);
        }
        if !matches!(
            cmd.format,
            command::FORMAT_B8G8R8A8_UNORM | command::FORMAT_B8G8R8X8_UNORM
        ) || pixel_len(cmd.width, cmd.height).is_none()
        {
            return Err(ResourceError::InvalidParameter);
        }
        if self.resources.len() >= MAX_RESOURCES {
            return Err(ResourceError::OutOfMemory);
        }
        self.resources.insert(
            cmd.resource_id,
            Resource {
                width: cmd.width,
                height: cmd.height,
                format: cmd.format,
                backing: Vec::new(),
            },
        );
        Ok(())
    }

    pub fn unref(&mut self, resource_id: u32) -> Result<(), ResourceError> {
        if self.cursor.as_ref().is_some_and(|cursor| cursor.resource_id == resource_id) {
            return Err(ResourceError::InvalidResourceId);
        }
        if self.scanout_resource_id == Some(resource_id) {
            return Err(ResourceError::InvalidResourceId);
        }
        self.resources
            .remove(&resource_id)
            .map(|_| ())
            .ok_or(ResourceError::InvalidResourceId)
    }

    pub fn attach_backing(
        &mut self,
        resource_id: u32,
        entries: Vec<command::MemEntry>,
    ) -> Result<(), ResourceError> {
        if entries.len() > MAX_BACKING_ENTRIES {
            return Err(ResourceError::InvalidParameter);
        }
        let resource = self
            .resources
            .get_mut(&resource_id)
            .ok_or(ResourceError::InvalidResourceId)?;
        if entries.iter().any(|entry| entry.length == 0) {
            return Err(ResourceError::InvalidParameter);
        }
        resource.backing = entries;
        Ok(())
    }

    pub fn set_scanout(
        &mut self,
        vm_id: usize,
        cmd: command::SetScanoutCmd,
    ) -> Result<(), ResourceError> {
        if cmd.scanout_id != SCANOUT_ID {
            return Err(ResourceError::InvalidParameter);
        }
        if cmd.resource_id == 0 {
            self.scanout_resource_id = None;
            return Ok(());
        }
        let resource = self
            .resources
            .get(&cmd.resource_id)
            .ok_or(ResourceError::InvalidResourceId)?;
        let (width, height) = (resource.width, resource.height);
        if self.scanout_dimensions != Some((width, height)) {
            return Err(ResourceError::InvalidParameter);
        }
        if !rect_covers_resource(cmd.rect, width, height) {
            return Err(ResourceError::InvalidParameter);
        }
        self.ensure_scanout(width, height)?;
        self.scanout_resource_id = Some(cmd.resource_id);
        self.copy_rect(vm_id, cmd.resource_id, full_rect(width, height), 0)?;
        self.redraw_cursor(vm_id)
    }

    pub fn transfer_to_host(
        &mut self,
        vm_id: usize,
        cmd: command::TransferToHost2dCmd,
    ) -> Result<(), ResourceError> {
        if !self.resources.contains_key(&cmd.resource_id) {
            return Err(ResourceError::InvalidResourceId);
        }
        if self.scanout_resource_id != Some(cmd.resource_id) {
            return Ok(());
        }
        self.copy_rect(vm_id, cmd.resource_id, cmd.rect, cmd.offset)?;
        self.redraw_cursor(vm_id)
    }

    pub fn update_cursor(
        &mut self,
        vm_id: usize,
        cmd: command::CursorCmd,
    ) -> Result<(), ResourceError> {
        if cmd.scanout_id != SCANOUT_ID {
            return Err(ResourceError::InvalidParameter);
        }
        let resource = self
            .resources
            .get(&cmd.resource_id)
            .ok_or(ResourceError::InvalidResourceId)?;
        if resource.width == 0
            || resource.height == 0
            || resource.width > 64
            || resource.height > 64
        {
            return Err(ResourceError::InvalidParameter);
        }
        self.cursor = Some(super::CursorState {
            resource_id: cmd.resource_id,
            x: cmd.x,
            y: cmd.y,
            hot_x: cmd.hot_x,
            hot_y: cmd.hot_y,
        });
        self.redraw_cursor(vm_id)
    }

    pub fn move_cursor(
        &mut self,
        vm_id: usize,
        cmd: command::CursorCmd,
    ) -> Result<(), ResourceError> {
        let cursor = self.cursor.as_mut().ok_or(ResourceError::InvalidResourceId)?;
        if cmd.scanout_id != SCANOUT_ID {
            return Err(ResourceError::InvalidParameter);
        }
        cursor.x = cmd.x;
        cursor.y = cmd.y;
        self.redraw_cursor(vm_id)
    }

}
