//! D03 conditional save: a bounded table of staging transactions and the
//! requests that drive them (`Observe`, `SaveBegin` … `SaveAbort`).
//!
//! A transaction stages bytes in a backend-owned object, seals them (exact
//! length and SHA-256 read back from storage, then the object's identity), and
//! publishes them at the destination named at `SaveBegin` only if that
//! destination still matches the caller's observation. The first commit's
//! outcome is recorded; a repeated commit or a query answers it and never runs
//! the publication again.

use alloc::string::String;
use alloc::vec::Vec;

use api::ipc::{save_refusal, Observation, SaveOutcome, VfsRequest, VfsResponse, VFS_SAVE_CHUNK, VFS_SAVE_MAX};
use sha2::{Digest, Sha256};

use crate::caller::Caller;
use crate::manager::VfsManager;
use crate::paths::{parent_of, ERR_DENIED, ERR_HANDLE, ERR_IO, ERR_QUOTA, ERR_UNSUPPORTED};

#[cfg(test)]
mod tests;
#[cfg(feature = "test-hooks")]
pub(crate) mod selftest;

/// Open transactions one caller generation may hold.
pub const MAX_SAVES_PER_CALLER: usize = 4;
/// Open transactions across all callers.
pub const MAX_SAVES: usize = 8;

struct Slot {
    id: u32,
    owner: Caller,
    dst: String,
    stage: String,
    written: u64,
    /// Identity of the staged object when sealed.
    sealed: Option<(u64, u64)>,
    outcome: Option<SaveOutcome>,
}

pub struct SaveTable {
    slots: Vec<Slot>,
    next: u32,
}

impl SaveTable {
    pub fn new() -> Self {
        Self { slots: Vec::new(), next: 1 }
    }

    fn owned(&mut self, caller: Caller, id: u32) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|slot| slot.id == id && slot.owner == caller)
    }

    fn staged_by(&self, caller: Caller) -> u64 {
        self.slots.iter().filter(|slot| slot.owner.cell == caller.cell && slot.outcome.is_none())
            .map(|slot| slot.written).sum()
    }
}

/// Serve a D03 request; `None` for every other request.
pub(crate) fn handle(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) -> Option<VfsResponse<'static>> {
    Some(match *req {
        VfsRequest::Observe { path } => observe(vfs, caller, path),
        VfsRequest::SaveBegin { dst } => begin(vfs, caller, dst),
        VfsRequest::SaveChunk { id, offset, bytes } => chunk(vfs, caller, id, offset, bytes),
        VfsRequest::SaveSeal { id, len, sha256 } => seal(vfs, caller, id, len, &sha256),
        VfsRequest::SaveCommit { id, expect } => commit(vfs, caller, id, expect),
        VfsRequest::SaveQuery { id } => match vfs.saves.owned(caller, id) {
            Some(Slot { outcome: Some(outcome), .. }) => VfsResponse::SaveOutcome(*outcome),
            _ => VfsResponse::Err(ERR_HANDLE),
        },
        VfsRequest::SaveAbort { id } => abort(vfs, caller, id),
        _ => return None,
    })
}

fn observe(vfs: &mut VfsManager, caller: Caller, path: &str) -> VfsResponse<'static> {
    if !vfs.access.can_read(caller, path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    if vfs.stage_path(path, 0).is_none() {
        return VfsResponse::Err(ERR_UNSUPPORTED);
    }
    if !vfs.healthy(path) {
        return VfsResponse::Err(ERR_IO);
    }
    match vfs.observe(path) {
        Some(observation) => VfsResponse::Observed(observation),
        None => VfsResponse::Err(ERR_IO),
    }
}

