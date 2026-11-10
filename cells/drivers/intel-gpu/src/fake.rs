//! Host test fixture: Intel iGPU MMIO registers, device memory and a GuC model.
//!
//! The registers model what bring-up and submission depend on: forcewake acks, the GuC
//! domain reset (optionally stuck), the write-once WOPCM registers with their lock bits,
//! the scratch parameter block, the uCode DMA and a scripted `GUC_STATUS` progression, the
//! MMIO host-to-GuC channel (`HOST2GUC_SELF_CFG`, `HOST2GUC_CONTROL_CTB`), and the GuC side
//! of the CT rings: it reads H2G messages from device memory through the GGTT entries the
//! driver wrote, records them, and answers context mode changes, deregistrations and TLB
//! invalidations with the G2H events upstream expects. Time is a counter the driver's
//! waits advance.

use crate::clock::Clock;
use crate::devmem::DmaMemory;
use crate::ggtt::GSM_OFFSET;
use crate::guc::{
    DMA_GUC_WOPCM_OFFSET, GUC_DMA_CTRL, GUC_DMA_CTRL_START, GUC_HOST_INTERRUPT, GUC_STATUS,
    GUC_STATUS_MIA_IN_RESET, GUC_STATUS_UKERNEL_READY, GUC_WOPCM_SIZE, VF_SW_FLAG_0,
};
use crate::regs::{
    RegisterFile, FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, FORCEWAKE_GT, GDRST, GMD_ID, GRDOM_GUC,
    PMINTRMSK, RPM_CONFIG0, XE2_GT_GEOMETRY_DSS_1, XE2_GT_GEOMETRY_DSS_2,
    XEHP_GT_COMPUTE_DSS_ENABLE, XELP_EU_ENABLE, XELP_GT_GEOMETRY_DSS_ENABLE,
};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use intel_gpu_core::guc_abi::{
    ACTION_DEREGISTER_CONTEXT, ACTION_DEREGISTER_CONTEXT_DONE, ACTION_HOST2GUC_CONTROL_CTB,
    ACTION_HOST2GUC_SELF_CFG, ACTION_SCHED_CONTEXT_MODE_DONE, ACTION_SCHED_CONTEXT_MODE_SET,
    ACTION_TLB_INVALIDATION, ACTION_TLB_INVALIDATION_DONE, HXG_ORIGIN_GUC, KLV_G2H_CTB_ADDR,
    KLV_G2H_CTB_DESCRIPTOR_ADDR, KLV_G2H_CTB_SIZE, KLV_H2G_CTB_ADDR, KLV_H2G_CTB_DESCRIPTOR_ADDR,
    KLV_H2G_CTB_SIZE,
};
use intel_gpu_core::guc_init::{
    GUC_WOPCM_OFFSET_MASK, GUC_WOPCM_OFFSET_VALID, GUC_WOPCM_SIZE_LOCKED, GUC_WOPCM_SIZE_MASK,
};
use intel_gpu_core::hw::{ExportError, ExportMapper, ExportRecord};
use intel_gpu_core::pte::ADDR_MASK_4K;
use types::ViResult;

/// Device memory and the GuC's view of it, shared by [`FakeRegs`] and [`FakeDma`].
#[derive(Default)]
pub struct FakeDevice {
    /// Allocations by device address.
    pub regions: BTreeMap<u64, Vec<u8>>,
    next_iova: u64,
    /// Pages still allocatable; `None` = unlimited.
    pub page_budget: Option<usize>,
    /// `HOST2GUC_SELF_CFG` KLVs received, by key.
    pub self_cfg: BTreeMap<u16, u64>,
    pub ct_enabled: bool,
    /// HXG part (action word onwards) of every H2G message the GuC consumed, in order.
    pub h2g: Vec<Vec<u32>>,
    /// The GuC stops answering H2G messages (they are still consumed).
    pub silent: bool,
    h2g_head: u32,
    g2h_tail: u32,
}

impl FakeDevice {
    fn locate(&self, iova: u64, len: usize) -> Option<(u64, usize)> {
        let (&base, bytes) = self.regions.range(..=iova).next_back()?;
        let offset = (iova - base) as usize;
        (offset + len <= bytes.len()).then_some((base, offset))
    }

