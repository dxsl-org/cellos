//! The `XE_GPU` IPC service: one received message in, one reply or client release out.
//!
//! Syscall-free so host tests drive it with real request frames; `main.rs` owns the
//! receive loop and supplies the kernel watch calls.

use crate::owner_watch::{OwnerWatches, WatchSys};
use api::caller_identity::CallerIdentity;
use intel_gpu_core::server::{Caller, XeServer};
use intel_gpu_core::uapi::ioctl::{
    DRM_IOCTL_XE_EXEC, DRM_IOCTL_XE_EXEC_QUEUE_CREATE, DRM_IOCTL_XE_GEM_CREATE,
};
use intel_gpu_core::uapi::query::DeviceInfo;
use intel_gpu_core::wire::{MAX_REQUEST_LEN, OP_OPEN};

/// Requests this driver answers `EOPNOTSUPP` rather than pretend to perform:
/// - `GEM_CREATE`: a buffer is client memory exported to the driver (CP1 D-G1 option B), and
///   the export syscalls (437/438) do not exist in the kernel yet, so there is no memory to
///   back an object;
/// - `EXEC_QUEUE_CREATE`, `EXEC`: queues are GuC contexts and submission goes through the
///   GuC CT channel, neither of which this driver drives yet.
pub const UNSERVED: [u32; 3] = [
    DRM_IOCTL_XE_GEM_CREATE,
    DRM_IOCTL_XE_EXEC_QUEUE_CREATE,
    DRM_IOCTL_XE_EXEC,
];

/// What the receive loop does with one message.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Send `len` reply bytes to the sender. `watch_unavailable` is set the first time a
    /// client could not be watched for death (log once).
    Reply { len: usize, watch_unavailable: bool },
    /// The kernel reported a watched client Cell dead; `files` DRM files were released.
    Released { files: usize },
    /// Not attributable to a client and not a watched death: no reply.
    Dropped,
}

/// Client sessions and their death watches.
pub struct Service {
    server: XeServer,
    watches: OwnerWatches,
}

impl Service {
    pub fn new(device: DeviceInfo) -> Self {
        Self {
            server: XeServer::new(device, &UNSERVED),
            watches: OwnerWatches::default(),
        }
    }

    /// Handle the message `sender` delivered into `buf`, the whole attested receive buffer
    /// (request first, the kernel's caller-identity trailer last; zeroed before the receive
    /// so a death record, which carries no trailer, reads as unattributed).
    pub fn handle(
        &mut self,
        buf: &[u8],
        sender: usize,
        reply: &mut [u8],
        sys: &mut impl WatchSys,
    ) -> Outcome {
        // A trailer naming another tid than the kernel-reported sender is not trusted.
        let identity = CallerIdentity::from_recv_buf(buf)
            .filter(|identity| identity.sender_tid == sender as u64);
        let Some(identity) = identity else {
            return match self.watches.take_death(sender) {
                Some(tids) => Outcome::Released {
                    files: tids
                        .iter()
                        .map(|&tid| self.server.release_client(tid))
                        .sum(),
                },
                None => Outcome::Dropped,
            };
        };
        let mut watch_unavailable = false;
        // Only OPEN creates state that outlives the call; watch before serving it, while
        // this message is the one the kernel lets the driver watch for.
        if buf.first() == Some(&OP_OPEN) {
            let admission = self.watches.admit(&identity, sender, sys);
            for tid in admission.stale {
                self.server.release_client(tid);
            }
            watch_unavailable = admission.unavailable;
        }
        let caller = Caller {
            tid: sender,
            cell_id: identity.cell_id,
            generation: identity.generation,
        };
        let request = &buf[..buf.len().min(MAX_REQUEST_LEN)];
        Outcome::Reply {
            len: self.server.handle(caller, request, reply),
            watch_unavailable,
        }
    }
}
