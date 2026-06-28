#![no_std]
#![forbid(unsafe_code)]

//! BCM2837 GPIO Driver Cell — Raspberry Pi 3 (BCM2837).
//!
//! Base: 0x3F200000, 54 pins, 32-bit GPFSEL/GPSET/GPCLR/GPLEV/GPREN/GPFEN.
//! Pin function select: 3 bits per pin, 10 pins per GPFSEL register.
//!
//! IRQ routing: BCM2835 legacy controller bank0 (pins 0–27) / bank1 (pins 28–45).
//! The kernel IRQ handler notifies the MMIO owner via opcode 0xA0 when a GPIO
//! edge fires — same protocol as the PL061 driver.
//!
//! Pull-up/down: BCM2837 requires a 3-step GPPUD + GPPUDCLK sequence (≥150 cycles
//! hold time). This driver omits the timing (no busy-wait in no_std), so pull
//! configuration is caller responsibility if needed.

use hal_gpio::{Edge, PinDir, ViGpio};
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

/// BCM2837 GPIO MMIO base on Raspberry Pi 3.
pub const BCM_GPIO_BASE: usize = 0x3F20_0000;
/// MMIO region size mapped for GPIO access.
pub const BCM_GPIO_SIZE: usize = 0x0001_0000; // 64 KiB (covers GPPUDCLK1 at 0x9C)

// Register offsets (u32, byte-addressed)
const GPFSEL: [usize; 6] = [0x00, 0x04, 0x08, 0x0C, 0x10, 0x14]; // function select
const GPSET:  [usize; 2] = [0x1C, 0x20]; // set output high
const GPCLR:  [usize; 2] = [0x28, 0x2C]; // set output low
const GPLEV:  [usize; 2] = [0x34, 0x38]; // read level
const GPEDS:  [usize; 2] = [0x40, 0x44]; // event detect status (write 1 to clear)
const GPREN:  [usize; 2] = [0x4C, 0x50]; // rising edge detect enable
const GPFEN:  [usize; 2] = [0x58, 0x5C]; // falling edge detect enable

// Function select encoding (3 bits per pin)
const FSEL_INPUT:  u32 = 0b000;
const FSEL_OUTPUT: u32 = 0b001;

/// BCM2837 GPIO controller for Raspberry Pi 3 (54 pins).
pub struct BcmGpio {
    mmio: MmioRegion,
}

impl BcmGpio {
    /// Request exclusive MMIO ownership from the kernel resource registry.
    ///
    /// Returns `Err(PermissionDenied)` when not running on a `board-rpi3` target
    /// (the kernel allowlist excludes 0x3F200000 on QEMU virt/x86_64/riscv64).
    pub fn open() -> ViResult<Self> {
        let mmio = ostd::mmio::request_region(BCM_GPIO_BASE, BCM_GPIO_SIZE)?;
        Ok(Self { mmio })
    }

    /// Read GPEDS (event detect status) for a pin bank (0 = pins 0-31, 1 = pins 32-53).
    pub fn read_eds(&self, bank: usize) -> ViResult<u32> {
        self.mmio.read_u32(GPEDS[bank])
    }

    /// Clear GPEDS bits indicated by `mask` in the given bank.
    pub fn clear_eds(&self, bank: usize, mask: u32) -> ViResult<()> {
        self.mmio.write_u32(GPEDS[bank], mask)
    }

    /// Set the function for a pin (GPFSEL register).
    fn set_fsel(&self, pin: u8, func: u32) -> ViResult<()> {
        let reg_idx = (pin / 10) as usize;
        let bit_pos = ((pin % 10) * 3) as u32;
        let cur = self.mmio.read_u32(GPFSEL[reg_idx])?;
        let new = (cur & !(0x7 << bit_pos)) | ((func & 0x7) << bit_pos);
        self.mmio.write_u32(GPFSEL[reg_idx], new)
    }
}

impl ViGpio for BcmGpio {
    fn set_direction(&mut self, pin: u8, dir: PinDir) -> ViResult<()> {
        if pin > 53 { return Err(ViError::InvalidInput); }
        let func = match dir { PinDir::Output => FSEL_OUTPUT, PinDir::Input => FSEL_INPUT };
        self.set_fsel(pin, func)
    }

    fn read_pin(&self, pin: u8) -> ViResult<bool> {
        if pin > 53 { return Err(ViError::InvalidInput); }
        let (bank, bit) = ((pin / 32) as usize, pin % 32);
        let val = self.mmio.read_u32(GPLEV[bank])?;
        Ok(val & (1 << bit) != 0)
    }

    fn write_pin(&mut self, pin: u8, high: bool) -> ViResult<()> {
        if pin > 53 { return Err(ViError::InvalidInput); }
        let (bank, bit) = ((pin / 32) as usize, pin % 32);
        if high {
            self.mmio.write_u32(GPSET[bank], 1 << bit)
        } else {
            self.mmio.write_u32(GPCLR[bank], 1 << bit)
        }
    }

    fn enable_edge_irq(&mut self, pin: u8, edge: Edge) -> ViResult<()> {
        if pin > 53 { return Err(ViError::InvalidInput); }
        let (bank, bit) = ((pin / 32) as usize, pin % 32);
        let mask = 1u32 << bit;
        if matches!(edge, Edge::Rising | Edge::Both) {
            let cur = self.mmio.read_u32(GPREN[bank])?;
            self.mmio.write_u32(GPREN[bank], cur | mask)?;
        }
        if matches!(edge, Edge::Falling | Edge::Both) {
            let cur = self.mmio.read_u32(GPFEN[bank])?;
            self.mmio.write_u32(GPFEN[bank], cur | mask)?;
        }
        Ok(())
    }

    fn disable_irq(&mut self, pin: u8) -> ViResult<()> {
        if pin > 53 { return Err(ViError::InvalidInput); }
        let (bank, bit) = ((pin / 32) as usize, pin % 32);
        let mask = 1u32 << bit;
        let cur_ren = self.mmio.read_u32(GPREN[bank])?;
        self.mmio.write_u32(GPREN[bank], cur_ren & !mask)?;
        let cur_fen = self.mmio.read_u32(GPFEN[bank])?;
        self.mmio.write_u32(GPFEN[bank], cur_fen & !mask)
    }
}
