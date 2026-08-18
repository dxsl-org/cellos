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
    GenericX86Pc,
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
    RtcPl031,
    VirtioMmio,
    PcieEcam,
    SdhciArasan,
    SdhciDwCqe,
    GpioBcm2837,
    GpioBcm2711,
    Uart16550PortIo,
    IoApic,
    Hpet,
    NvmePci,
    EthernetE1000,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareInterface {
    OpenSbi,
    Uefi,
    VideoCore,
    BiosOrUefi,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootProtocol {
    FlattenedDeviceTree,
    DeviceTreeWithFallbackMap,
    LimineMemoryMapAndAcpi,
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
    pub kernel_load_base: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRange {
    pub name: &'static str,
    pub base: u64,
    pub size: u64,
    pub kind: MemoryRangeKind,
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
    pub wiring: WiringLayout,
    pub enabled_drivers: &'static [DriverId],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError {
    EmptyCompatibles,
    MissingFallbackDts,
    MissingFallbackMemory,
    MissingEnabledDrivers,
    KernelLoadOutsideFallback,
    DuplicateEnabledDriver(DriverId),
    ZeroSizedFallbackRange(&'static str),
    OverflowingFallbackRange(&'static str),
    OverlappingFallbackRange(&'static str, &'static str),
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
        let limine_memory_map = matches!(
            self.boot.boot_protocol,
            BootProtocol::LimineMemoryMapAndAcpi
        );
        if !limine_memory_map && self.boot.fallback_dts_path.is_empty() {
            return Err(ValidationError::MissingFallbackDts);
        }
        if !limine_memory_map && self.fallback_memory.is_empty() {
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
        if !limine_memory_map
            && !self.fallback_memory.iter().any(|range| {
                range.kind == MemoryRangeKind::Kernel
                    && self.boot.kernel_load_base >= range.base
                    && self.boot.kernel_load_base < range.base.saturating_add(range.size)
            })
        {
            return Err(ValidationError::KernelLoadOutsideFallback);
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
