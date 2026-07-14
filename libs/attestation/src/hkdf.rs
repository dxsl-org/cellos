//! HKDF-SHA256 (RFC 5869) — extract + expand, hand-rolled over the local
//! [`super::sha256`] implementation.
//!
//! Not the `hkdf` crate on purpose (dossier-4 Decision 1 / YAGNI+KISS): SHA-256 is
//! already local, HKDF-SHA256 is ~40 lines on top of it, and this must build in
//! kernel-adjacent `no_std` contexts where a new dependency is unwanted.

use super::sha256::sha256;

const BLOCK_SIZE: usize = 64; // SHA-256 block size
const HASH_LEN: usize = 32; // SHA-256 output size

/// HMAC-SHA256(key, message).
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; HASH_LEN] {
    // Key preparation: hash if longer than the block size, zero-pad otherwise.
    // An empty key zero-pads to a full block of zero bytes — this is exactly
    // equivalent to RFC 5869's "salt defaults to HashLen zero bytes" rule, since
    // padding HashLen zero bytes out to BLOCK_SIZE with more zeros is still all
    // zeros (see `extract`'s doc comment).
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = sha256(key);
        key_block[..HASH_LEN].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    // inner = SHA256(ipad || message) — computed incrementally via a scratch
    // buffer would need alloc for arbitrary-length `message`; instead hash ipad
    // and message as two logically-concatenated inputs by re-using `sha256`
    // over a single buffer built from bounded stack space is not possible for
    // unbounded `message`, so we hash via two calls is NOT how SHA-256 works
    // (it is not a streaming API here) — build the exact concatenation on the
    // heap-free path by allocating on the stack sized to ipad+message. Since
    // `message` length is caller-controlled but always small in this crate
    // (32-byte IKM or a 145-byte token body), a fixed generous stack buffer is
    // acceptable and avoids alloc.
    let mut inner_buf = [0u8; BLOCK_SIZE + MAX_MESSAGE_LEN];
    assert!(
        message.len() <= MAX_MESSAGE_LEN,
        "hmac_sha256: message exceeds crate-internal bound"
    );
    inner_buf[..BLOCK_SIZE].copy_from_slice(&ipad);
    inner_buf[BLOCK_SIZE..BLOCK_SIZE + message.len()].copy_from_slice(message);
    let inner = sha256(&inner_buf[..BLOCK_SIZE + message.len()]);

    let mut outer_buf = [0u8; BLOCK_SIZE + HASH_LEN];
    outer_buf[..BLOCK_SIZE].copy_from_slice(&opad);
    outer_buf[BLOCK_SIZE..].copy_from_slice(&inner);
    sha256(&outer_buf)
}

/// The largest `message` this crate's HMAC ever hashes: HKDF-Expand's `T(i-1) ||
/// info || counter` where `T(i-1)` is 32 bytes, `info` is at most
/// `MAX_INFO_LEN`, and the counter is 1 byte. Bounded so `hmac_sha256` can use a
/// fixed stack buffer instead of `alloc`.
const MAX_INFO_LEN: usize = 32;
const MAX_MESSAGE_LEN: usize = HASH_LEN + MAX_INFO_LEN + 1;

/// HKDF-Extract: `PRK = HMAC-Hash(salt, IKM)`. An empty `salt` is RFC
/// 5869-compliant (see `hmac_sha256`'s key-padding note) — do not special-case it.
pub fn extract(salt: &[u8], ikm: &[u8]) -> [u8; HASH_LEN] {
    hmac_sha256(salt, ikm)
}

