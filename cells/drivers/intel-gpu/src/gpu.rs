//! The running GPU as the `xe` backend sees it ([`GpuHw`]): per-client PPGTTs in driver
//! memory, GuC contexts with their LRCs, ring submission, and the export leases of
//! CP1 D-G1 option B.
//!
//! Upstream reference (MIT): `drivers/gpu/drm/xe/xe_guc_submit.c` (`register_exec_queue`,
//! `submit_exec_queue`, `disable_scheduling_deregister`, `deregister_exec_queue`, the
//! G2H handlers), `xe_lrc.c`, `xe_ring_ops.c`, `xe_pt.c`, `xe_vm.c`
//! (`xe_vm_pdp4_descriptor`), `xe_guc_tlb_inval.c` and `xe_tlb_inval.c`.
//!
//! Bans come from the GuC (`CONTEXT_RESET_NOTIFICATION`, `NOTIFY_MEMORY_CAT_ERROR`,
//! `ENGINE_FAILURE_NOTIFICATION`), read from the G2H ring whenever a queue is touched.

use crate::clock::Clock;
use crate::devmem::{read_u32, write_u32s, DmaMemory, Pool, Region};
use crate::ggtt::Ggtt;
use crate::guc_ct::GucCt;
use crate::ppgtt_store::{ClientVm, PoolTables};
use crate::regs::RegisterFile;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use intel_gpu_core::backend::{
    XeError, XE_ENGINE_CLASS_RENDER, XE_EXEC_QUEUE_BAN_FLAG_BAD_FAULT, XE_EXEC_QUEUE_BAN_FLAG_HANG,
};
use intel_gpu_core::guc_abi::{self, G2hEvent, GUC_RENDER_CLASS};
use intel_gpu_core::hw::{ExportError, ExportMapper, ExportRecord, GpuHw, HwQueue, HwVm};
use intel_gpu_core::lrc::{self, LrcParams};
use intel_gpu_core::ppgtt::Layout;
use intel_gpu_core::pte::{self, PatIndex};
use types::ViError;

/// `disable_scheduling_deregister` waits `HZ * 5` for a pending enable/disable; the same
/// bound is used for each G2H answer of a context teardown.
pub const CONTEXT_G2H_TIMEOUT_US: u64 = 5_000_000;
/// `tlb_inval_timeout_delay`: `HZ / 4` of hardware time plus twice the time a full H2G
/// ring takes to drain (2 × 4 s).
pub const TLB_INVAL_TIMEOUT_US: u64 = 250_000 + 2 * 4_000_000;
/// GuC context ids this driver hands out (`GUC_ID_MAX` is far larger; one LRC per id).
pub const MAX_CONTEXTS: u32 = 1024;

struct Queue {
    lrc: Region,
    /// CPU ring tail (`lrc->ring.tail`), bytes.
    tail: u32,
    next_seqno: u32,
    /// Scheduling enabled (`exec_queue_enabled`).
    enabled: bool,
    /// `DRM_XE_EXEC_QUEUE_BAN_FLAG_*` the GuC reported.
    ban: u32,
}

/// Counters of device operations that did not complete; only hardware can tell why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    /// TLB invalidations the GuC did not confirm in time. The export revoke that follows
    /// still drains VT-d, so a stale GPU translation faults instead of reaching memory.
    pub tlb_timeouts: u32,
    /// Contexts whose disable/deregister was not confirmed: their LRC pages and id are
    /// never reused.
    pub stuck_contexts: u32,
    /// H2G sends that failed (CT broken or full).
    pub ct_failures: u32,
}

/// The driver's device: registers, clock, memory, the GuC channel and every client's
/// hardware objects.
pub struct Gpu<R: RegisterFile, C: Clock, M: DmaMemory, E: ExportMapper> {
    pub regs: R,
    pub clock: C,
    pub mem: M,
    pub exports: E,
    pub ggtt: Ggtt,
    pub pool: Pool,
    pub ct: GucCt,
    pub faults: Faults,
    vms: BTreeMap<u32, ClientVm>,
    next_vm: u32,
    queues: BTreeMap<u32, Queue>,
    free_ids: Vec<u32>,
    next_id: u32,
    tlb_sent: u32,
    tlb_done: u32,
}

