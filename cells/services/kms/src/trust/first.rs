//! First trust (Phase 09 S9-4c; abi-vfs-telemetry §4; D79-11/12).
//!
//! Challenge (`CommitTrustQuery` of a challenge query): for a host and port the store does
//! not hold, the service records a challenge bound to the caller principal, the exact
//! key, the store's current observation and a 60-second deadline, and registers its
//! immutable descriptor with the input owner (`RegisterTrustChallenge`). The input owner,
//! not the caller, then runs the confirmation on the caller's terminal session.
//!
//! `CommitTrustChallenge`: the caller's own challenge only. The owner-attested decision
//! (`CollectTrustDecision`) must accept and match every binding of the record (challenge,
//! caller, session and epoch, descriptor SHA-256); then the store is re-observed and,
//! still Unknown, extended by one `interactive` line and published through the VFS
//! conditional save against that observation. Another writer's line for the same host and
//! port wins: the same key answers KnownExact, a different one Mismatch, and nothing is
//! ever overwritten. A publication reports `Committed` (synced, not crash-proof; S9-4a).
//! Every final answer ends the challenge.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use api::cell_session::trust::{self as tr, TrustChallengeV1, TrustPromptV1};
use api::cell_session::{ObjectRefV1 as Ref, PrincipalV1, Status};
use api::ipc::{ssh_trust, Observation, SaveOutcome};
use kms_codec::{KmsErrorCode, TrustOutcomePayload};
use sha2::{Digest, Sha256};

use super::{
    base64_encode, blob_names, canonical_host, lookup, owner, parse, Principal, TrustIo, TrustLeaf,
    HEADER, MAX_BLOB, MAX_STORE,
};

/// Challenge lifetime: the input owner's ceiling, never extended.
const LIFETIME_NS: u64 = tr::EXPIRY_MAX_NS;
/// Open challenges per caller principal, and in total (§4: four per worker).
const MAX_PER_CALLER: usize = 1;
const MAX_CHALLENGES: usize = 4;

