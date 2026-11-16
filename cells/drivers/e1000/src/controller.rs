//! Intel e1000 (82540EM / 82545EM) NIC controller logic ported from nic_e1000.rs.
//!
//! MMIO goes through the bounds-checked [`MmioRegion`] and DMA rings and frame
//! buffers through `nic_driver::{dma, ring}`, so this file holds no `unsafe`.

use crate::identity::{self, sku_name, E1000Sku, QUERY_DEVICE_IDS, VENDOR_INTEL};
use core::sync::atomic::{compiler_fence, Ordering};
use nic_driver::dma::{DmaSet, Layout};
use nic_driver::{MacFmt, NicDevice, Probe};
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

// ── Register offsets (BAR0 MMIO) ─────────────────────────────────────────────

const CTRL: usize = 0x0000;
const ICR: usize = 0x00C0;
const IMC: usize = 0x00D8;
const RCTL: usize = 0x0100;
const TCTL: usize = 0x0400;
const TIPG: usize = 0x0410;
const RDBAL: usize = 0x2800;
const RDBAH: usize = 0x2804;
const RDLEN: usize = 0x2808;
const RDH: usize = 0x2810;
const RDT: usize = 0x2818;
const TDBAL: usize = 0x3800;
const TDBAH: usize = 0x3804;
const TDLEN: usize = 0x3808;
const TDH: usize = 0x3810;
const TDT: usize = 0x3818;
const RAL0: usize = 0x5400;
const RAH0: usize = 0x5404;
const MTA: usize = 0x5200;
const EERD: usize = 0x0014;

/// BAR0 window — e1000 register space is 128 KiB.
const E1000_BAR0_LEN: usize = 0x2_0000;
/// Highest register this cell touches (`RAH0`) plus one word.
const E1000_MMIO_MIN: usize = RAH0 + 4;

// ── Register constants ──────────────────── ────────────────────────────────────

const CTRL_RST: u32 = 1 << 26;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_ASDE: u32 = 1 << 5;
const RCTL_EN: u32 = 1 << 1;
const RCTL_UPE: u32 = 1 << 3;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT: u32 = 0x0F << 4;
const TCTL_COLD: u32 = 0x40 << 12;
const CMD_EOP: u8 = 0x01;
const CMD_IFCS: u8 = 0x02;
const CMD_RS: u8 = 0x08;
const STATUS_DD: u8 = 0x01;
const EERD_START: u32 = 1;
const EERD_DONE: u32 = 1 << 4;
const RAH_AV: u32 = 1 << 31;

const N_TX: usize = 16;
const N_RX: usize = 16;
const BUF_SIZE: usize = 2048;

// ── Descriptor types ──────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
struct TxDesc {
    buf_addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RxDesc {
    buf_addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

impl RxDesc {
    /// A descriptor handed to the controller with buffer `iova`.
    const fn posted(iova: u64) -> Self {
        Self {
            buf_addr: iova,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            special: 0,
        }
    }
}

const _: () = assert!(core::mem::size_of::<TxDesc>() == 16);
const _: () = assert!(core::mem::size_of::<RxDesc>() == 16);

// ── Controller state ──────────────────────────────────────────────────────────

pub struct E1000Controller {
    mmio: MmioRegion,
    dma: DmaSet<TxDesc, RxDesc, N_TX, N_RX, 0>,
    tx_next: usize,
    rx_head: usize,
    mac: [u8; 6],
}

impl NicDevice for E1000Controller {
    const TAG: &'static str = "[e1000]";
    const VENDOR: u16 = VENDOR_INTEL;
    const QUERY_IDS: &'static [u16] = &QUERY_DEVICE_IDS;
    /// No 8254x on this platform: exit silently. The `[e1000]` tag stays
    /// reserved for this cell's own device, so sibling NIC lanes can assert
    /// that it never touched theirs.
    const ABSENT_LINE: Option<&'static str> = None;
    type Identity = E1000Sku;
    type Error = ViError;

    fn classify(vendor: u16, device: u16) -> Option<E1000Sku> {
        match identity::classify(vendor, device) {
            E1000Sku::Unsupported => None,
            sku => Some(sku),
        }
    }

    fn identity_name(sku: &E1000Sku) -> &'static str {
        sku_name(*sku)
    }

    fn init(probe: &Probe, _sku: E1000Sku) -> ViResult<Self> {
        let mmio = probe.claim_bar0(E1000_BAR0_LEN, E1000_MMIO_MIN);
        Self::new(mmio, probe.bdf())
    }

    /// RX is request-driven (the net service asks for frames), so the cell
    /// stays polled.
    fn bind_detail(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "mac={} irq=polled reason=request-driven",
            MacFmt(self.mac)
        )
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn send(&mut self, frame: &[u8]) -> ViResult<()> {
        self.send_frame(frame)
    }

    fn recv(&mut self, out: &mut [u8]) -> usize {
        self.recv_frame(out)
    }
}

impl E1000Controller {
    fn rd32(&self, off: usize) -> ViResult<u32> {
        self.mmio.read_u32(off)
    }

    fn wr32(&self, off: usize, val: u32) -> ViResult<()> {
        self.mmio.write_u32(off, val)
    }

    /// One EEPROM word; 0 when the read never completes.
    fn eeprom_read(&self, addr: u8) -> ViResult<u16> {
        self.wr32(EERD, ((addr as u32) << 8) | EERD_START)?;
        let mut n = 0u32;
        loop {
            let v = self.rd32(EERD)?;
            if v & EERD_DONE != 0 {
                return Ok((v >> 16) as u16);
            }
            n += 1;
            if n > 1_000_000 {
                break;
            }
            core::hint::spin_loop();
        }
        Ok(0)
    }

