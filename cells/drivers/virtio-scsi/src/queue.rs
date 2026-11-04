//! The request virtqueue: a minimal polled split virtqueue that runs one
//! SCSI command at a time and implements [`ScsiExecutor`].
//!
//! Every command reuses descriptors `0..CHAIN_MAX` and the fixed request,
//! response and bounce areas of [`crate::ring`]: header (device-readable),
//! data-out, response (device-writable), data-in — in that order, as the
//! virtio-scsi request layout requires. A timeout or a nonsensical used-ring
//! entry poisons the queue: the device may still own the descriptors, so no
//! later command is submitted.

use crate::bars::Bars;
use crate::config::Limits;
use crate::dma::DmaArea;
use crate::proto::{self, VscsiError, RESP_FIXED};
use crate::ring::{self, RingLayout, AVAIL_F_NO_INTERRUPT, CHAIN_MAX, DATA_LEN, DATA_OFF};
use crate::ring::{DESC_F_NEXT, DESC_F_WRITE, REQ_LEN, REQ_OFF, RESP_LEN, RESP_OFF};
use crate::wait::{Deadline, IO_BUDGET_MS};
use core::sync::atomic::{fence, Ordering};
use ostd::syscall::sys_get_time_ms;
use scsi_block::{Completion, DataPhase, ScsiExecutor};
use virtio_drivers::transport::Transport;
use virtio_pci_transport::{InitFailure, VirtioPciTransport};

/// Queue 0 is control, 1 event, 2.. request; only the first request queue
/// is used.
pub const REQUEST_QUEUE: u16 = 2;
/// Head descriptor of every chain.
const HEAD: u16 = 0;

/// One chain segment: offset into the DMA area, length, flags.
type Segment = (usize, usize, u16);

pub struct RequestQueue {
    // Declared before `_bars`: dropping the transport resets the device
    // while its BARs are still claimed.
    transport: VirtioPciTransport,
    _bars: Bars,
    dma: DmaArea,
    ring: RingLayout,
    limits: Limits,
    avail_idx: u16,
    last_used: u16,
    next_tag: u64,
    poisoned: bool,
}

impl RequestQueue {
    /// Zeroes the rings, programs [`REQUEST_QUEUE`] with `size` entries in
    /// `dma` and sets `DRIVER_OK`.
    ///
    /// Precondition: `transport` has negotiated features (`FEATURES_OK`);
    /// `size` comes from [`ring::queue_size`] for this queue.
    pub fn start(
        transport: VirtioPciTransport,
        bars: Bars,
        dma: DmaArea,
        size: u16,
        limits: Limits,
    ) -> Result<Self, InitFailure> {
        let ring = RingLayout::new(size);
        dma.fill_zero(0, ring.end);
        dma.write_u16(ring.avail, AVAIL_F_NO_INTERRUPT);
        let mut queue = Self {
            transport,
            _bars: bars,
            dma,
            ring,
            limits,
            avail_idx: 0,
            last_used: 0,
            next_tag: 0,
            poisoned: false,
        };
        let addr = |off: usize| queue.dma.iova(off) as usize;
        let (desc, avail, used) = (addr(0), addr(ring.avail), addr(ring.used));
        let transport = &mut queue.transport;
        transport.queue_set(REQUEST_QUEUE, u32::from(size), desc, avail, used);
        transport.finish_init();
        match transport.init_failure() {
            Some(failure) => Err(failure),
            None => Ok(queue),
        }
    }

    fn submit(&mut self, chain: &[Segment]) {
        let dma = &self.dma;
        for (i, &(off, len, flags)) in chain.iter().enumerate() {
            let next = i + 1 < chain.len();
            let desc = self.ring.desc(i as u16);
            dma.write_u64(desc, dma.iova(off));
            dma.write_u32(desc + 8, len as u32);
            dma.write_u16(desc + 12, flags | if next { DESC_F_NEXT } else { 0 });
            dma.write_u16(desc + 14, if next { i as u16 + 1 } else { 0 });
        }
        dma.write_u16(self.ring.avail_entry(self.avail_idx), HEAD);
        self.avail_idx = self.avail_idx.wrapping_add(1);
        // Descriptors, buffers and the ring entry before the index.
        fence(Ordering::Release);
        self.dma.write_u16(self.ring.avail_idx(), self.avail_idx);
        // The index before the notify write.
        fence(Ordering::SeqCst);
        self.transport.notify(REQUEST_QUEUE);
    }

    /// Polls the used ring for the one in-flight chain within the budget.
    fn wait(&mut self) -> Result<(), VscsiError> {
        let mut deadline = Deadline::start(IO_BUDGET_MS, sys_get_time_ms());
        let used_idx = loop {
            let idx = self.dma.read_u16(self.ring.used_idx());
            if idx != self.last_used {
                break idx;
            }
            if !deadline.tick() {
                core::hint::spin_loop();
                continue;
            }
            ostd::task::yield_now();
            if deadline.expired(sys_get_time_ms()) {
                self.poisoned = true;
                return Err(VscsiError::Timeout);
            }
        };
        // The used element and every device write after the index.
        fence(Ordering::Acquire);
        let id = self.dma.read_u32(self.ring.used_entry(self.last_used));
        if ring::completions(used_idx, self.last_used) != 1 || id != u32::from(HEAD) {
            self.poisoned = true;
            return Err(VscsiError::BadCompletion);
        }
        self.last_used = used_idx;
        Ok(())
    }
}

impl ScsiExecutor for RequestQueue {
    type Error = VscsiError;

    fn execute(
        &mut self,
        target: u8,
        lun: [u8; 8],
        cdb: &[u8],
        data: DataPhase<'_>,
    ) -> Result<Completion, VscsiError> {
        if self.poisoned {
            return Err(VscsiError::Poisoned);
        }
        let lun = proto::virtio_lun(target, lun)?;
        let data_len = data.len();
        if data_len > DATA_LEN {
            return Err(VscsiError::DataTooLarge(data_len));
        }
        let mut req = [0u8; REQ_LEN];
        let req_len = proto::encode_req(&mut req, lun, self.next_tag, cdb, self.limits.cdb_size)?;
        self.next_tag = self.next_tag.wrapping_add(1);
        let resp_len = RESP_FIXED + self.limits.sense_size;
        self.dma.write_bytes(REQ_OFF, &req[..req_len]);
        self.dma.fill_zero(RESP_OFF, resp_len);

        let mut chain: [Segment; CHAIN_MAX] = [(0, 0, 0); CHAIN_MAX];
        let mut len = 0;
        let mut push = |segment: Segment| {
            chain[len] = segment;
            len += 1;
        };
        push((REQ_OFF, req_len, 0));
        if let DataPhase::Out(buf) = &data {
            if !buf.is_empty() {
                self.dma.write_bytes(DATA_OFF, buf);
                push((DATA_OFF, data_len, 0));
            }
        }
        push((RESP_OFF, resp_len, DESC_F_WRITE));
        if matches!(data, DataPhase::In(_)) && data_len > 0 {
            push((DATA_OFF, data_len, DESC_F_WRITE));
        }
        self.submit(&chain[..len]);
        self.wait()?;

        if let DataPhase::In(buf) = data {
            self.dma.read_bytes(DATA_OFF, buf);
        }
        let mut resp = [0u8; RESP_LEN];
        self.dma.read_bytes(RESP_OFF, &mut resp[..resp_len]);
        proto::decode_resp(&resp[..resp_len])
    }
}