fn begin(vfs: &mut VfsManager, caller: Caller, dst: &str) -> VfsResponse<'static> {
    if crate::access::is_guest_disk_path(dst) || !vfs.access.can_write(caller, dst) || !caller.may_own_state() {
        return VfsResponse::Err(ERR_DENIED);
    }
    let id = vfs.saves.next;
    let Some(stage) = vfs.stage_path(dst, id) else {
        return VfsResponse::Err(ERR_UNSUPPORTED);
    };
    if !vfs.healthy(dst) || !parent_of(dst).is_some_and(|parent| matches!(vfs.stat(parent), Some((_, true)))) {
        return VfsResponse::Err(ERR_IO);
    }
    // A successor of a dead cell generation inherits none of its transactions.
    let stale: Vec<String> = vfs.saves.slots.iter()
        .filter(|slot| slot.owner.cell == caller.cell && slot.owner.generation != caller.generation)
        .filter(|slot| !slot.outcome.is_some_and(|o| o.published().is_some()))
        .map(|slot| slot.stage.clone()).collect();
    vfs.saves.slots.retain(|slot| !(slot.owner.cell == caller.cell && slot.owner.generation != caller.generation));
    for stage in stale {
        let _ = vfs.stage_remove(&stage);
    }
    let held = vfs.saves.slots.iter().filter(|slot| slot.owner == caller).count();
    if held >= MAX_SAVES_PER_CALLER || vfs.saves.slots.len() >= MAX_SAVES {
        return VfsResponse::Err(ERR_QUOTA);
    }
    let Some(next) = id.checked_add(1) else {
        return VfsResponse::Err(ERR_HANDLE);
    };
    if !vfs.stage_create(&stage) {
        return VfsResponse::Err(ERR_IO);
    }
    vfs.saves.next = next;
    vfs.saves.slots.push(Slot { id, owner: caller, dst: String::from(dst), stage, written: 0, sealed: None, outcome: None });
    VfsResponse::SaveId(id)
}

fn chunk(vfs: &mut VfsManager, caller: Caller, id: u32, offset: u64, bytes: &[u8]) -> VfsResponse<'static> {
    let staged = vfs.saves.staged_by(caller);
    let Some(slot) = vfs.saves.owned(caller, id) else { return VfsResponse::Err(ERR_HANDLE) };
    if slot.sealed.is_some() || slot.outcome.is_some() || offset != slot.written || bytes.len() > VFS_SAVE_CHUNK {
        return VfsResponse::Err(ERR_HANDLE);
    }
    let end = slot.written + bytes.len() as u64;
    if end > VFS_SAVE_MAX {
        return VfsResponse::Err(ERR_QUOTA);
    }
    let stage = slot.stage.clone();
    if !vfs.quota.can_charge(caller.cell, staged + bytes.len() as u64) {
        return VfsResponse::Err(ERR_QUOTA);
    }
    if !vfs.stage_write(&stage, offset, bytes) {
        return VfsResponse::Err(ERR_IO);
    }
    if let Some(slot) = vfs.saves.owned(caller, id) {
        slot.written = end;
    }
    VfsResponse::Ok
}

fn seal(vfs: &mut VfsManager, caller: Caller, id: u32, len: u64, sha256: &[u8; 32]) -> VfsResponse<'static> {
    let Some(slot) = vfs.saves.owned(caller, id) else { return VfsResponse::Err(ERR_HANDLE) };
    if slot.sealed.is_some() || slot.outcome.is_some() || slot.written != len {
        return VfsResponse::Err(ERR_HANDLE);
    }
    let stage = slot.stage.clone();
    let Some(identity) = sealed_identity(vfs, &stage, len, sha256) else { return VfsResponse::Err(ERR_IO) };
    if let Some(slot) = vfs.saves.owned(caller, id) {
        slot.sealed = Some(identity);
    }
    VfsResponse::Ok
}

/// Seal check: storage holds exactly `len` bytes at `stage` with this SHA-256 (digested
/// from what storage holds, not what the requests carried); the staged object's identity.
fn sealed_identity(vfs: &VfsManager, stage: &str, len: u64, sha256: &[u8; 32]) -> Option<(u64, u64)> {
    let mut digest = Sha256::new();
    let mut buf = [0u8; 4096];
    let mut offset = 0u64;
    while offset < len {
        let want = (len - offset).min(buf.len() as u64) as usize;
        if vfs.read_at(stage, offset, &mut buf[..want]) != want {
            return None;
        }
        digest.update(&buf[..want]);
        offset += want as u64;
    }
    let identity = match vfs.observe(stage) {
        Some(Observation::Present { inode, generation, size, regular: true }) if size == len => (inode, generation),
        _ => return None,
    };
    (digest.finalize().as_slice() == sha256).then_some(identity)
}

/// Publish `bytes` at a service-owned `dst` (the SSH host-trust store) through the same
/// staging, read-back seal and conditional publication (`publish_if` against `expect`)
/// as a caller's save. It runs on the service's own authority: no caller access rule or
/// quota applies, the destination's own bounds do. Unpublished staging is removed.
pub(crate) fn publish_owned(vfs: &mut VfsManager, dst: &str, bytes: &[u8], expect: api::ipc::SaveExpect) -> SaveOutcome {
    let refused = SaveOutcome::NotPublished(save_refusal::REFUSED);
    let id = vfs.saves.next;
    let (Some(next), Some(stage)) = (id.checked_add(1), vfs.stage_path(dst, id)) else { return refused };
    if !vfs.healthy(dst) || !vfs.stage_create(&stage) {
        return refused;
    }
    vfs.saves.next = next;
    let outcome = stage_and_publish(vfs, &stage, dst, bytes, expect);
    if outcome.published().is_none() && vfs.observe(&stage).is_some_and(|o| o != Observation::Absent) {
        let _ = vfs.stage_remove(&stage);
    }
    outcome
}