    /// Initialise the e1000 controller reachable via `mmio` (BAR0).
    fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // Quiesce inherited device state before publishing fresh DMA mappings.
        // A restarted driver may otherwise inherit enabled TX/RX and stale rings.
        mmio.write_u32(CTRL, CTRL_RST)?;
        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
        let mut n = 0u32;
        loop {
            if mmio.read_u32(CTRL)? & CTRL_RST == 0 {
                break;
            }
            n += 1;
            if n > 1_000_000 {
                return Err(ViError::IO);
            }
            core::hint::spin_loop();
        }

        // Allocate and authorize every DMA object. No device-visible DMA
        // address is programmed until every object has an approved IOVA.
        // Any error from this constructor is terminal: DmaBuf has no Drop
        // cleanup, so task teardown deliberately reclaims the Cell's grants.
        let dma = DmaSet::alloc_authorized(&Layout::PLAIN, bdf)?;
        let mut ctrl = E1000Controller {
            mmio,
            dma,
            tx_next: 0,
            rx_head: 0,
            mac: [0u8; 6],
        };

        // Link-up + disable interrupts.
        ctrl.wr32(CTRL, CTRL_SLU | CTRL_ASDE)?;
        ctrl.wr32(IMC, 0xFFFF_FFFF)?;
        let _ = ctrl.rd32(ICR)?;

        // Read MAC from EEPROM.
        let mac_lo = ctrl.eeprom_read(0)?;
        let mac_hi = ctrl.eeprom_read(1)?;
        let mac_ex = ctrl.eeprom_read(2)?;
        ctrl.mac = [
            mac_lo as u8,
            (mac_lo >> 8) as u8,
            mac_hi as u8,
            (mac_hi >> 8) as u8,
            mac_ex as u8,
            (mac_ex >> 8) as u8,
        ];
        ctrl.wr32(RAL0, (mac_hi as u32) << 16 | mac_lo as u32)?;
        ctrl.wr32(RAH0, RAH_AV | mac_ex as u32)?;

        // Zero multicast table.
        for i in 0..128 {
            ctrl.wr32(MTA + i * 4, 0)?;
        }

        ctrl.program_rings()?;
        Ok(ctrl)
    }

    /// The sole initial device-address and TX/RX-enablement path, in order:
    /// TX ring base, every RX descriptor, RX ring base, enable. Every address
    /// is an authorized IOVA.
    fn program_rings(&mut self) -> ViResult<()> {
        let iovas = self.dma.iovas;
        self.wr32(TDBAL, iovas.tx_ring as u32)?;
        self.wr32(TDBAH, (iovas.tx_ring >> 32) as u32)?;
        self.wr32(TDLEN, (N_TX * 16) as u32)?;
        self.wr32(TDH, 0)?;
        self.wr32(TDT, 0)?;

        for (slot, &iova) in iovas.rx_bufs.iter().enumerate() {
            self.dma.rx_ring.write(slot, RxDesc::posted(iova));
        }

        self.wr32(RDBAL, iovas.rx_ring as u32)?;
        self.wr32(RDBAH, (iovas.rx_ring >> 32) as u32)?;
        self.wr32(RDLEN, (N_RX * 16) as u32)?;
        self.wr32(RDH, 0)?;
        self.wr32(RDT, (N_RX - 1) as u32)?;

        self.wr32(TIPG, 0x0060_200A)?;
        self.wr32(TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD)?;
        self.wr32(RCTL, RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SECRC)
    }

    /// Transmit `frame` (Ethernet + payload, no FCS).
    ///
    /// Blocks (polled) until the descriptor's DD bit is set.
    fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.len() > BUF_SIZE {
            return Err(ViError::InvalidInput);
        }

        let slot = self.tx_next;
        self.dma.tx_bufs.copy_in(slot, frame)?;
        self.tx_next = (slot + 1) % N_TX;

        self.dma.tx_ring.write(
            slot,
            TxDesc {
                buf_addr: self.dma.iovas.tx_bufs[slot],
                length: frame.len() as u16,
                cso: 0,
                cmd: CMD_EOP | CMD_IFCS | CMD_RS,
                status: 0,
                css: 0,
                special: 0,
            },
        );
        compiler_fence(Ordering::Release);
        self.wr32(TDT, self.tx_next as u32)?;

        // Poll for completion (DD bit in descriptor status).
        let mut iters = 0u32;
        loop {
            if self.dma.tx_ring.read(slot).status & STATUS_DD != 0 {
                break;
            }
            iters += 1;
            if iters > 1_000_000 {
                return Err(ViError::IO);
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Poll for a received frame; copies into `out_buf`.
    ///
    /// Returns the number of bytes written, or 0 if no frame is ready.
    fn recv_frame(&mut self, out_buf: &mut [u8]) -> usize {
        let head = self.rx_head;
        let desc = self.dma.rx_ring.read(head);
        if desc.status & STATUS_DD == 0 {
            return 0;
        }

        let len = (desc.length as usize).min(BUF_SIZE);
        let copy_len = self.dma.rx_bufs.copy_out(head, out_buf, len);

        // Recycle descriptor: clear status, give back to hardware.
        self.dma
            .rx_ring
            .write(head, RxDesc::posted(self.dma.iovas.rx_bufs[head]));
        self.rx_head = (head + 1) % N_RX;
        let _ = self.wr32(RDT, head as u32); // give the slot back to hardware
        copy_len
    }
}
