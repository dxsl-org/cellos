#![no_std]

//! Immutable QEMU ARM `virt` layout facts.
//!
//! Register offsets and device operations stay in shared drivers. This crate
//! only owns the addresses, spans, IRQ topology, and slot geometry supplied by
//! the machine model.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MmioRange {
    pub base: usize,
    pub size: usize,
}

impl MmioRange {
    pub const fn end(self) -> usize {
        self.base + self.size
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrqMmio {
    pub mmio: MmioRange,
    /// Device-tree SPI number, before the GIC shared-interrupt offset.
    pub spi: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtioMmioLayout {
    pub base: usize,
    pub stride: usize,
    pub count: usize,
    pub first_spi: u32,
}

impl VirtioMmioLayout {
    pub const fn checked_region(self) -> Option<MmioRange> {
        let Some(size) = self.stride.checked_mul(self.count) else {
            return None;
        };
        if self.base.checked_add(size).is_none() {
            return None;
        }
        Some(MmioRange {
            base: self.base,
            size,
        })
    }

    pub const fn region(self) -> MmioRange {
        match self.checked_region() {
            Some(region) => region,
            None => panic!("invalid VirtIO MMIO layout"),
        }
    }

    pub const fn slot_base(self, index: usize) -> Option<usize> {
        if index < self.count {
            let Some(offset) = index.checked_mul(self.stride) else {
                return None;
            };
            self.base.checked_add(offset)
        } else {
            None
        }
    }

    pub const fn spi(self, index: usize) -> Option<u32> {
        if index < self.count {
            Some(self.first_spi + index as u32)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArmVirtProfile {
    pub slug: &'static str,
    pub gic_distributor: MmioRange,
    pub gic_cpu: MmioRange,
    pub gic_map: MmioRange,
    pub uart: IrqMmio,
    pub rtc: MmioRange,
    pub gpio: IrqMmio,
    pub peripheral_map: MmioRange,
    pub virtio: VirtioMmioLayout,
    pub platform_bus_map: MmioRange,
    pub pcie_ecam_bus0: MmioRange,
}

impl ArmVirtProfile {
    pub const GIC_SPI_OFFSET: u32 = 32;

    pub const fn gic_id_for_spi(spi: u32) -> u32 {
        Self::GIC_SPI_OFFSET + spi
    }

    pub fn validate(self) -> Result<(), ValidationError> {
        let ranges = [
            self.gic_distributor,
            self.gic_cpu,
            self.gic_map,
            self.uart.mmio,
            self.rtc,
            self.gpio.mmio,
            self.peripheral_map,
            self.platform_bus_map,
            self.pcie_ecam_bus0,
        ];
        if ranges.iter().any(|range| range.size == 0) {
            return Err(ValidationError::ZeroSizedRange);
        }
        if ranges
            .iter()
            .any(|range| range.base.checked_add(range.size).is_none())
        {
            return Err(ValidationError::OverflowingRange);
        }
        if self.virtio.stride == 0 || self.virtio.count == 0 {
            return Err(ValidationError::InvalidVirtioLayout);
        }
        let Some(virtio_region) = self.virtio.checked_region() else {
            return Err(ValidationError::InvalidVirtioLayout);
        };
        if virtio_region.size == 0 {
            return Err(ValidationError::InvalidVirtioLayout);
        }
        if self.gic_distributor.base < self.gic_map.base
            || self.gic_distributor.end() > self.gic_map.end()
            || self.gic_cpu.base < self.gic_map.base
            || self.gic_cpu.end() > self.gic_map.end()
        {
            return Err(ValidationError::GicOutsideMappedRange);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError {
    ZeroSizedRange,
    OverflowingRange,
    InvalidVirtioLayout,
    GicOutsideMappedRange,
}

pub const QEMU_ARM_VIRT: ArmVirtProfile = ArmVirtProfile {
    slug: "qemu-arm-virt",
    gic_distributor: MmioRange {
        base: 0x0800_0000,
        size: 0x0001_0000,
    },
    gic_cpu: MmioRange {
        base: 0x0801_0000,
        size: 0x0001_0000,
    },
    gic_map: MmioRange {
        base: 0x0800_0000,
        size: 0x0100_0000,
    },
    uart: IrqMmio {
        mmio: MmioRange {
            base: 0x0900_0000,
            size: 0x1000,
        },
        spi: 1,
    },
    rtc: MmioRange {
        base: 0x0902_0000,
        size: 0x1000,
    },
    gpio: IrqMmio {
        mmio: MmioRange {
            base: 0x0903_0000,
            size: 0x1000,
        },
        spi: 7,
    },
    peripheral_map: MmioRange {
        base: 0x0900_0000,
        size: 0x0004_0000,
    },
    virtio: VirtioMmioLayout {
        base: 0x0A00_0000,
        stride: 0x200,
        count: 32,
        first_spi: 16,
    },
    platform_bus_map: MmioRange {
        base: 0x1000_0000,
        size: 0x0001_0000,
    },
    pcie_ecam_bus0: MmioRange {
        base: 0x3F00_0000,
        size: 0x0010_0000,
    },
};

#[cfg(test)]
mod tests;