    pub fn read(&self, iova: u64, out: &mut [u8]) -> bool {
        match self.locate(iova, out.len()) {
            Some((base, offset)) => {
                out.copy_from_slice(&self.regions[&base][offset..offset + out.len()]);
                true
            }
            None => false,
        }
    }

    pub fn write(&mut self, iova: u64, data: &[u8]) -> bool {
        match self.locate(iova, data.len()) {
            Some((base, offset)) => {
                self.regions.get_mut(&base).expect("located")[offset..offset + data.len()]
                    .copy_from_slice(data);
                true
            }
            None => false,
        }
    }

    pub fn read_u32(&self, iova: u64) -> u32 {
        let mut b = [0u8; 4];
        assert!(
            self.read(iova, &mut b),
            "read outside device memory at {iova:#x}"
        );
        u32::from_le_bytes(b)
    }

    pub fn write_u32(&mut self, iova: u64, value: u32) {
        assert!(
            self.write(iova, &value.to_le_bytes()),
            "write outside device memory"
        );
    }
}

/// [`DmaMemory`] over a [`FakeDevice`].
pub struct FakeDma(pub Rc<RefCell<FakeDevice>>);

/// Fake device addresses start at 1 GiB, 4 KiB aligned.
const FAKE_IOVA_BASE: u64 = 0x4000_0000;

impl DmaMemory for FakeDma {
    fn alloc(&mut self, pages: usize) -> Option<u64> {
        let mut dev = self.0.borrow_mut();
        if let Some(budget) = &mut dev.page_budget {
            *budget = budget.checked_sub(pages)?;
        }
        let iova = FAKE_IOVA_BASE + dev.next_iova;
        dev.next_iova += (pages as u64 + 1) * 4096; // a guard page between allocations
        dev.regions.insert(iova, vec![0u8; pages * 4096]);
        Some(iova)
    }

    fn write(&mut self, iova: u64, bytes: &[u8]) -> bool {
        self.0.borrow_mut().write(iova, bytes)
    }

    fn read(&self, iova: u64, out: &mut [u8]) -> bool {
        self.0.borrow().read(iova, out)
    }
}

/// Microseconds that pass only when the driver waits.
#[derive(Default)]
pub struct FakeClock {
    pub now: u64,
}

impl Clock for FakeClock {
    fn now_us(&self) -> u64 {
        self.now
    }

    fn relax(&mut self, us: u64) {
        self.now += us.max(1);
    }
}

pub struct FakeRegs {
    pub storage: RefCell<BTreeMap<usize, u32>>,
    pub writes: RefCell<Vec<(usize, u32)>>,
    pub auto_ack_forcewake: bool,
    /// What successive `GUC_STATUS` reads return once the firmware DMA completes; the
    /// last value repeats.
    pub guc_status_after_dma: Vec<u32>,
    status_script: RefCell<VecDeque<u32>>,
    /// `GRDOM_GUC` never self-clears.
    pub stuck_reset: bool,
    /// The WOPCM registers ignore writes and never lock.
    pub wopcm_lock_broken: bool,
    /// Every MMIO request is answered with this header instead.
    pub mmio_reply: Option<u32>,
    pub device: Rc<RefCell<FakeDevice>>,
}

impl FakeRegs {
    pub fn new() -> Self {
        Self {
            storage: RefCell::new(BTreeMap::new()),
            writes: RefCell::new(Vec::new()),
            auto_ack_forcewake: true,
            guc_status_after_dma: vec![GUC_STATUS_UKERNEL_READY],
            status_script: RefCell::new(VecDeque::new()),
            stuck_reset: false,
            wopcm_lock_broken: false,
            mmio_reply: None,
            device: Rc::new(RefCell::new(FakeDevice::default())),
        }
    }

