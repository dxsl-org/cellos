use core::{mem, slice};

use crate::fs::{BlockDevice, Result, pod::Pod};

use alloc::{boxed::Box, vec};

/// A buffer that provides byte-level access to an underlying BlockDevice.
pub struct BlockBuffer {
    dev: Box<dyn BlockDevice>,
    block_size: usize,
}

impl BlockBuffer {
    /// Creates a new `BlockBuffer` that wraps the given block device.
    pub fn new(dev: Box<dyn BlockDevice>) -> Self {
        let block_size = dev.block_size();

        Self { dev, block_size }
    }

    /// Reads a sequence of bytes starting at a specific offset.
    pub async fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let len = buf.len();

        if len == 0 {
            return Ok(());
        }

        let start_block = offset / self.block_size as u64;
        let end_offset = offset + len as u64;

        let end_block = (end_offset - 1) / self.block_size as u64;

        let num_blocks_to_read = end_block - start_block + 1;

        let mut temp_buf = vec![0; num_blocks_to_read as usize * self.block_size];

        self.dev.read(start_block, &mut temp_buf).await?;

        let start_in_temp_buf = (offset % self.block_size as u64) as usize;
        let end_in_temp_buf = start_in_temp_buf + len;

        buf.copy_from_slice(&temp_buf[start_in_temp_buf..end_in_temp_buf]);

        Ok(())
    }

    /// Reads a `Pod` struct directly from the device at a given offset.
    pub async fn read_obj<T: Pod>(&self, offset: u64) -> Result<T> {
        let mut dest = mem::MaybeUninit::<T>::uninit();

        // SAFETY: We create a mutable byte slice that points to our
        // uninitialized stack space.
        let buf: &mut [u8] =
            unsafe { slice::from_raw_parts_mut(dest.as_mut_ptr() as *mut u8, mem::size_of::<T>()) };

        // Read directly from the device into our stack-allocated space.
        self.read_at(offset, buf).await?;

        // SAFETY: T is Pod, so any bytes are valid.
        Ok(unsafe { dest.assume_init() })
    }

    /// Writes a sequence of bytes starting at a specific offset.
    pub async fn write_at(&mut self, offset: u64, buf: &[u8]) -> Result<()> {
        let len = buf.len();
        if len == 0 {
            return Ok(());
        }

        let start_block = offset / self.block_size as u64;
        let end_offset = offset + len as u64;
        let end_block = (end_offset - 1) / self.block_size as u64;

        let num_blocks_to_rw = end_block - start_block + 1;
        let mut temp_buf = vec![0; num_blocks_to_rw as usize * self.block_size];

        // Read all affected blocks from the device into our temporary buffer.
        self.dev.read(start_block, &mut temp_buf).await?;

        // Copy the user's data into the correct position.
        let start_in_temp_buf = (offset % self.block_size as u64) as usize;
        let end_in_temp_buf = start_in_temp_buf + len;

        temp_buf[start_in_temp_buf..end_in_temp_buf].copy_from_slice(buf);

        // Write the entire modified buffer back to the device.
        self.dev.write(start_block, &temp_buf).await?;

        Ok(())
    }

    /// Forwards a sync call to the underlying device.
    pub async fn sync(&self) -> Result<()> {
        self.dev.sync().await
    }
}
