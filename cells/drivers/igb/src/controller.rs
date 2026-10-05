//! Intel `igb` (82576-class) NIC controller logic.
//!
//! Register model, taken from the 82576 datasheet as implemented by QEMU's
//! `igb` model (`hw/net/igb_regs.h`, `hw/net/igb_core.c`, QEMU 8.2):
//!
//! * queue-0 ring registers live at `0x0C000` (RX) / `0x0E000` (TX), 0x40 apart
//!   per queue — *not* the 82540EM's `0x2800`/`0x3800`;
//! * the NVM read register `EERD` (0x14) carries the word address at bits 15:2
//!   and signals completion with bit 1 — *not* the 82540EM's bits 15:8 / bit 4;
//! * descriptors are the **advanced** 16-byte forms: an RX completion writes
//!   `status_error` at offset 8 and `length` at offset 12, and a TX descriptor
//!   must set `DEXT | DTYP_DATA` or the model treats it as a legacy descriptor.
//!
//! Every one of those differences is the reason this is a separate cell rather
//! than a re-run of the e1000 controller.
//!
//! Law 4 exception: `unsafe` is used for DMA descriptor and buffer access. Every
//! `unsafe` block carries a `// SAFETY:` comment. MMIO goes through the
//! bounds-checked [`MmioRegion`], so a bad offset returns `Err`, never a stray
//! access.

use crate::dma_layout::{
    for_each_initial_dma_program, try_init_array, with_authorized_dma_layout, DmaIovas, DmaSlot,
    InitialDmaProgram, RX_SLOTS, TX_SLOTS,
};
use core::sync::atomic::{compiler_fence, Ordering};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

// ── Register offsets (BAR0, 128 KiB window) ──────────────────────────────────

const CTRL: usize = 0x0_0000;
const STATUS: usize = 0x0_0008;
const EERD: usize = 0x0_0014;
const MDIC: usize = 0x0_0020;
const ICR: usize = 0x0_00C0;
const IMC: usize = 0x0_00D8;
const RCTL: usize = 0x0_0100;
const TCTL: usize = 0x0_0400;
const TIPG: usize = 0x0_0410;
const MTA: usize = 0x0_5200;
const RAL0: usize = 0x0_5400;
const RAH0: usize = 0x0_5404;

// Queue 0 (the igb `_n` register bank).
const RDBAL0: usize = 0x0_C000;
const RDBAH0: usize = 0x0_C004;
const RDLEN0: usize = 0x0_C008;
const SRRCTL0: usize = 0x0_C00C;
const RDH0: usize = 0x0_C010;
const RDT0: usize = 0x0_C018;
const RXDCTL0: usize = 0x0_C028;
const TDBAL0: usize = 0x0_E000;
const TDBAH0: usize = 0x0_E004;
const TDLEN0: usize = 0x0_E008;
const TDH0: usize = 0x0_E010;
const TDT0: usize = 0x0_E018;
const TXDCTL0: usize = 0x0_E028;
const TDWBAL0: usize = 0x0_E038;
const TDWBAH0: usize = 0x0_E03C;

// ── Register bits ────────────────────────────────────────────────────────────

const CTRL_RST: u32 = 1 << 26;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_ASDE: u32 = 1 << 5;
const STATUS_LU: u32 = 1 << 1;
const EERD_START: u32 = 1 << 0;
const EERD_DONE: u32 = 1 << 1;
const EERD_ADDR_SHIFT: u32 = 2;
const EERD_DATA_SHIFT: u32 = 16;
const RCTL_EN: u32 = 1 << 1;
const RCTL_UPE: u32 = 1 << 3;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT: u32 = 0x0F << 4;
const TCTL_COLD: u32 = 0x40 << 12;
const RAH_AV: u32 = 1 << 31;
const RXDCTL_QUEUE_ENABLE: u32 = 1 << 25;
const TXDCTL_QUEUE_ENABLE: u32 = 1 << 25;
/// `SRRCTL.DESCTYPE = 001b` (advanced, one buffer) — the only descriptor layout
/// the igb model implements.
const SRRCTL_DESCTYPE_ADV_ONEBUF: u32 = 0x0200_0000;
/// `SRRCTL.BSIZEPKT` counts KiB (`2048 >> 10 == 2`).
const SRRCTL_BSIZEPKT_2048: u32 = 2;

