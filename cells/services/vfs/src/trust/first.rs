//! First trust (Phase 09 S9-4c; abi-vfs-telemetry §4; D79-11/12).
//!
//! `TrustChallenge`: for a host and port the store does not hold, the service records a
//! challenge bound to the caller principal, the exact key, the store's current
//! observation and a 60-second deadline, and registers its immutable descriptor with the
//! input owner (`RegisterTrustChallenge`). The input owner, not the caller, then runs the
//! confirmation on the caller's terminal session.
//!
//! `TrustCommit`: the caller's own challenge only. The owner-attested decision
//! (`CollectTrustDecision`) must accept and match every binding of the record (challenge,
//! caller, session and epoch, descriptor SHA-256); then the store is re-observed and,
//! still Unknown, extended by one `interactive` line and published through the D03
//! machinery against that observation. Another writer's line for the same host and port
//! wins: the same key answers KnownExact, a different one Mismatch, and nothing is ever
//! overwritten. A publication reports `Committed` (synced, not crash-proof; S9-4a).
//! Every final answer ends the challenge.
use alloc::string::String;
use alloc::vec::Vec;

use api::abi::caller_identity::CALLER_FLAG_NET_CONNECT;
use api::cell_session::trust::{self as tr, TrustChallengeV1, TrustPromptV1};
use api::cell_session::{ObjectRefV1 as Ref, PrincipalV1, Status};
use api::ipc::{ssh_trust, Observation, SaveOutcome, VfsResponse};
use sha2::{Digest, Sha256};

use super::{base64_encode, blob_names, canonical_host, lookup, parse, HEADER, MAX_BLOB, STORE_PATH};
use crate::caller::Caller;
use crate::manager::VfsManager;
use crate::paths::ERR_QUOTA;

/// Input-owner seam: production calls service-input; host tests run a fake owner.
#[cfg(not(test))]
mod owner {
    pub use ostd::session::{collect_trust_decision as collect, monotonic_ns as now_ns, register_trust_challenge as register};
}
#[cfg(test)]
use super::tests::owner;

/// Challenge lifetime: the input owner's ceiling, never extended.
const LIFETIME_NS: u64 = tr::EXPIRY_MAX_NS;
/// Open challenges per caller principal, and in total (§4: four per worker).
const MAX_PER_CALLER: usize = 1;
const MAX_CHALLENGES: usize = 4;
const TRUST_DIR: &str = "/srv/.cellos-trust";

struct Challenge {
    challenge: Ref,
    owner: Caller,
    host: String,
    port: u16,
    algorithm: String,
    key: Vec<u8>,
    /// The store as it was when the challenge was issued.
    observation: Observation,
    expiry: u64,
    confirmation: Ref,
    session: Ref,
    epoch: u64,
    /// SHA-256 of the descriptor registered with the input owner.
    digest: [u8; 32],
}

/// Open challenges (not carried across a VFS restart: the input owner binds the old
/// VFS principal, so its decisions can never be collected by a successor).
pub struct Challenges {
    records: Vec<Challenge>,
    next: u64,
}

impl Challenges {
    pub const fn new() -> Self {
        Self { records: Vec::new(), next: 1 }
    }
}

fn principal(caller: Caller) -> PrincipalV1 {
    PrincipalV1 { cell_id: caller.cell.0, generation: caller.generation }
}

/// `SHA256:` and the unpadded base64 of the blob's digest, exactly as `vissh` prints it.
fn fingerprint(key: &[u8]) -> String {
    let mut s = String::from("SHA256:");
    base64_encode(&Sha256::digest(key), false, &mut s);
    s
}

