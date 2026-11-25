//! `backend_fat::rename` on an in-memory FAT volume.

use alloc::string::String;
use alloc::vec::Vec;

use fatfs::Write as _;

use super::rename;
use crate::fs_error::FsError;

/// In-memory FAT image.
pub(crate) struct MemDisk {
    data: Vec<u8>,
    pos: usize,
}

impl fatfs::IoBase for MemDisk {
    type Error = ();
}

impl fatfs::Read for MemDisk {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl fatfs::Write for MemDisk {
    fn write(&mut self, buf: &[u8]) -> Result<usize, ()> {
        let n = buf.len().min(self.data.len() - self.pos);
        self.data[self.pos..self.pos + n].copy_from_slice(&buf[..n]);
        self.pos += n;
        Ok(n)
    }

    fn flush(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl fatfs::Seek for MemDisk {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, ()> {
        let target = match pos {
            fatfs::SeekFrom::Start(offset) => i64::try_from(offset).map_err(|_| ())?,
            fatfs::SeekFrom::End(delta) => self.data.len() as i64 + delta,
            fatfs::SeekFrom::Current(delta) => self.pos as i64 + delta,
        };
        if target < 0 || target as usize > self.data.len() {
            return Err(());
        }
        self.pos = target as usize;
        Ok(self.pos as u64)
    }
}

pub(crate) type MemFs =
    fatfs::FileSystem<MemDisk, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>;

/// A freshly formatted 4 MiB FAT volume.
pub(crate) fn volume() -> MemFs {
    let mut disk = MemDisk {
        data: alloc::vec![0; 4 << 20],
        pos: 0,
    };
    fatfs::format_volume(&mut disk, fatfs::FormatVolumeOptions::new()).expect("format");
    disk.pos = 0;
    fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).expect("mount")
}

fn put(fs: &MemFs, path: &str, bytes: &[u8]) {
    let mut file = fs.root_dir().create_file(path).expect("create");
    file.write_all(bytes).expect("write");
}

fn get(fs: &MemFs, path: &str) -> Option<Vec<u8>> {
    let mut file = fs.root_dir().open_file(path).ok()?;
    super::super::read_to_end(&mut file).ok()
}

fn mkdir(fs: &MemFs, path: &str) {
    fs.root_dir().create_dir(path).expect("mkdir");
}

/// The names in directory `path` (`""` is the root), sorted, without `.`/`..`.
fn names(fs: &MemFs, path: &str) -> Vec<String> {
    let root = fs.root_dir();
    let dir = if path.is_empty() {
        root
    } else {
        root.open_dir(path).expect("dir")
    };
    let mut out: Vec<String> = dir
        .iter()
        .map(|entry| entry.expect("entry").file_name())
        .filter(|name| name != "." && name != "..")
        .collect();
    out.sort();
    out
}

#[test]
fn a_file_moves_within_its_directory_and_into_another() {
    let fs = volume();
    put(&fs, "a.txt", b"A");
    mkdir(&fs, "d");
    assert_eq!(rename(&fs, "a.txt", "b.txt"), Ok(()));
    assert_eq!(get(&fs, "b.txt").as_deref(), Some(&b"A"[..]));
    assert_eq!(get(&fs, "a.txt"), None);
    assert_eq!(rename(&fs, "b.txt", "d/c.txt"), Ok(()));
    assert_eq!(get(&fs, "d/c.txt").as_deref(), Some(&b"A"[..]));
    assert_eq!(names(&fs, ""), ["d"]);
    assert_eq!(rename(&fs, "missing", "x"), Err(FsError::NotFound));
    assert_eq!(rename(&fs, "d/c.txt", "nowhere/c"), Err(FsError::NotFound));
}

#[test]
fn a_file_or_an_empty_directory_is_replaced_but_never_a_non_empty_one() {
    let fs = volume();
    put(&fs, "new", b"N");
    put(&fs, "old", b"O");
    assert_eq!(rename(&fs, "new", "old"), Ok(()));
    assert_eq!(get(&fs, "old").as_deref(), Some(&b"N"[..]));
    assert_eq!(get(&fs, "new"), None);

    mkdir(&fs, "src");
    put(&fs, "src/x", b"X");
    mkdir(&fs, "empty");
    assert_eq!(rename(&fs, "src", "empty"), Ok(()));
    assert_eq!(get(&fs, "empty/x").as_deref(), Some(&b"X"[..]));

    mkdir(&fs, "full");
    put(&fs, "full/f", b"F");
    mkdir(&fs, "other");
    assert_eq!(
        rename(&fs, "other", "full"),
        Err(FsError::DirectoryNotEmpty)
    );
    assert_eq!(names(&fs, ""), ["empty", "full", "old", "other"]);
    assert_eq!(get(&fs, "full/f").as_deref(), Some(&b"F"[..]));
}

#[test]
fn kind_mismatches_and_a_move_into_its_own_subtree_change_nothing() {
    let fs = volume();
    put(&fs, "f", b"F");
    mkdir(&fs, "d");
    mkdir(&fs, "d/sub");
    assert_eq!(rename(&fs, "f", "d"), Err(FsError::IsADirectory));
    assert_eq!(rename(&fs, "d", "f"), Err(FsError::NotADirectory));
    assert_eq!(rename(&fs, "d", "d/sub/e"), Err(FsError::InvalidPath));
    // FAT matches names without case, so another spelling is the same tree.
    assert_eq!(rename(&fs, "d", "D/SUB/e"), Err(FsError::InvalidPath));
    assert_eq!(rename(&fs, "d", "d/e"), Err(FsError::InvalidPath));
    assert_eq!(rename(&fs, "d", "d"), Ok(()));
    assert_eq!(names(&fs, ""), ["d", "f"]);
    assert_eq!(names(&fs, "d"), ["sub"]);
    assert_eq!(get(&fs, "f").as_deref(), Some(&b"F"[..]));
}

/// A FAT directory's `..` must name its parent: a directory moved to another
/// parent, and every directory below it, resolve `..` to the new place.
#[test]
fn a_directory_moved_to_another_parent_names_its_new_parent() {
    let fs = volume();
    mkdir(&fs, "a");
    mkdir(&fs, "a/tree");
    mkdir(&fs, "a/tree/deep");
    put(&fs, "a/tree/f", b"F");
    put(&fs, "a/tree/deep/g", b"G");
    mkdir(&fs, "b");
    assert_eq!(rename(&fs, "a/tree", "b/moved"), Ok(()));
    assert_eq!(names(&fs, "a"), Vec::<String>::new());
    assert_eq!(get(&fs, "b/moved/f").as_deref(), Some(&b"F"[..]));
    assert_eq!(get(&fs, "b/moved/deep/g").as_deref(), Some(&b"G"[..]));
    assert_eq!(names(&fs, "b/moved/.."), ["moved"]);
    assert_eq!(names(&fs, "b/moved/deep/.."), ["deep", "f"]);

    assert_eq!(rename(&fs, "b/moved", "top"), Ok(()));
    assert_eq!(names(&fs, "top/.."), ["a", "b", "top"]);
    assert_eq!(names(&fs, "top/deep/.."), ["deep", "f"]);
}

/// fatfs checks a long name only after it has freed the source entry.
#[test]
fn a_name_fat_cannot_hold_is_refused_before_the_source_is_touched() {
    let fs = volume();
    put(&fs, "keep", b"K");
    let bad_names = ["a:b", "q?", "star*", "pipe|", "\"q\"", "<lt>", "\u{1F600}"];
    for bad in bad_names {
        assert_eq!(rename(&fs, "keep", bad), Err(FsError::InvalidPath), "{bad}");
    }
    assert_eq!(names(&fs, ""), ["keep"]);
    assert_eq!(get(&fs, "keep").as_deref(), Some(&b"K"[..]));
}

#[test]
fn a_change_of_case_is_applied_and_another_alias_of_the_entry_changes_nothing() {
    let fs = volume();
    put(&fs, "Report.txt", b"R");
    assert_eq!(rename(&fs, "Report.txt", "report.TXT"), Ok(()));
    assert_eq!(names(&fs, ""), ["report.TXT"]);
    assert_eq!(get(&fs, "report.TXT").as_deref(), Some(&b"R"[..]));

    put(&fs, "Long File Name.txt", b"L");
    let short = fs
        .root_dir()
        .iter()
        .map(|entry| entry.expect("entry"))
        .find(|entry| entry.file_name() == "Long File Name.txt")
        .expect("entry")
        .short_file_name();
    assert_ne!(short, "Long File Name.txt");
    assert_eq!(rename(&fs, &short, "Long File Name.txt"), Ok(()));
    assert_eq!(rename(&fs, "Long File Name.txt", &short), Ok(()));
    assert_eq!(names(&fs, ""), ["Long File Name.txt", "report.TXT"]);
}
