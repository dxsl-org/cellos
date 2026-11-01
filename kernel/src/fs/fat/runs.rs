//! Physical layout of one FAT file, mapped once so reads never re-walk its chain.

use alloc::vec::Vec;

use fatfs::{File, Read, ReadWriteSeek, Seek, SeekFrom};
use types::{ViError, ViResult};

/// One physically contiguous stretch of a file's data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    /// File offset of the run's first byte.
    file_start: u64,
    /// Volume offset of the run's first byte.
    disk_start: u64,
    len: u64,
}

/// A file's data as volume byte ranges, in file order, covering file offsets
/// `0..mapped_len()` without gaps.
///
/// The map is a snapshot: it stays correct only while neither the file's
/// cluster chain nor its directory-entry size changes.
pub(super) struct RunMap {
    runs: Vec<Run>,
}

impl RunMap {
    /// Walk `file`'s cluster chain once, from its first cluster, and record its
    /// data as merged contiguous runs.
    ///
    /// Mapping stops at the size the directory entry declares, so a chain longer
    /// than the file (or a cyclic one) is not followed past the file's data. A
    /// chain that ends early yields a map shorter than the declared size.
    ///
    /// # Errors
    /// `ViError::IO` when the FAT cannot be read; `ViError::OutOfMemory` when the
    /// run list cannot grow.
    pub(super) fn build<IO: ReadWriteSeek, TP, OCC>(
        file: &mut File<'_, IO, TP, OCC>,
    ) -> ViResult<Self> {
        let mut runs: Vec<Run> = Vec::new();
        let mut mapped = 0u64;
        for extent in file.extents() {
            let extent = extent.map_err(|_| ViError::IO)?;
            // Past the declared size fatfs keeps following the chain with empty extents.
            if extent.size == 0 {
                break;
            }
            let len = u64::from(extent.size);
            match runs.last_mut() {
                Some(last) if last.disk_start + last.len == extent.offset => last.len += len,
                _ => {
                    runs.try_reserve(1).map_err(|_| ViError::OutOfMemory)?;
                    runs.push(Run {
                        file_start: mapped,
                        disk_start: extent.offset,
                        len,
                    });
                }
            }
            mapped += len;
        }
        Ok(Self { runs })
    }

    /// Number of file bytes the map covers.
    #[cfg(test)]
    fn mapped_len(&self) -> u64 {
        self.runs.last().map_or(0, |run| run.file_start + run.len)
    }

