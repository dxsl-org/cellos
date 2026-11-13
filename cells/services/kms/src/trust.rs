//! SSH host trust (Phase 09 S9-3 lookup, S9-4c first trust in [`first`]; abi-vfs-telemetry
//! §4), served by KMS since Phase 12 (CP1 W3-W5).
//!
//! The store is one file under the KMS namespace (`/srv/cellos/kms`, which the VFS admits
//! only to the live KMS), seeded by the provisioner (D79-10). A lookup compares the
//! caller's configured host identifier, port, algorithm and complete key blob with it and
//! answers one [`wire::ipc::ssh_trust`] code; the store's contents never leave the
//! service. Only an attested caller holding the outbound-connect right may ask. A query
//! is staged in [`query`] (opcodes 25-27, 29) because host and key do not fit one frame.
//!
//! Store format v1 (UTF-8 text, LF line ends, no comments or blank lines):
//!
//! ```text
//! cellos-ssh-trust v1
//! HOST PORT ALGORITHM BASE64-KEY-BLOB PROVENANCE
//! ```
//!
//! `HOST` is the canonical configured identifier (lowercase ASCII name or canonical IP
//! literal), `PORT` decimal 1..=65535, `ALGORITHM` `ssh-ed25519`, the blob is the full
//! SSH wire key (standard base64, padded) and `PROVENANCE` is `preprovisioned` or
//! `interactive`. At most one line per (host, port); at most 1,024 entries, 4 KiB per
//! blob and 1 MiB in total. Anything else makes the whole store unavailable, never a
//! partial answer.

extern crate alloc;

use alloc::vec::Vec;

use abi::caller_identity::{CallerIdentity, CALLER_FLAG_NET_CONNECT};
use abi::cell_session::ObjectRefV1 as Ref;
use kms_codec::{
    KmsErrorCode, KmsOpcode, KmsTrustQueryKind, SshAbortRequestPayload, SshChunkRequestPayload,
    SshImportCommitRequestPayload, TrustChallengeCommitRequestPayload, TrustOutcomePayload,
    TrustQueryBeginRequestPayload, KMS_TRUST_ALGORITHM_MAX, KMS_TRUST_HOST_MAX, KMS_TRUST_KEY_MAX,
};
use wire::ipc::{ssh_trust, Observation, SaveExpect, SaveOutcome};

use crate::auth::attested_caller;
use crate::reply::SuccessPayload;

mod first;
#[cfg(not(test))]
mod io;
mod query;

const HEADER: &str = "cellos-ssh-trust v1";
const MAX_ENTRIES: usize = 1024;
const MAX_BLOB: usize = KMS_TRUST_KEY_MAX;
const MAX_STORE: usize = 1024 * 1024;
const MAX_HOST: usize = KMS_TRUST_HOST_MAX;
const _: () = assert!(KMS_TRUST_ALGORITHM_MAX == 64);

/// Input-owner seam and clock: production calls service-input; host tests run a fake owner.
#[cfg(not(test))]
mod owner {
    pub use ostd::session::{
        collect_trust_decision as collect, monotonic_ns as now_ns,
        register_trust_challenge as register,
    };
}
#[cfg(test)]
use crate::tests::trust::owner;

/// Storage seam: production goes through the VFS client; host tests use a fake store.
#[cfg(not(test))]
pub(crate) type Io = io::VfsTrustIo;
#[cfg(test)]
pub(crate) type Io = crate::tests::trust::MemStore;

/// The store as KMS reaches it. Every failure is `Err(())`: the caller answers
/// Unavailable, never a guess.
pub(crate) trait TrustIo {
    /// The store's exact identity.
    fn observe(&mut self) -> Result<Observation, ()>;
    /// The store's bytes; a store above `max` bytes is an error.
    fn read(&mut self, max: usize) -> Result<Vec<u8>, ()>;
    /// Make the store's directory (and its parent) exist.
    fn ensure_dir(&mut self) -> Result<(), ()>;
    /// Publish `bytes` as the store only if it still matches `expect`.
    fn publish(&mut self, bytes: &[u8], expect: SaveExpect) -> SaveOutcome;
}

/// Attested `(cell_id, generation)`: the key of staged queries and challenges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Principal {
    pub cell_id: u64,
    pub generation: u64,
}

/// The trust leaf of the service: open queries, open challenges and the store.
pub(crate) struct TrustLeaf {
    pub(crate) io: Io,
    queries: query::Queries,
    challenges: first::Challenges,
}

struct Entry<'a> {
    host: &'a str,
    port: u16,
    algorithm: &'a str,
    blob: Vec<u8>,
}

fn invalid<T>(value: Option<T>) -> Result<T, KmsErrorCode> {
    value.ok_or(KmsErrorCode::InvalidRequest)
}

