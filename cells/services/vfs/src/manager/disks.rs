//! Disk-backed mounts: the primary disk's fixed partition set and one FAT32
//! volume per secondary disk at `/mnt/disk/<name>`.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use crate::backend_bin_overlay::BinOverlay;
use crate::backend_cellosfs::CellosFsBackend;
use crate::backend_fat::FatBackend;
use crate::blk_router::{DeviceRef, Discovery};
use crate::mount::MountTable;

/// Parent of every secondary-disk mount point.
pub(crate) const SECONDARY_ROOT: &str = "/mnt/disk";

/// One disk-backed mount, in creation order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DiskMount {
    /// FAT volume at `base_lba`; only secondary-disk prefixes are built at
    /// runtime. A `secondary` volume that does not open is not mounted.
    Fat {
        prefix: Cow<'static, str>,
        dev: DeviceRef,
        base_lba: u64,
        secondary: bool,
    },
    CellosFs {
        prefix: &'static str,
        dev: DeviceRef,
        base_lba: u64,
        sectors: u64,
    },
    /// `/bin`: VIFS1 unioned with the FAT cell-store at `base_lba`.
    BinOverlay { dev: DeviceRef, base_lba: u64 },
}

/// The mounts for `discovery`: the primary disk carries exactly `/mnt/sd` (P1),
/// `/data` (P4), the `/bin` overlay (P6) and `/srv` (P5); each secondary only a
/// FAT32 volume at the P1 offset.
pub(crate) fn disk_mounts(discovery: &Discovery) -> Vec<DiskMount> {
    use api::disk::*;
    let dev = discovery.primary;
    let mut plan = alloc::vec![
        // FAT32 P1 remains the interoperability/config override volume.
        DiskMount::Fat {
            prefix: Cow::Borrowed("/mnt/sd"),
            dev,
            base_lba: PART_FAT32_BASE_LBA,
            secondary: false
        },
        // Native P4/P5 require explicit offline provisioning; runtime never formats.
        DiskMount::CellosFs {
            prefix: "/data",
            dev,
            base_lba: PART_LFS_BASE_LBA,
            sectors: PART_LFS_SECTORS
        },
        DiskMount::BinOverlay {
            dev,
            base_lba: PART_CELLSTORE_BASE_LBA
        },
        // /srv P5: existing native V1 volume, not an admitted durable journal.
        DiskMount::CellosFs {
            prefix: "/srv",
            dev,
            base_lba: PART_SRV_BASE_LBA,
            sectors: PART_SRV_SECTORS
        },
    ];
    plan.extend(
        discovery
            .secondaries
            .iter()
            .map(|(dev, name)| DiskMount::Fat {
                prefix: Cow::Owned(format!("{SECONDARY_ROOT}/{name}")),
                dev: *dev,
                base_lba: PART_FAT32_BASE_LBA,
                secondary: true,
            }),
    );
    plan
}

