use crate::{
    fs::{blk::BlockBuffer, Result, pod::Pod, FsError},
};
use log::warn;

use super::{Cluster, Sector};

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)] // Added Clone/Copy for Pod safety if needed
pub struct BiosParameterBlock {
    _jump: [u8; 3],
    _oem_id: [u8; 8],

    /* DOS 2.0 BPB */
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    // Number of sectors in the Reserved Region. Usually 32.
    pub reserved_sector_count: u16,
    pub num_fats: u8,
    _root_entry_count: u16,
    _total_sectors_16: u16,
    _media_type: u8,
    _fat_size_16: u16,
    _sectors_per_track: u16,
    _head_count: u16,
    _hidden_sector_count: u32,
    _total_sectors_32: u32,

    /* FAT32 Extended BPB */
    // The size of ONE FAT in sectors.
    pub fat_size_32: u32,
    _ext_flags: u16,
    _fs_version: u16,
    // The cluster number where the root directory starts.
    pub root_cluster: Cluster,
    pub fsinfo_sector: u16,
    // More stuff.  Ignored, for now.
}

unsafe impl Pod for BiosParameterBlock {}

impl BiosParameterBlock {
    pub async fn new(dev: &BlockBuffer) -> Result<Self> {
        let bpb: Self = dev.read_obj(0).await?;

        if bpb._fat_size_16 != 0 || bpb._root_entry_count != 0 {
            warn!("Not a FAT32 volume (FAT16 fields are non-zero)");
            return Err(FsError::InvalidFs);
        }

        if bpb.fat_size_32 == 0 {
            warn!("FAT32 size is zero");
            return Err(FsError::InvalidFs);
        }

        if bpb.num_fats == 0 {
            warn!("Volume has 0 FATs, which is invalid.");
            return Err(FsError::InvalidFs);
        }

        let bytes_per_sector = bpb.bytes_per_sector;
        match bytes_per_sector {
            512 | 1024 | 2048 | 4096 => {} // Good!
            _ => {
                warn!(
                    "Bytes per sector {} is not a valid value (must be 512, 1024, 2048, or 4096).",
                    bytes_per_sector
                );
                return Err(FsError::InvalidFs);
            }
        }

        if !bpb.bytes_per_sector.is_power_of_two() {
            let bytes_per_sector = bpb.bytes_per_sector;

            warn!(
                "Bytes per sector 0x{:X} not a power of two.",
                bytes_per_sector
            );
            return Err(FsError::InvalidFs);
        }

        if !bpb.sectors_per_cluster.is_power_of_two() {
            warn!(
                "Sectors per cluster 0x{:X} not a power of two.",
                bpb.sectors_per_cluster
            );
            return Err(FsError::InvalidFs);
        }

        if !bpb.root_cluster.is_valid() {
            let root_cluster = bpb.root_cluster;

            warn!("Root cluster {} < 2.", root_cluster);

            return Err(FsError::InvalidFs);
        }

        Ok(bpb)
    }

    pub fn sector_offset(&self, sector: Sector) -> u64 {
        sector.0 as u64 * self.bytes_per_sector as u64
    }

    pub fn fat_region(&self, fat_number: usize) -> Option<(Sector, Sector)> {
        if fat_number >= self.num_fats as _ {
            None
        } else {
            let start = self.fat_region_start() + self.fat_len() * fat_number;
            let end = start + self.fat_len();

            Some((start, end))
        }
    }

    pub fn fat_region_start(&self) -> Sector {
        Sector(self.reserved_sector_count as _)
    }

    pub fn fat_len(&self) -> Sector {
        Sector(self.fat_size_32 as _)
    }

    pub fn data_region_start(&self) -> Sector {
        self.fat_region_start() + self.fat_len() * self.num_fats as usize
    }

    pub fn sector_size(&self) -> usize {
        self.bytes_per_sector as _
    }

    pub fn cluster_to_sectors(&self, cluster: Cluster) -> Result<impl Iterator<Item = Sector>> {
        if cluster.0 < 2 {
            warn!("Cannot conver sentinel cluster number");
            Err(FsError::InvalidFs)
        } else {
            let root_sector = Sector(
                self.data_region_start().0 + (cluster.0 - 2) * self.sectors_per_cluster as u32,
            );

            Ok(root_sector.sectors_until(Sector(root_sector.0 + self.sectors_per_cluster as u32)))
        }
    }
}
