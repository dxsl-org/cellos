use crate::{
    fs::{blk::BlockBuffer, Result, FsError},
};

use alloc::vec;
use alloc::vec::Vec;

use super::{Cluster, bpb::BiosParameterBlock};

#[derive(PartialEq, Eq, Debug)]
pub enum FatEntry {
    Eoc,
    NextCluster(Cluster),
    Bad,
    Reserved,
    Free,
}

impl From<u32> for FatEntry {
    fn from(value: u32) -> Self {
        match value & 0x0fffffff {
            0 => Self::Free,
            1 => Self::Reserved,
            n @ 2..=0xFFFFFF6 => Self::NextCluster(Cluster(n)),
            0xFFFFFF7 => Self::Bad,
            0xFFFFFF8..=0xFFFFFFF => Self::Eoc,
            _ => unreachable!("The last nibble has been masked"),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct Fat {
    data: Vec<FatEntry>,
}

pub struct ClusterChainIterator<'a> {
    fat: &'a Fat,
    current_or_next: Option<Cluster>,
}

impl<'a> Iterator for ClusterChainIterator<'a> {
    type Item = Result<Cluster>;

    fn next(&mut self) -> Option<Self::Item> {
        let cluster_to_return = self.current_or_next?;

        let entry = match self.fat.data.get(cluster_to_return.value()) {
            Some(entry) => entry,
            None => {
                self.current_or_next = None;
                return Some(Err(FsError::IoError));
            }
        };

        match entry {
            FatEntry::Eoc => {
                self.current_or_next = None;
            }
            FatEntry::NextCluster(next) => {
                self.current_or_next = Some(*next);
            }
            FatEntry::Bad | FatEntry::Reserved | FatEntry::Free => {
                self.current_or_next = None;
                return Some(Err(FsError::IoError));
            }
        }

        Some(Ok(cluster_to_return))
    }
}

impl Fat {
    pub async fn read_fat(
        dev: &BlockBuffer,
        bpb: &BiosParameterBlock,
        fat_number: usize,
    ) -> Result<Self> {
        let (start, end) = bpb.fat_region(fat_number).ok_or(FsError::InvalidFs)?;

        let mut fat: Vec<FatEntry> = Vec::with_capacity(
            (bpb.sector_offset(end) as usize - bpb.sector_offset(start) as usize) / 4,
        );

        let mut buf = vec![0; bpb.sector_size()];

        for sec in start.sectors_until(end) {
            dev.read_at(bpb.sector_offset(sec), &mut buf).await?;

            fat.extend(
                buf.chunks_exact(4)
                    .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                    .map(|v| v.into()),
            );
        }

        Ok(Self { data: fat })
    }

    pub fn get_cluster_chain(&self, root: Cluster) -> impl Iterator<Item = Result<Cluster>> + '_ {
        ClusterChainIterator {
            fat: self,
            current_or_next: Some(root),
        }
    }
}
