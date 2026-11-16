//! Intel X710/XL710 controller initialization and Tx/Rx data path.

use crate::aq::{self, post_aq_command, AdminQueue, AQ_SLOTS};
use crate::identity::{self, Identity, QUERY_DEVICE_IDS, VENDOR_INTEL};
use crate::regs::*;
use crate::rings::{self, RxDesc, TxDesc, BUF_SIZE, RX_SLOTS, TX_SLOTS};
use core::sync::atomic::{compiler_fence, Ordering};
use nic_driver::dma::{DmaSet, Iovas, Layout, Position};
use nic_driver::ring::{DescRing, FrameBufs};
use nic_driver::{MacFmt, NicDevice, Probe};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

const I40E_BAR0_LEN: usize = 0x20_0000;
const I40E_MMIO_MIN: usize = 0x0013_0000;
const RESET_POLLS: u32 = 1_000_000;

/// Extra DMA slots, authorized before the rings: admin send queue, admin
/// receive queue, admin command buffer.
const ATQ: usize = 0;
const ARQ: usize = 1;
const AQ_BUFFER: usize = 2;
const LAYOUT: Layout<3> = Layout::new([Position::Before; 3]);

pub struct I40eController {
    mmio: MmioRegion,
    atq: AdminQueue,
    /// Programmed into `PF_ARQ*`; the cell does not consume events yet.
    #[allow(dead_code)]
    arq: DmaBuf,
    aq_buffer: DmaBuf,
    tx_ring: DescRing<TxDesc, TX_SLOTS>,
    tx_bufs: FrameBufs<TX_SLOTS>,
    rx_ring: DescRing<RxDesc, RX_SLOTS>,
    rx_bufs: FrameBufs<RX_SLOTS>,
    iovas: Iovas<TX_SLOTS, RX_SLOTS, 3>,
    tx_next: usize,
    rx_head: usize,
    atq_head: usize,
    mac: [u8; 6],
    link_up: bool,
}

impl NicDevice for I40eController {
    const TAG: &'static str = "[i40e]";
    const VENDOR: u16 = VENDOR_INTEL;
    const QUERY_IDS: &'static [u16] = &QUERY_DEVICE_IDS;
    const ABSENT_LINE: Option<&'static str> =
        Some("no X710/XL710 controller present; driver cell idle");
    type Identity = Identity;
    type Error = ViError;

    fn classify(vendor: u16, device: u16) -> Option<Identity> {
        identity::classify(vendor, device)
    }

    fn identity_name(id: &Identity) -> &'static str {
        id.name
    }

    fn init(probe: &Probe, _id: Identity) -> ViResult<Self> {
        let mmio = probe.claim_bar0(I40E_BAR0_LEN, I40E_MMIO_MIN);
        Self::new(mmio, probe.bdf())
    }

    fn bind_detail(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "link_up={} mac={}", self.link_up, MacFmt(self.mac))
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn send(&mut self, frame: &[u8]) -> ViResult<()> {
        self.send_frame(frame)
    }

    fn recv(&mut self, out: &mut [u8]) -> usize {
        self.poll_rx(out).unwrap_or_default()
    }
}

impl I40eController {
    fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // 1. Quiesce & reset controller.
        mmio.write_u32(GLGEN_RST, GLGEN_RST_DEV_ARM)?;
        for _ in 0..RESET_POLLS {
            if mmio.read_u32(GLGEN_RST)? & GLGEN_RST_DEV_ARM == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // 2. Allocate and authorize every DMA object (admin queues first);
        //    nothing is programmed before this returns.
        let DmaSet {
            tx_ring,
            tx_bufs,
            rx_ring,
            rx_bufs,
            extra: [atq, arq, aq_buffer],
            iovas,
        } = DmaSet::alloc_authorized(&LAYOUT, bdf)?;
        let mut ctrl = I40eController {
            mmio,
            atq: DescRing::new(atq)?,
            arq,
            aq_buffer,
            tx_ring,
            tx_bufs,
            rx_ring,
            rx_bufs,
            iovas,
            tx_next: 0,
            rx_head: 0,
            atq_head: 0,
            mac: [0; 6],
            link_up: false,
        };
        ctrl.program_rings()?;
        ctrl.init_admin_queue()?;
        ctrl.query_mac_and_link()?;
        Ok(ctrl)
    }

