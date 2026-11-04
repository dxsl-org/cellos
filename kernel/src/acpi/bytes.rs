// SPDX-License-Identifier: MPL-2.0
//! Bounds-checked little-endian reads inside one validated ACPI table.

use super::records::Gas;

/// A validated table body `[virt, virt + len)`. Every read is bounds-checked
/// against `len` and returns `None` past the end, so a parser can never read
/// outside the table the checksum covered.
#[derive(Clone, Copy)]
pub(super) struct TableBytes {
    virt: usize,
    len: usize,
}

impl TableBytes {
    /// # Safety
    /// `[virt, virt + len)` must stay readable for the lifetime of this value.
    pub(super) unsafe fn new(virt: usize, len: usize) -> Self {
        Self { virt, len }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    fn read<T: Copy>(&self, offset: usize) -> Option<T> {
        let end = offset.checked_add(core::mem::size_of::<T>())?;
        if end > self.len {
            return None;
        }
        // SAFETY: `[offset, end)` lies inside the readable range promised to `new`.
        Some(unsafe { core::ptr::read_unaligned((self.virt + offset) as *const T) })
    }

    pub(super) fn u8(&self, offset: usize) -> Option<u8> {
        self.read(offset)
    }

    pub(super) fn u16(&self, offset: usize) -> Option<u16> {
        self.read::<u16>(offset).map(u16::from_le)
    }

    pub(super) fn u32(&self, offset: usize) -> Option<u32> {
        self.read::<u32>(offset).map(u32::from_le)
    }

    pub(super) fn u64(&self, offset: usize) -> Option<u64> {
        self.read::<u64>(offset).map(u64::from_le)
    }

    /// A 12-byte Generic Address Structure at `offset`.
    pub(super) fn gas(&self, offset: usize) -> Option<Gas> {
        Some(Gas {
            space_id: self.u8(offset)?,
            bit_width: self.u8(offset + 1)?,
            bit_offset: self.u8(offset + 2)?,
            access_size: self.u8(offset + 3)?,
            address: self.u64(offset + 4)?,
        })
    }
}
