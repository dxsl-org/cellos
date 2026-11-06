use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DmaIsolation, DriverId,
    FirmwareInterface, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 1] = ["cellos,x86_64-vm"];

/// Drivers a generic x86_64 guest may use: the PC set plus virtio over PCI.
/// HPET is listed because many hypervisors offer it; the timer path no longer
/// requires it (phase 02 selects TSC, kvmclock, HPET or the PM timer).
const DRIVERS: [DriverId; 13] = [
    DriverId::Uart16550PortIo,
    DriverId::IoApic,
    DriverId::Hpet,
    DriverId::PcieEcam,
    DriverId::NvmePci,
    DriverId::StorageAhci,
    DriverId::UsbXhci,
    DriverId::EthernetE1000,
    DriverId::EthernetIgb,
    DriverId::EthernetE1000e,
    DriverId::EthernetVmxnet3,
    DriverId::StoragePvscsi,
    DriverId::VirtioPci,
];

/// Generic x86_64 **virtual machine** contract (ADR-0031 D-VM).
///
/// The same wiring as `x86_64-pc`, but DMA isolation is `Optional`: a default
/// KVM or VMware guest has no vIOMMU, and its devices are emulated by the
/// hypervisor that already owns guest memory. The weaker contract is reachable
/// only when the operator names it (`cellos.profile=x86_64-vm`) *and* CPUID
/// reports a hypervisor; it is never chosen from detected facts. When a vIOMMU
/// is present it is still activated, and virtio cells then require
/// `VIRTIO_F_ACCESS_PLATFORM`.
pub const X86_64_VM: BoardDescriptor = BoardDescriptor {
    slug: "x86_64-vm",
    vendor: "generic",
    model: "x86_64-vm",
    architecture: Architecture::X86_64,
    soc: SocId::GenericX86Vm,
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
    dma_isolation: DmaIsolation::Optional,
};
