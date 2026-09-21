//! DWC2 Host Controller hardware abstraction and initialization.

use crate::regs::*;
use crate::usb_channel::TransferMode;
use ostd::io::println;
use ostd::mmio::MmioRegion;
use ostd::syscall::sys_yield;
use types::{ViError, ViResult};

pub struct Dwc2Controller {
    mmio: MmioRegion,
}

impl Dwc2Controller {
    /// Open the DWC2 controller by requesting exclusive access from the kernel Resource Registry.
    pub fn open(base: usize, len: usize) -> ViResult<Self> {
        let mmio = ostd::mmio::request_region(base, len)?;
        Ok(Self { mmio })
    }

    /// Access the underlying MMIO region.
    #[inline(always)]
    pub fn mmio(&self) -> &MmioRegion {
        &self.mmio
    }

    /// Read a 32-bit register at `offset`.
    #[inline(always)]
    pub fn read32(&self, offset: usize) -> u32 {
        self.mmio.read::<u32>(offset).unwrap_or(0)
    }

    /// Write a 32-bit register at `offset`.
    #[inline(always)]
    pub fn write32(&self, offset: usize, val: u32) {
        let _ = self.mmio.write::<u32>(offset, val);
    }

    /// Read and verify the Synopsys Hardware Core ID (`GSNPSID`).
    ///
    /// Expected for DWC2 on BCM2837 is `0x4F54280A` (or `0x4F54xxxx`, where 0x4F54 = ASCII "OT").
    pub fn probe_core_id(&self) -> ViResult<u32> {
        let snpsid = self.read32(GSNPSID);
        if snpsid & 0xFFFF_0000 != 0x4F54_0000 {
            println("[dwc2] ERROR: invalid Synopsys Core ID: 0x");
            return Err(ViError::NotFound);
        }
        Ok(snpsid)
    }

    /// Perform a software reset of the DWC2 core.
    pub fn core_reset(&self) -> ViResult<()> {
        // 1. Wait for AHB Master Idle before resetting
        let mut count = 0;
        while self.read32(GRSTCTL) & GRSTCTL_AHBIDL == 0 {
            count += 1;
            if count > 100_000 {
                println("[dwc2] WARN: AHB master idle timeout before reset");
                break;
            }
            sys_yield();
        }

        // 2. Trigger Core Soft Reset (CSRST)
        self.write32(GRSTCTL, GRSTCTL_CSRST);
        count = 0;
        while self.read32(GRSTCTL) & GRSTCTL_CSRST != 0 {
            count += 1;
            if count > 100_000 {
                println("[dwc2] ERROR: core soft reset timeout");
                return Err(ViError::IO);
            }
            sys_yield();
        }

        // 3. Wait for AHB Master Idle after reset
        count = 0;
        while self.read32(GRSTCTL) & GRSTCTL_AHBIDL == 0 {
            count += 1;
            if count > 100_000 {
                break;
            }
            sys_yield();
        }

        Ok(())
    }

