//! Intel e1000 (82540EM) NIC controller logic ported from nic_e1000.rs.
//!
//! Law 4 exception: Driver Cells may use `unsafe` for MMIO register access.
//! Every `unsafe` block carries a `// SAFETY:` comment.

use core::sync::atomic::{compiler_fence, Ordering};
use ostd::mmio::MmioRegion;
use ostd::dma::DmaBuf;
use types::{ViError, ViResult};

// ── Register offsets (BAR0 MMIO) ─────────────────────────────────────────────

const CTRL:  usize = 0x0000;
const ICR:   usize = 0x00C0;
const IMC:   usize = 0x00D8;
const RCTL:  usize = 0x0100;
const TCTL:  usize = 0x0400;
const TIPG:  usize = 0x0410;
const RDBAL: usize = 0x2800;
const RDBAH: usize = 0x2804;
const RDLEN: usize = 0x2808;
const RDH:   usize = 0x2810;
const RDT:   usize = 0x2818;
const TDBAL: usize = 0x3800;
const TDBAH: usize = 0x3804;
const TDLEN: usize = 0x3808;
const TDH:   usize = 0x3810;
const TDT:   usize = 0x3818;
const RAL0:  usize = 0x5400;
const RAH0:  usize = 0x5404;
const MTA:   usize = 0x5200;
const EERD:  usize = 0x0014;

// ── Register constants ────────────────────────────────────────────────────────

const CTRL_RST:  u32 = 1 << 26;
const CTRL_SLU:  u32 = 1 << 6;
const CTRL_ASDE: u32 = 1 << 5;
const RCTL_EN:   u32 = 1 << 1;
const RCTL_UPE:  u32 = 1 << 3;
const RCTL_MPE:  u32 = 1 << 4;
const RCTL_BAM:  u32 = 1 << 15;
const RCTL_SECRC:u32 = 1 << 26;
const TCTL_EN:   u32 = 1 << 1;
const TCTL_PSP:  u32 = 1 << 3;
const TCTL_CT:   u32 = 0x0F << 4;
const TCTL_COLD: u32 = 0x40 << 12;
const CMD_EOP:   u8  = 0x01;
const CMD_IFCS:  u8  = 0x02;
const CMD_RS:    u8  = 0x08;
const STATUS_DD: u8  = 0x01;
const EERD_START:u32 = 1;
const EERD_DONE: u32 = 1 << 4;
const RAH_AV:    u32 = 1 << 31;

const N_TX: usize = 16;
const N_RX: usize = 16;
pub const BUF_SIZE: usize = 2048;

// ── Descriptor types ──────────────────────────────────────────────────────────

#[repr(C)]
struct TxDesc {
    buf_addr: u64,
    length:   u16,
    cso:      u8,
    cmd:      u8,
    status:   u8,
    css:      u8,
    special:  u16,
}

#[repr(C)]
struct RxDesc {
    buf_addr: u64,
    length:   u16,
    checksum: u16,
    status:   u8,
    errors:   u8,
    special:  u16,
}

const _: () = assert!(core::mem::size_of::<TxDesc>() == 16);
const _: () = assert!(core::mem::size_of::<RxDesc>() == 16);

// ── Controller state ──────────────────────────────────────────────────────────

pub struct E1000Controller {
    mmio:    MmioRegion,
    tx_ring: DmaBuf,
    rx_ring: DmaBuf,
    tx_bufs: [DmaBuf; N_TX],
    rx_bufs: [DmaBuf; N_RX],
    tx_next: usize,
    rx_head: usize,
    pub mac: [u8; 6],
}

// SAFETY: E1000Controller is only accessed from the single-threaded Cell event loop.
unsafe impl Send for E1000Controller {}

impl E1000Controller {
    fn rd32(&self, off: usize) -> u32 {
        // SAFETY: mmio region was granted by kernel; volatile prevents register caching.
        unsafe { core::ptr::read_volatile((self.mmio.base() + off) as *const u32) }
    }

    fn wr32(&self, off: usize, val: u32) {
        // SAFETY: same contract as rd32.
        unsafe { core::ptr::write_volatile((self.mmio.base() + off) as *mut u32, val) };
    }

