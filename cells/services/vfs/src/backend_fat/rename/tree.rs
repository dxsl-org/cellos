//! Moving a directory to another parent.
//!
//! A FAT directory's `..` entry must name its parent, and fatfs's rename
//! leaves it unchanged, so the directory is rebuilt under the new parent
//! instead: a new directory, its files relinked without copying data, its
//! subdirectories rebuilt the same way, the emptied source removed. A failure
//! part way leaves the tree split between both names. The tree is surveyed
//! first and refused (`Unsupported`) beyond the recursive delete's bounds or
//! for a name a directory entry could not be written under again.

use alloc::string::String;
use alloc::vec::Vec;

use super::super::{dir_entries, fat_error, DeviceError, REMOVE_MAX_DEPTH, REMOVE_MAX_ENTRIES};
use super::{long_name_permitted, Dir};
use crate::fs_error::FsError;

/// Check, before anything changes, the tree [`move_tree`] will rebuild.
pub(super) fn survey<IO, TP, OCC>(dir: &Dir<'_, IO, TP, OCC>) -> Result<(), FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
{
    let mut pending = alloc::vec![(dir.clone(), 0usize)];
    let mut seen = 0usize;
    while let Some((dir, depth)) = pending.pop() {
        for entry in dir_entries(&dir) {
            let entry = entry?;
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            seen += 1;
            // U+FFFD stands for a name fatfs could not decode, which it would
            // then not find again by that spelling.
            if seen > REMOVE_MAX_ENTRIES || name.contains('\u{FFFD}') || !long_name_permitted(&name)
            {
                return Err(FsError::Unsupported);
            }
            if entry.is_dir() {
                if depth >= REMOVE_MAX_DEPTH {
                    return Err(FsError::Unsupported);
                }
                pending.push((entry.to_dir(), depth + 1));
            }
        }
    }
    Ok(())
}

/// Rebuild directory `name` of `from` as `new_name` in `to`, then remove the
/// emptied source. Recursion depth is bounded by [`survey`].
pub(super) fn move_tree<IO, TP, OCC>(
    from: &Dir<'_, IO, TP, OCC>,
    name: &str,
    to: &Dir<'_, IO, TP, OCC>,
    new_name: &str,
) -> Result<(), FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
    IO::Error: DeviceError,
{
    {
        let source = from.open_dir(name).map_err(fat_error)?;
        let target = to.create_dir(new_name).map_err(fat_error)?;
        for (child, is_dir) in children(&source)? {
            if is_dir {
                move_tree(&source, &child, &target, &child)?;
            } else {
                source.rename(&child, &target, &child).map_err(fat_error)?;
            }
        }
    }
    from.remove(name).map_err(fat_error)
}

/// The children of `dir` other than `.` and `..`: long name, is a directory.
fn children<IO, TP, OCC>(dir: &Dir<'_, IO, TP, OCC>) -> Result<Vec<(String, bool)>, FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
{
    let mut out = Vec::new();
    for entry in dir_entries(dir) {
        let entry = entry?;
        let name = entry.file_name();
        if name != "." && name != ".." {
            out.push((name, entry.is_dir()));
        }
    }
    Ok(out)
}
