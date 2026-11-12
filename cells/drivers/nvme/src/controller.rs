//! NVMe controller logic ported from kernel/src/task/drivers/blk_nvme.rs.
//!
//! In the Driver Cell, every register and doorbell access goes through the
//! bounds-checked `ostd::mmio::MmioRegion`; device-reported geometry (doorbell
//! stride, LBA format) is validated in `layout` before it is used. DMA allocations
//! retain the device-visible IOVA returned by authorization.
//!
//! Law 4 exception: Driver Cells may use `unsafe` for DMA memory access.
//! Every `unsafe` block is annotated with `// SAFETY:`.

use crate::deadline::{self, Deadline, Spent};
use crate::dma::AuthorizedDma;
use crate::irq::CompletionIrq;
use crate::layout::{self, Doorbells};
use core::sync::atomic::{fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::io::print_fmt;
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
const NVM_OPC_FLUSH: u8 = 0x00;

/// How many slow completions (≥ `SLOW_COMPLETION_MS`) to log.
static SLOW_LOG: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(8);
static CLOCK_FALLBACK_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Monotonic milliseconds for the wait budgets (`None`: no clock on this profile).
fn now_ms() -> Option<u64> {
    let now = ostd::syscall::sys_get_time_ms();
    if now.is_none() && !CLOCK_FALLBACK_LOGGED.swap(true, Ordering::Relaxed) {
        let _ = print_fmt(format_args!(
            "[nvme] monotonic clock unavailable; using the 50000000-poll fallback\n"
        ));
    }
    now
}

/// One more poll of a wait: at each yield point, yield and report a spent budget.
///
/// A timed-out command is not aborted - the device still posts its completion later - so
/// giving up early wedged the queue for every command after it; a late completion is
/// recognised by its CID. The budgets are time (S10-3c): a Flush is a host fsync, and a
/// fixed poll count lasted only seconds in an idle guest.
fn poll_spent(deadline: &mut Deadline) -> Option<Spent> {
    if !deadline.tick() {
        return None;
    }
    ostd::task::yield_now();
    deadline.check(now_ms())
}

/// Empty reads of a command wait before the first park. A completion that lands while
/// the BSP is busy wakes the parked driver only at the next scheduler tick, so a short
/// spin keeps fast commands fast.
const SPIN_BEFORE_PARK: u64 = 64;

/// One more empty read of a command wait. Interrupt-driven: after the spin, park until
/// an MSI-X or the park slice ends, then check the budget. Polled: [`poll_spent`].
fn idle(deadline: &mut Deadline, irq: &mut CompletionIrq) -> Option<Spent> {
    if deadline.polls() >= SPIN_BEFORE_PARK && irq.park() {
        return deadline.check(now_ms());
    }
    poll_spent(deadline)
}

/// Log a completion that took at least `SLOW_COMPLETION_MS` (the first few only).
fn note_slow(deadline: &Deadline, what: &str, opc: u8) {
    if let Some(ms) = deadline.slow(now_ms()) {
        if SLOW_LOG
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(1)
            })
            .is_ok()
        {
            let _ = print_fmt(format_args!(
                "[nvme] slow completion {} opc={} ms={}\n",
                what, opc, ms
            ));
        }
    }
}

/// How many expected io rejections (reads past the namespace end) to log.
static IO_ERR_LOG: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(3);

/// Why an issued I/O command did not complete successfully. Either way the device
/// may have executed it, so the caller reports `Indeterminate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoFailure {
    /// The device answered with an error status: the command is finished.
    Status,
    /// The wait budget ran out with the command still outstanding: the device may
    /// still DMA to or from its buffer.
    TimedOut,
}

pub struct NvmeController {
    mmio: MmioRegion,
    admin: Queue,
    io: Queue,
    doorbells: Doorbells,
    pub n_sectors: u64,
    pub vwc: bool,
    // reason: only read at construction (to authorize admin/IO queue DMA);
    // kept on the struct for a planned re-authorization path if read/write_sector
    // start allocating DmaBufs directly on the controller instead of via Queue.
    #[allow(dead_code)]
    bdf: u32,
    /// MSI-X (vector 0: admin CQ, vector 1: I/O CQ) or polling.
    irq: CompletionIrq,
    /// Fault-injection builds only: exit (70) right after the next I/O doorbell.
    #[cfg(feature = "fault-inject")]
    exit_after_doorbell: bool,
}

