use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DriverId, FirmwareInterface, SocId,
    WiringLayout,
};

const COMPATIBLES: [&str; 1] = ["cellos,x86_64-pc"];

/// Drivers a generic x86_64 PC declares **today**.
///
/// Only mechanisms that exist in this tree are listed: `has_driver` gates real
/// kernel init, so listing a driver whose cell has not landed would claim an
/// initialisation that cannot happen. The storage (`StorageAhci`), USB
/// (`UsbXhci`), NIC (`EthernetIgb`) and extra-serial (`Uart16550Multi`) variants
/// are declared in `boards/src/descriptor.rs` and are added to this list by the
/// phase that ships their driver cell.
const DRIVERS: [DriverId; 6] = [
    DriverId::Uart16550PortIo,
    DriverId::IoApic,
    DriverId::Hpet,
    DriverId::PcieEcam,
    DriverId::NvmePci,
    DriverId::EthernetE1000,
];

/// Generic x86_64 PC/server **compatibility contract** (HCL R1–R7).
///
/// Declares the baseline a machine must expose to be listed in
/// `docs/hardware-compatibility-list.md` — standard legacy COM1 wiring, standard
/// firmware windows, the Limine ACPI boot path — and only drivers whose cells
/// exist. It is not a claim that every PC has that wiring: a machine exposing a
/// different console address, or failing any mandatory requirement, is rejected
/// (R1) rather than represented by this descriptor.
pub const X86_64_PC: BoardDescriptor = BoardDescriptor {
    slug: "x86_64-pc",
    vendor: "generic",
    model: "x86_64-pc",
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
        phy_links: &["legacy-com1", "pcie-root"],
    },
    enabled_drivers: &DRIVERS,
};
