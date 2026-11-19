//! Where a FAT file's bytes live on the volume, cached per file.
//!
//! fatfs seeks by walking the cluster chain from the file's first cluster on
//! every seek (rust-fatfs `File::seek`), and the backend opens and seeks the
//! file once per read: a read near the end of a 250 MB image walks ~60 000 FAT
//! entries before it moves a byte. An [`ExtentMap`] walks the chain once and
//! merges contiguous clusters into runs, so a later read maps its offset with a
//! binary search and reads the device directly.
//!
//! Invariant: a map is valid while its file's cluster chain is unchanged. A
//! write past a file's mapped size can extend its chain, so the backend drops
//! that file's map; a removal or truncation frees clusters another file may
//! reuse, so it drops every map.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// A run of file bytes stored contiguously on the volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    /// File offset of the run's first byte.
    file: u64,
    /// Volume offset (bytes from the partition start) of that byte.
    disk: u64,
    len: u64,
}

/// A whole file's runs, in file order, covering exactly `size` bytes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ExtentMap {
    size: u64,
    runs: Vec<Run>,
}

/// Files fragmented into more runs than this are read through fatfs instead:
/// the map would cost more heap than the walk it saves.
const MAX_RUNS: usize = 4096;

impl ExtentMap {
    /// Merge `(volume offset, length)` extents in file order, up to the first
    /// zero-length one; `None` when the file is more fragmented than
    /// [`MAX_RUNS`]. fatfs `File::extents` walks the whole cluster chain and
    /// yields zero-length extents past the file's size, forever on a chain that
    /// loops, so the size is what bounds the walk.
    pub(crate) fn build(extents: impl IntoIterator<Item = (u64, u64)>) -> Option<Self> {
        let mut runs: Vec<Run> = Vec::new();
        let mut size = 0u64;
        for (disk, len) in extents {
            // A zero-length extent is past the file's end (see the type docs).
            if len == 0 {
                break;
            }
            match runs.last_mut() {
                Some(last) if last.disk + last.len == disk => last.len += len,
                _ => {
                    if runs.len() == MAX_RUNS {
                        return None;
                    }
                    runs.push(Run {
                        file: size,
                        disk,
                        len,
                    });
                }
            }
            size += len;
        }
        Some(Self { size, runs })
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    /// The volume byte ranges holding file bytes `offset..offset + len`, in
    /// order, clipped to the file's end.
    pub(crate) fn pieces(&self, offset: u64, len: u64) -> impl Iterator<Item = (u64, u64)> + '_ {
        let end = offset.saturating_add(len).min(self.size);
        let first = self
            .runs
            .partition_point(|run| run.file + run.len <= offset);
        self.runs[first..]
            .iter()
            .take_while(move |run| run.file < end)
            .map(move |run| {
                let start = offset.max(run.file);
                let stop = end.min(run.file + run.len);
                (run.disk + (start - run.file), stop - start)
            })
    }
}

/// A file's identity on the volume: the volume offset of its first cluster.
/// Every path spelling fatfs resolves to the file (`a//b`, a short 8.3 name,
/// another case) yields the same key.
pub(crate) type FileKey = u64;

/// Maps of the most recently read files; `None` records a file that cannot be
/// mapped, so its reads go straight to fatfs instead of walking again.
pub(crate) struct ExtentCache {
    entries: VecDeque<(FileKey, Option<Arc<ExtentMap>>)>,
}

/// Files whose maps are kept: a guest reads one or two images at a time.
const CACHED_FILES: usize = 4;

impl ExtentCache {
    pub(crate) const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    /// `None` when `key` is not cached; `Some(None)` when it is unmappable.
    pub(crate) fn get(&mut self, key: FileKey) -> Option<Option<Arc<ExtentMap>>> {
        let at = self.position(key)?;
        let entry = self.entries.remove(at)?;
        let map = entry.1.clone();
        self.entries.push_front(entry);
        Some(map)
    }

    pub(crate) fn insert(&mut self, key: FileKey, map: Option<Arc<ExtentMap>>) {
        self.remove(key);
        if self.entries.len() == CACHED_FILES {
            self.entries.pop_back();
        }
        self.entries.push_front((key, map));
    }

    /// Drop `key`'s entry when a write ending at byte `end` may have extended
    /// its chain; a write inside the mapped size changes no cluster.
    pub(crate) fn written(&mut self, key: FileKey, end: u64) {
        if let Some(at) = self.position(key) {
            let inside = self.entries[at]
                .1
                .as_ref()
                .is_some_and(|map| end <= map.size());
            if !inside {
                self.entries.remove(at);
            }
        }
    }

    fn remove(&mut self, key: FileKey) {
        if let Some(at) = self.position(key) {
            self.entries.remove(at);
        }
    }

    fn position(&self, key: FileKey) -> Option<usize> {
        self.entries.iter().position(|(k, _)| *k == key)
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contiguous_clusters_merge_and_pieces_split_at_run_edges() {
        // Clusters of 4096: two contiguous, a jump, then a short last cluster.
        let map = ExtentMap::build([(8192, 4096), (12288, 4096), (40960, 100)]).unwrap();
        assert_eq!(map.size(), 8292);
        assert_eq!(map.runs.len(), 2);
        let pieces: Vec<_> = map.pieces(8000, 1000).collect();
        assert_eq!(
            pieces,
            [(8192 + 8000, 192), (40960, 100)],
            "clipped at the end"
        );
        assert_eq!(map.pieces(0, 512).collect::<Vec<_>>(), [(8192, 512)]);
        assert_eq!(map.pieces(8292, 10).count(), 0, "at end of file");
    }

    #[test]
    fn a_file_too_fragmented_to_map_is_refused() {
        let scattered = (0..=MAX_RUNS as u64).map(|k| (k * 8192, 4096));
        assert!(ExtentMap::build(scattered).is_none());
        let empty = ExtentMap::build([]).unwrap();
        assert_eq!(empty.pieces(0, 10).count(), 0);
    }

    #[test]
    fn a_chain_that_loops_past_the_file_end_stops_at_its_size() {
        let looping = [(8192, 4096)]
            .into_iter()
            .chain(core::iter::repeat((8192, 0)));
        let map = ExtentMap::build(looping).unwrap();
        assert_eq!(map.size(), 4096);
    }

    #[test]
    fn the_cache_keeps_recent_files_and_drops_one_a_write_may_extend() {
        let map = || Some(Arc::new(ExtentMap::build([(0, 4096)]).unwrap()));
        let mut cache = ExtentCache::new();
        for key in 0..=CACHED_FILES as u64 {
            cache.insert(key, map());
        }
        assert!(cache.get(0).is_none(), "oldest evicted");
        cache.insert(9, None);
        assert_eq!(cache.get(9), Some(None), "unmappable is remembered");

        cache.written(4, 4096);
        assert!(cache.get(4).is_some(), "a write inside the map keeps it");
        cache.written(4, 4097);
        assert!(cache.get(4).is_none(), "a write past it drops it");
        cache.written(9, 1);
        assert!(cache.get(9).is_none(), "an unmappable file may now map");
        cache.clear();
        assert!(cache.get(3).is_none());
    }
}