impl NvmeController {
    /// Initialise the NVMe controller reachable via `mmio` (BAR0).
    ///
    /// Returns `Err(IO)` if the controller fails to reach RDY or Identify fails, and
    /// `Err(NotSupported)` if its doorbells overrun the claimed BAR0 window or
    /// namespace 1 is not formatted with 512-byte blocks without metadata.
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // 1. Read capabilities — need CAP.DSTRD for doorbell stride.
        let cap =
            Self::read32(&mmio, REG_CAP)? as u64 | (Self::read32(&mmio, REG_CAP + 4)? as u64) << 32;
        let dstrd = ((cap >> 32) & 0xF) as u32;
        let doorbells = match Doorbells::new(dstrd, mmio.len()) {
            Ok(doorbells) => doorbells,
            Err(end) => {
                let _ = print_fmt(format_args!(
                    "[nvme] CAP.DSTRD={} puts doorbells up to 0x{:x}, past the 0x{:x}-byte BAR0 window: controller refused\n",
                    dstrd,
                    end,
                    mmio.len()
                ));
                return Err(ViError::NotSupported);
            }
        };
        let rdy_budget_ms = deadline::rdy_budget_ms(cap);
        let _ = print_fmt(format_args!(
            "[nvme] CAP=0x{:016x} ready budget {} ms (CAP.TO)\n",
            cap, rdy_budget_ms
        ));

        // 2. Allocate admin queues.
        //
        // Deliberately *before* the reset: authorizing them is what installs this
        // cell's IOMMU context for the device, and until that context exists the
        // device's stale accesses (firmware leaves the controller enabled with
        // its own admin queues) pass through untranslated. Once the context
        // exists, one of those accesses faults, the controller latches fatal
        // status, and every later command - including this driver's Identify -
        // times out. Resetting after the context is installed clears that state
        // instead of racing it.
        let admin = Queue::new(bdf, ADMIN_QUEUE_DEPTH)?;

        // 3. Reset controller: CC.EN=0, wait CSTS.RDY=0.
        Self::write32(&mmio, REG_CC, 0)?;
        let mut wait = Deadline::start(rdy_budget_ms, now_ms());
        loop {
            let csts = Self::read32(&mmio, REG_CSTS)?;
            if csts & CSTS_RDY == 0 {
                break;
            }
            if let Some(spent) = poll_spent(&mut wait) {
                let _ = print_fmt(format_args!(
                    "[nvme] reset wait failed: CSTS=0x{:x} never dropped RDY in {}\n",
                    csts, spent
                ));
                return Err(ViError::IO);
            }
            fence(Ordering::SeqCst);
        }
        // Reset proof for a replacement incarnation (Phase 08C): the controller
        // left whatever state its previous owner died in.
        let _ = print_fmt(format_args!(
            "[nvme] controller reset complete (CC.EN=0, CSTS.RDY=0)\n"
        ));

        // 4. Program AQA, ASQ, ACQ.
        let aqa = ((ADMIN_QUEUE_DEPTH as u32 - 1) << 16) | (ADMIN_QUEUE_DEPTH as u32 - 1);
        Self::write32(&mmio, REG_AQA, aqa)?;
        Self::write64(&mmio, REG_ASQ, admin.sq_iova())?;
        Self::write64(&mmio, REG_ACQ, admin.cq_iova())?;
        // What the controller is actually told to fetch, next to what the CPU
        // writes and what the kernel mapped: a device that fetches from an
        // address the driver never authorized shows up as a mismatch here.
        let _ = print_fmt(format_args!(
            "[nvme] regs: aqa=0x{:x} asq=0x{:x} acq=0x{:x} (sq_virt=0x{:x} cq_virt=0x{:x})\n",
            aqa,
            admin.sq_iova(),
            admin.cq_iova(),
            admin.sq_virt(),
            admin.cq_virt()
        ));

        // MSI-X must be live before CC.EN: enabling binds the admin CQ to vector 0,
        // and a controller that saw MSI-X off then never signals that vector (QEMU's
        // model drops it silently), leaving every admin wait to its park slices.
        let irq = CompletionIrq::setup(bdf, 2);

        // 5. Enable controller.
        let cc = CC_EN | CC_CSS_NVM | CC_MPS_4K | CC_AMS_RR | CC_IOSQES | CC_IOCQES;
        Self::write32(&mmio, REG_CC, cc)?;

