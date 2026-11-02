//! Sector read/write/flush over the [`Router`]: DrvRequest wire encoding and
//! the per-request outcome rules (08A).

use alloc::format;
use types::block_wire::{self, BlockOutcome, SECTOR_SIZE};

use super::router::{Exchange, Router, Target};
use super::{BlockSys, DeviceRef};

impl Router {
    /// Send a write/flush request and decode its one-byte status reply.
    fn exchange_status(
        &mut self,
        sys: &mut impl BlockSys,
        i: usize,
        tid: usize,
        req: &[u8],
    ) -> BlockOutcome {
        let mut reply = [0u8; 1];
        match self.exchange(sys, i, tid, req, &mut reply) {
            Exchange::Refused => BlockOutcome::NotIssued,
            Exchange::Replied => BlockOutcome::from_byte(reply[0]),
            // The provider may have issued the command before it died; a write
            // read as success would make the write-through page cache believe a
            // sector reached the device.
            Exchange::Missing => {
                let name = &self.devices()[i].name;
                sys.log(&format!(
                    "[blk-router] {name}: driver died before replying; outcome indeterminate"
                ));
                BlockOutcome::Indeterminate
            }
        }
    }

    /// Read one sector. A death notification is never accepted as data.
    pub fn read(
        &mut self,
        sys: &mut impl BlockSys,
        dev: DeviceRef,
        lba: u64,
        buf: &mut [u8; SECTOR_SIZE],
    ) -> bool {
        match self.target(sys, dev) {
            Target::Kernel => sys.kernel_read(lba, buf),
            Target::Refused => false,
            Target::Provider(i, tid) => {
                // Request: [op u16][lba u64]; reply: [status][512 data].
                let mut req = [0u8; 10];
                req[0..2].copy_from_slice(&block_wire::OP_READ.to_le_bytes());
                req[2..10].copy_from_slice(&lba.to_le_bytes());
                let mut reply = [0u8; 1 + SECTOR_SIZE];
                if !matches!(
                    self.exchange(sys, i, tid, &req, &mut reply),
                    Exchange::Replied
                ) || !BlockOutcome::from_byte(reply[0]).is_completed()
                {
                    return false;
                }
                buf.copy_from_slice(&reply[1..]);
                true
            }
        }
    }

    /// Write one sector. `Indeterminate` means the write may have been issued
    /// and must not be replayed; the kernel path reports only success or failure,
    /// so its failure is `Indeterminate`.
    pub fn write(
        &mut self,
        sys: &mut impl BlockSys,
        dev: DeviceRef,
        lba: u64,
        data: &[u8; SECTOR_SIZE],
    ) -> BlockOutcome {
        let outcome = match self.target(sys, dev) {
            Target::Kernel if sys.kernel_write(lba, data) => BlockOutcome::Completed,
            Target::Kernel => BlockOutcome::Indeterminate,
            Target::Refused => BlockOutcome::NotIssued,
            Target::Provider(i, tid) => {
                // Request: [op u16][lba u64][512 data].
                let mut req = [0u8; 10 + SECTOR_SIZE];
                req[0..2].copy_from_slice(&block_wire::OP_WRITE.to_le_bytes());
                req[2..10].copy_from_slice(&lba.to_le_bytes());
                req[10..].copy_from_slice(data);
                self.exchange_status(sys, i, tid, &req)
            }
        };
        if outcome == BlockOutcome::Indeterminate {
            sys.log(&format!(
                "[blk-router] {}: write lba={} indeterminate; not replayed",
                self.name(dev),
                lba
            ));
        }
        outcome
    }

    /// Flush the device's volatile write cache. Outcome rules as [`Self::write`].
    pub fn flush(&mut self, sys: &mut impl BlockSys, dev: DeviceRef) -> BlockOutcome {
        let outcome = match self.target(sys, dev) {
            Target::Kernel if sys.kernel_flush() => BlockOutcome::Completed,
            Target::Kernel => BlockOutcome::Indeterminate,
            Target::Refused => BlockOutcome::NotIssued,
            Target::Provider(i, tid) => {
                self.exchange_status(sys, i, tid, &block_wire::OP_FLUSH.to_le_bytes())
            }
        };
        if !outcome.is_completed() {
            sys.log(&format!(
                "[blk-router] {}: flush {:?}",
                self.name(dev),
                outcome
            ));
        }
        outcome
    }
}