    /// Preconfigure registers for an Intel Panther Lake iGPU (Xe3).
    pub fn for_panther_lake() -> Self {
        let fake = Self::new();
        // GMD_ID: major 30, minor 0, rev 0
        fake.set(GMD_ID, 30 << 22);
        // RPM_CONFIG0: crystal 24 MHz (code 0)
        fake.set(RPM_CONFIG0, 0);
        fake.set(XELP_GT_GEOMETRY_DSS_ENABLE, 0x0FFF);
        fake.set(XE2_GT_GEOMETRY_DSS_1, 0);
        fake.set(XE2_GT_GEOMETRY_DSS_2, 0);
        fake.set(XELP_EU_ENABLE, 0xFF);
        fake.set(XEHP_GT_COMPUTE_DSS_ENABLE, 0x0FFF);
        fake.set_gt_defaults();
        fake
    }

    /// Preconfigure registers for an Intel Lunar Lake iGPU (Xe2).
    pub fn for_lunar_lake() -> Self {
        let fake = Self::new();
        // GMD_ID: major 20, minor 4, rev 0
        fake.set(GMD_ID, (20 << 22) | (4 << 14));
        // RPM_CONFIG0: crystal 38.4 MHz (code 2) -> (2 << 3)
        fake.set(RPM_CONFIG0, 2 << 3);
        fake.set(XELP_GT_GEOMETRY_DSS_ENABLE, 0x00FF);
        fake.set(XE2_GT_GEOMETRY_DSS_1, 0);
        fake.set(XE2_GT_GEOMETRY_DSS_2, 0);
        fake.set(XELP_EU_ENABLE, 0xFF);
        // Fewer compute than geometry DSS, so a mix-up of the two masks shows.
        fake.set(XEHP_GT_COMPUTE_DSS_ENABLE, 0x003F);
        fake.set_gt_defaults();
        fake
    }

    fn set_gt_defaults(&self) {
        self.set(GUC_STATUS, GUC_STATUS_MIA_IN_RESET);
        // Every PM interrupt masked, as after reset.
        self.set(PMINTRMSK, u32::MAX);
        // Three doorbells per SQIDI field + 1, one SQIDI.
        self.set(crate::guc::DIST_DBS_POPULATED, 0x0003_0001);
    }

    /// Device memory this register file's GuC reads, as the driver's allocator.
    pub fn dma(&self) -> FakeDma {
        FakeDma(self.device.clone())
    }

    pub fn set(&self, offset: usize, value: u32) {
        self.storage.borrow_mut().insert(offset, value);
    }

    pub fn get(&self, offset: usize) -> u32 {
        *self.storage.borrow().get(&offset).unwrap_or(&0)
    }

    /// Every value written to `offset`, in order.
    pub fn written(&self, offset: usize) -> Vec<u32> {
        self.writes
            .borrow()
            .iter()
            .filter(|(o, _)| *o == offset)
            .map(|&(_, v)| v)
            .collect()
    }

    /// The device address behind GGTT address `ggtt`, from the entries in GSM.
    pub fn ggtt_to_iova(&self, ggtt: u64) -> u64 {
        let slot = GSM_OFFSET + (ggtt >> 12) as usize * 8;
        let pte = u64::from(self.get(slot)) | u64::from(self.get(slot + 4)) << 32;
        assert!(pte & 1 != 0, "GGTT address {ggtt:#x} not mapped");
        (pte & ADDR_MASK_4K) + (ggtt & 0xfff)
    }

    /// Queue a GuC-to-host event in the G2H ring, as the firmware would.
    pub fn inject_g2h(&self, action: u32, payload: &[u32]) {
        let mut msg = vec![HXG_ORIGIN_GUC | 1 << 28 | action];
        msg.extend_from_slice(payload);
        self.push_g2h(&msg);
    }

    fn cfg(&self, key: u16) -> u64 {
        *self
            .device
            .borrow()
            .self_cfg
            .get(&key)
            .expect("CT not registered")
    }

    fn push_g2h(&self, hxg: &[u32]) {
        let desc = self.ggtt_to_iova(self.cfg(KLV_G2H_CTB_DESCRIPTOR_ADDR));
        let ring = self.ggtt_to_iova(self.cfg(KLV_G2H_CTB_ADDR));
        let size = (self.cfg(KLV_G2H_CTB_SIZE) / 4) as u32;
        let mut dev = self.device.borrow_mut();
        let mut tail = dev.g2h_tail;
        let header = hxg.len() as u32; // fence 0, format HXG
        for &word in core::iter::once(&header).chain(hxg) {
            dev.write_u32(ring + u64::from(tail) * 4, word);
            tail = (tail + 1) % size;
        }
        dev.g2h_tail = tail;
        dev.write_u32(desc + 4, tail);
    }

