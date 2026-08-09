use api::hotswap::ViStateTransfer;
use ostd::prelude::*;

use crate::manager::VfsManager;

// VFS serialises its quota table so per-cell byte-usage accounting survives a
// live upgrade. The handle table is NOT serialised: open handles are session-
// scoped and client cells reopen files after the swap completes.
//
// Wire format (little-endian, schema v1):
//   [version: u32][cell_count: u32]
//     [cell_id: u64][bytes_used: u64]...
const VFS_SCHEMA_VERSION: u32 = 1;

impl ViStateTransfer for VfsManager {
    fn state_size(&self) -> usize {
        4 + 4 + self.quota.entry_count() * 16
    }

    fn serialize_state(&self, buf: &mut [u8]) -> ViResult<usize> {
        let needed = self.state_size();
        if buf.len() < needed {
            return Err(ViError::InvalidArgument);
        }
        let mut pos = 0;
        buf[pos..pos + 4].copy_from_slice(&VFS_SCHEMA_VERSION.to_le_bytes());
        pos += 4;
        let entries = self.quota.all_entries();
        buf[pos..pos + 4].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        pos += 4;
        for (id, used) in &entries {
            buf[pos..pos + 8].copy_from_slice(&id.to_le_bytes());
            pos += 8;
            buf[pos..pos + 8].copy_from_slice(&used.to_le_bytes());
            pos += 8;
        }
        Ok(pos)
    }

    fn deserialize_state(&mut self, buf: &[u8]) -> ViResult<()> {
        if buf.len() < 8 {
            return Err(ViError::InvalidInput);
        }
        let count = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
        let mut pos = 8;
        for _ in 0..count {
            if pos + 16 > buf.len() {
                return Err(ViError::InvalidInput);
            }
            let id = u64::from_le_bytes(
                buf[pos..pos + 8]
                    .try_into()
                    .map_err(|_| ViError::InvalidInput)?,
            );
            let used = u64::from_le_bytes(
                buf[pos + 8..pos + 16]
                    .try_into()
                    .map_err(|_| ViError::InvalidInput)?,
            );
            self.quota.restore(types::CellId(id), used);
            pos += 16;
        }
        Ok(())
    }
}