fn stage_and_publish(vfs: &mut VfsManager, stage: &str, dst: &str, bytes: &[u8], expect: api::ipc::SaveExpect) -> SaveOutcome {
    let refused = SaveOutcome::NotPublished(save_refusal::REFUSED);
    for (n, chunk) in bytes.chunks(4096).enumerate() {
        if !vfs.stage_write(stage, (n * 4096) as u64, chunk) {
            return refused;
        }
    }
    let sha256: [u8; 32] = Sha256::digest(bytes).into();
    let Some(sealed) = sealed_identity(vfs, stage, bytes.len() as u64, &sha256) else { return refused };
    let _lease = if dst.starts_with("/srv/") {
        let Ok(key) = crate::namespace::NamespaceKey::parse(dst) else { return refused };
        let Ok(lease) = vfs.ledger.acquire_transient(&key) else { return refused };
        Some(lease)
    } else {
        None
    };
    vfs.publish_if(stage, sealed, dst, expect)
}

fn commit(vfs: &mut VfsManager, caller: Caller, id: u32, expect: api::ipc::SaveExpect) -> VfsResponse<'static> {
    let Some(slot) = vfs.saves.owned(caller, id) else { return VfsResponse::Err(ERR_HANDLE) };
    if let Some(outcome) = slot.outcome {
        return VfsResponse::SaveOutcome(outcome);
    }
    let (dst, stage, sealed, len) = (slot.dst.clone(), slot.stage.clone(), slot.sealed, slot.written);
    let outcome = 'outcome: {
        let Some(sealed) = sealed else { break 'outcome SaveOutcome::NotPublished(save_refusal::NOT_SEALED) };
        // Policy may have changed since `SaveBegin`; nothing is attempted then.
        if !vfs.access.can_write(caller, &dst) {
            return VfsResponse::Err(ERR_DENIED);
        }
        let _lease = if dst.starts_with("/srv/") {
            let Ok(key) = crate::namespace::NamespaceKey::parse(&dst) else { return VfsResponse::Err(ERR_DENIED) };
            let Ok(lease) = vfs.ledger.acquire_transient(&key) else { return VfsResponse::Err(ERR_DENIED) };
            Some(lease)
        } else {
            None
        };
        // Same net-delta rule as `paths::write_file`.
        let old_size = match expect {
            api::ipc::SaveExpect::Absent => 0,
            api::ipc::SaveExpect::Present { .. } => vfs.file_size(&dst),
        };
        let refund = if vfs.quota.writer_of(&dst) == Some(caller.cell) { old_size } else { 0 };
        let net = len.saturating_sub(refund);
        if net > 0 && !vfs.quota.can_charge(caller.cell, net) {
            break 'outcome SaveOutcome::NotPublished(save_refusal::QUOTA);
        }
        let outcome = vfs.publish_if(&stage, sealed, &dst, expect);
        if outcome.published().is_some() {
            vfs.quota.release_path(&dst, old_size);
            let _ = vfs.quota.charge(caller.cell, len);
            vfs.quota.set_writer(&dst, caller.cell);
        }
        outcome
    };
    if let Some(slot) = vfs.saves.owned(caller, id) {
        slot.outcome = Some(outcome);
    }
    VfsResponse::SaveOutcome(outcome)
}

fn abort(vfs: &mut VfsManager, caller: Caller, id: u32) -> VfsResponse<'static> {
    let Some(index) = vfs.saves.slots.iter().position(|slot| slot.id == id && slot.owner == caller) else {
        return VfsResponse::Err(ERR_HANDLE);
    };
    let slot = vfs.saves.slots.remove(index);
    // A committed stage is the destination now. After an indeterminate commit
    // the stage is either gone (published) or still a stage; never the target.
    if !slot.outcome.is_some_and(|o| o.published().is_some()) && vfs.observe(&slot.stage).is_some_and(|o| o != Observation::Absent) {
        let _ = vfs.stage_remove(&slot.stage);
    }
    VfsResponse::Ok
}