    /// The firmware's side of `xe_guc_mmio_send_recv`.
    fn answer_mmio(&self) {
        let request: Vec<u32> = (0..4).map(|i| self.get(VF_SW_FLAG_0 + 4 * i)).collect();
        let reply = self
            .mmio_reply
            .unwrap_or_else(|| match request[0] & 0xffff {
                ACTION_HOST2GUC_SELF_CFG => {
                    let key = (request[1] >> 16) as u16;
                    let value = u64::from(request[2]) | u64::from(request[3]) << 32;
                    self.device.borrow_mut().self_cfg.insert(key, value);
                    0xF000_0001 // success, one KLV accepted
                }
                ACTION_HOST2GUC_CONTROL_CTB => {
                    self.device.borrow_mut().ct_enabled = request[1] == 1;
                    0xF000_0000
                }
                _ => 0xE000_0030, // failure: unknown action
            });
        self.set(VF_SW_FLAG_0, reply);
    }

    /// The firmware's side of the H2G ring: consume, record, answer.
    fn consume_h2g(&self) {
        let desc = self.ggtt_to_iova(self.cfg(KLV_H2G_CTB_DESCRIPTOR_ADDR));
        let ring = self.ggtt_to_iova(self.cfg(KLV_H2G_CTB_ADDR));
        let size = (self.cfg(KLV_H2G_CTB_SIZE) / 4) as u32;
        let mut answers = Vec::new();
        {
            let mut dev = self.device.borrow_mut();
            let tail = dev.read_u32(desc + 4);
            let mut head = dev.h2g_head;
            while head != tail {
                let header = dev.read_u32(ring + u64::from(head) * 4);
                let len = header & 0xff;
                let msg: Vec<u32> = (1..=len)
                    .map(|i| dev.read_u32(ring + u64::from((head + i) % size) * 4))
                    .collect();
                head = (head + 1 + len) % size;
                if msg.is_empty() {
                    continue; // zero fill before a wrap
                }
                let action = msg[0] & 0xffff;
                match action {
                    ACTION_SCHED_CONTEXT_MODE_SET => {
                        answers.push(vec![ACTION_SCHED_CONTEXT_MODE_DONE, msg[1], msg[2]]);
                    }
                    ACTION_DEREGISTER_CONTEXT => {
                        answers.push(vec![ACTION_DEREGISTER_CONTEXT_DONE, msg[1]]);
                    }
                    ACTION_TLB_INVALIDATION => {
                        answers.push(vec![ACTION_TLB_INVALIDATION_DONE, msg[1]]);
                    }
                    _ => {}
                }
                dev.h2g.push(msg);
            }
            dev.h2g_head = head;
            dev.write_u32(desc, head);
            if dev.silent {
                answers.clear();
            }
        }
        for answer in answers {
            self.inject_g2h(answer[0], &answer[1..]);
        }
    }
}

impl RegisterFile for FakeRegs {
    fn read32(&self, offset: usize) -> ViResult<u32> {
        if offset == GUC_STATUS {
            let mut script = self.status_script.borrow_mut();
            if script.len() > 1 {
                let value = script.pop_front().expect("non-empty");
                self.set(GUC_STATUS, value);
            } else if let Some(&last) = script.front() {
                self.set(GUC_STATUS, last);
            }
        }
        Ok(self.get(offset))
    }

