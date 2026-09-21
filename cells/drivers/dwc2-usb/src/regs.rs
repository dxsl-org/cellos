//! Synopsys DesignWare Hi-Speed USB 2.0 OTG Controller (DWC2) register definitions.
//!
//! Offsets and bitfields verified against Synopsys DWC2 Databook & BCM2835 ARM Peripherals.

// ── Global Registers (0x000 .. 0x0FC) ──────────────────────────────────────────

/// OTG Control and Status Register
pub const GOTGCTL: usize = 0x000;
/// OTG Interrupt Register
pub const GOTGINT: usize = 0x004;
/// Core AHB Configuration Register
pub const GAHBCFG: usize = 0x008;
/// Core USB Configuration Register
pub const GUSBCFG: usize = 0x00C;
/// Core Reset Register
pub const GRSTCTL: usize = 0x010;
/// Core Interrupt Status Register (W1C)
pub const GINTSTS: usize = 0x014;
/// Core Interrupt Mask Register
pub const GINTMSK: usize = 0x018;
/// Receive Status Debug Read Register
pub const GRXSTSR: usize = 0x01C;
/// Receive FIFO Size Register
pub const GRXFSIZ: usize = 0x024;
/// Non-Periodic Transmit FIFO Size Register (write) / Non-Periodic Transmit FIFO Status (read)
pub const GNPTXFSIZ: usize = 0x028;
pub const GNPTXSTS: usize = 0x028;
/// Synopsys ID Register (Hardware core ID)
pub const GSNPSID: usize = 0x040;
/// User ID Register
pub const GUSERID: usize = 0x03C;
/// Hardware Configuration 1 Register
pub const GHWCFG1: usize = 0x044;
/// Hardware Configuration 2 Register
pub const GHWCFG2: usize = 0x048;
/// Hardware Configuration 3 Register
pub const GHWCFG3: usize = 0x04C;
/// Hardware Configuration 4 Register
pub const GHWCFG4: usize = 0x050;
/// Host Periodic Transmit FIFO Size Register
pub const HPTXFSIZ: usize = 0x100;
/// Power and Clock Gating Control Register
pub const PCGCCTL: usize = 0xE00;

// ── Host-Mode Registers (0x400 .. 0x4FC) ───────────────────────────────────────

/// Host Configuration Register
pub const HCFG: usize = 0x400;
/// Host Frame Interval Register
pub const HFIR: usize = 0x404;
/// Host Frame Number / Frame Time Remaining Register
pub const HFNUM: usize = 0x408;
/// Host Periodic Transmit FIFO / Queue Status Register
pub const HPTXSTS: usize = 0x410;
/// Host All Channels Interrupt Register
pub const HAINT: usize = 0x414;
/// Host All Channels Interrupt Mask Register
pub const HAINTMSK: usize = 0x418;
/// Host Port 0 Control and Status Register
pub const HPRT0: usize = 0x440;

// ── Host Channel Registers (0x500 + ch * 0x20) ────────────────────────────────

pub const fn hcchar(ch: usize) -> usize {
    0x500 + ch * 0x20
}
pub const fn hcsplt(ch: usize) -> usize {
    0x500 + ch * 0x20 + 0x04
}
pub const fn hcint(ch: usize) -> usize {
    0x500 + ch * 0x20 + 0x08
}
pub const fn hcintmsk(ch: usize) -> usize {
    0x500 + ch * 0x20 + 0x0C
}
pub const fn hctsiz(ch: usize) -> usize {
    0x500 + ch * 0x20 + 0x10
}
pub const fn hcdma(ch: usize) -> usize {
    0x500 + ch * 0x20 + 0x14
}

// ── Register Bit Masks & Constants ────────────────────────────────────────────

// GRSTCTL bits
pub const GRSTCTL_CSRST: u32 = 1 << 0; // Core Soft Reset
pub const GRSTCTL_HSRST: u32 = 1 << 1; // HCLK Soft Reset
pub const GRSTCTL_FCRST: u32 = 1 << 2; // Host Frame Counter Reset
pub const GRSTCTL_RXFFLSH: u32 = 1 << 4; // RxFIFO Flush
pub const GRSTCTL_TXFFLSH: u32 = 1 << 5; // TxFIFO Flush
pub const GRSTCTL_TXFNUM_ALL: u32 = 0x10 << 6; // Flush all Tx FIFOs
pub const GRSTCTL_AHBIDL: u32 = 1 << 31; // AHB Master Idle

// GAHBCFG bits
pub const GAHBCFG_GLBLINTRMSK: u32 = 1 << 0; // Global Interrupt Mask (1 = unmask)
pub const GAHBCFG_HBSTLEN_INCR16: u32 = 7 << 1; // AHB burst length: INCR16
pub const GAHBCFG_DMAEN: u32 = 1 << 5; // DMA Enable

// HCFG bits
pub const HCFG_FSLSPCLKSEL_MASK: u32 = 0x3;
pub const HCFG_FSLSPCLKSEL_48MHZ: u32 = 0x1; // 48 MHz PHY clock