/// Create and mount every entry of `plan`; longest prefix wins over `/`.
pub(crate) fn mount_disks(mounts: &mut MountTable, plan: Vec<DiskMount>) {
    for mount in plan {
        match mount {
            DiskMount::Fat {
                prefix,
                dev,
                base_lba,
                secondary,
            } => {
                let fat = FatBackend::mount(&prefix, dev, base_lba);
                if secondary && !fat.is_mounted() {
                    ostd::io::println(&format!("[vfs] {prefix}: no FAT32 volume"));
                    continue;
                }
                let index = mounts.add_backend(Box::new(fat));
                mounts.mount(&prefix, index);
            }
            DiskMount::CellosFs {
                prefix,
                dev,
                base_lba,
                sectors,
            } => {
                let index = mounts.add_backend(Box::new(CellosFsBackend::mount(
                    prefix, dev, base_lba, sectors,
                )));
                mounts.mount(prefix, index);
            }
            DiskMount::BinOverlay { dev, base_lba } => {
                // VIFS1 ramdisk (bootstrap cells) unioned with the on-disk FAT
                // cell-store (non-bootstrap cells).
                let index = mounts.add_backend(Box::new(BinOverlay::new(dev, base_lba)));
                mounts.mount("/bin", index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{disk_mounts, DiskMount};
    use crate::blk_router::{DeviceRef, Discovery};
    use alloc::string::String;
    use alloc::vec::Vec;
    use api::disk::*;

    fn prefixes(plan: &[DiskMount]) -> Vec<&str> {
        plan.iter()
            .map(|m| match m {
                DiskMount::Fat { prefix, .. } => prefix.as_ref(),
                DiskMount::CellosFs { prefix, .. } => prefix,
                DiskMount::BinOverlay { .. } => "/bin",
            })
            .collect()
    }

    #[test]
    fn primary_mounts_exactly_the_fixed_set_on_its_device() {
        let primary = DeviceRef::Device(1);
        let plan = disk_mounts(&Discovery {
            primary,
            secondaries: Vec::new(),
        });
        assert_eq!(prefixes(&plan), ["/mnt/sd", "/data", "/bin", "/srv"]);
        assert_eq!(
            plan[0],
            DiskMount::Fat {
                prefix: "/mnt/sd".into(),
                dev: primary,
                base_lba: PART_FAT32_BASE_LBA,
                secondary: false
            }
        );
        assert_eq!(
            plan[1],
            DiskMount::CellosFs {
                prefix: "/data",
                dev: primary,
                base_lba: PART_LFS_BASE_LBA,
                sectors: PART_LFS_SECTORS
            }
        );
        assert_eq!(
            plan[2],
            DiskMount::BinOverlay {
                dev: primary,
                base_lba: PART_CELLSTORE_BASE_LBA
            }
        );
        assert_eq!(
            plan[3],
            DiskMount::CellosFs {
                prefix: "/srv",
                dev: primary,
                base_lba: PART_SRV_BASE_LBA,
                sectors: PART_SRV_SECTORS
            }
        );
    }

    #[test]
    fn kernel_primary_keeps_the_fixed_set_and_no_secondaries() {
        let plan = disk_mounts(&Discovery {
            primary: DeviceRef::Kernel,
            secondaries: Vec::new(),
        });
        assert_eq!(prefixes(&plan), ["/mnt/sd", "/data", "/bin", "/srv"]);
        assert!(plan.iter().all(|m| match m {
            DiskMount::Fat { dev, .. }
            | DiskMount::CellosFs { dev, .. }
            | DiskMount::BinOverlay { dev, .. } => *dev == DeviceRef::Kernel,
        }));
    }

    #[test]
    fn each_secondary_gets_only_a_fat32_volume_under_mnt_disk() {
        let secondaries = alloc::vec![
            (DeviceRef::Device(0), String::from("pci-00-1f-2")),
            (DeviceRef::Device(2), String::from("mmio-10001000")),
        ];
        let plan = disk_mounts(&Discovery {
            primary: DeviceRef::Device(1),
            secondaries,
        });
        assert_eq!(
            &plan[4..],
            [
                DiskMount::Fat {
                    prefix: "/mnt/disk/pci-00-1f-2".into(),
                    dev: DeviceRef::Device(0),
                    base_lba: PART_FAT32_BASE_LBA,
                    secondary: true
                },
                DiskMount::Fat {
                    prefix: "/mnt/disk/mmio-10001000".into(),
                    dev: DeviceRef::Device(2),
                    base_lba: PART_FAT32_BASE_LBA,
                    secondary: true
                },
            ]
        );
        assert!(plan[..4].iter().all(|m| match m {
            DiskMount::Fat { dev, .. }
            | DiskMount::CellosFs { dev, .. }
            | DiskMount::BinOverlay { dev, .. } => *dev == DeviceRef::Device(1),
        }));
    }
}