impl<R: RegisterFile, C: Clock, M: DmaMemory, E: ExportMapper> Gpu<R, C, M, E> {
    pub fn new(regs: R, clock: C, mem: M, exports: E, ggtt: Ggtt, pool: Pool, ct: GucCt) -> Self {
        Self {
            regs,
            clock,
            mem,
            exports,
            ggtt,
            pool,
            ct,
            faults: Faults::default(),
            vms: BTreeMap::new(),
            next_vm: 1,
            queues: BTreeMap::new(),
            free_ids: Vec::new(),
            next_id: 0,
            tlb_sent: 0,
            tlb_done: 0,
        }
    }

    /// Live client VMs (PPGTT roots).
    #[cfg(test)]
    pub fn vm_count(&self) -> usize {
        self.vms.len()
    }

    /// Live GuC contexts.
    #[cfg(test)]
    pub fn queue_count(&self) -> usize {
        self.queues.len()
    }

    /// The device address `va` reaches in `vm`, if mapped.
    #[cfg(test)]
    pub fn translate(&mut self, vm: HwVm, va: u64) -> Option<u64> {
        let client = self.vms.get(&vm.0)?;
        let mut tables = PoolTables {
            pool: &mut self.pool,
            mem: &mut self.mem,
            regs: &mut self.regs,
            ggtt: &mut self.ggtt,
        };
        client.translate(&mut tables, va).ok().flatten()
    }

    /// The raw PPGTT leaf for `va` in `vm` (write and NULL bits).
    #[cfg(test)]
    pub fn leaf(&mut self, vm: HwVm, va: u64) -> Option<u64> {
        let root = self.vms.get(&vm.0)?.root_table();
        let mut table = root;
        for level in (0..=Layout::PTL.max_level).rev() {
            let index = (va >> (12 + 9 * level)) & 511;
            let mut b = [0u8; 8];
            self.mem.read(table + index * 8, &mut b).then_some(())?;
            let entry = u64::from_le_bytes(b);
            if level == 0 {
                return Some(entry);
            }
            if entry & pte::PAGE_PRESENT == 0 {
                return None;
            }
            table = entry & pte::ADDR_MASK_4K;
        }
        None
    }

    /// Send one H2G message; a broken channel is counted, and reported as `Io`.
    fn send(&mut self, action: &[u32]) -> Result<(), XeError> {
        self.ct
            .send(&mut self.mem, &mut self.regs, &mut self.clock, action)
            .map_err(|_| {
                self.faults.ct_failures += 1;
                XeError::Io
            })
    }

    /// Drain the G2H ring and apply what it says.
    fn poll_g2h(&mut self) {
        let Ok(events) = self.ct.receive(&mut self.mem) else {
            self.faults.ct_failures += 1;
            return;
        };
        for event in events {
            self.apply(event);
        }
    }

    fn ban(&mut self, guc_id: u32, flag: u32) {
        if let Some(q) = self.queues.get_mut(&guc_id) {
            q.ban |= flag;
        }
    }

    /// Poll the G2H ring until `done` holds or `timeout_us` passes.
    fn wait_g2h(&mut self, timeout_us: u64, mut done: impl FnMut(&G2hEvent) -> bool) -> bool {
        let deadline = self.clock.now_us().saturating_add(timeout_us);
        let mut wait = 10u64;
        loop {
            let Ok(events) = self.ct.receive(&mut self.mem) else {
                self.faults.ct_failures += 1;
                return false;
            };
            let mut hit = false;
            for event in events {
                hit |= done(&event);
                self.apply(event);
            }
            if hit {
                return true;
            }
            let now = self.clock.now_us();
            if now >= deadline {
                return false;
            }
            self.clock.relax(wait.min(deadline - now));
            wait = (wait * 2).min(100_000);
        }
    }

