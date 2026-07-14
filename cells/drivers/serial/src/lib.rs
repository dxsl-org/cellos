#![no_std]
#![forbid(unsafe_code)]

//! UART Driver Cell — ARM PL011 on QEMU ARM virt.
//!
//! Base address: 0x0900_0000 (ARM virt machine, 4 KiB region).
//!
//! Init sequence: disable → set baud → set line control → enable.
//! Clock: 24 MHz (QEMU ARM virt uart_clk).

use hal_uart::{SerialPort, UartConfig, ViUart};
use ostd::mmio::{request_region, MmioRegion};
use types::{HalError, HalResult, ViError, ViResult};

/// QEMU ARM virt PL011 MMIO base and size.
pub const PL011_BASE: usize = 0x0900_0000;
pub const PL011_SIZE: usize = 0x1000;

// Register offsets
const UARTDR: usize = 0x000;
const UARTFR: usize = 0x018;
const UARTIBRD: usize = 0x024;
const UARTFBRD: usize = 0x028;
const UARTLCR_H: usize = 0x02C;
const UARTCR: usize = 0x030;
const UARTIMSC: usize = 0x038;
const UARTICR: usize = 0x044;

const FR_RXFE: u32 = 1 << 4;
const FR_TXFF: u32 = 1 << 5;
const LCR_FEN: u32 = 1 << 4;
const LCR_WLEN_8: u32 = 0b11 << 5;
const CR_UARTEN: u32 = 1 << 0;
const CR_TXE: u32 = 1 << 8;
const CR_RXE: u32 = 1 << 9;

fn vi_to_hal(_: ViError) -> HalError {
    HalError::IoError
}

/// PL011 UART driver for QEMU ARM virt.
pub struct Pl011Uart {
    mmio: MmioRegion,
}

impl Pl011Uart {
    /// Acquire exclusive UART MMIO access from the kernel.
    pub fn open() -> ViResult<Self> {
        let mmio = request_region(PL011_BASE, PL011_SIZE)?;
        Ok(Self { mmio })
    }

    fn baud_divisors(baud: u32) -> (u32, u32) {
        let uart_clk: u32 = 24_000_000;
        // baud_div × 64 (avoids float): uart_clk × 4 / baud = baud_div × 64
        let div64 = uart_clk / baud * 4 + (uart_clk % baud * 4) / baud;
        let ibrd = div64 / 64;
        let fbrd = (div64 % 64).div_ceil(2); // round to nearest
        (ibrd, fbrd)
    }

    fn do_configure(&mut self, baud: u32) -> ViResult<()> {
        self.mmio.write_u32(UARTCR, 0)?;
        self.mmio.write_u32(UARTIMSC, 0)?;
        self.mmio.write_u32(UARTICR, 0x7FF)?;
        self.mmio.write_u32(UARTLCR_H, 0)?;
        let (ibrd, fbrd) = Self::baud_divisors(baud);
        self.mmio.write_u32(UARTIBRD, ibrd)?;
        self.mmio.write_u32(UARTFBRD, fbrd)?;
        self.mmio.write_u32(UARTLCR_H, LCR_WLEN_8 | LCR_FEN)?;
        self.mmio.write_u32(UARTCR, CR_UARTEN | CR_TXE | CR_RXE)
    }
}

impl SerialPort for Pl011Uart {
    fn init(&mut self) -> HalResult<()> {
        self.do_configure(115200).map_err(vi_to_hal)
    }

    fn send(&mut self, data: u8) -> HalResult<()> {
        loop {
            let fr = self.mmio.read_u32(UARTFR).map_err(vi_to_hal)?;
            if fr & FR_TXFF == 0 {
                break;
            }
        }
        self.mmio.write_u32(UARTDR, data as u32).map_err(vi_to_hal)
    }

    fn receive(&mut self) -> HalResult<u8> {
        loop {
            let fr = self.mmio.read_u32(UARTFR).map_err(vi_to_hal)?;
            if fr & FR_RXFE == 0 {
                break;
            }
        }
        let dr = self.mmio.read_u32(UARTDR).map_err(vi_to_hal)?;
        Ok((dr & 0xFF) as u8)
    }
}

impl ViUart for Pl011Uart {
    fn configure(&mut self, cfg: UartConfig) -> ViResult<()> {
        self.do_configure(cfg.baud as u32)
    }

    fn rx_ready(&self) -> bool {
        self.mmio
            .read_u32(UARTFR)
            .map(|fr| fr & FR_RXFE == 0)
            .unwrap_or(false)
    }

    fn tx_ready(&self) -> bool {
        self.mmio
            .read_u32(UARTFR)
            .map(|fr| fr & FR_TXFF == 0)
            .unwrap_or(false)
    }
}

impl Pl011Uart {
    /// Enable UARTCR.LBE (bit 7) — TX data feeds directly back into RX FIFO.
    /// Used in integration tests to verify the full send/receive path without
    /// an external physical loopback wire.
    pub fn enable_loopback(&mut self) -> ViResult<()> {
        let cr = self.mmio.read_u32(UARTCR)?;
        self.mmio.write_u32(UARTCR, cr | (1 << 7))
    }

    /// Clear UARTCR.LBE — restore normal TX→external, RX←external path.
    pub fn disable_loopback(&mut self) -> ViResult<()> {
        let cr = self.mmio.read_u32(UARTCR)?;
        self.mmio.write_u32(UARTCR, cr & !(1 << 7))
    }
}