/// HKDF-Expand: fills `okm` (any length up to `255 * HASH_LEN`, RFC 5869 §2.3)
/// with `T(1) || T(2) || ...`, `T(i) = HMAC-Hash(PRK, T(i-1) || info || i)`.
/// `info` must fit in `MAX_INFO_LEN` bytes (32) — generous for this crate's only
/// use (empty info in `cdi::derive_cdi`); asserts rather than truncates.
pub fn expand(prk: &[u8; HASH_LEN], info: &[u8], okm: &mut [u8]) {
    assert!(
        info.len() <= MAX_INFO_LEN,
        "hkdf::expand: info exceeds crate-internal bound"
    );
    assert!(
        okm.len() <= 255 * HASH_LEN,
        "hkdf::expand: requested length exceeds RFC 5869 bound"
    );

    let mut t_prev: [u8; HASH_LEN] = [0u8; HASH_LEN];
    let mut t_prev_len = 0usize;
    let mut counter: u8 = 0;
    let mut written = 0usize;

    while written < okm.len() {
        counter += 1;
        let mut msg = [0u8; MAX_MESSAGE_LEN];
        msg[..t_prev_len].copy_from_slice(&t_prev[..t_prev_len]);
        let mut off = t_prev_len;
        msg[off..off + info.len()].copy_from_slice(info);
        off += info.len();
        msg[off] = counter;
        off += 1;

        let t = hmac_sha256(prk, &msg[..off]);
        let take = core::cmp::min(HASH_LEN, okm.len() - written);
        okm[written..written + take].copy_from_slice(&t[..take]);
        written += take;

        t_prev = t;
        t_prev_len = HASH_LEN;
    }
}

#[cfg(test)]
mod tests {
    use super::{expand, extract};

    // RFC 5869 §A.1 — Test Case 1: Basic test case with SHA-256.
    #[test]
    fn rfc5869_test_case_1() {
        let ikm = [0x0bu8; 22];
        let salt: [u8; 13] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
        ];
        let info: [u8; 10] = [0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9];

        let prk = extract(&salt, &ikm);
        assert_eq!(
            prk,
            [
                0x07, 0x77, 0x09, 0x36, 0x2c, 0x2e, 0x32, 0xdf, 0x0d, 0xdc, 0x3f, 0x0d, 0xc4, 0x7b,
                0xba, 0x63, 0x90, 0xb6, 0xc7, 0x3b, 0xb5, 0x0f, 0x9c, 0x31, 0x22, 0xec, 0x84, 0x4a,
                0xd7, 0xc2, 0xb3, 0xe5,
            ],
            "PRK mismatch (RFC 5869 A.1)"
        );

        let mut okm = [0u8; 42];
        expand(&prk, &info, &mut okm);
        assert_eq!(
            okm,
            [
                0x3c, 0xb2, 0x5f, 0x25, 0xfa, 0xac, 0xd5, 0x7a, 0x90, 0x43, 0x4f, 0x64, 0xd0, 0x36,
                0x2f, 0x2a, 0x2d, 0x2d, 0x0a, 0x90, 0xcf, 0x1a, 0x5a, 0x4c, 0x5d, 0xb0, 0x2d, 0x56,
                0xec, 0xc4, 0xc5, 0xbf, 0x34, 0x00, 0x72, 0x08, 0xd5, 0xb8, 0x87, 0x18, 0x58, 0x65,
            ],
            "OKM mismatch (RFC 5869 A.1)"
        );
    }

    // RFC 5869 §A.3 — Test Case 3: zero-length salt and info.
    #[test]
    fn rfc5869_test_case_3() {
        let ikm = [0x0bu8; 22];

        let prk = extract(&[], &ikm);
        assert_eq!(
            prk,
            [
                0x19, 0xef, 0x24, 0xa3, 0x2c, 0x71, 0x7b, 0x16, 0x7f, 0x33, 0xa9, 0x1d, 0x6f, 0x64,
                0x8b, 0xdf, 0x96, 0x59, 0x67, 0x76, 0xaf, 0xdb, 0x63, 0x77, 0xac, 0x43, 0x4c, 0x1c,
                0x29, 0x3c, 0xcb, 0x04,
            ],
            "PRK mismatch (RFC 5869 A.3)"
        );

        let mut okm = [0u8; 42];
        expand(&prk, &[], &mut okm);
        assert_eq!(
            okm,
            [
                0x8d, 0xa4, 0xe7, 0x75, 0xa5, 0x63, 0xc1, 0x8f, 0x71, 0x5f, 0x80, 0x2a, 0x06, 0x3c,
                0x5a, 0x31, 0xb8, 0xa1, 0x1f, 0x5c, 0x5e, 0xe1, 0x87, 0x9e, 0xc3, 0x45, 0x4e, 0x5f,
                0x3c, 0x73, 0x8d, 0x2d, 0x9d, 0x20, 0x13, 0x95, 0xfa, 0xa4, 0xb6, 0x1a, 0x96, 0xc8,
            ],
            "OKM mismatch (RFC 5869 A.3)"
        );
    }
}
