//! The adapter: ring setup and a one-command-at-a-time [`ScsiExecutor`].
//!
//! Each command copies its request descriptor into the request ring, bumps
//! `req_prod_idx` in the ring state, kicks, and waits (bounded, interrupt-
//! assisted) for `cmp_prod_idx` to move. A timeout, or a completion that
//! does not describe the request in flight, poisons the adapter: the device
//! may still own the bounce buffer, so no later command is issued.

use crate::dma::DmaArea;
use crate::error::{InitError, PvscsiError};
use crate::irq::CompletionIrq;
use crate::proto::{decode_completion, encode_request, Request};
use crate::regs::*;
use crate::rings::*;
use core::sync::atomic::{fence, Ordering};
use ostd::mmio::MmioRegion;
use scsi_block::{Completion, DataPhase, ScsiExecutor};

pub struct Adapter {
    regs: MmioRegion,
    dma: DmaArea,
    pub(crate) irq: CompletionIrq,
    next_context: u64,
    req_prod: u32,
    cmp_cons: u32,
    poisoned: bool,
}

impl Adapter {
    /// Reset the adapter on its register window `regs`, hand it the rings and
    /// unmask completion interrupts when 435 granted a vector.
    pub fn start(regs: MmioRegion, bdf: u32) -> Result<Self, InitError> {
        regs.wr32(REG_INTR_MASK, 0)?;
        match command(&regs, CMD_ADAPTER_RESET, &[])? {
            0 => {}
            status => return Err(InitError::Reset(status)),
        }
        let dma = DmaArea::new(DMA_PAGES, bdf).ok_or(InitError::Dma)?;
        dma.zero(0, DMA_PAGES * 4096);
        // From here on, dropping the adapter resets the device.
        let mut adapter = Self {
            regs,
            dma,
            irq: CompletionIrq::setup(bdf),
            next_context: 1,
            req_prod: 0,
            cmp_cons: 0,
            poisoned: false,
        };
        let dma = &adapter.dma;
        let words = setup_rings_words(
            dma.iova(STATE_OFF),
            dma.iova(REQ_RING_OFF),
            dma.iova(CMP_RING_OFF),
        );
        fence(Ordering::SeqCst);
        match command(&adapter.regs, CMD_SETUP_RINGS, &words)? {
            0 => {}
            status => return Err(InitError::SetupRings(status)),
        }
        let (req_log2, cmp_log2) = (dma.read32(REQ_LOG2), dma.read32(CMP_LOG2));
        if 1 << req_log2.min(31) != REQ_ENTRIES || 1 << cmp_log2.min(31) != CMP_ENTRIES {
            return Err(InitError::RingGeometry(req_log2, cmp_log2));
        }
        adapter.req_prod = dma.read32(REQ_PROD);
        adapter.cmp_cons = dma.read32(CMP_CONS);
        if adapter.irq.mode().is_some() {
            adapter.regs.wr32(REG_INTR_MASK, INTR_CMPL_MASK)?;
        }
        Ok(adapter)
    }

    /// Wait (bounded) for the completion of the one request in flight and
    /// consume it.
    fn complete(&mut self) -> Result<[u8; CMP_LEN], PvscsiError> {
        let (dma, cons) = (&self.dma, self.cmp_cons);
        let arrived = self
            .irq
            .wait(&self.regs, || dma.read32(CMP_PROD) != cons)
            .unwrap_or(false);
        if !arrived {
            self.poisoned = true;
            return Err(PvscsiError::Timeout);
        }
        fence(Ordering::Acquire);
        if dma.read32(CMP_PROD).wrapping_sub(cons) != 1 {
            self.poisoned = true;
            return Err(PvscsiError::BadCompletion);
        }
        let mut cmp = [0u8; CMP_LEN];
        dma.copy_out(
            CMP_RING_OFF + (cons % CMP_ENTRIES) as usize * CMP_LEN,
            &mut cmp,
        );
        self.cmp_cons = cons.wrapping_add(1);
        fence(Ordering::Release);
        dma.write32(CMP_CONS, self.cmp_cons);
        Ok(cmp)
    }
}

impl Drop for Adapter {
    /// Stop all DMA before the cell lets go of the adapter.
    fn drop(&mut self) {
        let _ = self.regs.wr32(REG_INTR_MASK, 0);
        let _ = command(&self.regs, CMD_ADAPTER_RESET, &[]);
    }
}

impl ScsiExecutor for Adapter {
    type Error = PvscsiError;

    fn execute(
        &mut self,
        target: u8,
        lun: [u8; 8],
        cdb: &[u8],
        data: DataPhase<'_>,
    ) -> Result<Completion, PvscsiError> {
        if self.poisoned {
            return Err(PvscsiError::Poisoned);
        }
        let len = data.len();
        if len > DATA_LEN {
            return Err(PvscsiError::DataTooLarge(len));
        }
        let context = self.next_context;
        let request = encode_request(&Request {
            context,
            target,
            lun,
            cdb,
            to_host: match data {
                DataPhase::None => None,
                DataPhase::In(_) => Some(true),
                DataPhase::Out(_) => Some(false),
            },
            data_addr: self.dma.iova(DATA_OFF),
            data_len: len,
            sense_addr: self.dma.iova(SENSE_OFF),
        })?;
        let dma = &self.dma;
        if self.req_prod.wrapping_sub(dma.read32(REQ_CONS)) >= REQ_ENTRIES {
            return Err(PvscsiError::RingFull);
        }
        self.next_context = context.wrapping_add(1).max(1);
        match &data {
            DataPhase::Out(buf) => dma.copy_in(DATA_OFF, buf),
            _ => dma.zero(DATA_OFF, len),
        }
        dma.zero(SENSE_OFF, SENSE_LEN);
        let slot = (self.req_prod % REQ_ENTRIES) as usize;
        dma.copy_in(REQ_RING_OFF + slot * REQ_LEN, &request);
        // The descriptor and buffers before the index, the index before the kick.
        fence(Ordering::Release);
        self.req_prod = self.req_prod.wrapping_add(1);
        dma.write32(REQ_PROD, self.req_prod);
        fence(Ordering::SeqCst);
        kick(&self.regs, cdb.first().copied().unwrap_or(0)).map_err(|_| {
            self.poisoned = true;
            PvscsiError::BadCompletion
        })?;

        let cmp = self.complete()?;
        let mut sense = [0u8; SENSE_LEN];
        self.dma.copy_out(SENSE_OFF, &mut sense);
        let done = decode_completion(&cmp, context, len, &sense).inspect_err(|error| {
            if error.poisons() {
                self.poisoned = true;
            }
        })?;
        if let DataPhase::In(buf) = data {
            self.dma.copy_out(DATA_OFF, buf);
        }
        Ok(done)
    }
}
