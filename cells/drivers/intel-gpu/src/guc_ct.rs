//! The GuC command transport (CT) as the driver runs it: two buffer objects in device
//! memory, registered and enabled over MMIO, then H2G messages written into the H2G ring
//! and G2H events drained from the G2H ring by polling.
//!
//! Upstream reference (MIT): `drivers/gpu/drm/xe/xe_guc_ct.c` (`xe_guc_ct_init` object
//! layout, `guc_ct_ctb_h2g_register`, `guc_ct_ctb_g2h_register`,
//! `guc_ct_control_toggle`, `__xe_guc_ct_start`, `h2g_write`, `__guc_ct_send_locked`,
//! `g2h_read`), `abi/guc_communication_ctb_abi.h` (descriptor and message formats).
//! The ring arithmetic is [`intel_gpu_core::ct`]. Interrupts are not used: the driver
//! drains the G2H ring whenever it needs an answer or touches a queue.

use crate::clock::Clock;
use crate::devmem::{read_u32, write_u32s, DmaMemory, Pool, Region, PAGE};
use crate::ggtt::Ggtt;
use crate::guc::{mmio_send, notify, GucError};
use crate::regs::RegisterFile;
use alloc::vec;
use alloc::vec::Vec;
use intel_gpu_core::ct::{
    self, CtError, HxgType, DESC_HEAD, DESC_STATUS, DESC_TAIL, MAX_MSG_DWORDS,
};
use intel_gpu_core::guc_abi::{
    self, decode_g2h, G2hEvent, CTB_BUFFER_OFFSET, CTB_G2H_BUFFER_SIZE, CTB_G2H_DESC_OFFSET,
    CTB_H2G_BUFFER_SIZE, CT_SEQNO_MASK, CT_SEQNO_UNTRACKED, KLV_G2H_CTB_ADDR,
    KLV_G2H_CTB_DESCRIPTOR_ADDR, KLV_G2H_CTB_SIZE, KLV_H2G_CTB_ADDR, KLV_H2G_CTB_DESCRIPTOR_ADDR,
    KLV_H2G_CTB_SIZE,
};

/// `xe_guc_ct_queue_proc_time_jiffies`: a full 16 KiB H2G ring takes about 4 s to drain,
/// so a sender waits that long for space before declaring the channel dead.
pub const H2G_SPACE_TIMEOUT_US: u64 = 4_000_000;

/// Why a CT operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtFailure {
    /// The buffers could not be allocated.
    NoMemory,
    /// The MMIO registration or enable failed.
    Setup(GucError),
    /// A descriptor reports an error status, or a pointer is out of range.
    Broken(u32),
    /// The GuC did not make room in the H2G ring in time.
    NoSpace,
}

/// One enabled CT channel.
pub struct GucCt {
    h2g: Region,
    g2h: Region,
    /// CPU copy of the H2G ring; the ring code writes here and changed dwords are copied
    /// to device memory before the descriptor tail publishes them.
    shadow: Vec<u32>,
    tail: u32,
    g2h_head: u32,
    next_fence: u16,
}

impl GucCt {
    /// `xe_guc_ct_init` + `__xe_guc_ct_start(needs_register)`: allocate both objects
    /// (descriptors at 0 and 2 KiB, rings at 4 KiB), register their GGTT addresses and
    /// sizes with `HOST2GUC_SELF_CFG`, then enable with `HOST2GUC_CONTROL_CTB`.
    pub fn enable(
        pool: &mut Pool,
        mem: &mut impl DmaMemory,
        regs: &mut impl RegisterFile,
        ggtt: &mut Ggtt,
        clock: &mut impl Clock,
    ) -> Result<Self, CtFailure> {
        let pages = |bytes: u32| (CTB_BUFFER_OFFSET + bytes) as usize / PAGE;
        let h2g = pool
            .alloc(mem, regs, ggtt, pages(CTB_H2G_BUFFER_SIZE))
            .ok_or(CtFailure::NoMemory)?;
        let g2h = pool
            .alloc(mem, regs, ggtt, pages(CTB_G2H_BUFFER_SIZE))
            .ok_or(CtFailure::NoMemory)?;
        let self_cfg = [
            (KLV_H2G_CTB_DESCRIPTOR_ADDR, 2, u64::from(h2g.ggtt)),
            (KLV_H2G_CTB_ADDR, 2, u64::from(h2g.ggtt + CTB_BUFFER_OFFSET)),
            (KLV_H2G_CTB_SIZE, 1, u64::from(CTB_H2G_BUFFER_SIZE)),
            (
                KLV_G2H_CTB_DESCRIPTOR_ADDR,
                2,
                u64::from(g2h.ggtt + CTB_G2H_DESC_OFFSET),
            ),
            (KLV_G2H_CTB_ADDR, 2, u64::from(g2h.ggtt + CTB_BUFFER_OFFSET)),
            (KLV_G2H_CTB_SIZE, 1, u64::from(CTB_G2H_BUFFER_SIZE)),
        ];
        for (key, len, value) in self_cfg {
            // `guc_self_cfg`: the GuC answers with the number of KLVs it accepted, 1.
            match mmio_send(regs, clock, &guc_abi::self_cfg(key, len, value)) {
                Ok(1) => {}
                Ok(n) => return Err(CtFailure::Setup(GucError::MmioProtocol(n))),
                Err(e) => return Err(CtFailure::Setup(e)),
            }
        }
        match mmio_send(regs, clock, &guc_abi::control_ctb(true)) {
            Ok(0) => {}
            Ok(n) => return Err(CtFailure::Setup(GucError::MmioProtocol(n))),
            Err(e) => return Err(CtFailure::Setup(e)),
        }
        Ok(Self {
            h2g,
            g2h,
            shadow: vec![0; ct::H2G_DWORDS as usize],
            tail: 0,
            g2h_head: 0,
            next_fence: 0,
        })
    }

