// SPDX-License-Identifier: MPL-2.0
use crate::{
    Architecture, BoardDescriptor, BootContract, BootProtocol, DmaIsolation, DriverId,
    FirmwareInterface, MemoryRange, MemoryRangeKind, SocId, WiringLayout,
};

const COMPATIBLES: [&str; 3] = ["apple,j473", "apple,t8112", "apple,arm-platform"];

const DRIVERS: [DriverId; 2] = [DriverId::UartAppleS5l, DriverId::IrqAppleAic2];

/// Fallback DRAM memory map for Mac mini M2 (t8112, J473).
///
/// DRAM starts at 0x8_0000_0000 (32 GiB base, 39-bit physical address).
const FALLBACK_MEMORY: [MemoryRange; 1] = [MemoryRange {
    name: "dram",
    base: 0x8_0000_0000,
    size: 0x2_0000_0000, // 8 GiB baseline
    kind: MemoryRangeKind::Usable,
}];

/// Mac mini M2 (t8112, J473) descriptor (Phase 09, ADR-0032 §4–5).
///
/// Bound when booting bare metal under m1n1 stage 2 / hypervisor.
/// Requires DART DMA isolation (`DmaIsolation::Required`).
pub const MACMINI_M2: BoardDescriptor = BoardDescriptor {
    slug: "macmini-m2",
    vendor: "apple",
    model: "Mac mini (M2, 2023)",
    architecture: Architecture::Aarch64,
    soc: SocId::AppleT8112,
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
        phy_links: &["uart-s5l", "aic2"],
    },
    enabled_drivers: &DRIVERS,
    dma_isolation: DmaIsolation::Required,
};