const ADVTXD_DCMD_DEXT: u32 = 1 << 29;
const ADVTXD_DTYP_DATA: u32 = 0x0030_0000;
const TXD_CMD_EOP: u32 = 1 << 24;
const TXD_CMD_IFCS: u32 = 1 << 25;
const TXD_CMD_RS: u32 = 1 << 27;
const TXD_STAT_DD: u32 = 1;
const RXD_STAT_DD: u32 = 1;

// Management Data Interface Control (`MDIC`) — the PHY register window. The igb
// model, like the hardware, only ties `STATUS.LU` to PHY autonegotiation: after
// a global reset the PHY restarts from its power-on defaults and the link stays
// down until software restarts autoneg through here.
const MDIC_DATA_MASK: u32 = 0x0000_FFFF;
const MDIC_REG_SHIFT: u32 = 16;
const MDIC_PHY_SHIFT: u32 = 21;
const MDIC_OP_WRITE: u32 = 0x0400_0000;
/// PHY address 1 — the model rejects any other (`igb_set_mdic`).
const MDIC_PHY_ADDR: u32 = 1;
/// MII register 0 = Basic Mode Control Register.
const MII_BMCR: u32 = 0;
/// `BMCR = SPEED1000 | FULLDPLX | AUTOEN | ANRESTART` (hw/net/mii.h).
const MII_BMCR_SPEED1000: u32 = 1 << 6;
const MII_BMCR_FD: u32 = 1 << 8;
const MII_BMCR_ANRESTART: u32 = 1 << 9;
const MII_BMCR_AUTOEN: u32 = 1 << 12;

// ── Poll budgets ─────────────────────────────────────────────────────────────

/// Global-reset self-clear poll.
const RESET_POLLS: u32 = 1_000_000;
/// NVM read completion poll.
const NVM_POLLS: u32 = 100_000;
/// TX descriptor completion poll.
const TX_POLLS: u32 = 1_000_000;
/// Link-up poll. The igb model brings the link up only when the PHY autoneg
/// timer expires, ~500 ms after `restart_phy_autoneg`; the budget is several
/// times that so the wait is bounded but comfortably covers it.
const LINK_POLLS: u32 = 6_000_000;

const N_TX: usize = TX_SLOTS;
const N_RX: usize = RX_SLOTS;

/// Largest frame this cell moves in one descriptor (Ethernet MTU + headers).
pub const BUF_SIZE: usize = 2048;

// ── Descriptors ──────────────────────────────────────────────────────────────

/// Advanced transmit descriptor. `status` shares offset 12 with
/// `olinfo_status`: the model writes `TXD_STAT_DD` there after `RS`.
#[repr(C)]
struct TxDesc {
    buf_addr: u64,
    cmd_type_len: u32,
    status: u32,
}

/// Advanced receive descriptor. Programming uses `buf_addr` (offset 0); a
/// completion writes `status_error` (offset 8) and `length` (offset 12).
#[repr(C)]
struct RxDesc {
    buf_addr: u64,
    status_error: u32,
    length: u16,
    vlan: u16,
}

const _: () = assert!(core::mem::size_of::<TxDesc>() == 16);
const _: () = assert!(core::mem::size_of::<RxDesc>() == 16);

// ── Bounds-checked MMIO helpers ──────────────────────────────────────────────

#[inline]
fn rd(mmio: &MmioRegion, off: usize) -> ViResult<u32> {
    mmio.read_u32(off)
}

#[inline]
fn wr(mmio: &MmioRegion, off: usize, val: u32) -> ViResult<()> {
    mmio.write_u32(off, val)
}