    fn eeprom_read(&self, addr: u8) -> u16 {
        self.wr32(EERD, ((addr as u32) << 8) | EERD_START);
        let mut n = 0u32;
        loop {
            let v = self.rd32(EERD);
            if v & EERD_DONE != 0 { return (v >> 16) as u16; }
            n += 1;
            if n > 1_000_000 { break; }
            core::hint::spin_loop();
        }
        0
    }

    /// Initialise the e1000 controller reachable via `mmio` (BAR0).
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // Soft-reset.
        // SAFETY: CTRL is at offset 0 in the granted BAR0 MMIO region.
        unsafe { core::ptr::write_volatile((mmio.base() + CTRL) as *mut u32, CTRL_RST) };
        for _ in 0..10_000 { core::hint::spin_loop(); }
        let mut n = 0u32;
        loop {
            // SAFETY: CTRL is within the granted BAR0 region.
            if unsafe { core::ptr::read_volatile((mmio.base() + CTRL) as *const u32) }
                & CTRL_RST == 0 { break; }
            n += 1;
            if n > 1_000_000 { return Err(ViError::IO); }
            core::hint::spin_loop();
        }

        // Allocate TX ring + buffers.
        let tx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?; // holds 16×TxDesc = 256B
        let _ = tx_ring.authorize(bdf);
        unsafe { core::ptr::write_bytes(tx_ring.virt(), 0, tx_ring.size()) };

        // SAFETY: N_TX = 16 — safe to init array slot-by-slot with MaybeUninit.
        let tx_bufs = core::array::from_fn(|_| {
            let b = DmaBuf::alloc(1).expect("e1000 tx DmaBuf OOM");
            let _ = b.authorize(bdf);
            b
        });

        let rx_ring = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        let _ = rx_ring.authorize(bdf);
        unsafe { core::ptr::write_bytes(rx_ring.virt(), 0, rx_ring.size()) };

        let rx_bufs = core::array::from_fn(|_| {
            let b = DmaBuf::alloc(1).expect("e1000 rx DmaBuf OOM");
            let _ = b.authorize(bdf);
            b
        });

        let mut ctrl = E1000Controller {
            mmio, tx_ring, rx_ring, tx_bufs, rx_bufs,
            tx_next: 0, rx_head: 0, mac: [0u8; 6],
        };

        // Link-up + disable interrupts.
        ctrl.wr32(CTRL, CTRL_SLU | CTRL_ASDE);
        ctrl.wr32(IMC, 0xFFFF_FFFF);
        let _ = ctrl.rd32(ICR);

        // Read MAC from EEPROM.
        let mac_lo = ctrl.eeprom_read(0);
        let mac_hi = ctrl.eeprom_read(1);
        let mac_ex = ctrl.eeprom_read(2);
        ctrl.mac = [
            mac_lo as u8, (mac_lo >> 8) as u8,
            mac_hi as u8, (mac_hi >> 8) as u8,
            mac_ex as u8, (mac_ex >> 8) as u8,
        ];
        ctrl.wr32(RAL0, (mac_hi as u32) << 16 | mac_lo as u32);
        ctrl.wr32(RAH0, RAH_AV | mac_ex as u32);

        // Zero multicast table.
        for i in 0..128 { ctrl.wr32(MTA + i * 4, 0); }

        // Program TX ring.
        let tx_phys = ctrl.tx_ring.phys() as u64;
        ctrl.wr32(TDBAL, tx_phys as u32);
        ctrl.wr32(TDBAH, (tx_phys >> 32) as u32);
        ctrl.wr32(TDLEN, (N_TX * 16) as u32);
        ctrl.wr32(TDH, 0);
        ctrl.wr32(TDT, 0);

        // Fill RX descriptors.
        for i in 0..N_RX {
            let phys = ctrl.rx_bufs[i].phys() as u64;
            // SAFETY: rx_ring covers N_RX×16 bytes; i < N_RX.
            unsafe {
                let desc = (ctrl.rx_ring.virt() as *mut RxDesc).add(i);
                core::ptr::write_volatile(&mut (*desc).buf_addr, phys);
                core::ptr::write_volatile(&mut (*desc).status, 0);
            }
        }
        let rx_phys = ctrl.rx_ring.phys() as u64;
        ctrl.wr32(RDBAL, rx_phys as u32);
        ctrl.wr32(RDBAH, (rx_phys >> 32) as u32);
        ctrl.wr32(RDLEN, (N_RX * 16) as u32);
        ctrl.wr32(RDH, 0);
        ctrl.wr32(RDT, (N_RX - 1) as u32);

