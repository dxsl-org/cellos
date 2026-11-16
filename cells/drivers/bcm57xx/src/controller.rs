//! Broadcom BCM57xx controller bring-up, APE handshake, and data path.

use crate::ape::{self, with_ape_lock};
use crate::identity::{self, Identity, QUERY_DEVICE_IDS, VENDOR_BROADCOM};
use crate::phy;
use crate::regs::*;
use crate::rings::{self, BcmRxDesc, BcmTxDesc, BUF_SIZE, RX_SLOTS, TX_SLOTS};
use core::sync::atomic::{compiler_fence, Ordering};
use nic_driver::dma::{DmaSet, Layout, Position};
use nic_driver::{MacFmt, NicDevice, Probe};
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

const BCM_BAR0_LEN: usize = 0x1_0000;
const BCM_MMIO_MIN: usize = 0x6808;
const RESET_POLLS: u32 = 1_000_000;

/// One extra DMA slot, authorized after the rings: the status block.
const LAYOUT: Layout<1> = Layout::new([Position::After]);

pub struct BcmController {
    mmio: MmioRegion,
    dma: DmaSet<BcmTxDesc, BcmRxDesc, TX_SLOTS, RX_SLOTS, 1>,
    tx_next: usize,
    rx_head: usize,
    mac: [u8; 6],
    link_up: bool,
    func: u8,
}

impl Drop for BcmController {
    fn drop(&mut self) {
        // Unload notification to APE processor prevents dropping BMC NC-SI link.
        let _ = ape::notify_driver_state(&self.mmio, self.func, ape::APE_HOST_DRVR_STATE_UNLOAD);
    }
}

impl NicDevice for BcmController {
    const TAG: &'static str = "[bcm57xx]";
    const VENDOR: u16 = VENDOR_BROADCOM;
    const QUERY_IDS: &'static [u16] = &QUERY_DEVICE_IDS;
    const ABSENT_LINE: Option<&'static str> =
        Some("no BCM5719/BCM5720 controller present; driver cell idle");
    type Identity = Identity;
    type Error = ViError;

    fn classify(vendor: u16, device: u16) -> Option<Identity> {
        identity::classify(vendor, device)
    }

    fn identity_name(id: &Identity) -> &'static str {
        id.name
    }

    fn init(probe: &Probe, _id: Identity) -> ViResult<Self> {
        let mmio = probe.claim_bar0(BCM_BAR0_LEN, BCM_MMIO_MIN);
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

impl BcmController {
    fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        let func = (bdf & 0x07) as u8;
        // 1. Notify APE processor that driver is starting.
        ape::notify_driver_state(&mmio, func, ape::APE_HOST_DRVR_STATE_START)?;

        // 2. Perform GRC core reset under GRC lock.
        with_ape_lock(&mmio, func, ape::APE_LOCK_GRC, || {
            let mut val = mmio.read_u32(MISC_CFG)?;
            val |= MISCCFG_RESET_CORE_CLOCKS;
            mmio.write_u32(MISC_CFG, val)?;

            for _ in 0..RESET_POLLS {
                if mmio.read_u32(MISC_CFG)? & MISCCFG_RESET_CORE_CLOCKS == 0 {
                    return Ok(());
                }
                core::hint::spin_loop();
            }
            Err(ViError::IO)
        })?;

        // 3. Read station MAC address from MAC_ADDR1 registers.
        let lo = mmio.read_u32(MAC_ADDR1_LO)?;
        let hi = mmio.read_u32(MAC_ADDR1_HI)?;
        let mac = [
            (lo >> 24) as u8,
            (lo >> 16) as u8,
            (lo >> 8) as u8,
            lo as u8,
            (hi >> 8) as u8,
            hi as u8,
        ];
        if mac == [0u8; 6] {
            return Err(ViError::IO);
        }

        // 4. Restart PHY autonegotiation under PHY lock.
        phy::restart_autoneg(&mmio, func)?;

        // 5. Allocate and authorize DMA (status block last); nothing is
        //    programmed before this returns.
        let dma = DmaSet::alloc_authorized(&LAYOUT, bdf)?;
        let mut ctrl = BcmController {
            mmio,
            dma,
            tx_next: 0,
            rx_head: 0,
            mac,
            link_up: false,
            func,
        };
        ctrl.program_rings();
        ctrl.finish_init();
        Ok(ctrl)
    }

    /// Initial DMA programming: every RX descriptor, from authorized IOVAs.
    /// The ring and status-block bases are not programmed yet.
    fn program_rings(&mut self) {
        let iovas = self.dma.iovas;
        for (index, &iova) in iovas.rx_bufs.iter().enumerate() {
            self.dma.rx_ring.write(
                index,
                BcmRxDesc {
                    addr_hi: (iova >> 32) as u32,
                    addr_lo: iova as u32,
                    idx_len: (index as u32) << 16 | (BUF_SIZE as u32),
                    type_flags: 0,
                },
            );
        }
    }

    fn finish_init(&mut self) {
        let _ = self.mmio.write_u32(RX_STD_PRODIDX_REG, RX_SLOTS as u32);
        self.link_up = true;
    }

    fn send_frame(&mut self, frame: &[u8]) -> ViResult<()> {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return Err(ViError::InvalidArgument);
        }
        let slot = self.tx_next;
        self.dma.tx_bufs.copy_in(slot, frame)?;
        compiler_fence(Ordering::Release);

        let iova = self.dma.iovas.tx_bufs[slot];
        self.dma.tx_ring.write(
            slot,
            BcmTxDesc {
                addr_hi: (iova >> 32) as u32,
                addr_lo: iova as u32,
                len_flags: ((frame.len() as u32) << 16)
                    | rings::BCMD_FLAGS_END
                    | rings::BCMD_FLAGS_PACKET_END,
                vlan_tag: 0,
            },
        );
        compiler_fence(Ordering::Release);

        self.tx_next = (slot + 1) % TX_SLOTS;
        self.mmio.write_u32(TX_PRODIDX_REG, self.tx_next as u32)?;

        compiler_fence(Ordering::Acquire);
        Ok(())
    }

    fn poll_rx(&mut self, target: &mut [u8]) -> ViResult<usize> {
        let slot = self.rx_head;
        let len = self.dma.rx_bufs.copy_out(slot, target, BUF_SIZE);
        self.rx_head = (slot + 1) % RX_SLOTS;
        self.mmio
            .write_u32(RX_STD_PRODIDX_REG, self.rx_head as u32)?;
        Ok(len)
    }
}