impl TrustLeaf {
    pub(crate) fn new() -> Self {
        Self {
            io: Io::new(),
            queries: query::Queries::new(),
            challenges: first::Challenges::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn queries_open(&self) -> usize {
        self.queries.open()
    }

    /// Handle one trust opcode (25-29). Admission first: an attested caller holding the
    /// outbound-connect right; nothing is decoded before it passes.
    pub(crate) fn handle(
        &mut self,
        opcode: KmsOpcode,
        payload: &[u8],
        sender: usize,
        caller: Option<CallerIdentity>,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let caller = attested_caller(sender, caller)?;
        if caller.flags & CALLER_FLAG_NET_CONNECT == 0 {
            return Err(KmsErrorCode::PermissionDenied);
        }
        let principal = Principal {
            cell_id: caller.cell_id,
            generation: caller.generation,
        };
        match opcode {
            KmsOpcode::BeginTrustQuery => {
                let request = invalid(TrustQueryBeginRequestPayload::decode(payload))?;
                let begin = self.queries.begin(principal, &request)?;
                SuccessPayload::new(&begin.encode())
            }
            KmsOpcode::TrustQueryChunk => {
                let chunk = invalid(SshChunkRequestPayload::decode(payload))?;
                let ack = self.queries.chunk(principal, &chunk)?;
                SuccessPayload::new(&ack.encode())
            }
            KmsOpcode::CommitTrustQuery => {
                let request = invalid(SshImportCommitRequestPayload::decode(payload))?;
                let staged = self.queries.commit(principal, &request)?;
                let (host, algorithm, key) = staged.fields()?;
                let outcome = match staged.kind {
                    KmsTrustQueryKind::Lookup => TrustOutcomePayload::outcome(
                        first::current(&mut self.io, host, staged.port, algorithm, key)
                            .map_or(ssh_trust::UNAVAILABLE, |(_, _, code)| code),
                    ),
                    KmsTrustQueryKind::Challenge => first::challenge(
                        self,
                        principal,
                        host,
                        staged.port,
                        algorithm,
                        key,
                        staged.session,
                        staged.epoch,
                    )?,
                };
                SuccessPayload::new(&outcome.encode())
            }
            KmsOpcode::CommitTrustChallenge => {
                let request = invalid(TrustChallengeCommitRequestPayload::decode(payload))?;
                let (id, generation) = request.challenge;
                let code = first::commit(self, principal, Ref { id, generation });
                SuccessPayload::new(&TrustOutcomePayload::outcome(code).encode())
            }
            KmsOpcode::AbortTrustQuery => {
                let request = invalid(SshAbortRequestPayload::decode(payload))?;
                self.queries.abort(principal, request.handle)?;
                SuccessPayload::new(&[])
            }
            _ => Err(KmsErrorCode::UnknownOpcode),
        }
    }
}

/// The answer for (`host`, `port`, `algorithm`, `key`) against store `bytes`.
pub(crate) fn lookup(bytes: &[u8], host: &str, port: u16, algorithm: &str, key: &[u8]) -> u8 {
    let Ok(entries) = parse(bytes) else {
        return ssh_trust::UNAVAILABLE;
    };
    match entries.iter().find(|e| e.host == host && e.port == port) {
        None => ssh_trust::UNKNOWN,
        Some(e) if e.algorithm == algorithm && e.blob == key => ssh_trust::KNOWN_EXACT,
        Some(_) => ssh_trust::MISMATCH,
    }
}

fn parse(bytes: &[u8]) -> Result<Vec<Entry<'_>>, ()> {
    if bytes.len() > MAX_STORE {
        return Err(());
    }
    let text = core::str::from_utf8(bytes).map_err(|_| ())?;
    let body = text.strip_suffix('\n').ok_or(())?;
    let mut lines = body.split('\n');
    if lines.next() != Some(HEADER) {
        return Err(());
    }
    let mut entries: Vec<Entry> = Vec::new();
    for line in lines {
        let mut f = line.split(' ');
        let (Some(host), Some(port), Some(algorithm), Some(blob), Some(provenance), None) =
            (f.next(), f.next(), f.next(), f.next(), f.next(), f.next())
        else {
            return Err(());
        };
        let port = parse_port(port)?;
        if !canonical_host(host)
            || algorithm != "ssh-ed25519"
            || !matches!(provenance, "preprovisioned" | "interactive")
        {
            return Err(());
        }
        let blob = base64_decode(blob)?;
        if blob.is_empty() || blob.len() > MAX_BLOB || !blob_names(&blob, algorithm) {
            return Err(());
        }
        if entries.len() == MAX_ENTRIES || entries.iter().any(|e| e.host == host && e.port == port)
        {
            return Err(());
        }
        entries.push(Entry {
            host,
            port,
            algorithm,
            blob,
        });
    }
    Ok(entries)
}

fn parse_port(s: &str) -> Result<u16, ()> {
    if s.is_empty() || s.len() > 5 || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    match s.parse::<u32>() {
        Ok(n) if (1..=65535).contains(&n) => Ok(n as u16),
        _ => Err(()),
    }
}

/// Lowercase ASCII name labels or an IP literal; never empty, a wildcard or a port.
fn canonical_host(h: &str) -> bool {
    (1..=MAX_HOST).contains(&h.len())
        && h.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-.:".contains(&b))
        && !h.ends_with('.')
}

/// The blob's first SSH string must name the line's algorithm.
fn blob_names(blob: &[u8], algorithm: &str) -> bool {
    blob.len() >= 4 + algorithm.len()
        && u32::from_be_bytes([blob[0], blob[1], blob[2], blob[3]]) as usize == algorithm.len()
        && &blob[4..4 + algorithm.len()] == algorithm.as_bytes()
}

fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
    let b = s.as_bytes();
    if b.is_empty() || !b.len().is_multiple_of(4) {
        return Err(());
    }
    let mut out = Vec::with_capacity(b.len() / 4 * 3);
    for (i, quad) in b.chunks(4).enumerate() {
        let last = i == b.len() / 4 - 1;
        let mut n = 0u32;
        let mut pad = 0;
        for (j, &c) in quad.iter().enumerate() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if last && j >= 2 => {
                    pad += 1;
                    0
                }
                _ => return Err(()),
            };
            if pad > 0 && c != b'=' {
                return Err(());
            }
            n = n << 6 | u32::from(v);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Ok(out)
}

/// Standard base64 of `data` appended to `out`, `=`-padded or not.
fn base64_encode(data: &[u8], padded: bool, out: &mut alloc::string::String) {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else if padded {
                out.push('=');
            }
        }
    }
}