        // Enable TX + RX.
        ctrl.wr32(TIPG, 0x0060_200A);
        ctrl.wr32(TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD);
        ctrl.wr32(RCTL, RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SECRC);

        Ok(ctrl)
    }

    /// Transmit `frame` (Ethernet + payload, no FCS).
    ///
    /// Blocks (polled) until the descriptor's DD bit is set.
    pub fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.len() > BUF_SIZE { return Err(ViError::InvalidInput); }

        let slot = self.tx_next;
        self.tx_next = (slot + 1) % N_TX;

        // Copy into DMA buffer.
        // SAFETY: tx_bufs[slot].virt() is valid DMA memory of size BUF_SIZE.
        unsafe {
            core::ptr::copy_nonoverlapping(
                frame.as_ptr(), self.tx_bufs[slot].virt(), frame.len(),
            );
        }
        let phys = self.tx_bufs[slot].phys() as u64;

        // SAFETY: tx_ring covers N_TX×TxDesc; slot < N_TX.
        unsafe {
            let desc = (self.tx_ring.virt() as *mut TxDesc).add(slot);
            core::ptr::write_volatile(&mut (*desc).buf_addr, phys);
            core::ptr::write_volatile(&mut (*desc).length, frame.len() as u16);
            core::ptr::write_volatile(&mut (*desc).cso, 0);
            core::ptr::write_volatile(&mut (*desc).cmd, CMD_EOP | CMD_IFCS | CMD_RS);
            core::ptr::write_volatile(&mut (*desc).status, 0);
            core::ptr::write_volatile(&mut (*desc).css, 0);
            core::ptr::write_volatile(&mut (*desc).special, 0);
        }
        compiler_fence(Ordering::Release);
        self.wr32(TDT, self.tx_next as u32);

        // Poll for completion (DD bit in descriptor status).
        let mut iters = 0u32;
        loop {
            // SAFETY: tx_ring[slot] is valid DMA memory.
            let dd = unsafe {
                core::ptr::read_volatile(&(*((self.tx_ring.virt() as *const TxDesc).add(slot))).status)
            };
            if dd & STATUS_DD != 0 { break; }
            iters += 1;
            if iters > 1_000_000 { return Err(ViError::IO); }
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Poll for a received frame; copies into `out_buf`.
    ///
    /// Returns the number of bytes written, or 0 if no frame is ready.
    pub fn recv_frame(&mut self, out_buf: &mut [u8]) -> usize {
        let head = self.rx_head;
        // SAFETY: rx_ring[head] is valid DMA memory.
        let (dd, len) = unsafe {
            let desc = (self.rx_ring.virt() as *const RxDesc).add(head);
            let status = core::ptr::read_volatile(&(*desc).status);
            let length = core::ptr::read_volatile(&(*desc).length);
            (status & STATUS_DD != 0, length as usize)
        };
        if !dd { return 0; }

        let copy_len = len.min(out_buf.len()).min(BUF_SIZE);
        // SAFETY: rx_bufs[head].virt() holds `len` bytes of received frame data.
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.rx_bufs[head].virt(), out_buf.as_mut_ptr(), copy_len,
            );
        }

        // Recycle descriptor: clear status, give back to hardware.
        // SAFETY: rx_ring[head] and rx_bufs[head] are valid DMA memory.
        unsafe {
            let desc = (self.rx_ring.virt() as *mut RxDesc).add(head);
            let phys = self.rx_bufs[head].phys() as u64;
            core::ptr::write_volatile(&mut (*desc).buf_addr, phys);
            core::ptr::write_volatile(&mut (*desc).status, 0);
        }
        self.rx_head = (head + 1) % N_RX;
        self.wr32(RDT, head as u32); // give the slot back to hardware
        copy_len
    }
}
