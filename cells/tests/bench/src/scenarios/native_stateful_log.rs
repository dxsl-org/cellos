//! The native-stateful checkpoint log, `checkpoint.log` in the bench's working
//! directory (the lane runs it from `/srv`).
//!
//! A shell-launched Cell is path-sealed by VFS: it reaches files only through
//! the cwd handle its launch context grants. VFS has no append through a
//! handle, so each checkpoint rewrites the whole log; the log's content is the
//! record list either way.

use alloc::vec::Vec;
use api::dir_handles::ViDirHandle;
use api::ipc::IPC_BUF_SIZE;
use api::services::ipc::{VfsRequest, VfsResponse};
use cellos_fs::crc32::crc32c;
use ostd::service::ServiceRef;

const LOG_NAME: &str = "checkpoint.log";
const RECORD_LEN: usize = 12;
/// One checkpoint every this many operations; record `k` holds counter `k * 100`.
pub const CHECKPOINT_INTERVAL: u32 = 100;

/// Record `seq`: sequence, counter at that checkpoint, CRC32C of both.
pub fn record(seq: u32) -> [u8; RECORD_LEN] {
    let mut bytes = [0u8; RECORD_LEN];
    bytes[0..4].copy_from_slice(&seq.to_le_bytes());
    bytes[4..8].copy_from_slice(&(seq * CHECKPOINT_INTERVAL).to_le_bytes());
    let checksum = crc32c(&bytes[..8]);
    bytes[8..12].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

/// The log as it must read after `count` checkpoints.
pub fn expected(count: u32) -> Vec<u8> {
    (1..=count).flat_map(record).collect()
}

/// The cwd handle of this launch, which must be `/srv`.
pub fn srv_dir() -> Result<ViDirHandle, &'static str> {
    let context = ostd::child_context::ChildContext::read_own()
        .map_err(|_| "no launch context; run from the shell")?;
    if context.cwd() != b"/srv" {
        return Err("run from /srv (`cd /srv` first)");
    }
    context
        .cwd_grant()
        .map(|grant| ViDirHandle(grant.id))
        .ok_or("launch context carries no cwd handle")
}

/// One VFS exchange; `None` when the call itself fails.
fn call<R>(request: &VfsRequest<'_>, answer: impl FnOnce(VfsResponse<'_>) -> R) -> Option<R> {
    let mut reply = [0u8; IPC_BUF_SIZE];
    let mut vfs = ServiceRef::<{ api::syscall::service::VFS }>::new();
    vfs.call::<VfsRequest, VfsResponse>(request, &mut reply)
        .into_result()
        .ok()
        .map(answer)
}

/// The log's bytes; `None` when VFS refuses (the handle is stale) or the log
/// is absent.
pub fn read(dir: ViDirHandle) -> Option<Vec<u8>> {
    call(
        &VfsRequest::ReadAt {
            dir,
            name: LOG_NAME,
        },
        |response| match response {
            VfsResponse::Data(bytes) => Some(bytes.to_vec()),
            _ => None,
        },
    )
    .flatten()
}

/// Remove the log; absent is fine.
pub fn clear(dir: ViDirHandle) {
    let _ = call(
        &VfsRequest::UnlinkAt {
            dir,
            name: LOG_NAME,
        },
        |_| (),
    );
}

/// Commit checkpoint `seq`: the log must hold exactly records `1..seq`, and
/// afterwards holds `1..=seq`.
pub fn commit(dir: ViDirHandle, seq: u32) -> Result<(), &'static str> {
    let before = if seq == 1 {
        Vec::new()
    } else {
        read(dir).ok_or("cannot read the checkpoint log")?
    };
    if before != expected(seq - 1) {
        return Err("checkpoint log does not hold the earlier records");
    }
    let content = expected(seq);
    let request = VfsRequest::WriteAt {
        dir,
        name: LOG_NAME,
        content: &content,
    };
    match call(&request, |response| matches!(response, VfsResponse::Ok)) {
        Some(true) => Ok(()),
        _ => Err("VFS refused the checkpoint write"),
    }
}