    /// Copy file bytes starting at file offset `pos` from `disk` (the volume the
    /// map was built on) into `buf`.
    ///
    /// # Returns
    /// Bytes copied: `buf.len()`, or fewer when the map ends first (0 at or past
    /// its end).
    ///
    /// # Errors
    /// `ViError::IO` when `disk` fails or ends inside a mapped run.
    pub(super) fn read_at<D: Read + Seek>(
        &self,
        disk: &mut D,
        mut pos: u64,
        buf: &mut [u8],
    ) -> ViResult<usize> {
        let mut index = self
            .runs
            .partition_point(|run| run.file_start + run.len <= pos);
        let mut total = 0usize;
        while total < buf.len() {
            let Some(run) = self.runs.get(index) else {
                break;
            };
            let offset_in_run = pos - run.file_start;
            let remaining = buf.len() - total;
            let take = usize::try_from(run.len - offset_in_run)
                .map_or(remaining, |left| left.min(remaining));
            disk.seek(SeekFrom::Start(run.disk_start + offset_in_run))
                .map_err(|_| ViError::IO)?;
            let mut filled = 0usize;
            while filled < take {
                let n = disk
                    .read(&mut buf[total + filled..total + take])
                    .map_err(|_| ViError::IO)?;
                if n == 0 {
                    return Err(ViError::IO);
                }
                filled += n;
            }
            total += take;
            pos += take as u64;
            index += 1;
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    use alloc::vec;
    use core::cell::RefCell;
    use fatfs::{FileSystem, FormatVolumeOptions, FsOptions, IoBase, Write};

    /// Cursor over an in-memory volume. Cursors made by `rewound` share the
    /// bytes, so the volume outlives the `FileSystem` that consumes one cursor.
    struct MemDisk {
        data: Rc<RefCell<Vec<u8>>>,
        pos: usize,
    }

    impl MemDisk {
        fn rewound(&self) -> Self {
            Self {
                data: self.data.clone(),
                pos: 0,
            }
        }
    }

    impl IoBase for MemDisk {
        type Error = ();
    }

    impl Read for MemDisk {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            let data = self.data.borrow();
            let n = buf.len().min(data.len().saturating_sub(self.pos));
            buf[..n].copy_from_slice(&data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    impl Write for MemDisk {
        fn write(&mut self, buf: &[u8]) -> Result<usize, ()> {
            let mut data = self.data.borrow_mut();
            let n = buf.len().min(data.len().saturating_sub(self.pos));
            data[self.pos..self.pos + n].copy_from_slice(&buf[..n]);
            self.pos += n;
            Ok(n)
        }

        fn flush(&mut self) -> Result<(), ()> {
            Ok(())
        }
    }

    impl Seek for MemDisk {
        fn seek(&mut self, pos: SeekFrom) -> Result<u64, ()> {
            let len = self.data.borrow().len() as i64;
            let target = match pos {
                SeekFrom::Start(off) => i64::try_from(off).map_err(|_| ())?,
                SeekFrom::Current(off) => self.pos as i64 + off,
                SeekFrom::End(off) => len + off,
            };
            self.pos = usize::try_from(target).map_err(|_| ())?;
            Ok(self.pos as u64)
        }
    }

    const CLUSTER: usize = 512;

    fn pattern(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| {
                (i as u8)
                    .wrapping_mul(31)
                    .wrapping_add(seed ^ (i >> 9) as u8)
            })
            .collect()
    }

    /// A volume holding `BIG.BIN` (`big` bytes, its clusters interleaved with
    /// `PAD.BIN`'s so its chain is fragmented) and `ONE.BIN` (contiguous).
    fn volume(big: &[u8], one: &[u8]) -> MemDisk {
        let mut disk = MemDisk {
            data: Rc::new(RefCell::new(vec![0; 2 * 1024 * 1024])),
            pos: 0,
        };
        fatfs::format_volume(
            &mut disk,
            FormatVolumeOptions::new().bytes_per_cluster(CLUSTER as u32),
        )
        .unwrap();
        let fs = FileSystem::new(disk.rewound(), FsOptions::new()).unwrap();
        {
            let root = fs.root_dir();
            let mut big_file = root.create_file("BIG.BIN").unwrap();
            let mut pad_file = root.create_file("PAD.BIN").unwrap();
            for (i, piece) in big.chunks(3 * CLUSTER / 2).enumerate() {
                big_file.write_all(piece).unwrap();
                pad_file.write_all(&[i as u8; CLUSTER]).unwrap();
            }
            root.create_file("ONE.BIN").unwrap().write_all(one).unwrap();
        }
        fs.unmount().unwrap();
        disk.rewound()
    }

    fn map_of(disk: &MemDisk, path: &str) -> RunMap {
        let fs = FileSystem::new(disk.rewound(), FsOptions::new()).unwrap();
        let map = RunMap::build(&mut fs.root_dir().open_file(path).unwrap()).unwrap();
        map
    }

    fn read_in_chunks(map: &RunMap, disk: &mut MemDisk, start: usize, chunk: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; chunk];
        let mut pos = start as u64;
        loop {
            let n = map.read_at(disk, pos, &mut buf).unwrap();
            if n == 0 {
                return out;
            }
            out.extend_from_slice(&buf[..n]);
            pos += n as u64;
        }
    }

    #[test]
    fn fragmented_multi_cluster_file_reads_back_exactly_across_chunk_boundaries() {
        let big = pattern(40 * CLUSTER + 123, 7);
        let mut disk = volume(&big, &pattern(10, 1));
        let map = map_of(&disk, "BIG.BIN");
        assert!(map.runs.len() > 1, "fixture must fragment the chain");
        assert_eq!(map.mapped_len(), big.len() as u64);
        for chunk in [4096, CLUSTER, CLUSTER - 1, CLUSTER + 1, 777, big.len() + 1] {
            assert_eq!(
                read_in_chunks(&map, &mut disk, 0, chunk),
                big,
                "chunk {chunk}"
            );
        }
        for start in [
            1,
            CLUSTER - 1,
            CLUSTER,
            3 * CLUSTER + 5,
            big.len() - 1,
            big.len(),
        ] {
            assert_eq!(
                read_in_chunks(&map, &mut disk, start, 4096),
                big[start..],
                "start {start}"
            );
        }
        assert_eq!(
            map.read_at(&mut disk, big.len() as u64 + 9, &mut [0; 8]),
            Ok(0)
        );
    }

    #[test]
    fn contiguous_file_maps_to_one_run_ending_at_its_size() {
        let one = pattern(5 * CLUSTER + 1, 3);
        let mut disk = volume(&pattern(CLUSTER, 9), &one);
        let map = map_of(&disk, "ONE.BIN");
        assert_eq!(map.runs.len(), 1);
        assert_eq!(map.mapped_len(), one.len() as u64);
        assert_eq!(read_in_chunks(&map, &mut disk, 0, 4096), one);
    }
}
