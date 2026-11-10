//! Server half of the `XE_GPU` transport ([`crate::wire`]): DRM files per client, keyed by
//! the kernel-stamped sender tid.
//!
//! A client is one sending thread of one Cell incarnation. Its files are reachable only
//! through its own tid, so file ids and every object id inside a file are private to it
//! (two clients may both hold file 1 and VM 1). The incarnation (`cell_id`, `generation`)
//! is recorded with the tid: a tid the kernel reuses for another Cell or a restarted one
//! finds the previous owner's files released, never inherited.

use crate::uapi::ioctl::{EBADF, EINVAL, EMFILE, ENFILE, EOPNOTSUPP};
use crate::uapi::query::DeviceInfo;
use crate::uapi::DrmFile;
use crate::wire::{self, OP_CLOSE, OP_IOCTL, OP_OPEN};
use alloc::collections::BTreeMap;

/// Clients served at once (`ENFILE` beyond).
pub const MAX_CLIENTS: usize = 64;
/// DRM files one client may hold open (`EMFILE` beyond).
pub const MAX_FILES_PER_CLIENT: usize = 16;

/// The kernel-attested sender of one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caller {
    pub tid: usize,
    pub cell_id: u64,
    pub generation: u64,
}

struct Client {
    cell_id: u64,
    generation: u64,
    next_file: u32,
    files: BTreeMap<u32, DrmFile>,
}

/// Every client's DRM files and the device they are served for.
pub struct XeServer {
    device: DeviceInfo,
    /// Requests answered `EOPNOTSUPP` before decoding: the ones the serving backend
    /// cannot honour, so they never report success for work that does not happen.
    unserved: &'static [u32],
    clients: BTreeMap<usize, Client>,
}

impl XeServer {
    pub fn new(device: DeviceInfo, unserved: &'static [u32]) -> Self {
        Self {
            device,
            unserved,
            clients: BTreeMap::new(),
        }
    }

    /// Serve one request frame from `caller`, writing the reply into `reply` (at least
    /// [`wire::MAX_REPLY_LEN`] bytes). Returns the reply length.
    pub fn handle(&mut self, caller: Caller, frame: &[u8], reply: &mut [u8]) -> usize {
        let request = match wire::decode_request(frame) {
            Ok(request) => request,
            Err(errno) => return wire::encode_reply(reply, errno, 0, &[], None),
        };
        self.evict_stale(caller);
        match request.op {
            OP_OPEN => match self.open(caller) {
                Ok(file) => wire::encode_reply(reply, 0, file, &[], None),
                Err(errno) => wire::encode_reply(reply, errno, 0, &[], None),
            },
            OP_CLOSE => {
                let errno = self.close(caller.tid, request.file).err().unwrap_or(0);
                wire::encode_reply(reply, errno, 0, &[], None)
            }
            OP_IOCTL => {
                let Some(file) = self
                    .clients
                    .get_mut(&caller.tid)
                    .and_then(|c| c.files.get_mut(&request.file))
                else {
                    return wire::encode_reply(reply, EBADF, 0, &[], None);
                };
                if self.unserved.contains(&request.request) {
                    return wire::encode_reply(reply, EOPNOTSUPP, 0, &[], None);
                }
                let mut arg = request.arg.to_vec();
                let mut mem = request.memory();
                let errno = file
                    .ioctl(&self.device, request.request, &mut arg, &mut mem)
                    .err()
                    .unwrap_or(0);
                // `drm_ioctl` copies the struct back whether or not the handler failed.
                wire::encode_reply(reply, errno, 0, &arg, Some(&mem))
            }
            _ => wire::encode_reply(reply, EINVAL, 0, &[], None),
        }
    }

    /// Release every file of the client that sent from `tid` (its thread or Cell died).
    /// Returns the number of files released.
    pub fn release_client(&mut self, tid: usize) -> usize {
        self.clients.remove(&tid).map_or(0, release_files)
    }

    /// Clients currently holding at least one open file.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    fn evict_stale(&mut self, caller: Caller) {
        let stale = self
            .clients
            .get(&caller.tid)
            .is_some_and(|c| (c.cell_id, c.generation) != (caller.cell_id, caller.generation));
        if stale {
            self.release_client(caller.tid);
        }
    }

    fn open(&mut self, caller: Caller) -> Result<u32, i32> {
        if !self.clients.contains_key(&caller.tid) && self.clients.len() >= MAX_CLIENTS {
            return Err(ENFILE);
        }
        let client = self.clients.entry(caller.tid).or_insert(Client {
            cell_id: caller.cell_id,
            generation: caller.generation,
            next_file: 1,
            files: BTreeMap::new(),
        });
        if client.files.len() >= MAX_FILES_PER_CLIENT {
            return Err(EMFILE);
        }
        let file = client.next_file;
        client.next_file = file.checked_add(1).ok_or(EMFILE)?;
        client.files.insert(file, DrmFile::new());
        Ok(file)
    }

    fn close(&mut self, tid: usize, file: u32) -> Result<(), i32> {
        let client = self.clients.get_mut(&tid).ok_or(EBADF)?;
        let mut drm = client.files.remove(&file).ok_or(EBADF)?;
        // Export ids are 0 until GEM_CREATE carries the D-G1 export attachment; then the
        // returned leases must also be revoked (`RevokeDmaExport`, 439).
        drm.release();
        if client.files.is_empty() {
            self.clients.remove(&tid);
        }
        Ok(())
    }
}

fn release_files(client: Client) -> usize {
    let count = client.files.len();
    for mut file in client.files.into_values() {
        // See `close`: no export lease can exist yet.
        file.release();
    }
    count
}

#[cfg(test)]
#[path = "server/tests.rs"]
mod tests;
