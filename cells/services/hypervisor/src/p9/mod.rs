//! A 9P2000.L file server over one share root (virtio-9p, `crate::virtio_9p`).
//!
//! The guest names files only as fids reached by walking from the root one
//! component at a time; `..` at the root stays there and no component may
//! carry a separator, so nothing above the root can be named. The files
//! themselves are a [`Backend`]: the VFS service ([`VfsShare`]) in the Cell,
//! an in-memory fake in host tests.

mod backend;
mod codec;
mod fids;
mod server;
mod server_attr;
mod server_dir;
mod server_file;
mod server_walk;
mod vfs_backend;
mod vfs_io;
mod vfs_share;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_io;
#[cfg(test)]
mod tests_walk;

pub use server::Server;
pub use vfs_share::VfsShare;

/// Largest message either side may send: a 128 KiB read or write payload plus
/// its header.
pub const MAX_MSIZE: u32 = 128 * 1024 + 24;

/// Message types (9P2000.L); a reply is its request's type plus one.
pub mod msg {
    pub const RLERROR: u8 = 7;
    pub const TSTATFS: u8 = 8;
    pub const TLOPEN: u8 = 12;
    pub const TLCREATE: u8 = 14;
    pub const TSYMLINK: u8 = 16;
    pub const TMKNOD: u8 = 18;
    pub const TRENAME: u8 = 20;
    pub const TREADLINK: u8 = 22;
    pub const TGETATTR: u8 = 24;
    pub const TSETATTR: u8 = 26;
    pub const TXATTRWALK: u8 = 30;
    pub const TREADDIR: u8 = 40;
    pub const TFSYNC: u8 = 50;
    pub const TLOCK: u8 = 52;
    pub const TGETLOCK: u8 = 54;
    pub const TLINK: u8 = 70;
    pub const TMKDIR: u8 = 72;
    pub const TRENAMEAT: u8 = 74;
    pub const TUNLINKAT: u8 = 76;
    pub const TVERSION: u8 = 100;
    pub const TATTACH: u8 = 104;
    pub const TFLUSH: u8 = 108;
    pub const TWALK: u8 = 110;
    pub const TREAD: u8 = 116;
    pub const TWRITE: u8 = 118;
    pub const TCLUNK: u8 = 120;
}

/// Linux errno values carried in Rlerror.
pub mod errno {
    pub const ENOENT: u32 = 2;
    pub const EIO: u32 = 5;
    pub const EBADF: u32 = 9;
    pub const EACCES: u32 = 13;
    pub const EBUSY: u32 = 16;
    pub const EEXIST: u32 = 17;
    pub const ENOTDIR: u32 = 20;
    pub const EISDIR: u32 = 21;
    pub const EINVAL: u32 = 22;
    pub const EMFILE: u32 = 24;
    pub const EFBIG: u32 = 27;
    pub const ENOSPC: u32 = 28;
    pub const ERANGE: u32 = 34;
    pub const ENAMETOOLONG: u32 = 36;
    pub const ENOTEMPTY: u32 = 39;
    pub const EOPNOTSUPP: u32 = 95;
}