    fn write32(&mut self, offset: usize, value: u32) -> ViResult<()> {
        self.writes.borrow_mut().push((offset, value));
        match offset {
            // Write-once WOPCM registers: the hardware sets the lock/valid bit itself.
            GUC_WOPCM_SIZE => {
                if !self.wopcm_lock_broken && self.get(offset) & GUC_WOPCM_SIZE_LOCKED == 0 {
                    self.set(offset, value & GUC_WOPCM_SIZE_MASK | GUC_WOPCM_SIZE_LOCKED);
                }
            }
            DMA_GUC_WOPCM_OFFSET => {
                if !self.wopcm_lock_broken && self.get(offset) & GUC_WOPCM_OFFSET_VALID == 0 {
                    self.set(
                        offset,
                        value & (GUC_WOPCM_OFFSET_MASK | 2) | GUC_WOPCM_OFFSET_VALID,
                    );
                }
            }
            // A GuC domain reset leaves the MIA core held in reset, unless it hangs.
            GDRST if value & GRDOM_GUC != 0 => {
                self.set(GDRST, value);
                if !self.stuck_reset {
                    self.set(GDRST, 0);
                    self.set(GUC_STATUS, GUC_STATUS_MIA_IN_RESET);
                    self.status_script.borrow_mut().clear();
                }
            }
            GUC_HOST_INTERRUPT => {
                if self.get(VF_SW_FLAG_0) & HXG_ORIGIN_GUC == 0 {
                    self.answer_mmio();
                } else if self.device.borrow().ct_enabled {
                    self.consume_h2g();
                }
            }
            _ => self.set(offset, value),
        }
        // Forcewake ack follows the masked request bit.
        if offset == FORCEWAKE_GT && self.auto_ack_forcewake && (value >> 16) & 1 != 0 {
            let ack = self.get(FORCEWAKE_ACK_GT);
            let ack = if value & 1 != 0 {
                ack | FORCEWAKE_ACK_BIT
            } else {
                ack & !FORCEWAKE_ACK_BIT
            };
            self.set(FORCEWAKE_ACK_GT, ack);
        }
        // DMA_CTRL is masked: START only takes effect with its write-enable bit.
        let start = u32::from(GUC_DMA_CTRL_START);
        if offset == GUC_DMA_CTRL && value & start != 0 && value & (start << 16) != 0 {
            // DMA finishes at once; the firmware then boots through the script.
            self.set(GUC_DMA_CTRL, 0);
            *self.status_script.borrow_mut() = self.guc_status_after_dma.iter().copied().collect();
        }
        Ok(())
    }
}

/// The kernel's export table as the driver sees it through 438/439. On every revoke it
/// checks the invariant the driver must keep: no present PPGTT leaf anywhere in device
/// memory still points into the window being revoked.
pub struct FakeExports {
    pub records: BTreeMap<u64, ExportRecord>,
    pub device: Rc<RefCell<FakeDevice>>,
    /// Export ids revoked, in order.
    pub revoked: Vec<u64>,
    /// Revokes that found a live leaf into their window.
    pub violations: Vec<u64>,
}

impl FakeExports {
    pub fn new(device: Rc<RefCell<FakeDevice>>) -> Self {
        Self {
            records: BTreeMap::new(),
            device,
            revoked: Vec::new(),
            violations: Vec::new(),
        }
    }

    /// A lease of `len` bytes at window `iova` owned by `(cell, generation)`.
    pub fn add(&mut self, id: u64, cell: u64, generation: u64, iova: u64, len: u64, write: bool) {
        self.records.insert(
            id,
            ExportRecord {
                export_id: id,
                owner_cell_id: cell,
                owner_generation: generation,
                iova,
                len,
                device_write: write,
            },
        );
    }

    fn window_reachable(&self, record: &ExportRecord) -> bool {
        let dev = self.device.borrow();
        dev.regions.values().any(|bytes| {
            bytes.chunks_exact(8).any(|e| {
                let entry = u64::from_le_bytes(e.try_into().expect("8 bytes"));
                let addr = entry & ADDR_MASK_4K;
                entry & 1 != 0 && addr >= record.iova && addr < record.iova + record.len
            })
        })
    }
}

impl ExportMapper for FakeExports {
    fn map_export(&mut self, export_id: u64) -> Result<ExportRecord, ExportError> {
        self.records
            .get(&export_id)
            .copied()
            .ok_or(ExportError::NotFound)
    }

    fn revoke_export(&mut self, export_id: u64) -> Result<(), ExportError> {
        let record = self
            .records
            .remove(&export_id)
            .ok_or(ExportError::NotFound)?;
        if self.window_reachable(&record) {
            self.violations.push(export_id);
        }
        self.revoked.push(export_id);
        Ok(())
    }
}
