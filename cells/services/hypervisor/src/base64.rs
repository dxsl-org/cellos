//! Standard base64 (RFC 4648 §4: `A-Z a-z 0-9 + /`, `=` padding), for the
//! clipboard lines on the guest's console (`clipboard_line`).

extern crate alloc;

use alloc::vec::Vec;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PAD: u8 = b'=';

/// Append the padded encoding of `data` to `out`.
pub fn encode_into(data: &[u8], out: &mut Vec<u8>) {
    out.reserve(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextet = |shift: u32| ALPHABET[((n >> shift) & 0x3f) as usize];
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { PAD });
        out.push(if chunk.len() > 2 { sextet(0) } else { PAD });
    }
}

fn value(c: u8) -> Option<u32> {
    let v = match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    Some(u32::from(v))
}

/// Decode padded base64. `None` for a length that is not a multiple of 4, a
/// byte outside the alphabet, or padding anywhere but the last one or two
/// places of the last quantum.
pub fn decode(text: &[u8]) -> Option<Vec<u8>> {
    let (quads, []) = text.as_chunks::<4>() else {
        return None;
    };
    let mut out = Vec::with_capacity(quads.len() * 3);
    for (i, quad) in quads.iter().enumerate() {
        let pads = match (quad[2], quad[3]) {
            (PAD, PAD) => 2,
            (_, PAD) => 1,
            _ => 0,
        };
        if pads > 0 && i + 1 != quads.len() {
            return None;
        }
        let mut n = 0u32;
        for &c in &quad[..4 - pads] {
            n = (n << 6) | value(c)?;
        }
        n <<= 6 * pads as u32;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pads]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10.
    const VECTORS: [(&str, &str); 7] = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];

    #[test]
    fn base64_encodes_rfc4648_vectors() {
        for (plain, encoded) in VECTORS {
            let mut out = Vec::new();
            encode_into(plain.as_bytes(), &mut out);
            assert_eq!(out, encoded.as_bytes());
        }
    }

    #[test]
    fn base64_decodes_rfc4648_vectors() {
        for (plain, encoded) in VECTORS {
            assert_eq!(decode(encoded.as_bytes()).unwrap(), plain.as_bytes());
        }
    }

    #[test]
    fn base64_round_trips_every_byte_value() {
        let data: Vec<u8> = (0..=255u8).collect();
        let mut out = Vec::new();
        encode_into(&data, &mut out);
        assert_eq!(decode(&out).unwrap(), data);
    }

    #[test]
    fn base64_refuses_bad_length_alphabet_and_inner_padding() {
        for bad in ["Zg=", "Zm9", "Zm9-", "Zg==Zm9v", "Z===", "=m9v", "Zm 9"] {
            assert!(decode(bad.as_bytes()).is_none(), "{bad}");
        }
    }
}
