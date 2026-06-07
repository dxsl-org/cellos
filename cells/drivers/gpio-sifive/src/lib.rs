#![no_std]
#![forbid(unsafe_code)]

//! SiFive GPIO driver — QEMU `sifive_u` machine (FU540/FU740 GPIO0).
//!
//! GPIO0 base: 0x1001_2000, 32 pins, 32-bit registers (one bit per pin).
//! The kernel allowlist must include this range for `open()` to succeed;
//! on QEMU virt (riscv64gc) the allowlist is empty, so `open()` returns
//! `PermissionDenied` and callers should skip or fall back.

use hal_gpio::{Edge, PinDir, ViGpio};
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

const GPIO_BASE: usize = 0x1001_2000;
const GPIO_SIZE: usize = 0x1000;

// Register offsets (byte addresses, 32-bit wide).
const INPUT_VAL:  usize = 0x00;
const INPUT_EN:   usize = 0x04;
const OUTPUT_EN:  usize = 0x08;
const OUTPUT_VAL: usize = 0x0C;
const RISE_IE:    usize = 0x18;
const FALL_IE:    usize = 0x20;

/// SiFive GPIO controller (32-pin bank, QEMU `sifive_u` machine).
pub struct SiFiveGpio {
    mmio: MmioRegion,
}

impl SiFiveGpio {
    /// Request exclusive MMIO ownership from the kernel resource registry.
    ///
    /// Returns `Err(PermissionDenied)` when not running on a `sifive_u` QEMU target
    /// (allowlist empty for virt/aarch64/x86_64).
    pub fn open() -> ViResult<Self> {
        let mmio = ostd::mmio::request_region(GPIO_BASE, GPIO_SIZE)?;
        Ok(Self { mmio })
    }

    /// Set or clear bit `pin` in a 32-bit register at `reg`.
    fn rw_bit(mmio: &MmioRegion, reg: usize, pin: u8, set: bool) -> ViResult<()> {
        let cur = mmio.read_u32(reg)?;
        let bit = 1u32 << pin;
        mmio.write_u32(reg, if set { cur | bit } else { cur & !bit })
    }

    /// Test bit `pin` in a 32-bit register at `reg`.
    fn test_bit(mmio: &MmioRegion, reg: usize, pin: u8) -> ViResult<bool> {
        Ok((mmio.read_u32(reg)? >> pin) & 1 != 0)
    }
}

impl ViGpio for SiFiveGpio {
    fn set_direction(&mut self, pin: u8, dir: PinDir) -> ViResult<()> {
        match dir {
            PinDir::Input => {
                // Disable output first to avoid glitch, then enable input.
                Self::rw_bit(&self.mmio, OUTPUT_EN, pin, false)?;
                Self::rw_bit(&self.mmio, INPUT_EN,  pin, true)
            }
            PinDir::Output => {
                Self::rw_bit(&self.mmio, INPUT_EN,  pin, false)?;
                Self::rw_bit(&self.mmio, OUTPUT_EN, pin, true)
            }
        }
    }

    fn read_pin(&self, pin: u8) -> ViResult<bool> {
        Self::test_bit(&self.mmio, INPUT_VAL, pin)
    }

    fn write_pin(&mut self, pin: u8, high: bool) -> ViResult<()> {
        // SiFive does not enforce direction in hardware — ViGpio contract says
        // callers must set Output direction before write_pin.
        if !Self::test_bit(&self.mmio, OUTPUT_EN, pin)? {
            return Err(ViError::InvalidInput);
        }
        Self::rw_bit(&self.mmio, OUTPUT_VAL, pin, high)
    }

    fn enable_edge_irq(&mut self, pin: u8, edge: Edge) -> ViResult<()> {
        if matches!(edge, Edge::Rising | Edge::Both) {
            Self::rw_bit(&self.mmio, RISE_IE, pin, true)?;
        }
        if matches!(edge, Edge::Falling | Edge::Both) {
            Self::rw_bit(&self.mmio, FALL_IE, pin, true)?;
        }
        Ok(())
    }

    fn disable_irq(&mut self, pin: u8) -> ViResult<()> {
        Self::rw_bit(&self.mmio, RISE_IE, pin, false)?;
        Self::rw_bit(&self.mmio, FALL_IE, pin, false)
    }
}
