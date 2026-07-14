//! NVMe controller logic ported from kernel/src/task/drivers/blk_nvme.rs.
//!
//! In the Driver Cell, MMIO access goes through `ostd::mmio::MmioRegion` instead
//! of identity-mapped raw pointers.  DMA allocation uses `ostd::dma::DmaBuf`.
//!
//! Law 4 exception: Driver Cells may use `unsafe` for MMIO register access.
//! Every `unsafe` block is annotated with `// SAFETY:`.

use core::sync::atomic::{fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

use crate::queue::{Queue, SqEntry};

// ── BAR0 register offsets (NVMe 1.x §3.1) ─────────────────────────────────────

const REG_CAP: usize = 0x00;
const REG_CC: usize = 0x14;
const REG_CSTS: usize = 0x1C;
const REG_AQA: usize = 0x24;
const REG_ASQ: usize = 0x28;
const REG_ACQ: usize = 0x30;
const REG_DB_BASE: usize = 0x1000;

const CC_EN: u32 = 1 << 0;
const CC_CSS_NVM: u32 = 0;
const CC_MPS_4K: u32 = 0;
const CC_AMS_RR: u32 = 0;
const CC_IOSQES: u32 = 6 << 16;
const CC_IOCQES: u32 = 4 << 20;

const CSTS_RDY: u32 = 1 << 0;
const CSTS_CFS: u32 = 1 << 1;

const ADMIN_QUEUE_DEPTH: u16 = 64;
const IO_QUEUE_DEPTH: u16 = 64;

const ADMIN_OPC_CREATE_SQ: u8 = 0x01;
const ADMIN_OPC_CREATE_CQ: u8 = 0x05;
const ADMIN_OPC_IDENTIFY: u8 = 0x06;
const CNS_IDENTIFY_NS: u32 = 0;
const CNS_IDENTIFY_CTRL: u32 = 1;
const NVM_OPC_WRITE: u8 = 0x01;
const NVM_OPC_READ: u8 = 0x02;

const POLL_WARN_ITERS: u64 = 1_000_000;

pub struct NvmeController {
    mmio: MmioRegion,
    admin: Queue,
    io: Queue,
    db_stride: usize,
    pub n_sectors: u64,
    pub lba_bytes: u32,
    pub vwc: bool,
    // reason: only read at construction (to authorize admin/IO queue DMA);
    // kept on the struct for a planned re-authorization path if read/write_sector
    // start allocating DmaBufs directly on the controller instead of via Queue.
    #[allow(dead_code)]
    bdf: u32,
}

impl NvmeController {
    /// Initialise the NVMe controller reachable via `mmio` (BAR0).
    ///
    /// Returns `Err(IO)` if the controller fails to reach RDY or Identify fails.
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // 1. Read capabilities — need CAP.DSTRD for doorbell stride.
        // SAFETY: BAR0 is MMIO; read_volatile is required for hardware registers.
        let cap = unsafe {
            let lo = core::ptr::read_volatile((mmio.base() + REG_CAP) as *const u32) as u64;
            let hi = core::ptr::read_volatile((mmio.base() + REG_CAP + 4) as *const u32) as u64;
            lo | (hi << 32)
        };
        let dstrd = ((cap >> 32) & 0xF) as usize;
        let db_stride = 4 << dstrd;

        // 2. Reset controller: CC.EN=0, wait CSTS.RDY=0.
        Self::write32(&mmio, REG_CC, 0)?;
        let mut spin = 0u64;
        loop {
            let csts = Self::read32(&mmio, REG_CSTS)?;
            if csts & CSTS_RDY == 0 {
                break;
            }
            spin += 1;
            if spin > POLL_WARN_ITERS {
                return Err(ViError::IO);
            }
            fence(Ordering::SeqCst);
        }

        // 3. Allocate admin queues.
        let admin = Queue::new(bdf, ADMIN_QUEUE_DEPTH).ok_or(ViError::OutOfMemory)?;

        // 4. Program AQA, ASQ, ACQ.
        let aqa = ((ADMIN_QUEUE_DEPTH as u32 - 1) << 16) | (ADMIN_QUEUE_DEPTH as u32 - 1);
        Self::write32(&mmio, REG_AQA, aqa)?;
        Self::write64(&mmio, REG_ASQ, admin.sq_phys())?;
        Self::write64(&mmio, REG_ACQ, admin.cq_phys())?;

        // 5. Enable controller.
        let cc = CC_EN | CC_CSS_NVM | CC_MPS_4K | CC_AMS_RR | CC_IOSQES | CC_IOCQES;
        Self::write32(&mmio, REG_CC, cc)?;

        // 6. Wait CSTS.RDY=1.
        let mut spin = 0u64;
        loop {
            let csts = Self::read32(&mmio, REG_CSTS)?;
            if csts & CSTS_CFS != 0 {
                return Err(ViError::IO);
            }
            if csts & CSTS_RDY != 0 {
                break;
            }
            spin += 1;
            if spin > POLL_WARN_ITERS {
                return Err(ViError::IO);
            }
            fence(Ordering::SeqCst);
        }

        let mut ctrl = NvmeController {
            mmio,
            admin,
            io: Queue::new(bdf, IO_QUEUE_DEPTH).ok_or(ViError::OutOfMemory)?,
            db_stride,
            n_sectors: 0,
            lba_bytes: 512,
            vwc: false,
            bdf,
        };

        // 7. Identify Controller → VWC flag.
        {
            let id_buf = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
            let _ = id_buf.authorize(bdf);
            let id_phys = id_buf.phys() as u64;
            ctrl.admin_cmd(
                ADMIN_OPC_IDENTIFY,
                0,
                id_phys,
                0,
                CNS_IDENTIFY_CTRL,
                0,
                0,
                0,
                0,
                0,
            )?;
            // SAFETY: id_buf is DMA memory we own; VWC is byte 525 in the Identify Controller data.
            let vwc_byte = unsafe { *(id_buf.virt().add(525)) };
            ctrl.vwc = vwc_byte & 1 != 0;
            id_buf.free();
        }

        // 8. Identify Namespace 1 → LBA count + format.
        {
            let id_buf = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
            let _ = id_buf.authorize(bdf);
            let id_phys = id_buf.phys() as u64;
            ctrl.admin_cmd(
                ADMIN_OPC_IDENTIFY,
                1,
                id_phys,
                0,
                CNS_IDENTIFY_NS,
                0,
                0,
                0,
                0,
                0,
            )?;
            // SAFETY: id_buf is a valid 4-KiB Identify Namespace response.
            unsafe {
                let ptr = id_buf.virt();
                ctrl.n_sectors = core::ptr::read_volatile(ptr as *const u64);
                let flbas = *ptr.add(26); // FLBAS: current LBA format index
                let lba_fmt_idx = (flbas & 0x0F) as usize;
                let lba_ds = *ptr.add(128 + lba_fmt_idx * 4 + 1); // LBA data shift
                ctrl.lba_bytes = 1u32 << lba_ds;
            }
            id_buf.free();
        }

        // 9. Create I/O CQ (admin opcode 0x05).
        // CDW10 layout (NVMe 1.x §5.3/§5.4): QSIZE[31:16] (0-based) | QID[15:0].
        // The first port of this file inverted the two fields — QEMU then created
        // CQ with QID=63 and Create-SQ failed with Invalid CQID (CQ 1 absent).
        let io_cq_phys = ctrl.io.cq_phys();
        ctrl.admin_cmd(
            ADMIN_OPC_CREATE_CQ,
            0,
            io_cq_phys,
            0,
            ((IO_QUEUE_DEPTH as u32 - 1) << 16) | 1, // CDW10: QSIZE | QID=1
            0x1, // CDW11: IEN=0 (polled), PC=1 (physically contiguous)
            0,
            0,
            0,
            0,
        )?;

        // 10. Create I/O SQ (admin opcode 0x01).
        let io_sq_phys = ctrl.io.sq_phys();
        ctrl.admin_cmd(
            ADMIN_OPC_CREATE_SQ,
            0,
            io_sq_phys,
            0,
            ((IO_QUEUE_DEPTH as u32 - 1) << 16) | 1, // CDW10: QSIZE | QID=1
            (1 << 16) | 0x1,                         // CDW11: CQID=1 | PC=1
            0,
            0,
            0,
            0,
        )?;

        Ok(ctrl)
    }

    // ── Register access ───────────────────────────────────────────────────────

    fn read32(mmio: &MmioRegion, off: usize) -> ViResult<u32> {
        // SAFETY: mmio region was granted by kernel; volatile prevents caching.
        Ok(unsafe { core::ptr::read_volatile((mmio.base() + off) as *const u32) })
    }

    fn write32(mmio: &MmioRegion, off: usize, val: u32) -> ViResult<()> {
        // SAFETY: same contract as read32.
        unsafe { core::ptr::write_volatile((mmio.base() + off) as *mut u32, val) };
        Ok(())
    }

    fn write64(mmio: &MmioRegion, off: usize, val: u64) -> ViResult<()> {
        // SAFETY: same contract as read32.
        unsafe { core::ptr::write_volatile((mmio.base() + off) as *mut u64, val) };
        Ok(())
    }

    fn ring_sq_tail(&self, qid: usize, tail: u16) {
        let db_off = REG_DB_BASE + qid * 2 * self.db_stride;
        // SAFETY: doorbell is within the granted BAR0 MMIO region.
        unsafe { core::ptr::write_volatile((self.mmio.base() + db_off) as *mut u32, tail as u32) };
    }

    fn ring_cq_head(&self, qid: usize, head: u16) {
        let db_off = REG_DB_BASE + (qid * 2 + 1) * self.db_stride;
        // SAFETY: doorbell is within the granted BAR0 MMIO region.
        unsafe { core::ptr::write_volatile((self.mmio.base() + db_off) as *mut u32, head as u32) };
    }

    // ── Admin command submit + poll ───────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn admin_cmd(
        &mut self,
        opc: u8,
        nsid: u32,
        prp1: u64,
        prp2: u64,
        cdw10: u32,
        cdw11: u32,
        cdw12: u32,
        cdw13: u32,
        cdw14: u32,
        cdw15: u32,
    ) -> ViResult<()> {
        let depth = self.admin.depth as usize;
        let tail = self.admin.sq_tail as usize;
        let cid = self.admin.next_cid();

        // SAFETY: tail < depth; sq_buf holds depth SqEntry slots.
        let sqe = unsafe { &mut *self.admin.sq_entry(tail) };
        *sqe = SqEntry::default();
        sqe.cdw0 = (opc as u32) | ((cid as u32) << 16);
        sqe.nsid = nsid;
        sqe.prp1 = prp1;
        sqe.prp2 = prp2;
        sqe.cdw10 = cdw10;
        sqe.cdw11 = cdw11;
        sqe.cdw12 = cdw12;
        sqe.cdw13 = cdw13;
        sqe.cdw14 = cdw14;
        sqe.cdw15 = cdw15;

        self.admin.sq_tail = ((tail + 1) % depth) as u16;
        fence(Ordering::Release);
        self.ring_sq_tail(0, self.admin.sq_tail);

        let expected_phase = self.admin.cq_phase;
        let cq_head = self.admin.cq_head as usize;
        let mut iters = 0u64;
        loop {
            // SAFETY: cq_head < depth; cq_buf holds depth CqEntry slots.
            let phase_status =
                unsafe { core::ptr::read_volatile(&(*self.admin.cq_entry(cq_head)).phase_status) };
            if (phase_status & 1 != 0) == expected_phase {
                let status = phase_status >> 1;
                let new_head = (cq_head + 1) % depth;
                if new_head == 0 {
                    self.admin.cq_phase = !self.admin.cq_phase;
                }
                self.admin.cq_head = new_head as u16;
                self.ring_cq_head(0, self.admin.cq_head);
                if status != 0 {
                    return Err(ViError::IO);
                }
                return Ok(());
            }
            iters += 1;
            if iters == POLL_WARN_ITERS {
                return Err(ViError::IO);
            }
            fence(Ordering::Acquire);
        }
    }

    // ── I/O command submit + poll ─────────────────────────────────────────────

    fn submit_io(&mut self, opc: u8, nsid: u32, lba: u64, nlb: u16, prp1: u64) -> ViResult<()> {
        let depth = self.io.depth as usize;
        let tail = self.io.sq_tail as usize;
        let cid = self.io.next_cid();

        // SAFETY: tail < depth; sq_buf holds depth SqEntry slots.
        let sqe = unsafe { &mut *self.io.sq_entry(tail) };
        *sqe = SqEntry::default();
        sqe.cdw0 = (opc as u32) | ((cid as u32) << 16);
        sqe.nsid = nsid;
        sqe.prp1 = prp1;
        sqe.cdw10 = (lba & 0xFFFF_FFFF) as u32;
        sqe.cdw11 = (lba >> 32) as u32;
        sqe.cdw12 = nlb as u32;

        self.io.sq_tail = ((tail + 1) % depth) as u16;
        fence(Ordering::Release);
        self.ring_sq_tail(1, self.io.sq_tail);

        let expected_phase = self.io.cq_phase;
        let cq_head = self.io.cq_head as usize;
        let mut iters = 0u64;
        loop {
            // SAFETY: cq_head < depth; cq_buf holds depth CqEntry slots.
            let phase_status =
                unsafe { core::ptr::read_volatile(&(*self.io.cq_entry(cq_head)).phase_status) };
            if (phase_status & 1 != 0) == expected_phase {
                let status = phase_status >> 1;
                let new_head = (cq_head + 1) % depth;
                if new_head == 0 {
                    self.io.cq_phase = !self.io.cq_phase;
                }
                self.io.cq_head = new_head as u16;
                self.ring_cq_head(1, self.io.cq_head);
                if status != 0 {
                    return Err(ViError::IO);
                }
                return Ok(());
            }
            iters += 1;
            if iters == POLL_WARN_ITERS {
                return Err(ViError::IO);
            }
            fence(Ordering::Acquire);
        }
    }

    /// Read one 512-byte sector into `buf` (must be DMA-capable).
    pub fn read_sector(&mut self, sector: u64, buf_phys: u64) -> ViResult<()> {
        self.submit_io(NVM_OPC_READ, 1, sector, 0, buf_phys)
    }

    /// Write one 512-byte sector from `buf` (must be DMA-capable).
    pub fn write_sector(&mut self, sector: u64, buf_phys: u64) -> ViResult<()> {
        self.submit_io(NVM_OPC_WRITE, 1, sector, 0, buf_phys)
    }
}