    /// One G2H event, as the `xe_guc_*_handler`s treat it.
    fn apply(&mut self, event: G2hEvent) {
        match event {
            G2hEvent::ContextReset { guc_id } => self.ban(guc_id, XE_EXEC_QUEUE_BAN_FLAG_HANG),
            G2hEvent::CatError { guc_id } => self.ban(guc_id, XE_EXEC_QUEUE_BAN_FLAG_BAD_FAULT),
            // `xe` resets the whole GT on an engine failure: every context on it is lost.
            G2hEvent::EngineFailure { .. } => {
                for q in self.queues.values_mut() {
                    q.ban |= XE_EXEC_QUEUE_BAN_FLAG_HANG;
                }
            }
            G2hEvent::SchedDone { guc_id, enabled } => {
                if let Some(q) = self.queues.get_mut(&guc_id) {
                    q.enabled = enabled;
                }
            }
            G2hEvent::TlbInvalidationDone { seqno } => {
                if seqno.wrapping_sub(self.tlb_done) as i32 > 0 {
                    self.tlb_done = seqno;
                }
            }
            G2hEvent::DeregisterDone { .. } | G2hEvent::Other { .. } => {}
        }
    }

    /// A full GPU TLB invalidation through the GuC, waited for (`send_tlb_inval_ppgtt`
    /// without range support, then the `TLB_INVALIDATION_DONE` seqno).
    fn invalidate_tlbs(&mut self) {
        self.tlb_sent = self.tlb_sent.wrapping_add(1);
        let seqno = self.tlb_sent;
        if self.send(&guc_abi::tlb_invalidation_full(seqno)).is_err() {
            self.faults.tlb_timeouts += 1;
            return;
        }
        if self.tlb_done.wrapping_sub(seqno) as i32 >= 0 {
            return;
        }
        let confirmed = self.wait_g2h(TLB_INVAL_TIMEOUT_US, |e| {
            matches!(e, G2hEvent::TlbInvalidationDone { seqno: s } if s.wrapping_sub(seqno) as i32 >= 0)
        });
        if !confirmed {
            self.faults.tlb_timeouts += 1;
        }
    }

    fn alloc_guc_id(&mut self) -> Option<u32> {
        if let Some(id) = self.free_ids.pop() {
            return Some(id);
        }
        (self.next_id < MAX_CONTEXTS).then(|| {
            self.next_id += 1;
            self.next_id - 1
        })
    }

    fn lrc_reg(lrc: &Region, index: usize) -> u64 {
        lrc.iova + u64::from(lrc::ctx_reg_offset(index))
    }
}

impl<R: RegisterFile, C: Clock, M: DmaMemory, E: ExportMapper> ExportMapper for Gpu<R, C, M, E> {
    fn map_export(&mut self, export_id: u64) -> Result<ExportRecord, ExportError> {
        self.exports.map_export(export_id)
    }

    fn revoke_export(&mut self, export_id: u64) -> Result<(), ExportError> {
        self.exports.revoke_export(export_id)
    }
}

fn vi_to_xe(e: ViError) -> XeError {
    match e {
        ViError::OutOfMemory => XeError::OutOfMemory,
        _ => XeError::Invalid,
    }
}

impl<R: RegisterFile, C: Clock, M: DmaMemory, E: ExportMapper> GpuHw for Gpu<R, C, M, E> {
    fn vm_create(&mut self) -> Result<HwVm, XeError> {
        let id = self.next_vm;
        let next = id.checked_add(1).ok_or(XeError::OutOfMemory)?;
        let mut tables = PoolTables {
            pool: &mut self.pool,
            mem: &mut self.mem,
            regs: &mut self.regs,
            ggtt: &mut self.ggtt,
        };
        let vm = ClientVm::new(&mut tables).map_err(vi_to_xe)?;
        self.next_vm = next;
        self.vms.insert(id, vm);
        Ok(HwVm(id))
    }