// GUSBCFG bits
pub const GUSBCFG_PHYIF16: u32 = 1 << 3; // 16-bit UTMI+ Interface
pub const GUSBCFG_ULPI_UTMI_SEL: u32 = 1 << 4;
pub const GUSBCFG_TRD_TIM_MASK: u32 = 0xF << 10;
pub const GUSBCFG_TRD_TIM_9: u32 = 9 << 10; // Turnaround time for 30-60MHz AHB
pub const GUSBCFG_FORCEDEVMODE: u32 = 1 << 30;
pub const GUSBCFG_FORCEHOSTMODE: u32 = 1 << 29;

// GINTSTS / GINTMSK bits
pub const GINTSTS_CURMODE_HOST: u32 = 1 << 0; // Current Mode: 1 = Host, 0 = Device
pub const GINTSTS_MODEMIS: u32 = 1 << 1;
pub const GINTSTS_OTGINT: u32 = 1 << 2;
pub const GINTSTS_SOF: u32 = 1 << 3;
pub const GINTSTS_RXFLVL: u32 = 1 << 4;
pub const GINTSTS_NPTXFEMP: u32 = 1 << 5;
pub const GINTSTS_HPRTINT: u32 = 1 << 24; // Host Port Interrupt
pub const GINTSTS_HCHINT: u32 = 1 << 25; // Host Channels Interrupt
pub const GINTSTS_PTXFEMP: u32 = 1 << 26;
pub const GINTSTS_CONIDSTSCHNG: u32 = 1 << 28;
pub const GINTSTS_DISCONNINT: u32 = 1 << 29;
pub const GINTSTS_SESSREQINT: u32 = 1 << 30;
pub const GINTSTS_WKUPINT: u32 = 1 << 31;

// HPRT0 bits (Host Port 0 Control and Status)
pub const HPRT0_PRTCONNSTS: u32 = 1 << 0; // Port Connect Status (RO)
pub const HPRT0_PRTCONNDET: u32 = 1 << 1; // Port Connect Detected (W1C)
pub const HPRT0_PRTENA: u32 = 1 << 2; // Port Enable (W1C to disable)
pub const HPRT0_PRTENCHNG: u32 = 1 << 3; // Port Enable/Disable Change (W1C)
pub const HPRT0_PRTOVRCURRACT: u32 = 1 << 4; // Port Overcurrent Active
pub const HPRT0_PRTOVRCURRCHNG: u32 = 1 << 5; // Port Overcurrent Change (W1C)
pub const HPRT0_PRTRES: u32 = 1 << 6; // Port Resume
pub const HPRT0_PRTSUSP: u32 = 1 << 7; // Port Suspend
pub const HPRT0_PRTRST: u32 = 1 << 8; // Port Reset
pub const HPRT0_PRTPWR: u32 = 1 << 12; // Port Power
pub const HPRT0_PRTSPD_MASK: u32 = 0x3 << 17; // Port Speed bits
pub const HPRT0_PRTSPD_HIGH: u32 = 0x0 << 17; // High Speed (480 Mbps)
pub const HPRT0_PRTSPD_FULL: u32 = 0x1 << 17; // Full Speed (12 Mbps)
pub const HPRT0_PRTSPD_LOW: u32 = 0x2 << 17; // Low Speed (1.5 Mbps)

// HCSPLT bits (Host Channel Split Control). A channel that talks to a full- or
// low-speed device behind a high-speed hub addresses it through these fields;
// the bit layout is from the core's own header (dwc2_core.h).
pub const HCSPLT_PRTADDR_MASK: u32 = 0x7F; // hub port, bits 6:0
pub const HCSPLT_HUBADDR_SHIFT: u32 = 7; // hub device address, bits 13:7
pub const HCSPLT_HUBADDR_MASK: u32 = 0x7F << HCSPLT_HUBADDR_SHIFT;
pub const HCSPLT_COMPSPLT: u32 = 1 << 16; // 1 = complete split, 0 = start split
pub const HCSPLT_SPLTENA: u32 = 1 << 31; // split enable

/// `HCCHAR.LSPDDEV` — the target is a low-speed device.
///
/// Not a cosmetic hint: a low-speed device only understands transactions
/// preceded by a full-speed preamble. Behind a hub the same bit tells the hub's
/// transaction translator to use that preamble, and without it the hub runs a
/// full-speed transaction to a device that cannot hear it.
pub const HCCHAR_LSPDDEV: u32 = 1 << 17;

/// `HFNUM.FRNUM` — the USB frame counter, in 125 us frames.
pub const HFNUM_FRNUM_MASK: u32 = 0xFFFF;

/// Mask of W1C bits in HPRT0. When writing to HPRT0 (e.g. to set PRTRST or PRTPWR),
/// these bits must be written as ZERO to prevent inadvertently clearing port events!
pub const HPRT0_W1C_MASK: u32 =
    HPRT0_PRTCONNDET | HPRT0_PRTENA | HPRT0_PRTENCHNG | HPRT0_PRTOVRCURRCHNG;