    /// GGTT addresses of the H2G and G2H objects.
    #[cfg(test)]
    pub fn ggtt(&self) -> (u32, u32) {
        (self.h2g.ggtt, self.g2h.ggtt)
    }

    fn h2g_desc(&self, field: usize) -> u64 {
        self.h2g.iova + (field * 4) as u64
    }

    fn g2h_desc(&self, field: usize) -> u64 {
        self.g2h.iova + u64::from(CTB_G2H_DESC_OFFSET) + (field * 4) as u64
    }

    /// `__guc_ct_send_locked` for a message no response is expected to (a
    /// `FAST_REQUEST`, fence marked untracked): write it, publish the tail, notify.
    pub fn send(
        &mut self,
        mem: &mut impl DmaMemory,
        regs: &mut impl RegisterFile,
        clock: &mut impl Clock,
        action: &[u32],
    ) -> Result<(), CtFailure> {
        let fence = self.next_fence & CT_SEQNO_MASK | CT_SEQNO_UNTRACKED;
        self.next_fence = self.next_fence.wrapping_add(1);
        let deadline = clock.now_us().saturating_add(H2G_SPACE_TIMEOUT_US);
        loop {
            let status = read_u32(mem, self.h2g_desc(DESC_STATUS)).ok_or(CtFailure::NoMemory)?;
            if status != 0 {
                return Err(CtFailure::Broken(status));
            }
            let head = read_u32(mem, self.h2g_desc(DESC_HEAD)).ok_or(CtFailure::NoMemory)?;
            let old = self.tail;
            match ct::h2g_write(
                &mut self.shadow,
                head,
                old,
                fence,
                HxgType::FastRequest,
                action,
            ) {
                Ok(tail) => {
                    self.publish(mem, old, tail)?;
                    return notify(regs).map_err(CtFailure::Setup);
                }
                // The message would cross the end: the rest was zero-filled (NOPs); publish
                // the wrapped tail and try again from 0.
                Err(CtError::Wrapped { new_tail }) => {
                    self.publish(mem, old, new_tail)?;
                }
                Err(CtError::NoSpace) if clock.now_us() < deadline => clock.relax(1_000),
                Err(CtError::NoSpace) => return Err(CtFailure::NoSpace),
                Err(_) => return Err(CtFailure::Broken(head)),
            }
        }
    }

    /// Copy the shadow dwords `[old, new)` (or `[old, end)` on a wrap to 0) to the ring,
    /// then move the descriptor tail: the GuC must never see a tail ahead of its data.
    fn publish(&mut self, mem: &mut impl DmaMemory, old: u32, new: u32) -> Result<(), CtFailure> {
        let end = if new > old { new } else { ct::H2G_DWORDS };
        let ring = self.h2g.iova + u64::from(CTB_BUFFER_OFFSET);
        if !write_u32s(
            mem,
            ring + u64::from(old) * 4,
            &self.shadow[old as usize..end as usize],
        ) || !write_u32s(mem, self.h2g_desc(DESC_TAIL), &[new])
        {
            return Err(CtFailure::NoMemory);
        }
        self.tail = new;
        Ok(())
    }

    /// `g2h_read` until the ring is empty: decode every message and hand back the events.
    pub fn receive(&mut self, mem: &mut impl DmaMemory) -> Result<Vec<G2hEvent>, CtFailure> {
        let size = ct::G2H_DWORDS;
        let ring = self.g2h.iova + u64::from(CTB_BUFFER_OFFSET);
        let status = read_u32(mem, self.g2h_desc(DESC_STATUS)).ok_or(CtFailure::NoMemory)?;
        if status != 0 {
            return Err(CtFailure::Broken(status));
        }
        let tail = read_u32(mem, self.g2h_desc(DESC_TAIL)).ok_or(CtFailure::NoMemory)?;
        if tail >= size {
            return Err(CtFailure::Broken(tail));
        }
        let mut events = Vec::new();
        while self.g2h_head != tail {
            let available = tail.wrapping_sub(self.g2h_head) & (size - 1);
            let at = |i: u32| ring + u64::from((self.g2h_head + i) % size) * 4;
            let header = read_u32(mem, at(0)).ok_or(CtFailure::NoMemory)?;
            let len = header & 0xff;
            if header >> 12 & 0xf != 0 || len as usize > MAX_MSG_DWORDS || len + 1 > available {
                return Err(CtFailure::Broken(header));
            }
            let mut hxg = Vec::with_capacity(len as usize);
            for i in 1..=len {
                hxg.push(read_u32(mem, at(i)).ok_or(CtFailure::NoMemory)?);
            }
            events.push(decode_g2h(&hxg));
            self.g2h_head = (self.g2h_head + 1 + len) % size;
        }
        write_u32s(mem, self.g2h_desc(DESC_HEAD), &[self.g2h_head])
            .then_some(events)
            .ok_or(CtFailure::NoMemory)
    }
}