    /// Initial DMA programming: admin send queue, admin receive queue, then
    /// every RX descriptor, all from authorized IOVAs.
    fn program_rings(&mut self) -> ViResult<()> {
        let iovas = self.iovas;
        let m = &self.mmio;
        let atq = iovas.extra[ATQ];
        m.write_u32(PF_ATQBAL, atq as u32)?;
        m.write_u32(PF_ATQBAH, (atq >> 32) as u32)?;
        m.write_u32(PF_ATQLEN, (AQ_SLOTS as u32) | PF_ATQLEN_ENABLE)?;
        m.write_u32(PF_ATQH, 0)?;
        m.write_u32(PF_ATQT, 0)?;
        let arq = iovas.extra[ARQ];
        m.write_u32(PF_ARQBAL, arq as u32)?;
        m.write_u32(PF_ARQBAH, (arq >> 32) as u32)?;
        m.write_u32(PF_ARQLEN, (AQ_SLOTS as u32) | PF_ARQLEN_ENABLE)?;
        m.write_u32(PF_ARQH, 0)?;
        m.write_u32(PF_ARQT, 0)?;
        for (index, &iova) in iovas.rx_bufs.iter().enumerate() {
            self.rx_ring.write(
                index,
                RxDesc {
                    pkt_addr: iova,
                    hdr_addr: 0,
                },
            );
        }
        Ok(())
    }

    fn init_admin_queue(&mut self) -> ViResult<()> {
        let cmd = aq::format_get_version();
        let resp = post_aq_command(&self.mmio, &self.atq, self.atq_head, cmd)?;
        self.atq_head = (self.atq_head + 1) % AQ_SLOTS;
        // Verify API major version is 1.
        let api_major = (resp.addr_high >> 16) as u16;
        if api_major != 1 && api_major != 0 {
            return Err(ViError::NotSupported);
        }
        Ok(())
    }

    fn query_mac_and_link(&mut self) -> ViResult<()> {
        let iova = self.iovas.extra[AQ_BUFFER];
        let cmd = aq::format_mac_read(iova, 24);
        let _ = post_aq_command(&self.mmio, &self.atq, self.atq_head, cmd)?;
        self.atq_head = (self.atq_head + 1) % AQ_SLOTS;

        let mut mac = [0u8; 6];
        self.aq_buffer.read_bytes(0, &mut mac);
        if mac == [0u8; 6] {
            return Err(ViError::IO);
        }
        self.mac = mac;

        let cmd = aq::format_get_link_status();
        let resp = post_aq_command(&self.mmio, &self.atq, self.atq_head, cmd)?;
        self.atq_head = (self.atq_head + 1) % AQ_SLOTS;
        self.link_up = resp.param0 & 1 != 0;
        Ok(())
    }

    fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidArgument);
        }
        let slot = self.tx_next;
        self.tx_bufs.copy_in(slot, frame)?;
        compiler_fence(Ordering::Release);

        let iova = self.iovas.tx_bufs[slot];
        self.tx_ring.write(
            slot,
            TxDesc {
                buffer_addr: iova,
                cmd_type_offset_bsz: (rings::I40E_TXD_QW1_CMD_EOP | rings::I40E_TXD_QW1_CMD_RS)
                    | ((frame.len() as u64) << 34),
            },
        );
        compiler_fence(Ordering::Release);
        self.tx_next = (slot + 1) % TX_SLOTS;
        self.mmio.write_u32(QTX_TAIL, self.tx_next as u32)?;
        Ok(())
    }

    fn poll_rx(&mut self, target: &mut [u8]) -> ViResult<usize> {
        let slot = self.rx_head;
        let len = self.rx_bufs.copy_out(slot, target, BUF_SIZE);
        self.rx_head = (slot + 1) % RX_SLOTS;
        self.mmio.write_u32(QRX_TAIL, self.rx_head as u32)?;
        Ok(len)
    }
}