/// Read one 16-bit NVM word through `EERD` (igb layout: address at bits 15:2,
/// done at bit 1, data at bits 31:16).
fn nvm_read(mmio: &MmioRegion, word: u16) -> ViResult<u16> {
    wr(mmio, EERD, ((word as u32) << EERD_ADDR_SHIFT) | EERD_START)?;
    for _ in 0..NVM_POLLS {
        let value = rd(mmio, EERD)?;
        if value & EERD_DONE != 0 {
            return Ok((value >> EERD_DATA_SHIFT) as u16);
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

/// Restart PHY autonegotiation, which is the only path to `STATUS.LU` on this
/// part. The global reset in `new` leaves the PHY at its power-on default and
/// clears the link, so a driver that skipped this would enable RX on a link the
/// controller reports as down and never receive a frame.
fn restart_phy_autoneg(mmio: &MmioRegion) -> ViResult<()> {
    let bmcr = MII_BMCR_SPEED1000 | MII_BMCR_FD | MII_BMCR_AUTOEN | MII_BMCR_ANRESTART;
    let command = (bmcr & MDIC_DATA_MASK)
        | (MII_BMCR << MDIC_REG_SHIFT)
        | (MDIC_PHY_ADDR << MDIC_PHY_SHIFT)
        | MDIC_OP_WRITE;
    wr(mmio, MDIC, command)?;
    // The model applies the write synchronously; the link then comes up when the
    // autoneg timer expires (~500 ms of guest time).
    let _ = rd(mmio, MDIC)?;
    Ok(())
}

// ── Controller state ─────────────────────────────────────────────────────────

pub struct IgbController {
    mmio: MmioRegion,
    tx_ring: DmaBuf,
    rx_ring: DmaBuf,
    tx_bufs: [DmaBuf; N_TX],
    rx_bufs: [DmaBuf; N_RX],
    dma_iovas: DmaIovas,
    tx_next: usize,
    rx_head: usize,
    /// MAC read from the NVM (words 0..3).
    pub mac: [u8; 6],
    /// Whether `STATUS.LU` was observed up before RX was enabled.
    pub link_up: bool,
}

// SAFETY: IgbController is only accessed from the single-threaded Cell event loop.
unsafe impl Send for IgbController {}

impl IgbController {
    fn rd32(&self, off: usize) -> ViResult<u32> {
        rd(&self.mmio, off)
    }

    fn wr32(&self, off: usize, val: u32) -> ViResult<()> {
        wr(&self.mmio, off, val)
    }

    /// Bring the controller up on `mmio` (BAR0) for the PCIe device at `bdf`.
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // 1. Quiesce inherited state. A restarted driver may otherwise inherit an
        //    enabled RX/TX pair and stale ring addresses.
        wr(&mmio, CTRL, CTRL_RST)?;
        let mut polls = 0u32;
        loop {
            if rd(&mmio, CTRL)? & CTRL_RST == 0 {
                break;
            }
            polls += 1;
            if polls > RESET_POLLS {
                return Err(ViError::IO);
            }
            core::hint::spin_loop();
        }

        // 2. Restart PHY autonegotiation. The reset left `STATUS.LU` cleared and
        //    the autoneg timer disarmed, so this is what brings the link back;
        //    without it RX is permanently refused by the controller.
        restart_phy_autoneg(&mmio)?;

        // 3. Force link up, mask interrupts, clear any latched cause.
        wr(&mmio, CTRL, CTRL_SLU | CTRL_ASDE)?;
        wr(&mmio, IMC, 0xFFFF_FFFF)?;
        let _ = rd(&mmio, ICR)?;

        // 4. MAC from the NVM, then the unicast filter (RAL0/RAH0), then the
        //    multicast table. RAL0/RAH0 also make an exact-match (non-promiscuous)
        //    frame pass, so the filter is real even though RCTL also opens the
        //    promiscuous paths for the DHCP broadcast.
        let mac_lo = nvm_read(&mmio, 0)?;
        let mac_mid = nvm_read(&mmio, 1)?;
        let mac_hi = nvm_read(&mmio, 2)?;
        let mac = [
            mac_lo as u8,
            (mac_lo >> 8) as u8,
            mac_mid as u8,
            (mac_mid >> 8) as u8,
            mac_hi as u8,
            (mac_hi >> 8) as u8,
        ];
        // A MAC of all zeros means the NVM read never completed; binding such a
        // device would publish an unusable NIC route, so fail before any DMA.
        if mac == [0u8; 6] {
            return Err(ViError::IO);
        }
        wr(&mmio, RAL0, ((mac_mid as u32) << 16) | mac_lo as u32)?;
        wr(&mmio, RAH0, RAH_AV | mac_hi as u32)?;
        for index in 0..128 {
            wr(&mmio, MTA + index * 4, 0)?;
        }

        // 4. Allocate every DMA object before authorization. No device-visible
        //    address is programmed until every object has an approved IOVA.
        let tx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?; // 16 × TxDesc
        let tx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;
        let rx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?; // 16 × RxDesc
        let rx_bufs = try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;

        with_authorized_dma_layout(
            (mmio, tx_ring, rx_ring, tx_bufs, rx_bufs),
            |resources, slot| {
                let result = match slot {
                    DmaSlot::TxRing => resources.1.authorize(bdf),
                    DmaSlot::TxBuffer(index) => resources.3[index].authorize(bdf),
                    DmaSlot::RxRing => resources.2.authorize(bdf),
                    DmaSlot::RxBuffer(index) => resources.4[index].authorize(bdf),
                };
                result.map_err(|_| ViError::PermissionDenied)
            },
            |(mmio, tx_ring, rx_ring, tx_bufs, rx_bufs), dma_iovas| {
                // SAFETY: both ring allocations cover one writable DMA page.
                unsafe {
                    core::ptr::write_bytes(tx_ring.virt(), 0, tx_ring.size());
                    core::ptr::write_bytes(rx_ring.virt(), 0, rx_ring.size());
                }

                let mut ctrl = IgbController {
                    mmio,
                    tx_ring,
                    rx_ring,
                    tx_bufs,
                    rx_bufs,
                    dma_iovas,
                    tx_next: 0,
                    rx_head: 0,
                    mac,
                    link_up: false,
                };

                let layout = ctrl.dma_iovas;
                for_each_initial_dma_program(&layout, |program| match program {
                    InitialDmaProgram::TxRingBase(iova) => {
                        let _ = ctrl.wr32(TDBAL0, iova as u32);
                        let _ = ctrl.wr32(TDBAH0, (iova >> 32) as u32);
                        let _ = ctrl.wr32(TDLEN0, (N_TX * 16) as u32);
                        let _ = ctrl.wr32(TDH0, 0);
                        let _ = ctrl.wr32(TDT0, 0);
                        // Descriptor write-back, not head-index write-back: the
                        // model writes DD into the descriptor only when
                        // TDWBAL0.0 is clear.
                        let _ = ctrl.wr32(TDWBAL0, 0);
                        let _ = ctrl.wr32(TDWBAH0, 0);
                        let _ = ctrl.wr32(TXDCTL0, TXDCTL_QUEUE_ENABLE);
                    }
                    InitialDmaProgram::RxDescriptor { slot, iova } => {
                        // SAFETY: rx_ring covers N_RX × RxDesc; slot < N_RX.
                        unsafe {
                            let desc = (ctrl.rx_ring.virt() as *mut RxDesc).add(slot);
                            core::ptr::write_volatile(&mut (*desc).buf_addr, iova);
                            core::ptr::write_volatile(&mut (*desc).status_error, 0);
                            core::ptr::write_volatile(&mut (*desc).length, 0);
                            core::ptr::write_volatile(&mut (*desc).vlan, 0);
                        }
                    }
                    InitialDmaProgram::RxRingBase(iova) => {
                        let _ = ctrl.wr32(SRRCTL0, SRRCTL_DESCTYPE_ADV_ONEBUF | SRRCTL_BSIZEPKT_2048);
                        let _ = ctrl.wr32(RDBAL0, iova as u32);
                        let _ = ctrl.wr32(RDBAH0, (iova >> 32) as u32);
                        let _ = ctrl.wr32(RDLEN0, (N_RX * 16) as u32);
                        let _ = ctrl.wr32(RDH0, 0);
                        let _ = ctrl.wr32(RXDCTL0, RXDCTL_QUEUE_ENABLE);
                        let _ = ctrl.wr32(RDT0, (N_RX - 1) as u32);
                    }
                    InitialDmaProgram::Enable => {
                        let _ = ctrl.wr32(TIPG, 0x0060_200A);
                        let _ = ctrl.wr32(TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD);
                        // Link must be up before RCTL_EN means anything to the
                        // model (`e1000x_rx_ready` requires STATUS.LU).
                        let link = ctrl.wait_link_up().unwrap_or(false);
                        ctrl.link_up = link;
                        let _ = ctrl
                            .wr32(RCTL, RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SECRC);
                    }
                });

                Ok(ctrl)
            },
        )?
    }

    /// Poll `STATUS.LU` for a bounded window. Returns whether link is up; a
    /// timeout is not fatal (the model brings link up on its own schedule) but
    /// the caller reports it.
    pub fn wait_link_up(&self) -> ViResult<bool> {
        for _ in 0..LINK_POLLS {
            if self.rd32(STATUS)? & STATUS_LU != 0 {
                return Ok(true);
            }
            core::hint::spin_loop();
        }
        Ok(false)
    }

    /// Transmit `frame` (Ethernet frame without FCS), polled until the
    /// descriptor reports done.
    pub fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidInput);
        }

        let slot = self.tx_next;
        self.tx_next = (slot + 1) % N_TX;

        // SAFETY: tx_bufs[slot].virt() is valid DMA memory of size BUF_SIZE, and
        // frame.len() <= BUF_SIZE.
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx_bufs[slot].virt(), frame.len());
        }
        let iova = self.dma_iovas.tx_descriptor_iova(slot);
        let cmd = ADVTXD_DCMD_DEXT
            | ADVTXD_DTYP_DATA
            | TXD_CMD_EOP
            | TXD_CMD_IFCS
            | TXD_CMD_RS
            | frame.len() as u32;

        // SAFETY: tx_ring covers N_TX × TxDesc; slot < N_TX.
        unsafe {
            let desc = (self.tx_ring.virt() as *mut TxDesc).add(slot);
            core::ptr::write_volatile(&mut (*desc).buf_addr, iova);
            core::ptr::write_volatile(&mut (*desc).cmd_type_len, cmd);
            core::ptr::write_volatile(&mut (*desc).status, 0);
        }
        compiler_fence(Ordering::Release);
        self.wr32(TDT0, self.tx_next as u32)?;

        for _ in 0..TX_POLLS {
            // SAFETY: tx_ring[slot] is valid DMA memory.
            let status = unsafe {
                core::ptr::read_volatile(&(*((self.tx_ring.virt() as *const TxDesc).add(slot))).status)
            };
            if status & TXD_STAT_DD != 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(ViError::IO)
    }

    /// Poll for a received frame; copies into `out_buf`. Returns the number of
    /// bytes written, or 0 when nothing is ready.
    pub fn recv_frame(&mut self, out_buf: &mut [u8]) -> usize {
        let head = self.rx_head;
        // SAFETY: rx_ring[head] is valid DMA memory.
        let (done, length) = unsafe {
            let desc = (self.rx_ring.virt() as *const RxDesc).add(head);
            let status_error = core::ptr::read_volatile(&(*desc).status_error);
            let length = core::ptr::read_volatile(&(*desc).length);
            (status_error & RXD_STAT_DD != 0, length as usize)
        };
        if !done {
            return 0;
        }

        let copy_len = length.min(out_buf.len()).min(BUF_SIZE);
        // SAFETY: rx_bufs[head].virt() holds the received frame, and copy_len is
        // bounded by both the frame length and the caller's slice.
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.rx_bufs[head].virt(),
                out_buf.as_mut_ptr(),
                copy_len,
            );
        }

        // Recycle the descriptor: restore the buffer address, clear the
        // completion fields, and hand the slot back to the controller.
        let iova = self.dma_iovas.rx_descriptor_iova(head);
        // SAFETY: rx_ring[head] and rx_bufs[head] are valid DMA memory.
        unsafe {
            let desc = (self.rx_ring.virt() as *mut RxDesc).add(head);
            core::ptr::write_volatile(&mut (*desc).buf_addr, iova);
            core::ptr::write_volatile(&mut (*desc).status_error, 0);
            core::ptr::write_volatile(&mut (*desc).length, 0);
            core::ptr::write_volatile(&mut (*desc).vlan, 0);
        }
        self.rx_head = (head + 1) % N_RX;
        let _ = self.wr32(RDT0, head as u32);
        copy_len
    }
}
