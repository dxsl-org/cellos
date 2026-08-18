#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Architecture {
    Riscv64,
    Aarch64,
    X86_64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocId {
    GenericRiscvVirt,
    Jh7110,
    Sg2042,
    QemuArmVirt,
    Bcm2837,
    Bcm2711,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverId {
    ConsoleSbiDbcn,
    UartNs16550a,
    UartDwApb,
    UartPl011,
    UartBcmMini,
    PlicSifive,
    ClintSifive,
    GicV2,
    IrqBcm2836Local,
    IrqBcm2835Legacy,
    TimerBcm2835System,
    RtcGoldfish,
    VirtioMmio,
    PcieEcam,
    SdhciArasan,
    SdhciDwCqe,
    GpioBcm2837,
    GpioBcm2711,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareInterface {
    OpenSbi,
    Uefi,
    VideoCore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootProtocol {
    FlattenedDeviceTree,
    DeviceTreeWithFallbackMap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRangeKind {
    Bootloader,
    Kernel,
    Usable,
    Reserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootContract {
    pub firmware: FirmwareInterface,
    pub boot_protocol: BootProtocol,
    pub requires_firmware_dtb: bool,
    pub fallback_dts_path: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRange {
    pub name: &'static str,
    pub base: u64,
    pub size: u64,
    pub kind: MemoryRangeKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MmioRegion {
    pub compatible: &'static str,
    pub base: u64,
    pub size: u64,
    pub irq: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WiringLayout {
    pub pinmux_groups: &'static [&'static str],
    pub phy_links: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoardDescriptor {
    pub slug: &'static str,
    pub vendor: &'static str,
    pub model: &'static str,
    pub architecture: Architecture,
    pub soc: SocId,
    pub compatibles: &'static [&'static str],
    pub boot: BootContract,
    pub fallback_memory: &'static [MemoryRange],
    pub uart: MmioRegion,
    pub plic: Option<MmioRegion>,
    pub clint: Option<MmioRegion>,
    pub rtc: Option<MmioRegion>,
    pub virtio_mmio: &'static [MmioRegion],
    pub wiring: WiringLayout,
    pub enabled_drivers: &'static [DriverId],
}

pub const MAX_VIRTIO_MMIO_SLOTS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError {
    EmptyCompatibles,
    MissingFallbackMemory,
    MissingEnabledDrivers,
    DuplicateEnabledDriver(DriverId),
    TooManyVirtioSlots {
        found: usize,
        max: usize,
    },
    ZeroSizedFallbackRange(&'static str),
    OverflowingFallbackRange(&'static str),
    OverlappingFallbackRange(&'static str, &'static str),
    ZeroSizedMmioCore(&'static str),
    ZeroSizedMmioRange(&'static str),
    OverflowingMmioRange(&'static str),
    UnsortedMmioRange(&'static str, &'static str),
    ArchitectureMismatch {
        expected: Architecture,
        found: Architecture,
    },
}

impl BoardDescriptor {
    /// Reports whether the selected board enables a shared driver mechanism.
    pub fn has_driver(&self, driver: DriverId) -> bool {
        self.enabled_drivers.contains(&driver)
    }

    /// Checks that build-time board data is safe to consume as fallback state.
    ///
    /// Returns the first structural violation; validation performs no allocation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.compatibles.is_empty() || self.compatibles.iter().any(|value| value.is_empty()) {
            return Err(ValidationError::EmptyCompatibles);
        }
        if self.fallback_memory.is_empty() {
            return Err(ValidationError::MissingFallbackMemory);
        }
        if self.enabled_drivers.is_empty() {
            return Err(ValidationError::MissingEnabledDrivers);
        }
        for (index, driver) in self.enabled_drivers.iter().enumerate() {
            if self.enabled_drivers[..index].contains(driver) {
                return Err(ValidationError::DuplicateEnabledDriver(*driver));
            }
        }
        if self.virtio_mmio.len() > MAX_VIRTIO_MMIO_SLOTS {
            return Err(ValidationError::TooManyVirtioSlots {
                found: self.virtio_mmio.len(),
                max: MAX_VIRTIO_MMIO_SLOTS,
            });
        }
        for region in core::iter::once(self.uart)
            .chain([self.plic, self.clint, self.rtc].into_iter().flatten())
        {
            if region.size == 0 {
                return Err(ValidationError::ZeroSizedMmioCore(region.compatible));
            }
            if region.base.checked_add(region.size).is_none() {
                return Err(ValidationError::OverflowingMmioRange(region.compatible));
            }
        }
        for range in self.fallback_memory {
            if range.size == 0 {
                return Err(ValidationError::ZeroSizedFallbackRange(range.name));
            }
            if range.base.checked_add(range.size).is_none() {
                return Err(ValidationError::OverflowingFallbackRange(range.name));
            }
        }
        for pair in self.fallback_memory.windows(2) {
            let previous = pair[0];
            let current = pair[1];
            if current.base < previous.base + previous.size {
                return Err(ValidationError::OverlappingFallbackRange(
                    previous.name,
                    current.name,
                ));
            }
        }
        for (index, region) in self.virtio_mmio.iter().enumerate() {
            if region.size == 0 {
                return Err(ValidationError::ZeroSizedMmioRange(region.compatible));
            }
            if region.base.checked_add(region.size).is_none() {
                return Err(ValidationError::OverflowingMmioRange(region.compatible));
            }
            if let Some(previous) = index.checked_sub(1).map(|i| self.virtio_mmio[i]) {
                if region.base <= previous.base {
                    return Err(ValidationError::UnsortedMmioRange(
                        previous.compatible,
                        region.compatible,
                    ));
                }
            }
        }
        Ok(())
    }

    /// Validates the descriptor and confirms that it targets `expected`.
    ///
    /// Returns the first structural error or an architecture mismatch.
    pub fn validate_for(&self, expected: Architecture) -> Result<(), ValidationError> {
        self.validate()?;
        if self.architecture != expected {
            return Err(ValidationError::ArchitectureMismatch {
                expected,
                found: self.architecture,
            });
        }
        Ok(())
    }
}
