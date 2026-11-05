//! The persisted Ed25519 host key.
//!
//! The 32-byte seed lives at `/srv/sshd/host_ed25519.seed` on the CellosFS
//! `/srv` volume. It is loaded when present and exactly 32 bytes; when absent
//! (or the wrong size) a fresh seed is drawn from kernel entropy, written, and
//! read back before use.
//! Every failure — no entropy, no `/srv`, a write that does not persist, a VFS
//! error that cannot be told apart from "absent" — stops sshd: serving a key
//! that is constant, guessable, or gone after the next boot would teach clients
//! to accept host key changes.

extern crate alloc;
use alloc::string::String;

use ed25519_compact::{KeyPair, Seed};
use sha2::{Digest, Sha256};

use crate::base64;

#[cfg(target_os = "none")]
const DIR: &str = "/srv/sshd";
#[cfg(target_os = "none")]
const SEED_PATH: &str = "/srv/sshd/host_ed25519.seed";
const SEED_NAME: &str = "host_ed25519.seed";

/// The key pair for a stored seed, or `None` unless it is exactly 32 bytes.
pub fn from_seed_bytes(bytes: &[u8]) -> Option<KeyPair> {
    let seed: [u8; 32] = bytes.try_into().ok()?;
    Some(KeyPair::from_seed(Seed::new(seed)))
}

/// OpenSSH-style fingerprint: `SHA256:` + unpadded base64 of SHA-256(blob).
pub fn fingerprint(keypair: &KeyPair) -> String {
    let digest = Sha256::digest(crate::kex::host_key_blob(keypair));
    let mut s = String::from("SHA256:");
    base64::encode_unpadded(&digest, &mut s);
    s
}

/// Whether a `ListDir` answer names `name` (entries are `f:NAME`, `d:NAME` or
/// a bare `NAME`, one per line).
pub fn listing_names(listing: &[u8], name: &str) -> bool {
    listing.split(|&b| b == b'\n').any(|entry| {
        let entry = entry
            .strip_prefix(b"f:")
            .or_else(|| entry.strip_prefix(b"d:"))
            .unwrap_or(entry);
        entry == name.as_bytes()
    })
}

#[cfg(target_os = "none")]
pub use store::load_or_create;

#[cfg(target_os = "none")]
mod store {
    use super::*;
    use crate::kex::Entropy;
    use ostd::clients::vfs::VfsClient;
    use ostd::ViError;

    enum Stored {
        Valid(KeyPair),
        Absent,
        Invalid,
    }

    fn ensure_dir(vfs: &mut VfsClient) -> Result<(), &'static str> {
        match vfs.stat(DIR) {
            Ok((_, true)) => Ok(()),
            Ok((_, false)) => Err("/srv/sshd is a file"),
            Err(_) => vfs.mkdir(DIR).map_err(|_| "cannot create /srv/sshd"),
        }
    }

    fn read(vfs: &mut VfsClient) -> Result<Stored, &'static str> {
        match vfs.stat(SEED_PATH) {
            Ok((_, true)) => Err("host key seed path is a directory"),
            Ok((32, false)) => match vfs.read_file_bounded(SEED_PATH, 32) {
                Ok(bytes) => Ok(from_seed_bytes(&bytes).map_or(Stored::Invalid, Stored::Valid)),
                Err(_) => Err("host key seed unreadable"),
            },
            Ok(_) => Ok(Stored::Invalid),
            // VFS answers "missing" and "backend failed" with the same code;
            // only a clean listing without the name counts as absent.
            Err(ViError::IO) => match vfs.list_dir(DIR) {
                Ok(listing) if !listing_names(&listing, SEED_NAME) => Ok(Stored::Absent),
                _ => Err("host key seed state unknown"),
            },
            Err(_) => Err("host key seed unreadable"),
        }
    }

    /// Load the persisted host key, or create and persist one.
    pub fn load_or_create(entropy: Entropy) -> Result<KeyPair, &'static str> {
        let mut vfs = VfsClient::new();
        ensure_dir(&mut vfs)?;
        match read(&mut vfs)? {
            Stored::Valid(keypair) => return Ok(keypair),
            Stored::Invalid => crate::log("[sshd] host key seed is not 32 bytes: replacing it"),
            Stored::Absent => crate::log("[sshd] no host key yet: generating one"),
        }
        let mut seed = [0u8; 32];
        if !entropy(&mut seed) {
            return Err("kernel entropy unavailable: host key not generated");
        }
        vfs.write_file(SEED_PATH, &seed)
            .map_err(|_| "cannot write host key seed")?;
        match vfs.read_file_bounded(SEED_PATH, 32) {
            Ok(stored) if stored == seed => Ok(KeyPair::from_seed(Seed::new(seed))),
            _ => Err("host key seed did not persist"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_must_be_exactly_32_bytes_and_listing_matches_exact_names() {
        assert!(from_seed_bytes(&[7; 31]).is_none());
        assert!(from_seed_bytes(&[7; 33]).is_none());
        let kp = from_seed_bytes(&[7; 32]).unwrap();
        assert!(fingerprint(&kp).starts_with("SHA256:"));
        assert_eq!(fingerprint(&kp).len(), 7 + 43);
        assert!(listing_names(b"f:a\nf:host_ed25519.seed", SEED_NAME));
        assert!(listing_names(b"host_ed25519.seed\n", SEED_NAME));
        assert!(!listing_names(b"f:host_ed25519.seed.tmp\nd:x", SEED_NAME));
    }
}