    /// Initialize the DWC2 core into Host Mode using `mode` for payload transfer.
    ///
    /// The core is reset by this call, so it must be re-run to switch modes —
    /// which is exactly what the driver does when one mode's enumeration fails.
    pub fn init_host(&self, mode: TransferMode) -> ViResult<()> {
        // 0. Restart PHY clock by clearing Power and Clock Gating Control (PCGCCTL)
        self.write32(PCGCCTL, 0);

        // 1. Perform core soft reset
        self.core_reset()?;

        // Restart PHY clock again after reset
        self.write32(PCGCCTL, 0);

        // 2. Configure USB parameters in GUSBCFG:
        // Force Host Mode, 16-bit PHY interface, Turnaround time (TRDT = 9)
        let mut usbcfg = self.read32(GUSBCFG);
        usbcfg &= !GUSBCFG_FORCEDEVMODE;
        usbcfg |= GUSBCFG_FORCEHOSTMODE;
        usbcfg &= !GUSBCFG_TRD_TIM_MASK;
        usbcfg |= GUSBCFG_TRD_TIM_9;
        self.write32(GUSBCFG, usbcfg);

        // Allow 50ms for host mode to take effect
        for _ in 0..500 {
            sys_yield();
        }

        // 3. Verify current mode is Host mode (GINTSTS bit 0)
        let gintsts = self.read32(GINTSTS);
        if gintsts & GINTSTS_CURMODE_HOST == 0 {
            println("[dwc2] ERROR: failed to force Host Mode");
            return Err(ViError::IO);
        }

        // 4. Configure Host Clock in HCFG (30-60 MHz High-Speed PHY)
        self.write32(HCFG, 0);

        // 5. Configure dynamic Host FIFOs (total capacity on BCM2837 is 4096 words):
        // Rx FIFO: 1024 words
        self.write32(GRXFSIZ, 1024);
        // Non-Periodic Tx FIFO: start at 1024, depth 1024 words
        self.write32(GNPTXFSIZ, (1024 << 16) | 1024);
        // Periodic Tx FIFO: start at 2048, depth 1024 words
        self.write32(HPTXFSIZ, (1024 << 16) | 2048);

        // 6. Configure AHB bus in GAHBCFG: unmask global interrupts, and enable
        //    DMA only in DMA mode. The FIFO depths above still apply in DMA mode
        //    (the RX FIFO stages inbound packets either way).
        let mut ahbcfg = GAHBCFG_GLBLINTRMSK;
        if mode == TransferMode::Dma {
            ahbcfg |= GAHBCFG_DMAEN;
        }
        self.write32(GAHBCFG, ahbcfg);

        // 7. Flush RX and TX FIFOs
        self.write32(GRSTCTL, GRSTCTL_RXFFLSH);
        let mut count = 0;
        while self.read32(GRSTCTL) & GRSTCTL_RXFFLSH != 0 {
            count += 1;
            if count > 10_000 {
                break;
            }
            sys_yield();
        }

        self.write32(GRSTCTL, GRSTCTL_TXFFLSH | GRSTCTL_TXFNUM_ALL);
        count = 0;
        while self.read32(GRSTCTL) & GRSTCTL_TXFFLSH != 0 {
            count += 1;
            if count > 10_000 {
                break;
            }
            sys_yield();
        }

        // 8. Clear pending interrupts by writing 1 to all W1C bits
        self.write32(GINTSTS, 0xFFFF_FFFF);

        // 9. Unmask essential interrupts in GINTMSK: Host Port, Host Channels, Disconnect, SOF
        let intmsk = GINTSTS_HPRTINT | GINTSTS_HCHINT | GINTSTS_DISCONNINT | GINTSTS_SOF;
        self.write32(GINTMSK, intmsk);

        // 10. Unmask all 16 host channels in HAINTMSK
        self.write32(HAINTMSK, 0xFFFF);

        // Diagnostic: dump key registers after init
        Self::dump_reg("GINTSTS", self.read32(GINTSTS));
        Self::dump_reg("GAHBCFG", self.read32(GAHBCFG));
        Self::dump_reg("HPRT0  ", self.read32(HPRT0));
        Self::dump_reg("HCFG   ", self.read32(HCFG));
        Self::dump_reg("GRXFSIZ", self.read32(GRXFSIZ));
        Self::dump_reg("GNPTXFS", self.read32(GNPTXFSIZ));
        Self::dump_reg("HPTXFSZ", self.read32(HPTXFSIZ));
        Self::dump_reg("GNPTXST", self.read32(GNPTXSTS));
        Self::dump_reg("PCGCCTL", self.read32(PCGCCTL));

        Ok(())
    }

    /// Power on the Root Port (HPRT0).
    pub fn power_on_port(&self) {
        let mut hprt0 = self.read32(HPRT0);
        // Clear W1C bits so we don't clear port state
        hprt0 &= !HPRT0_W1C_MASK;
        hprt0 |= HPRT0_PRTPWR;
        self.write32(HPRT0, hprt0);

        // Settle delay for power rail stabilization
        for _ in 0..1000 {
            sys_yield();
        }
    }

    /// Check if a downstream device (e.g. LAN9514 Hub) is connected to Root Port 0.
    pub fn is_port_connected(&self) -> bool {
        self.read32(HPRT0) & HPRT0_PRTCONNSTS != 0
    }

    /// Reset Root Port 0 and wait for speed negotiation. Returns negotiated speed.
    pub fn reset_port(&self) -> ViResult<u32> {
        let mut hprt0 = self.read32(HPRT0);
        hprt0 &= !HPRT0_W1C_MASK;
        hprt0 |= HPRT0_PRTRST;
        self.write32(HPRT0, hprt0);

        // Keep reset asserted for ~50ms
        for _ in 0..5000 {
            sys_yield();
        }

        // De-assert reset
        hprt0 = self.read32(HPRT0);
        hprt0 &= !HPRT0_W1C_MASK;
        hprt0 &= !HPRT0_PRTRST;
        self.write32(HPRT0, hprt0);

        // Wait for port enable (PRTENA)
        let mut count = 0;
        loop {
            let status = self.read32(HPRT0);
            if status & HPRT0_PRTENA != 0 {
                let speed = (status & HPRT0_PRTSPD_MASK) >> 17;
                Self::dump_reg("HPRT0-R", status);
                return Ok(speed);
            }
            count += 1;
            if count > 100_000 {
                println("[dwc2] WARN: port enable timeout after reset");
                return Err(ViError::IO);
            }
            sys_yield();
        }
    }

    fn dump_reg(name: &str, val: u32) {
        use ostd::io::{print, println};
        print("[dwc2] ");
        print(name);
        print("=0x");
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut buf = [0u8; 8];
        for i in 0..8 {
            buf[7 - i] = HEX[((val >> (i * 4)) & 0xF) as usize];
        }
        if let Ok(s) = core::str::from_utf8(&buf) {
            print(s);
        }
        println("");
    }
}
