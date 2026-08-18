use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface, SocId,
    WiringLayout,
};

const COMPATIBLES: [&str; 2] = ["cellos,generic-x86_64-pc", "acpi,pnp0a08"];
const DRIVERS: [DriverId; 6] = [
    DriverId::Uart16550PortIo,
    DriverId::IoApic,
    DriverId::Hpet,
    DriverId::PcieEcam,
    DriverId::NvmePci,
    DriverId::EthernetE1000,
];

/// Generic PC-compatible x86_64 integration contract.
pub const GENERIC_X86_64_PC: BoardDescriptor = BoardDescriptor {
    slug: "generic-x86_64-pc",
    vendor: "pc-compatible",
    model: "ACPI x86_64 PC",
    architecture: Architecture::X86_64,
    soc: SocId::GenericX86Pc,
    compatibles: &COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::BiosOrUefi,
        boot_protocol: BootProtocol::LimineMemoryMapAndAcpi,
        requires_firmware_dtb: false,
        fallback_dts_path: "",
        kernel_load_base: 0,
    },
    fallback_memory: &[],
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &["legacy-com1", "acpi-pcie-root"],
    },
    enabled_drivers: &DRIVERS,
};