/// The store's observation and its answer for this host, port and key.
fn current(vfs: &VfsManager, host: &str, port: u16, algorithm: &str, key: &[u8]) -> Option<(Observation, Vec<u8>, u8)> {
    match vfs.observe(STORE_PATH)? {
        Observation::Absent => Some((Observation::Absent, Vec::new(), ssh_trust::UNKNOWN)),
        observation @ Observation::Present { size, regular: true, .. } => {
            let bytes = vfs.read_to_vec(STORE_PATH);
            if bytes.len() as u64 != size {
                return None;
            }
            let code = lookup(&bytes, host, port, algorithm, key);
            Some((observation, bytes, code))
        }
        Observation::Present { .. } => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn challenge(
    vfs: &mut VfsManager, caller: Caller, host: &str, port: u16, algorithm: &str, key: &[u8], session: Ref, epoch: u64,
) -> VfsResponse<'static> {
    if caller.flags & CALLER_FLAG_NET_CONNECT == 0 {
        return VfsResponse::Trust(ssh_trust::DENIED);
    }
    // Only what a store line can hold is ever offered to the user.
    if !canonical_host(host) || port == 0 || algorithm != "ssh-ed25519" || key.is_empty() || key.len() > MAX_BLOB
        || !blob_names(key, algorithm) || session.is_zero()
    {
        return VfsResponse::Err(0xFF);
    }
    let Some((observation, _, code)) = current(vfs, host, port, algorithm, key) else {
        return VfsResponse::Trust(ssh_trust::UNAVAILABLE);
    };
    // KnownExact needs no prompt; a changed key never prompts.
    if code != ssh_trust::UNKNOWN {
        return VfsResponse::Trust(code);
    }
    let Ok(now) = owner::now_ns() else { return VfsResponse::Trust(ssh_trust::UNAVAILABLE) };
    let table = &mut vfs.trust_challenges;
    // A caller's own expired challenge gives way; any other is released one lifetime later.
    table.records.retain(|r| now < r.expiry.saturating_add(LIFETIME_NS) && !(r.owner == caller && now >= r.expiry));
    if table.records.iter().filter(|r| r.owner == caller).count() >= MAX_PER_CALLER || table.records.len() >= MAX_CHALLENGES {
        return VfsResponse::Err(ERR_QUOTA);
    }
    let challenge = Ref { id: table.next, generation: 1 };
    let Some(next) = table.next.checked_add(1) else { return VfsResponse::Err(ERR_QUOTA) };
    let expiry = now.saturating_add(LIFETIME_NS);
    let fingerprint = fingerprint(key);
    let descriptor = TrustChallengeV1 {
        challenge, target: principal(caller), expiry_ns: expiry,
        prompt: TrustPromptV1 { port, host, algorithm, fingerprint: &fingerprint },
    };
    let mut bytes = [0u8; TrustChallengeV1::MAX_LEN];
    let Ok(len) = descriptor.encode(&mut bytes) else { return VfsResponse::Err(0xFF) };
    let digest: [u8; 32] = Sha256::digest(&bytes[..len]).into();
    let Ok(confirmation) = owner::register(&descriptor) else { return VfsResponse::Trust(ssh_trust::UNAVAILABLE) };
    table.next = next;
    table.records.push(Challenge {
        challenge, owner: caller, host: String::from(host), port, algorithm: String::from(algorithm), key: key.to_vec(),
        observation, expiry, confirmation, session, epoch, digest,
    });
    VfsResponse::TrustChallenged { confirmation, challenge }
}

pub fn commit(vfs: &mut VfsManager, caller: Caller, challenge: Ref) -> VfsResponse<'static> {
    if caller.flags & CALLER_FLAG_NET_CONNECT == 0 {
        return VfsResponse::Trust(ssh_trust::DENIED);
    }
    // Only the caller's own challenge: a guessed or another caller's ref is refused.
    let Some(n) = vfs.trust_challenges.records.iter().position(|r| r.challenge == challenge && r.owner == caller) else {
        return VfsResponse::Trust(ssh_trust::DENIED);
    };
    let decision = match owner::collect(vfs.trust_challenges.records[n].confirmation) {
        Ok(decision) => decision,
        // Uncertain transport: the decision is still the owner's to give.
        Err(Status::Indeterminate | Status::WouldBlock | Status::TimedOut) => return VfsResponse::Trust(ssh_trust::PENDING),
        Err(_) => {
            vfs.trust_challenges.records.swap_remove(n);
            return VfsResponse::Trust(ssh_trust::UNAVAILABLE);
        }
    };
    if !decision.is_final() {
        return VfsResponse::Trust(ssh_trust::PENDING);
    }
    let record = vfs.trust_challenges.records.swap_remove(n);
    let code = match decision.decision {
        tr::DECISION_ACCEPT => {
            // The owner's reply must bind exactly this record: challenge, caller, the
            // session and epoch the caller named, and the descriptor this service registered.
            let bound = decision.challenge == record.challenge && decision.target == principal(caller)
                && decision.session == record.session && decision.epoch == record.epoch
                && decision.descriptor_sha256 == record.digest;
            match owner::now_ns() {
                _ if !bound => ssh_trust::REFUSED,
                Ok(now) if now < record.expiry => persist(vfs, &record),
                _ => ssh_trust::EXPIRED,
            }
        }
        tr::DECISION_EXPIRED => ssh_trust::EXPIRED,
        _ => ssh_trust::REFUSED,
    };
    VfsResponse::Trust(code)
}

/// Add the record's line to the store: at most two publication attempts, each against
/// the observation it was built from.
fn persist(vfs: &mut VfsManager, r: &Challenge) -> u8 {
    let mut expected = r.observation;
    for _ in 0..2 {
        let Some((observation, current, code)) = current(vfs, &r.host, r.port, &r.algorithm, &r.key) else {
            return ssh_trust::UNAVAILABLE;
        };
        // Changed since the challenge (or the last attempt): another writer may have
        // stored this host and port meanwhile; its line is never overwritten.
        if observation != expected && code != ssh_trust::UNKNOWN {
            return code;
        }
        let mut bytes = if observation == Observation::Absent { String::from(HEADER) + "\n" } else {
            match String::from_utf8(current) { Ok(text) => text, Err(_) => return ssh_trust::UNAVAILABLE }
        };
        bytes.push_str(&r.host);
        bytes.push_str(&alloc::format!(" {} {} ", r.port, r.algorithm));
        base64_encode(&r.key, true, &mut bytes);
        bytes.push_str(" interactive\n");
        // The store's own bounds (entries, blob, total size) hold for the new revision too.
        if parse(bytes.as_bytes()).is_err() {
            return ssh_trust::UNAVAILABLE;
        }
        match vfs.stat(TRUST_DIR) {
            Some((_, true)) => {}
            None if vfs.mkdir(TRUST_DIR) => {}
            _ => return ssh_trust::UNAVAILABLE,
        }
        let Some(expect) = observation.expect() else { return ssh_trust::UNAVAILABLE };
        match crate::save::publish_owned(vfs, STORE_PATH, bytes.as_bytes(), expect) {
            outcome if outcome.published().is_some() => return ssh_trust::KNOWN_EXACT,
            SaveOutcome::Conflict => expected = observation,
            _ => return ssh_trust::UNAVAILABLE,
        }
    }
    ssh_trust::UNAVAILABLE
}
