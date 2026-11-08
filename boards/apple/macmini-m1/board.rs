// SPDX-License-Identifier: MPL-2.0
use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DmaIsolation, DriverId,
    FirmwareInterface, MemoryRange, MemoryRangeKind, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 3] = ["apple,j274", "apple,t8103", "apple,arm-platform"];

const DRIVERS: [DriverId; 2] = [DriverId::UartAppleS5l, DriverId::IrqAppleAic1];

/// Fallback DRAM memory map for Mac mini M1 (t8103, J274) when m1n1 passes memory.
///
/// DRAM starts at 0x8_0000_0000 (32 GiB base, 39-bit physical address).
const FALLBACK_MEMORY: [MemoryRange; 2] = [
    MemoryRange {
        name: "kernel",
        base: 0x8_0008_0000,
        size: 0x0_0200_0000, // 32 MiB kernel span
        kind: MemoryRangeKind::Kernel,
    },
    MemoryRange {
        name: "usable",
        base: 0x8_0208_0000,
        size: 0x1_FDF8_0000, // 8 GiB baseline usable RAM
        kind: MemoryRangeKind::Usable,
    },
];

/// Mac mini M1 (t8103, J274) descriptor (Phase 04, ADR-0032 §4–5).
///
/// Bound when booting bare metal under m1n1 stage 2 / hypervisor.
/// Requires DART DMA isolation (`DmaIsolation::Required`).
pub const MACMINI_M1: BoardDescriptor = BoardDescriptor {
    slug: "macmini-m1",
    vendor: "apple",
    model: "Mac mini (M1, 2020)",
    architecture: Architecture::Aarch64,
    soc: SocId::AppleT8103,
    compatibles: &COMPATIBLES,
    boot: BootContract {
        firmware: FirmwareInterface::Uefi,
        boot_protocol: BootProtocol::DeviceTreeWithFallbackMap,
        requires_firmware_dtb: true,
        fallback_dts_path: "",
        kernel_load_base: 0x8_0008_0000,
    },
    fallback_memory: &FALLBACK_MEMORY,
    wiring: WiringLayout {
        pinmux_groups: &[],
        phy_links: &["uart-s5l", "aic1"],
    },
    enabled_drivers: &DRIVERS,
    dma_isolation: DmaIsolation::Required,
};