        // 6. Wait CSTS.RDY=1.
        let mut wait = Deadline::start(rdy_budget_ms, now_ms());
        loop {
            let csts = Self::read32(&mmio, REG_CSTS)?;
            if csts & CSTS_CFS != 0 {
                let _ = print_fmt(format_args!(
                    "[nvme] controller fatal status after enable: CSTS=0x{:x}\n",
                    csts
                ));
                return Err(ViError::IO);
            }
            if csts & CSTS_RDY != 0 {
                break;
            }
            if let Some(spent) = poll_spent(&mut wait) {
                let _ = print_fmt(format_args!(
                    "[nvme] RDY wait failed: CSTS=0x{:x} never reached RDY in {}\n",
                    csts, spent
                ));
                return Err(ViError::IO);
            }
            fence(Ordering::SeqCst);
        }

        let mut ctrl = NvmeController {
            mmio,
            admin,
            io: Queue::new(bdf, IO_QUEUE_DEPTH)?,
            doorbells,
            n_sectors: 0,
            vwc: false,
            bdf,
            irq,
            #[cfg(feature = "fault-inject")]
            exit_after_doorbell: false,
        };

        // The device-visible addresses this instance programmed, next to the
        // kernel's '[vtd] Cell … phys=0x…' lines: a mismatch there is what turns
        // a correct request into a device that reads or writes somewhere else.
        let _ = print_fmt(format_args!(
            "[nvme] dma iovas: admin.sq=0x{:x} admin.cq=0x{:x} io.sq=0x{:x} io.cq=0x{:x}\n",
            ctrl.admin.sq_iova(),
            ctrl.admin.cq_iova(),
            ctrl.io.sq_iova(),
            ctrl.io.cq_iova()
        ));

        // 7. Identify Controller → VWC flag.
        {
            let id_buf = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
            let id_buf = AuthorizedDma::authorize(id_buf, |buf| buf.authorize(bdf))
                .map_err(|_| ViError::PermissionDenied)?;
            let _ = print_fmt(format_args!(
                "[nvme] dma iovas: identify=0x{:x}\n",
                id_buf.iova()
            ));
            ctrl.admin_cmd(
                ADMIN_OPC_IDENTIFY,
                0,
                id_buf.iova(),
                0,
                CNS_IDENTIFY_CTRL,
                0,
                0,
                0,
                0,
                0,
            )?;
            // SAFETY: id_buf is DMA memory we own; VWC is byte 525 in the Identify Controller data.
            let vwc_byte = unsafe { *(id_buf.inner().virt().add(525)) };
            ctrl.vwc = vwc_byte & 1 != 0;
            id_buf.into_inner().free();
        }

        // 8. Identify Namespace 1 → LBA count + format.
        {
            let id_buf = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
            let id_buf = AuthorizedDma::authorize(id_buf, |buf| buf.authorize(bdf))
                .map_err(|_| ViError::PermissionDenied)?;
            ctrl.admin_cmd(
                ADMIN_OPC_IDENTIFY,
                1,
                id_buf.iova(),
                0,
                CNS_IDENTIFY_NS,
                0,
                0,
                0,
                0,
                0,
            )?;
            // SAFETY: id_buf is the 4-KiB Identify Namespace response this cell owns;
            // every offset read here is below 4096 (`layout::lba_format_offset`).
            let (nsze, flbas, nlbaf, lbaf) = unsafe {
                let ptr = id_buf.inner().virt();
                let flbas = *ptr.add(layout::ID_NS_FLBAS);
                let lbaf_off = layout::lba_format_offset(layout::lba_format_index(flbas));
                (
                    core::ptr::read_volatile(ptr as *const u64),
                    flbas,
                    *ptr.add(layout::ID_NS_NLBAF),
                    core::ptr::read_volatile(ptr.add(lbaf_off) as *const u32),
                )
            };
            id_buf.into_inner().free();
            if let Err(why) = layout::validate_lba_format(flbas, nlbaf, u32::from_le(lbaf)) {
                let _ = print_fmt(format_args!(
                    "[nvme] namespace 1 LBA format refused: {:?} (FLBAS=0x{:x} NLBAF={} LBAF=0x{:08x}); this driver serves 512-byte blocks without metadata\n",
                    why, flbas, nlbaf, u32::from_le(lbaf)
                ));
                return Err(ViError::NotSupported);
            }
            ctrl.n_sectors = u64::from_le(nsze);
        }

