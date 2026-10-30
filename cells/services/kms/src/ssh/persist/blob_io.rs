//! The persister's only window on storage: two files and nothing else.
//!
//! The trait has no delete, no rename, no path argument and no directory
//! removal, so a persister built on it can touch exactly `ssh-a.bin` and
//! `ssh-b.bin`. Paths live in this file only. `stat` and `list` are
//! implementation details of [`VfsBlobIo`]'s `read`.

/// Largest record: 217-byte header + 2,048-byte wrapped section + 32-byte MAC.
pub(crate) const RECORD_MAX: usize = 217 + 2_048 + 32;

/// The two record files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    A,
    B,
}

impl Slot {
    /// The value stored in the record header.
    pub(crate) fn id(self) -> u8 {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }

    pub(crate) fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    /// File name inside [`KMS_DIR`].
    pub(crate) fn file_name(self) -> &'static str {
        match self {
            Self::A => "ssh-a.bin",
            Self::B => "ssh-b.bin",
        }
    }

    #[cfg(target_os = "none")]
    fn path(self) -> &'static str {
        match self {
            Self::A => "/srv/cellos/kms/ssh-a.bin",
            Self::B => "/srv/cellos/kms/ssh-b.bin",
        }
    }
}

/// Any I/O failure. No detail reaches a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlobError;

/// What `read` found. `Present(len)` filled `out[..len]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Read {
    Absent,
    Present(usize),
    /// A file larger than [`RECORD_MAX`]: present, never a valid record.
    Oversize,
}

pub(crate) trait SshBlobIo {
    /// Make sure `/srv/cellos` and `/srv/cellos/kms` exist.
    fn ensure_dirs(&mut self) -> Result<(), BlobError>;
    /// `Absent` only when the backend is healthy and the file does not exist.
    fn read(&mut self, slot: Slot, out: &mut [u8; RECORD_MAX]) -> Result<Read, BlobError>;
    /// Create or overwrite the whole file.
    fn write(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), BlobError>;
}

/// What a directory listing says about one file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listed {
    /// No entry with exactly this name.
    Missing,
    /// `f:NAME` or a bare `NAME`.
    File,
    /// `d:NAME`: a directory in the way.
    Directory,
}

/// Pure listing matcher: entries are `\n`-separated; an entry is `f:NAME`,
/// `d:NAME` or a bare `NAME`. Only an exact name matches (`xssh-a.bin`,
/// `ssh-a.bin.tmp` and `ssh-a.binx` do not). A file wins over a directory of
/// the same name.
pub(crate) fn classify_listing(listing: &[u8], name: &str) -> Listed {
    let name = name.as_bytes();
    let mut found = Listed::Missing;
    for entry in listing.split(|byte| *byte == b'\n') {
        let (directory, entry_name) = match entry {
            [b'f', b':', rest @ ..] => (false, rest),
            [b'd', b':', rest @ ..] => (true, rest),
            other => (false, other),
        };
        if entry_name != name {
            continue;
        }
        if !directory {
            return Listed::File;
        }
        found = Listed::Directory;
    }
    found
}

#[cfg(target_os = "none")]
pub(crate) use vfs::VfsBlobIo;

#[cfg(target_os = "none")]
mod vfs {
    use super::{classify_listing, BlobError, Listed, Read, Slot, SshBlobIo, RECORD_MAX};

    const SRV_CELLOS: &str = "/srv/cellos";
    const KMS_DIR: &str = "/srv/cellos/kms";

    /// The VFS-backed store (x86_64 bare metal).
    ///
    /// Absent versus failed: the VFS answers a missing path and a failing
    /// backend with the same wire code, so an `IO` error from `stat` is
    /// resolved with one directory listing. Every uncertainty resolves toward
    /// [`BlobError`], never toward `Absent`: a wrong guess can only make the key
    /// unavailable, never make it silently vanish.
    pub(crate) struct VfsBlobIo {
        client: ostd::clients::vfs::VfsClient,
    }

    impl VfsBlobIo {
        pub(crate) fn new() -> Self {
            Self {
                client: ostd::clients::vfs::VfsClient::new(),
            }
        }

        fn ensure_one(&mut self, path: &str) -> Result<(), BlobError> {
            match self.client.stat(path) {
                Ok((_, true)) => Ok(()),
                Ok((_, false)) => Err(BlobError),
                Err(_) => self.client.mkdir(path).map_err(|_| BlobError),
            }
        }

        fn read_present(
            &mut self,
            path: &str,
            out: &mut [u8; RECORD_MAX],
        ) -> Result<Read, BlobError> {
            match self.client.read_file_bounded(path, RECORD_MAX) {
                Ok(bytes) => match out.get_mut(..bytes.len()) {
                    Some(dst) => {
                        dst.copy_from_slice(&bytes);
                        Ok(Read::Present(bytes.len()))
                    }
                    None => Ok(Read::Oversize),
                },
                // The file grew between `stat` and the read.
                Err(ostd::ViError::OutOfMemory) => Ok(Read::Oversize),
                Err(_) => Err(BlobError),
            }
        }

        fn absent_or_failed(&mut self, slot: Slot) -> Result<Read, BlobError> {
            let listing = self.client.list_dir(KMS_DIR).map_err(|_| BlobError)?;
            match classify_listing(&listing, slot.file_name()) {
                Listed::Missing => Ok(Read::Absent),
                Listed::File | Listed::Directory => Err(BlobError),
            }
        }
    }

    impl SshBlobIo for VfsBlobIo {
        fn ensure_dirs(&mut self) -> Result<(), BlobError> {
            self.ensure_one(SRV_CELLOS)?;
            self.ensure_one(KMS_DIR)
        }

        fn read(&mut self, slot: Slot, out: &mut [u8; RECORD_MAX]) -> Result<Read, BlobError> {
            let path = slot.path();
            match self.client.stat(path) {
                // A directory in the way.
                Ok((_, true)) => Err(BlobError),
                Ok((size, false)) => {
                    if size > RECORD_MAX as u64 {
                        Ok(Read::Oversize)
                    } else {
                        self.read_present(path, out)
                    }
                }
                Err(ostd::ViError::IO) => self.absent_or_failed(slot),
                Err(_) => Err(BlobError),
            }
        }

        fn write(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), BlobError> {
            self.client
                .write_file(slot.path(), bytes)
                .map_err(|_| BlobError)
        }
    }
}
