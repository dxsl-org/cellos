//! Standard-alphabet base64 (RFC 4648 §4): strict decoding for `authorized_keys`
//! blobs and unpadded encoding for the OpenSSH `SHA256:` fingerprint.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a') as u32 + 26),
        b'0'..=b'9' => Some((c - b'0') as u32 + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode padded base64. `None` for any character outside the alphabet, a
/// length that is not a multiple of 4, misplaced `=`, or non-zero trailing
/// bits: a key line that is not exactly one canonical encoding is refused
/// rather than guessed at.
pub fn decode(text: &[u8]) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let quads = text.len() / 4;
    for (i, quad) in text.chunks_exact(4).enumerate() {
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != quads) {
            return None;
        }
        let mut acc = 0u32;
        for &c in &quad[..4 - pad] {
            acc = (acc << 6) | value(c)?;
        }
        acc <<= 6 * pad as u32;
        let bytes = acc.to_be_bytes();
        let keep = 3 - pad;
        if bytes[1 + keep..].iter().any(|&b| b != 0) {
            return None;
        }
        out.extend_from_slice(&bytes[1..1 + keep]);
    }
    Some(out)
}

/// Encode without `=` padding (the form OpenSSH prints after `SHA256:`).
pub fn encode_unpadded(bytes: &[u8], out: &mut String) {
    for chunk in bytes.chunks(3) {
        let mut acc = [0u8; 3];
        acc[..chunk.len()].copy_from_slice(chunk);
        let n = u32::from_be_bytes([0, acc[0], acc[1], acc[2]]);
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_non_canonical_text() {
        assert_eq!(decode(b"Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(decode(b"Zm9vYg==").unwrap(), b"foob");
        assert_eq!(decode(b"Zm9vYmE=").unwrap(), b"fooba");
        for bad in [
            &b"Zm9vYg="[..],
            b"Zm9v=mFy",
            b"Zm9vYh==",
            b"Zm9v*mFy",
            b"Zg==Zg==",
        ] {
            assert_eq!(decode(bad), None, "{bad:?}");
        }
        let mut s = String::new();
        encode_unpadded(b"foob", &mut s);
        assert_eq!(s, "Zm9vYg");
    }
}
