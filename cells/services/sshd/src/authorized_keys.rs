//! `/srv/sshd/authorized_keys`: the only source of login authority.
//!
//! Format, one key per line: `ssh-ed25519 <base64 blob> [comment]`. Blank lines
//! and lines starting with `#` are skipped; a trailing `\r` is tolerated. Any
//! other key type, option prefix (`from=…`, `command=…`) or malformed blob makes
//! that line authorize nothing — sshd does not implement OpenSSH key options, so
//! honouring the key while dropping its restriction would widen what the
//! administrator wrote.

extern crate alloc;
use alloc::vec::Vec;

use crate::base64;
use crate::ssh_packet::PacketReader;

/// Path of the store. `/srv/<service>/` is the per-service convention on the
/// CellosFS service volume.
#[cfg(target_os = "none")]
const PATH: &str = "/srv/sshd/authorized_keys";
/// Largest file read; a longer file is refused as a whole (no key authorizes).
#[cfg(target_os = "none")]
const MAX_FILE: usize = 16 * 1024;

pub const KEY_TYPE: &[u8] = b"ssh-ed25519";

/// The 32-byte key inside an `ssh-ed25519` public key blob
/// (`string "ssh-ed25519" || string key`), with no trailing bytes.
pub fn ed25519_from_blob(blob: &[u8]) -> Option<[u8; 32]> {
    let mut r = PacketReader::new(blob);
    if r.read_string()? != KEY_TYPE {
        return None;
    }
    let key: [u8; 32] = r.read_string()?.try_into().ok()?;
    r.is_at_end().then_some(key)
}

/// One line's key, or `None` when the line authorizes nothing.
fn parse_line(line: &[u8]) -> Option<[u8; 32]> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let mut fields = line
        .split(|&b| b == b' ' || b == b'\t')
        .filter(|f| !f.is_empty());
    if fields.next()? != KEY_TYPE {
        return None;
    }
    ed25519_from_blob(&base64::decode(fields.next()?)?)
}

/// Every authorized Ed25519 key in `text` (comments and invalid lines skipped).
pub fn parse(text: &[u8]) -> Vec<[u8; 32]> {
    text.split(|&b| b == b'\n')
        .filter(|line| {
            let first = line.iter().find(|&&b| b != b' ' && b != b'\t');
            !matches!(first, None | Some(b'#') | Some(b'\r'))
        })
        .filter_map(parse_line)
        .collect()
}

/// Read the store. Missing, unreadable or oversized → no key (fail closed:
/// nobody logs in rather than everybody).
#[cfg(target_os = "none")]
pub fn load() -> Vec<[u8; 32]> {
    let mut vfs = ostd::clients::vfs::VfsClient::new();
    match vfs.read_file_bounded(PATH, MAX_FILE) {
        Ok(text) => {
            let keys = parse(&text);
            if keys.is_empty() {
                crate::log("[sshd] authorized_keys holds no ssh-ed25519 key: logins refused");
            }
            keys
        }
        Err(_) => {
            crate::log("[sshd] /srv/sshd/authorized_keys unreadable: logins refused");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    /// `ssh-ed25519` blob of the key `0x01 0x02 … 0x20`, base64.
    const B64: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8g";

    fn key() -> [u8; 32] {
        core::array::from_fn(|i| i as u8 + 1)
    }

    #[test]
    fn accepts_valid_lines_with_comments_and_crlf() {
        let text = alloc::format!(
            "# admin keys\r\n\r\nssh-ed25519 {B64}\nssh-ed25519 {B64} admin@laptop\r\n  \n"
        );
        assert_eq!(parse(text.as_bytes()), [key(), key()]);
    }

    #[test]
    fn rejects_bad_base64_and_other_key_types() {
        let wrong_type_blob = "AAAAB3NzaC1yc2EAAAAgAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=";
        for line in [
            alloc::format!("ssh-ed25519 {}!", &B64[..B64.len() - 1]),
            alloc::format!("ssh-ed25519 {}", &B64[..B64.len() - 2]),
            alloc::format!("ssh-rsa {B64}"),
            alloc::format!("ssh-ed25519 {wrong_type_blob}"),
            alloc::format!("from=\"10.0.0.1\" ssh-ed25519 {B64}"),
            String::from("ssh-ed25519"),
        ] {
            assert!(parse(line.as_bytes()).is_empty(), "{line}");
        }
    }
}