    fn vm_map(
        &mut self,
        vm: HwVm,
        va: u64,
        dma: u64,
        len: u64,
        pat: PatIndex,
        writable: bool,
    ) -> Result<(), XeError> {
        let client = self.vms.get_mut(&vm.0).ok_or(XeError::NoEntry)?;
        let mut tables = PoolTables {
            pool: &mut self.pool,
            mem: &mut self.mem,
            regs: &mut self.regs,
            ggtt: &mut self.ggtt,
        };
        client
            .bind_range(&mut tables, va, dma, len, writable, pat)
            .map_err(vi_to_xe)
    }

    fn vm_map_null(&mut self, vm: HwVm, va: u64, len: u64, pat: PatIndex) -> Result<(), XeError> {
        let client = self.vms.get_mut(&vm.0).ok_or(XeError::NoEntry)?;
        let mut tables = PoolTables {
            pool: &mut self.pool,
            mem: &mut self.mem,
            regs: &mut self.regs,
            ggtt: &mut self.ggtt,
        };
        client
            .bind_null_range(&mut tables, va, len, pat)
            .map_err(vi_to_xe)
    }

    fn vm_unmap(&mut self, vm: HwVm, va: u64, len: u64) {
        let Some(client) = self.vms.get_mut(&vm.0) else {
            return;
        };
        let mut tables = PoolTables {
            pool: &mut self.pool,
            mem: &mut self.mem,
            regs: &mut self.regs,
            ggtt: &mut self.ggtt,
        };
        client.unbind_range(&mut tables, va, len);
        self.invalidate_tlbs();
    }

    fn vm_destroy(&mut self, vm: HwVm) {
        let Some(client) = self.vms.remove(&vm.0) else {
            return;
        };
        let mut tables = PoolTables {
            pool: &mut self.pool,
            mem: &mut self.mem,
            regs: &mut self.regs,
            ggtt: &mut self.ggtt,
        };
        client.destroy(&mut tables);
    }

    /// `xe_lrc_create` + `register_exec_queue`: build the render LRC pointing at the VM's
    /// PPGTT and register it with the GuC as a normal KMD context on RCS0.
    fn queue_create(&mut self, vm: HwVm, engine_class: u16) -> Result<HwQueue, XeError> {
        if engine_class != XE_ENGINE_CLASS_RENDER {
            return Err(XeError::Invalid);
        }
        let root = self.vms.get(&vm.0).ok_or(XeError::NoEntry)?.root_table();
        let pdp = pte::encode_table(root, PatIndex::WB, Layout::PTL.dma_bits)
            .map_err(|_| XeError::Invalid)?;
        let guc_id = self.alloc_guc_id().ok_or(XeError::OutOfMemory)?;
        let pages = (lrc::LRC_BYTES as usize).div_ceil(crate::devmem::PAGE);
        let Some(region) = self
            .pool
            .alloc(&mut self.mem, &mut self.regs, &mut self.ggtt, pages)
        else {
            self.free_ids.push(guc_id);
            return Err(XeError::OutOfMemory);
        };
        let (image, tail) = lrc::render_lrc(&LrcParams {
            ggtt: region.ggtt,
            pdp,
            asid: vm.0,
            initial_seqno: 0,
        });
        let registered = write_u32s(&mut self.mem, region.iova, &image)
            .then_some(())
            .ok_or(XeError::Io)
            .and_then(|()| {
                self.send(&guc_abi::register_context(
                    guc_id,
                    GUC_RENDER_CLASS,
                    1,
                    lrc::descriptor(region.ggtt),
                ))
            });
        if let Err(e) = registered {
            self.pool.free(&mut self.mem, region);
            self.free_ids.push(guc_id);
            return Err(e);
        }
        self.queues.insert(
            guc_id,
            Queue {
                lrc: region,
                tail,
                next_seqno: 1,
                enabled: false,
                ban: 0,
            },
        );
        Ok(HwQueue(guc_id))
    }

