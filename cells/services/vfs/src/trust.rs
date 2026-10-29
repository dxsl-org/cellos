//! SSH host trust (Phase 09 S9-3 lookup, S9-4c first trust in [`first`]; abi-vfs-telemetry §4).
//!
//! The store is one file the VFS keeps under a path no caller may read or write
//! (`access::rules`), seeded by the provisioner (D79-10). `TrustLookup` compares the
//! caller's configured host identifier, port, algorithm and complete key blob with it
//! and answers one [`api::ipc::ssh_trust`] code; the store's contents never leave the
//! service. Only a caller holding the attested outbound-connect right may ask.
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
use alloc::vec::Vec;

use api::abi::caller_identity::CALLER_FLAG_NET_CONNECT;
use api::ipc::{ssh_trust, VfsRequest, VfsResponse};

use crate::caller::Caller;
use crate::manager::VfsManager;

/// The protected store (see `access::rules`, `TRUST_DIR`).
pub const STORE_PATH: &str = "/srv/.cellos-trust/ssh_known_hosts";
const HEADER: &str = "cellos-ssh-trust v1";
const MAX_ENTRIES: usize = 1024;
const MAX_BLOB: usize = 4096;
const MAX_STORE: usize = 1024 * 1024;
const MAX_HOST: usize = 253;
const MAX_ALGORITHM: usize = 64;

struct Entry<'a> {
    host: &'a str,
    port: u16,
    algorithm: &'a str,
    blob: Vec<u8>,
}

mod first;
pub use first::Challenges;

pub fn handle(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest) -> Option<VfsResponse<'static>> {
    let (host, port, algorithm, key) = match *req {
        VfsRequest::TrustLookup { host, port, algorithm, key } => (host, port, algorithm, key),
        VfsRequest::TrustChallenge { host, port, algorithm, key, session, epoch } =>
            return Some(first::challenge(vfs, caller, host, port, algorithm, key, session, epoch)),
        VfsRequest::TrustCommit { challenge } => return Some(first::commit(vfs, caller, challenge)),
        _ => return None,
    };
    if caller.flags & CALLER_FLAG_NET_CONNECT == 0 {
        return Some(VfsResponse::Trust(ssh_trust::DENIED));
    }
    if host.is_empty() || host.len() > MAX_HOST || algorithm.is_empty() || algorithm.len() > MAX_ALGORITHM
        || key.is_empty() || key.len() > MAX_BLOB || port == 0
    {
        return Some(VfsResponse::Err(0xFF));
    }
    let code = match vfs.stat(STORE_PATH) {
        None => ssh_trust::UNKNOWN,
        Some((_, true)) => ssh_trust::UNAVAILABLE,
        Some((size, false)) => {
            let bytes = vfs.read_to_vec(STORE_PATH);
            if bytes.len() as u64 != size { ssh_trust::UNAVAILABLE } else { lookup(&bytes, host, port, algorithm, key) }
        }
    };
    Some(VfsResponse::Trust(code))
}

/// The answer for (`host`, `port`, `algorithm`, `key`) against store `bytes`.
pub fn lookup(bytes: &[u8], host: &str, port: u16, algorithm: &str, key: &[u8]) -> u8 {
    let Ok(entries) = parse(bytes) else { return ssh_trust::UNAVAILABLE };
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
        if !canonical_host(host) || algorithm != "ssh-ed25519" || !matches!(provenance, "preprovisioned" | "interactive") {
            return Err(());
        }
        let blob = base64_decode(blob)?;
        if blob.is_empty() || blob.len() > MAX_BLOB || !blob_names(&blob, algorithm) {
            return Err(());
        }
        if entries.len() == MAX_ENTRIES || entries.iter().any(|e| e.host == host && e.port == port) {
            return Err(());
        }
        entries.push(Entry { host, port, algorithm, blob });
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
        && h.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-.:".contains(&b))
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
    if b.is_empty() || b.len() % 4 != 0 {
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
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() { out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char); } else if padded { out.push('='); }
        }
    }
}

#[cfg(test)]
mod tests;