struct Challenge {
    challenge: Ref,
    owner: Principal,
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

/// Open challenges (not carried across a KMS restart: the input owner binds the old KMS
/// principal, so its decisions can never be collected by a successor).
pub(super) struct Challenges {
    records: Vec<Challenge>,
    next: u64,
}

impl Challenges {
    pub const fn new() -> Self {
        Self {
            records: Vec::new(),
            next: 1,
        }
    }
}

fn principal(caller: Principal) -> PrincipalV1 {
    PrincipalV1 {
        cell_id: caller.cell_id,
        generation: caller.generation,
    }
}

/// `SHA256:` and the unpadded base64 of the blob's digest, exactly as `vissh` prints it.
fn fingerprint(key: &[u8]) -> String {
    let mut s = String::from("SHA256:");
    base64_encode(&Sha256::digest(key), false, &mut s);
    s
}

/// The store's observation, its bytes and its answer for this host, port and key; `None`
/// when the store cannot be read.
pub(super) fn current(
    io: &mut impl TrustIo,
    host: &str,
    port: u16,
    algorithm: &str,
    key: &[u8],
) -> Option<(Observation, Vec<u8>, u8)> {
    match io.observe().ok()? {
        Observation::Absent => Some((Observation::Absent, Vec::new(), ssh_trust::UNKNOWN)),
        observation @ Observation::Present {
            size,
            regular: true,
            ..
        } => {
            if size > MAX_STORE as u64 {
                return None;
            }
            let bytes = io.read(MAX_STORE).ok()?;
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
pub(super) fn challenge(
    leaf: &mut TrustLeaf,
    caller: Principal,
    host: &str,
    port: u16,
    algorithm: &str,
    key: &[u8],
    session: Ref,
    epoch: u64,
) -> Result<TrustOutcomePayload, KmsErrorCode> {
    let answer = |code| Ok(TrustOutcomePayload::outcome(code));
    // Only what a store line can hold is ever offered to the user.
    if !canonical_host(host)
        || port == 0
        || algorithm != "ssh-ed25519"
        || key.is_empty()
        || key.len() > MAX_BLOB
        || !blob_names(key, algorithm)
        || session.is_zero()
    {
        return Err(KmsErrorCode::InvalidRequest);
    }
    let Some((observation, _, code)) = current(&mut leaf.io, host, port, algorithm, key) else {
        return answer(ssh_trust::UNAVAILABLE);
    };
    // KnownExact needs no prompt; a changed key never prompts.
    if code != ssh_trust::UNKNOWN {
        return answer(code);
    }
    let Ok(now) = owner::now_ns() else {
        return answer(ssh_trust::UNAVAILABLE);
    };
    let table = &mut leaf.challenges;
    // A caller's own expired challenge gives way; any other is released one lifetime later.
    table.records.retain(|r| {
        now < r.expiry.saturating_add(LIFETIME_NS) && !(r.owner == caller && now >= r.expiry)
    });
    if table.records.iter().filter(|r| r.owner == caller).count() >= MAX_PER_CALLER
        || table.records.len() >= MAX_CHALLENGES
    {
        return Err(KmsErrorCode::Busy);
    }
    let challenge = Ref {
        id: table.next,
        generation: 1,
    };
    let Some(next) = table.next.checked_add(1) else {
        return Err(KmsErrorCode::Busy);
    };
    let expiry = now.saturating_add(LIFETIME_NS);
    let fingerprint = fingerprint(key);
    let descriptor = TrustChallengeV1 {
        challenge,
        target: principal(caller),
        expiry_ns: expiry,
        prompt: TrustPromptV1 {
            port,
            host,
            algorithm,
            fingerprint: &fingerprint,
        },
    };
    let mut bytes = [0u8; TrustChallengeV1::MAX_LEN];
    let Ok(len) = descriptor.encode(&mut bytes) else {
        return Err(KmsErrorCode::InvalidRequest);
    };
    let digest: [u8; 32] = Sha256::digest(&bytes[..len]).into();
    // Spent before registering: input may have recorded the challenge even
    // when its reply was lost, and it refuses a reused ref.
    table.next = next;
    let Ok(confirmation) = owner::register(&descriptor) else {
        return answer(ssh_trust::UNAVAILABLE);
    };
    table.records.push(Challenge {
        challenge,
        owner: caller,
        host: String::from(host),
        port,
        algorithm: String::from(algorithm),
        key: key.to_vec(),
        observation,
        expiry,
        confirmation,
        session,
        epoch,
        digest,
    });
    Ok(TrustOutcomePayload::challenged(
        (confirmation.id, confirmation.generation),
        (challenge.id, challenge.generation),
    ))
}

/// One `api::ipc::ssh_trust` code for the caller's own `challenge`.
pub(super) fn commit(leaf: &mut TrustLeaf, caller: Principal, challenge: Ref) -> u8 {
    // Only the caller's own challenge: a guessed or another caller's ref is refused.
    let Some(n) = leaf
        .challenges
        .records
        .iter()
        .position(|r| r.challenge == challenge && r.owner == caller)
    else {
        return ssh_trust::DENIED;
    };
    let decision = match owner::collect(leaf.challenges.records[n].confirmation) {
        Ok(decision) => decision,
        // Uncertain transport: the decision is still the owner's to give.
        Err(Status::Indeterminate | Status::WouldBlock | Status::TimedOut) => {
            return ssh_trust::PENDING
        }
        Err(_) => {
            leaf.challenges.records.swap_remove(n);
            return ssh_trust::UNAVAILABLE;
        }
    };
    if !decision.is_final() {
        return ssh_trust::PENDING;
    }
    let record = leaf.challenges.records.swap_remove(n);
    match decision.decision {
        tr::DECISION_ACCEPT => {
            // The owner's reply must bind exactly this record: challenge, caller, the
            // session and epoch the caller named, and the descriptor this service registered.
            let bound = decision.challenge == record.challenge
                && decision.target == principal(caller)
                && decision.session == record.session
                && decision.epoch == record.epoch
                && decision.descriptor_sha256 == record.digest;
            match owner::now_ns() {
                _ if !bound => ssh_trust::REFUSED,
                Ok(now) if now < record.expiry => persist(&mut leaf.io, &record),
                _ => ssh_trust::EXPIRED,
            }
        }
        tr::DECISION_EXPIRED => ssh_trust::EXPIRED,
        _ => ssh_trust::REFUSED,
    }
}

/// Add the record's line to the store: at most two publication attempts, each against
/// the observation it was built from.
fn persist(io: &mut impl TrustIo, r: &Challenge) -> u8 {
    let mut expected = r.observation;
    for _ in 0..2 {
        let Some((observation, current, code)) = current(io, &r.host, r.port, &r.algorithm, &r.key)
        else {
            return ssh_trust::UNAVAILABLE;
        };
        // Changed since the challenge (or the last attempt): another writer may have
        // stored this host and port meanwhile; its line is never overwritten.
        if observation != expected && code != ssh_trust::UNKNOWN {
            return code;
        }
        let mut bytes = if observation == Observation::Absent {
            String::from(HEADER) + "\n"
        } else {
            match String::from_utf8(current) {
                Ok(text) => text,
                Err(_) => return ssh_trust::UNAVAILABLE,
            }
        };
        bytes.push_str(&r.host);
        bytes.push_str(&alloc::format!(" {} {} ", r.port, r.algorithm));
        base64_encode(&r.key, true, &mut bytes);
        bytes.push_str(" interactive\n");
        // The store's own bounds (entries, blob, total size) hold for the new revision too.
        if parse(bytes.as_bytes()).is_err() || io.ensure_dir().is_err() {
            return ssh_trust::UNAVAILABLE;
        }
        let Some(expect) = observation.expect() else {
            return ssh_trust::UNAVAILABLE;
        };
        match io.publish(bytes.as_bytes(), expect) {
            outcome if outcome.published().is_some() => return ssh_trust::KNOWN_EXACT,
            SaveOutcome::Conflict => expected = observation,
            _ => return ssh_trust::UNAVAILABLE,
        }
    }
    ssh_trust::UNAVAILABLE
}