        // 9. Create I/O CQ (admin opcode 0x05).
        // CDW10 layout (NVMe 1.x §5.3/§5.4): QSIZE[31:16] (0-based) | QID[15:0].
        // The first port of this file inverted the two fields — QEMU then created
        // CQ with QID=63 and Create-SQ failed with Invalid CQID (CQ 1 absent).
        // Interrupts (CDW11.IEN, IV[31:16]) only when 435 granted vectors: the I/O CQ
        // takes vector 1 when there are two, else shares vector 0 with the admin CQ.
        let io_cq_iova = ctrl.io.cq_iova();
        let io_cq_irq = match ctrl.irq.vectors() {
            0 => 0,
            1 => 1 << 1,
            _ => (1 << 16) | (1 << 1),
        };
        ctrl.admin_cmd(
            ADMIN_OPC_CREATE_CQ,
            0,
            io_cq_iova,
            0,
            ((IO_QUEUE_DEPTH as u32 - 1) << 16) | 1, // CDW10: QSIZE | QID=1
            io_cq_irq | 0x1,                         // CDW11: IV | IEN | PC=1
            0,
            0,
            0,
            0,
        )?;

        // 10. Create I/O SQ (admin opcode 0x01).
        let io_sq_iova = ctrl.io.sq_iova();
        ctrl.admin_cmd(
            ADMIN_OPC_CREATE_SQ,
            0,
            io_sq_iova,
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
        mmio.read_u32(off)
    }

    fn write32(mmio: &MmioRegion, off: usize, val: u32) -> ViResult<()> {
        mmio.write_u32(off, val)
    }

    fn write64(mmio: &MmioRegion, off: usize, val: u64) -> ViResult<()> {
        mmio.write::<u64>(off, val)
    }

    /// `Doorbells::new` proved every doorbell of a queue this driver creates lies in
    /// the window, so the region's bounds check cannot refuse these writes; it stays
    /// as the guard that turns a miscomputed offset into a dropped write instead of
    /// a store outside the claimed BAR.
    fn ring(mmio: &MmioRegion, off: usize, val: u16) {
        let _ = mmio.write_u32(off, val as u32);
    }

    fn ring_sq_tail(&self, qid: usize, tail: u16) {
        Self::ring(&self.mmio, self.doorbells.sq_tail(qid), tail);
    }

    fn wait_command(&mut self, qid: usize, cid: u16, wait: &mut Deadline) -> Result<u16, Spent> {
        let doorbell = self.doorbells.cq_head(qid);
        let mmio = &self.mmio;
        let irq = &mut self.irq;
        let queue = if qid == 0 {
            &mut self.admin
        } else {
            &mut self.io
        };
        queue.wait_completion(
            cid,
            |head| Self::ring(mmio, doorbell, head),
            || idle(wait, irq),
        )
    }

    /// Cold failure diagnostics: actual registers and the current (not cached) CQ slot.
    fn admin_timeout(&self, opc: u8, cid: u16, spent: Spent) -> ViResult<()> {
        let entry = (self.admin.cq_virt() as *const crate::queue::CqEntry)
            .wrapping_add(self.admin.cq.head as usize);
        // SAFETY: head is in range and the queue DMA allocations remain owned here.
        let raw = unsafe { core::ptr::read_volatile(&(*entry).phase_status) };
        let done_cid = unsafe { core::ptr::read_volatile(&(*entry).cid) };
        let cc = Self::read32(&self.mmio, REG_CC)?;
        let csts = Self::read32(&self.mmio, REG_CSTS)?;
        let aqa = Self::read32(&self.mmio, REG_AQA)?;
        let asq = Self::read32(&self.mmio, REG_ASQ)? as u64
            | ((Self::read32(&self.mmio, REG_ASQ + 4)? as u64) << 32);
        let acq = Self::read32(&self.mmio, REG_ACQ)? as u64
            | ((Self::read32(&self.mmio, REG_ACQ + 4)? as u64) << 32);
        let _ = print_fmt(format_args!(
            "[nvme] admin timeout opc={} cid={} after {} (cq_head={} phase={} raw_status=0x{:x} done_cid={} CC=0x{:x} CSTS=0x{:x} AQA=0x{:x} ASQ=0x{:x} ACQ=0x{:x})\n",
            opc, cid, spent, self.admin.cq.head, self.admin.cq.phase, raw, done_cid,
            cc, csts, aqa, asq, acq
        ));
        Ok(())
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

        let mut wait = Deadline::start(deadline::ADMIN_BUDGET_MS, now_ms());
        let status = match self.wait_command(0, cid, &mut wait) {
            Ok(status) => status,
            Err(spent) => {
                self.admin_timeout(opc, cid, spent)?;
                return Err(ViError::IO);
            }
        };
        note_slow(&wait, "admin", opc);
        if status != 0 {
            ostd::io::println(&alloc::format!(
                "[nvme] admin error opc={} status={}",
                opc,
                status
            ));
            return Err(ViError::IO);
        }
        Ok(())
    }