    /// `__emit_job_gen12_render_compute` (reduced to the batch and its seqnos) +
    /// `submit_exec_queue`: write the job, move the context's ring tail, tell the GuC.
    fn queue_submit(&mut self, queue: HwQueue, batch_va: u64) -> Result<(), XeError> {
        self.poll_g2h();
        let q = self.queues.get(&queue.0).ok_or(XeError::NoEntry)?;
        if q.ban != 0 {
            return Err(XeError::Canceled);
        }
        let (lrc_region, tail, seqno, enabled) = (q.lrc, q.tail, q.next_seqno, q.enabled);
        let head = read_u32(&self.mem, Self::lrc_reg(&lrc_region, lrc::CTX_RING_HEAD))
            .ok_or(XeError::Io)?
            & lrc::HEAD_ADDR;
        let job_bytes = (lrc::JOB_DWORDS * 4) as u32;
        // Never fill the ring completely: head == tail means empty.
        if lrc::ring_space(head, tail) <= job_bytes {
            return Err(XeError::TryAgain);
        }
        let job = lrc::job(lrc_region.ggtt, batch_va, seqno);
        // `xe_lrc_write_ring`: split at the end of the ring.
        let first = ((lrc::RING_SIZE - tail) / 4).min(lrc::JOB_DWORDS as u32) as usize;
        let ring = lrc_region.iova + u64::from(lrc::RING_OFFSET);
        let written = write_u32s(&mut self.mem, ring + u64::from(tail), &job[..first])
            && write_u32s(&mut self.mem, ring, &job[first..]);
        let new_tail = (tail + job_bytes) % lrc::RING_SIZE;
        // `xe_lrc_set_ring_tail`, after the ring contents.
        if !written
            || !write_u32s(
                &mut self.mem,
                Self::lrc_reg(&lrc_region, lrc::CTX_RING_TAIL),
                &[new_tail],
            )
        {
            return Err(XeError::Io);
        }
        if let Some(q) = self.queues.get_mut(&queue.0) {
            q.tail = new_tail;
            q.next_seqno = seqno.wrapping_add(1);
        }
        if enabled {
            self.send(&guc_abi::sched_context(queue.0))
        } else {
            self.send(&guc_abi::sched_context_mode_set(queue.0, true))?;
            if let Some(q) = self.queues.get_mut(&queue.0) {
                q.enabled = true;
            }
            Ok(())
        }
    }

    fn queue_ban(&mut self, queue: HwQueue) -> u32 {
        self.poll_g2h();
        self.queues.get(&queue.0).map_or(0, |q| q.ban)
    }

    /// `disable_scheduling_deregister` then `deregister_exec_queue`, each waited for; the
    /// LRC goes back to the pool only once the GuC confirmed it no longer uses it.
    fn queue_destroy(&mut self, queue: HwQueue) {
        self.poll_g2h();
        let Some(q) = self.queues.get(&queue.0) else {
            return;
        };
        let id = queue.0;
        let mut quiet = true;
        if q.enabled {
            quiet = self.send(&guc_abi::sched_context_mode_set(id, false)).is_ok()
                && self.wait_g2h(CONTEXT_G2H_TIMEOUT_US, |e| {
                    matches!(e, G2hEvent::SchedDone { guc_id, enabled: false } if *guc_id == id)
                });
        }
        quiet = quiet
            && self.send(&guc_abi::deregister_context(id)).is_ok()
            && self.wait_g2h(
                CONTEXT_G2H_TIMEOUT_US,
                |e| matches!(e, G2hEvent::DeregisterDone { guc_id } if *guc_id == id),
            );
        let Some(q) = self.queues.remove(&id) else {
            return;
        };
        if quiet {
            self.pool.free(&mut self.mem, q.lrc);
            self.free_ids.push(id);
        } else {
            self.faults.stuck_contexts += 1;
        }
    }
}
