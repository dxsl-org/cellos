// SPDX-License-Identifier: MPL-2.0
//! Handle-addressed routing for the path shims (`stat`, `unlink`, `mkdir`,
//! `rmdir`, `rename`).
//!
//! A cell launched with a cwd grant — every shell-launched foreground child — is
//! sealed by VFS: every path-string request it sends is refused, and the granted
//! working directory is its only reach into the tree. The shim cannot read that
//! grant itself (the invocation context is read over the correlated kernel
//! session protocol, which lives in `ostd`), so the cell installs it once with
//! [`install_working_dir`]. From then on each path is resolved below it:
//! `OpenDir` down to the parent, one `*At` request on the leaf, then `CloseDir`
//! on the first derived handle, which revokes the whole chain.
//!
//! A path outside the working directory, or one that climbs with `..`, cannot
//! be expressed by a handle and fails with `-1`, the answer the seal gives.

// The crate root has a `pub mod alloc` (malloc/free), so the `alloc` crate is
// named here rather than at the root.
extern crate alloc;

use alloc::boxed::Box;
use core::ffi::c_int;
use core::sync::atomic::{AtomicPtr, Ordering};

use super::sysio::{stat, vfs_call, vfs_mutate, write_vfs_stat};
use abi::dir_handles::ViDirHandle;
use wire::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};

struct WorkingDir {
    dir: ViDirHandle,
    path: Box<str>,
}

/// Set once and never freed: the shims hand out `&'static` views of it.
static WORKING_DIR: AtomicPtr<WorkingDir> = AtomicPtr::new(core::ptr::null_mut());

/// Route the path shims through `dir`, the handle granted for the absolute
/// directory `path`.
///
/// The first installation wins for the life of the cell, matching the one-way
/// seal it answers. Returns `false` when `path` is not absolute or a working
/// directory is already installed.
pub fn install_working_dir(dir: ViDirHandle, path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    let entry = Box::into_raw(Box::new(WorkingDir {
        dir,
        path: path.into(),
    }));
    let installed = WORKING_DIR
        .compare_exchange(
            core::ptr::null_mut(),
            entry,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok();
    if !installed {
        // SAFETY: `entry` came from `Box::into_raw` above and was never published.
        drop(unsafe { Box::from_raw(entry) });
    }
    installed
}

/// The installed working directory handle and its absolute path.
pub fn working_dir() -> Option<(ViDirHandle, &'static str)> {
    // SAFETY: the pointer is null or a leaked `Box` that is never freed or mutated.
    unsafe { WORKING_DIR.load(Ordering::Acquire).as_ref() }.map(|w| (w.dir, &*w.path))
}

/// `path` relative to the directory `cwd`; `None` when an absolute `path` lies
/// outside it. A relative `path` is already relative to it.
fn below<'a>(cwd: &str, path: &'a str) -> Option<&'a str> {
    if !path.starts_with('/') {
        return Some(path);
    }
    let rest = path.strip_prefix(cwd.trim_end_matches('/'))?;
    (rest.is_empty() || rest.starts_with('/')).then_some(rest)
}

/// The directory holding a path's last component. Dropping it closes every
/// handle derived on the way down.
struct Parent {
    dir: ViDirHandle,
    first_derived: Option<ViDirHandle>,
}

impl Drop for Parent {
    fn drop(&mut self) {
        if let Some(dir) = self.first_derived {
            // SAFETY: plain IPC round trip on stack buffers.
            let _ = unsafe { vfs_mutate(&VfsRequest::CloseDir { dir }) };
        }
    }
}

/// Walk from the working directory to the parent of `path` and return it with
/// the leaf name. `None` for an empty path, a `..` component, a path outside the
/// working directory, or a component that is not an openable directory.
unsafe fn parent_of<'p>(cwd: (ViDirHandle, &str), path: &'p str) -> Option<(Parent, &'p str)> {
    let mut names = below(cwd.1, path)?
        .split('/')
        .filter(|name| !name.is_empty() && *name != ".");
    let mut parent = Parent {
        dir: cwd.0,
        first_derived: None,
    };
    let mut leaf = names.next()?;
    for next in names {
        if leaf == ".." {
            return None;
        }
        let mut recv = [0u8; IPC_BUF_SIZE];
        let req = VfsRequest::OpenDir {
            dir: parent.dir,
            name: leaf,
        };
        let Some(VfsResponse::DirHandle(sub)) = vfs_call(&req, &mut recv) else {
            return None;
        };
        parent.first_derived.get_or_insert(sub);
        parent.dir = sub;
        leaf = next;
    }
    (leaf != "..").then_some((parent, leaf))
}

/// One mutating `*At` request on `path`'s leaf; `None` when no working
/// directory is installed and the path-string route still applies.
unsafe fn mutate_at(
    path: &str,
    request: impl FnOnce(ViDirHandle, &str) -> VfsRequest<'_>,
) -> Option<c_int> {
    let cwd = working_dir()?;
    Some(match parent_of(cwd, path) {
        Some((parent, leaf)) => vfs_mutate(&request(parent.dir, leaf)),
        None => -1,
    })
}

pub(super) unsafe fn unlink(path: &str) -> Option<c_int> {
    mutate_at(path, |dir, name| VfsRequest::UnlinkAt { dir, name })
}

pub(super) unsafe fn mkdir(path: &str) -> Option<c_int> {
    mutate_at(path, |dir, name| VfsRequest::MkdirAt { dir, name })
}

pub(super) unsafe fn rmdir(path: &str) -> Option<c_int> {
    mutate_at(path, |dir, name| VfsRequest::RmdirAt { dir, name })
}

pub(super) unsafe fn rename(old: &str, new: &str) -> Option<c_int> {
    let cwd = working_dir()?;
    let (Some((old_parent, old)), Some((new_parent, new))) =
        (parent_of(cwd, old), parent_of(cwd, new))
    else {
        return Some(-1);
    };
    Some(vfs_mutate(&VfsRequest::RenameAt {
        old_dir: old_parent.dir,
        old,
        new_dir: new_parent.dir,
        new,
    }))
}

pub(super) unsafe fn stat_at(path: &str, st: *mut stat) -> Option<c_int> {
    let cwd = working_dir()?;
    let Some((parent, name)) = parent_of(cwd, path) else {
        return Some(-1);
    };
    let mut recv = [0u8; IPC_BUF_SIZE];
    let reply = vfs_call(
        &VfsRequest::StatAt {
            dir: parent.dir,
            name,
        },
        &mut recv,
    );
    Some(write_vfs_stat(reply, st))
}
