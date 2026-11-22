//! Sector read/write/flush over the [`Router`]: DrvRequest wire encoding and
//! the per-request outcome rules (08A).

use alloc::format;
use types::block_wire::{
    self, BlockOutcome, ReadRunRequest, MAX_REPLY_SIZE, MAX_RUN_SECTORS, SECTOR_SIZE,
};

use super::router::{Exchange, Router, Target};
use super::{BlockSys, DeviceRef};

/// Bytes of the largest `OP_READ_RUN`.
const RUN_BYTES: usize = MAX_RUN_SECTORS as usize * SECTOR_SIZE;

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
            Exchange::Refused | Exchange::Stalled { dispatched: false } => BlockOutcome::NotIssued,
            // An empty reply is no status: the request may have run.
            Exchange::Replied(0) => BlockOutcome::Indeterminate,
            Exchange::Replied(_) => BlockOutcome::from_byte(reply[0]),
            // Taken but never answered within the bound: it may yet run.
            Exchange::Stalled { dispatched: true } => BlockOutcome::Indeterminate,
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
                    Exchange::Replied(len) if len == reply.len()
                ) || !BlockOutcome::from_byte(reply[0]).is_completed()
                {
                    return false;
                }
                buf.copy_from_slice(&reply[1..]);
                true
            }
        }
    }

    /// Read `buf.len() / 512` whole sectors starting at `lba`: one
    /// `OP_READ_RUN` per [`MAX_RUN_SECTORS`] for a routed device.
    pub fn read_sectors(
        &mut self,
        sys: &mut impl BlockSys,
        dev: DeviceRef,
        lba: u64,
        buf: &mut [u8],
    ) -> bool {
        if !buf.len().is_multiple_of(SECTOR_SIZE) {
            return false;
        }
        buf.chunks_mut(RUN_BYTES).enumerate().all(|(k, chunk)| {
            let first = lba + (k * usize::from(MAX_RUN_SECTORS)) as u64;
            match self.target(sys, dev) {
                Target::Refused => false,
                Target::Provider(i, tid) => self.read_run(sys, i, tid, first, chunk),
                Target::Kernel => chunk
                    .as_chunks_mut::<SECTOR_SIZE>()
                    .0
                    .iter_mut()
                    .enumerate()
                    .all(|(n, sector)| sys.kernel_read(first + n as u64, sector)),
            }
        })
    }

    /// One `OP_READ_RUN`; the sectors are copied out only when it completed.
    fn read_run(
        &mut self,
        sys: &mut impl BlockSys,
        i: usize,
        tid: usize,
        lba: u64,
        buf: &mut [u8],
    ) -> bool {
        let request = ReadRunRequest {
            sector: lba,
            count: (buf.len() / SECTOR_SIZE) as u16,
        };
        let mut reply = [0u8; MAX_REPLY_SIZE];
        let reply = &mut reply[..request.reply_len()];
        let expected = reply.len();
        if !matches!(
            self.exchange(sys, i, tid, &request.encode(), reply),
            Exchange::Replied(len) if len == expected
        ) || !BlockOutcome::from_byte(reply[0]).is_completed()
        {
            return false;
        }
        buf.copy_from_slice(&reply[1..]);
        true
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
