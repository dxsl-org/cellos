//! Block-device discovery at VFS start: device table, names, primary disk.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::block_route::{DeviceKey, BLOCK_DEVICE_PCI, MAX_BLOCK_ROUTES};
use types::block_wire::SECTOR_SIZE;

use super::router::Router;
use super::{BlockSys, DeviceRef};

/// Partition type of the Cellos cell bootstrap table (P2, `api::disk`).
const CELLOS_PARTITION_TYPE: u8 = 0x7F;

/// Stable device name: `pci-BB-DD-F` for a PCI function, `mmio-{base:x}` otherwise.
pub fn device_name(key: DeviceKey) -> String {
    if key.kind == BLOCK_DEVICE_PCI {
        let bdf = key.id;
        format!(
            "pci-{:02x}-{:02x}-{:x}",
            (bdf >> 8) & 0xff,
            (bdf >> 3) & 0x1f,
            bdf & 7
        )
    } else {
        format!("mmio-{:x}", key.id)
    }
}

/// Whether `mbr` is a signed MBR with a partition entry of the Cellos P2 type.
pub fn has_cellos_partition(mbr: &[u8; SECTOR_SIZE]) -> bool {
    mbr[510] == 0x55
        && mbr[511] == 0xAA
        && (0..4).any(|slot| mbr[446 + slot * 16 + 4] == CELLOS_PARTITION_TYPE)
}

/// Why a device was chosen as the primary disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryReason {
    CellosMbr,
    LowestKey,
}

impl PrimaryReason {
    fn label(self) -> &'static str {
        match self {
            Self::CellosMbr => "cellos mbr",
            Self::LowestKey => "lowest key",
        }
    }
}

/// Index of the primary disk among `(key, carries a Cellos MBR)` entries: the
/// unique Cellos-MBR device, else the lowest `(kind, id)`. `None` when empty.
pub fn choose_primary(devices: &[(DeviceKey, bool)]) -> Option<(usize, PrimaryReason)> {
    let mut marked = devices
        .iter()
        .enumerate()
        .filter(|(_, (_, cellos))| *cellos);
    if let (Some((i, _)), None) = (marked.next(), marked.next()) {
        return Some((i, PrimaryReason::CellosMbr));
    }
    let lowest = devices
        .iter()
        .enumerate()
        .min_by_key(|(_, (key, _))| *key)?
        .0;
    Some((lowest, PrimaryReason::LowestKey))
}

/// The discovered disks: one primary and every other device by name.
pub struct Discovery {
    pub primary: DeviceRef,
    pub secondaries: Vec<(DeviceRef, String)>,
}

/// Rebuild `router`'s device table from the kernel route slots and choose the
/// primary disk. Zero routes leave the kernel block path as the only disk.
pub fn discover(router: &mut Router, sys: &mut impl BlockSys) -> Discovery {
    *router = Router::new();
    for index in 0..MAX_BLOCK_ROUTES as u32 {
        let Some(record) = sys.lookup_route(index) else {
            break;
        };
        let dev = router.add(index, &record);
        let tid = record.provider().map_or(0, |(tid, _, _)| tid);
        sys.log(&format!(
            "[vfs] block route {index} {} tid={tid}",
            router.name(dev)
        ));
    }
    let marks: Vec<(DeviceKey, bool)> = (0..router.devices().len())
        .map(|i| {
            let mut mbr = [0u8; SECTOR_SIZE];
            let dev = DeviceRef::Device(i as u8);
            let cellos = router.read(sys, dev, 0, &mut mbr) && has_cellos_partition(&mbr);
            (router.devices()[i].key, cellos)
        })
        .collect();
    let Some((primary, reason)) = choose_primary(&marks) else {
        sys.log("[vfs] primary disk kernel (no block routes)");
        return Discovery {
            primary: DeviceRef::Kernel,
            secondaries: Vec::new(),
        };
    };
    let primary = DeviceRef::Device(primary as u8);
    sys.log(&format!(
        "[vfs] primary disk {} ({})",
        router.name(primary),
        reason.label()
    ));
    let secondaries = (0..router.devices().len())
        .map(|i| DeviceRef::Device(i as u8))
        .filter(|dev| *dev != primary)
        .map(|dev| (dev, String::from(router.name(dev))))
        .collect();
    Discovery {
        primary,
        secondaries,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{cellos_mbr, AHCI, NVME};
    use super::{choose_primary, device_name, has_cellos_partition, PrimaryReason};
    use api::block_route::{DeviceKey, BLOCK_DEVICE_MMIO, BLOCK_DEVICE_PCI};
    use types::block_wire::SECTOR_SIZE;

    #[test]
    fn names_follow_pci_location_and_mmio_base() {
        assert_eq!(
            device_name(DeviceKey {
                kind: BLOCK_DEVICE_PCI,
                id: AHCI
            }),
            "pci-00-1f-2"
        );
        assert_eq!(
            device_name(DeviceKey {
                kind: BLOCK_DEVICE_PCI,
                id: NVME
            }),
            "pci-03-00-0"
        );
        assert_eq!(
            device_name(DeviceKey {
                kind: BLOCK_DEVICE_PCI,
                id: 0xFFFF
            }),
            "pci-ff-1f-7"
        );
        assert_eq!(
            device_name(DeviceKey {
                kind: BLOCK_DEVICE_MMIO,
                id: 0x1000_1000
            }),
            "mmio-10001000"
        );
    }

    #[test]
    fn cellos_mbr_needs_signature_and_p2_type() {
        let mbr = cellos_mbr();
        assert!(has_cellos_partition(&mbr));
        let mut unsigned = mbr;
        unsigned[511] = 0;
        assert!(!has_cellos_partition(&unsigned));
        let mut fat_only = [0u8; SECTOR_SIZE];
        fat_only[446 + 4] = 0x0C;
        fat_only[510..].copy_from_slice(&[0x55, 0xAA]);
        assert!(!has_cellos_partition(&fat_only));
    }

    #[test]
    fn primary_is_the_unique_cellos_disk_else_the_lowest_key() {
        let pci = |id| DeviceKey {
            kind: BLOCK_DEVICE_PCI,
            id,
        };
        let mmio = DeviceKey {
            kind: BLOCK_DEVICE_MMIO,
            id: 0x1000,
        };
        assert_eq!(
            choose_primary(&[(pci(NVME), false), (mmio, true)]),
            Some((1, PrimaryReason::CellosMbr))
        );
        assert_eq!(
            choose_primary(&[(mmio, false), (pci(NVME), false), (pci(AHCI), false)]),
            Some((2, PrimaryReason::LowestKey))
        );
        assert_eq!(
            choose_primary(&[(pci(NVME), true), (mmio, false), (pci(AHCI), true)]),
            Some((2, PrimaryReason::LowestKey))
        );
        assert_eq!(choose_primary(&[]), None);
    }
}
