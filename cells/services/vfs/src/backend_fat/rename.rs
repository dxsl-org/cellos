//! POSIX `rename` inside one FAT volume, for `FsBackend::rename_replace`.
//!
//! Contract:
//! - files and directories move within the volume, across directories too;
//! - an existing file at the new name is replaced by a file, an existing empty
//!   directory by a directory;
//! - refused before anything changes: a non-empty directory at the new name
//!   (`DirectoryNotEmpty`), a file onto a directory (`IsADirectory`), a
//!   directory onto a file (`NotADirectory`), a directory into its own subtree
//!   or a new name the FAT specification does not permit in a long name
//!   (`InvalidPath`);
//! - an entry renamed onto itself under any of its FAT names changes nothing,
//!   except that a change of case in its long name is applied.
//!
//! Not atomic: FAT has no rename-over, and fatfs frees the old entry before it
//! writes the new one, so a device failure may lose the moved entry. A
//! replaced entry is removed first; a failure after that leaves the new name
//! absent. A directory moving to another parent is rebuilt there ([`tree`]).

mod tree;

#[cfg(test)]
pub(crate) mod tests;

use alloc::format;
use alloc::string::String;

use super::{dir_entries, fat_error, split_last, DeviceError};
use crate::fs_error::FsError;

type Dir<'a, IO, TP, OCC> = fatfs::Dir<'a, IO, TP, OCC>;
type Entry<'a, IO, TP, OCC> = fatfs::DirEntry<'a, IO, TP, OCC>;

/// Longest long name, in bytes (FAT specification: 255 characters).
const MAX_LONG_NAME: usize = 255;

/// Rename `old` to `new`, both relative to the volume root.
pub(crate) fn rename<IO, TP, OCC>(
    fs: &fatfs::FileSystem<IO, TP, OCC>,
    old: &str,
    new: &str,
) -> Result<(), FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
    IO::Error: DeviceError,
{
    let (old_parent, old_name) = split_last(old);
    let (new_parent, new_name) = split_last(new);
    if old_name.is_empty() || !long_name_permitted(new_name) {
        return Err(FsError::InvalidPath);
    }
    let root = fs.root_dir();
    let (src_dir, src_parent) = walk(&root, old_parent)?;
    let src = lookup(&src_dir, old_name)?.ok_or(FsError::NotFound)?;
    let (dst_dir, dst_parent) = walk(&root, new_parent)?;
    let src_name = src.file_name();
    if src.is_dir() && within(&dst_parent, &join(&src_parent, &src_name)) {
        return Err(FsError::InvalidPath);
    }
    let replaced = match lookup(&dst_dir, new_name)? {
        Some(dst) if dst_parent == src_parent && dst.file_name() == src_name => {
            if new_name != src_name && new_name.eq_ignore_ascii_case(&src_name) {
                return recase(&src_dir, old_name, new_name);
            }
            return Ok(());
        }
        Some(dst) if src.is_dir() && !dst.is_dir() => return Err(FsError::NotADirectory),
        Some(dst) if !src.is_dir() && dst.is_dir() => return Err(FsError::IsADirectory),
        Some(_) => true,
        None => false,
    };
    let rebuild = src.is_dir() && src_parent != dst_parent;
    if rebuild {
        tree::survey(&src.to_dir())?;
    }
    if replaced {
        dst_dir.remove(new_name).map_err(fat_error)?;
    }
    // The source is named as the caller spelled it, which `lookup` and fatfs
    // resolve alike; its decoded long name may not resolve (a lossy decode).
    if rebuild {
        tree::move_tree(&src_dir, old_name, &dst_dir, new_name)
    } else {
        src_dir
            .rename(old_name, &dst_dir, new_name)
            .map_err(fat_error)
    }
}

/// Whether the FAT specification permits `name` as a long name in the form
/// fatfs writes it (Basic Multilingual Plane only). fatfs checks this only
/// after it has freed the source entry, so a rename checks it first.
fn long_name_permitted(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_LONG_NAME
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || ('\u{80}'..='\u{FFFF}').contains(&c)
                || "$%'-_@~`!(){}. +,;=[]^#&".contains(c)
        })
}

/// The entry fatfs resolves `name` to in `dir`: its long or its short name,
/// ASCII case ignored (fatfs is built without `unicode`).
fn lookup<'a, IO, TP, OCC>(
    dir: &Dir<'a, IO, TP, OCC>,
    name: &str,
) -> Result<Option<Entry<'a, IO, TP, OCC>>, FsError>
where
    IO: fatfs::ReadWriteSeek + 'a,
    TP: fatfs::TimeProvider + 'a,
    OCC: fatfs::OemCpConverter + 'a,
{
    for entry in dir_entries(dir) {
        let entry = entry?;
        let long = entry.file_name();
        if long == "." || long == ".." {
            continue;
        }
        if long.eq_ignore_ascii_case(name) || entry.short_file_name().eq_ignore_ascii_case(name) {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

/// Directory `rel` and its path spelled in on-disk long names, which names
/// one directory exactly however `rel` spelled it.
fn walk<'a, IO, TP, OCC>(
    root: &Dir<'a, IO, TP, OCC>,
    rel: &str,
) -> Result<(Dir<'a, IO, TP, OCC>, String), FsError>
where
    IO: fatfs::ReadWriteSeek + 'a,
    TP: fatfs::TimeProvider + 'a,
    OCC: fatfs::OemCpConverter + 'a,
{
    let mut dir = root.clone();
    let mut path = String::new();
    for part in rel.split('/').filter(|part| !part.is_empty()) {
        let entry = lookup(&dir, part)?.ok_or(FsError::NotFound)?;
        if !entry.is_dir() {
            return Err(FsError::NotADirectory);
        }
        path = join(&path, &entry.file_name());
        dir = entry.to_dir();
    }
    Ok((dir, path))
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        String::from(name)
    } else {
        format!("{parent}/{name}")
    }
}

/// Whether `path` is `root` or lies below it.
fn within(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Change the case of `from`'s long name: fatfs treats a rename onto the same
/// entry as done, so the entry passes through a free temporary name, where a
/// failure of the second step leaves it.
fn recase<IO, TP, OCC>(dir: &Dir<'_, IO, TP, OCC>, from: &str, to: &str) -> Result<(), FsError>
where
    IO: fatfs::ReadWriteSeek,
    TP: fatfs::TimeProvider,
    OCC: fatfs::OemCpConverter,
    IO::Error: DeviceError,
{
    let mut temp = None;
    for attempt in 0..64u32 {
        let name = format!(".cellos-rename-{attempt}");
        if lookup(dir, &name)?.is_none() {
            temp = Some(name);
            break;
        }
    }
    let temp = temp.ok_or(FsError::AlreadyExists)?;
    dir.rename(from, dir, &temp).map_err(fat_error)?;
    dir.rename(&temp, dir, to).map_err(fat_error)
}