    // ── I/O command submit + poll ─────────────────────────────────────────────

    /// The SQ tail doorbell is the issue point and nothing before it can fail,
    /// so every error here is issued: the controller may already have executed
    /// the command.
    fn submit_io(
        &mut self,
        opc: u8,
        nsid: u32,
        lba: u64,
        nlb: u16,
        prp1: u64,
    ) -> Result<(), IoFailure> {
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
        #[cfg(feature = "fault-inject")]
        if core::mem::take(&mut self.exit_after_doorbell) {
            let _ = print_fmt(format_args!(
                "[nvme] fault-inject: driver exiting after doorbell at lba={}\n",
                lba
            ));
            ostd::syscall::sys_exit(70);
        }

        let mut wait = Deadline::start(deadline::IO_BUDGET_MS, now_ms());
        let status = match self.wait_command(1, cid, &mut wait) {
            Ok(status) => status,
            Err(spent) => {
                let entry = (self.io.cq_virt() as *const crate::queue::CqEntry)
                    .wrapping_add(self.io.cq.head as usize);
                // SAFETY: head is in range and this controller owns the live DMA queue.
                let raw = unsafe { core::ptr::read_volatile(&(*entry).phase_status) };
                let _ = print_fmt(format_args!(
                    "[nvme] io timeout opc={} lba={} cid={} after {} (cq_head={} sq_tail={} phase={} raw_status=0x{:x} depth={})\n",
                    opc, lba, cid, spent, self.io.cq.head, self.io.sq_tail, self.io.cq.phase, raw, depth
                ));
                return Err(IoFailure::TimedOut);
            }
        };
        note_slow(&wait, "io", opc);
        if status != 0 {
            // Reads past the namespace end are expected during volume probes.
            if IO_ERR_LOG.fetch_sub(1, Ordering::Relaxed) > 0 {
                ostd::io::println(&alloc::format!(
                    "[nvme] io error opc={} lba={} nlb={} status={}",
                    opc,
                    lba,
                    nlb,
                    status
                ));
            }
            return Err(IoFailure::Status);
        }
        Ok(())
    }

    /// Read one 512-byte block into the page at `iova`. The namespace format is
    /// 512-byte blocks without metadata (`layout::validate_lba_format`) and NLB=0,
    /// so the transfer is 512 bytes and PRP1 alone describes it as long as it does
    /// not cross a page (`layout::blocks_in_one_prp`, enforced by `BounceBuffer`).
    pub fn read_sector(&mut self, sector: u64, iova: u64) -> Result<(), IoFailure> {
        self.submit_io(NVM_OPC_READ, 1, sector, 0, iova)
    }

    /// Read `count` 512-byte blocks into the page at `iova`. The caller keeps the
    /// transfer inside that page (`layout::blocks_in_one_prp`), so PRP1 alone
    /// describes it, as for [`Self::read_sector`].
    pub fn read_sectors(&mut self, sector: u64, count: u16, iova: u64) -> Result<(), IoFailure> {
        debug_assert!(count >= 1 && usize::from(count) <= crate::layout::blocks_in_one_prp(iova));
        self.submit_io(NVM_OPC_READ, 1, sector, count - 1, iova)
    }

    /// Write one 512-byte block from the page at `iova`; see [`Self::read_sector`].
    pub fn write_sector(&mut self, sector: u64, iova: u64) -> Result<(), IoFailure> {
        self.submit_io(NVM_OPC_WRITE, 1, sector, 0, iova)
    }

    /// Fault-injection builds only: exit (70) right after ringing the SQ tail
    /// doorbell of the next I/O command, before polling its completion.
    #[cfg(feature = "fault-inject")]
    pub fn exit_after_next_doorbell(&mut self) {
        self.exit_after_doorbell = true;
    }

    /// Send an NVMe flush command for the namespace.
    pub fn flush(&mut self) -> Result<(), IoFailure> {
        self.submit_io(NVM_OPC_FLUSH, 1, 0, 0, 0)
    }
}
