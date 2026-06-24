// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Block device API traits.

use crate::*;

/// Block device interface.
pub trait ViBlockDevice: Send + Sync {
    /// Read a sector.
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()>;

    /// Write a sector.
    fn write_sector(&self, sector: u64, buf: &[u8]) -> ViResult<()>;

    /// Get total number of sectors.
    fn sector_count(&self) -> u64;

    /// Get sector size in bytes.
    fn sector_size(&self) -> usize;

    /// Flush pending writes.
    fn flush(&self) -> ViResult<()>;
}
