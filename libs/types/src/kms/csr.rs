// SPDX-License-Identifier: MPL-2.0
//! Frozen bounded profile and canonical RFC 2986 assembly for the relay CSR.
//!
//! KMS and the relay provider independently build byte-identical
//! CertificationRequestInfo values from these definitions alone. The profile
//! is deliberately minimal: exactly one subject attribute (CN, UTF8String),
//! no requested extensions (the managed CA adds `clientAuth` EKU and the
//! NodeId extension), a P-256 SPKI, and `ecdsa-with-SHA256`. Nothing here can
//! express an arbitrary subject, extension, scheme, or digest.

/// Maximum relay hostname length (also the CN bound). Frozen.
pub const RELAY_HOSTNAME_MAX: usize = 64;
/// Maximum canonical CSR length. Frozen; larger CSRs never leave KMS.
pub const RELAY_CSR_MAX_LEN: usize = 1024;
/// CSR chunk capacity inside one fixed 112-byte KMS payload. Frozen.
pub const RELAY_CSR_CHUNK_LEN: usize = 104;
/// Maximum certificates in a mounted client chain. Frozen.
pub const RELAY_CHAIN_MAX_CERTS: usize = 3;
/// Maximum total bytes of a mounted client chain. Frozen.
pub const RELAY_CHAIN_MAX_LEN: usize = 12 * 1024;

/// DER content bytes of the exact NodeId extension OID
/// `1.3.6.1.4.1.55555.1.1` (`06 09 ...`).
pub const NODE_ID_OID_CONTENT: [u8; 10] =
    [0x2B, 0x06, 0x01, 0x04, 0x01, 0x83, 0xB2, 0x03, 0x01, 0x01];
/// DER content bytes of `id-ecPublicKey` (`1.2.840.10045.2.1`).
const EC_PUBLIC_KEY_OID: [u8; 7] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
/// DER content bytes of `prime256v1` (`1.2.840.10045.3.1.7`).
const PRIME256V1_OID: [u8; 8] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
/// DER content bytes of `ecdsa-with-SHA256` (`1.2.840.10045.4.3.2`).
const ECDSA_WITH_SHA256_OID: [u8; 8] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
/// DER content bytes of the commonName attribute type (`2.5.4.3`).
const COMMON_NAME_OID: [u8; 3] = [0x55, 0x04, 0x03];

const TAG_INTEGER: u8 = 0x02;
const TAG_BIT_STRING: u8 = 0x03;
const TAG_UTF8_STRING: u8 = 0x0C;
const TAG_SEQUENCE: u8 = 0x30;
const TAG_SET: u8 = 0x31;
const TAG_OID: u8 = 0x06;

/// Upper bound on the canonical CRI for this frozen profile.
pub const CRI_MAX_LEN: usize = 256;

/// Validate a hostname against the frozen DNS profile.
///
/// Lowercase letters, digits, hyphen, and dot only; non-empty labels of at
/// most 63 characters that neither start nor end with a hyphen; single dots
/// between labels; no leading or trailing dot; total length within
/// [`RELAY_HOSTNAME_MAX`].
pub fn validate_hostname(hostname: &[u8]) -> bool {
    if hostname.is_empty() || hostname.len() > RELAY_HOSTNAME_MAX {
        return false;
    }
    let mut label_len = 0usize;
    let mut ends_with_hyphen = false;
    for (index, &byte) in hostname.iter().enumerate() {
        match byte {
            b'.' => {
                if label_len == 0 || label_len > 63 || ends_with_hyphen {
                    return false; // empty, oversized, or hyphen-terminated label
                }
                if index + 1 == hostname.len() {
                    return false; // trailing dot
                }
                label_len = 0;
                ends_with_hyphen = false;
            }
            b'a'..=b'z' | b'0'..=b'9' => {
                label_len += 1;
                ends_with_hyphen = false;
            }
            b'-' => {
                if label_len == 0 {
                    return false; // hyphen may not open a label
                }
                label_len += 1;
                ends_with_hyphen = true;
            }
            _ => return false,
        }
    }
    label_len != 0 && label_len <= 63 && !ends_with_hyphen
}

/// Build the canonical 91-byte P-256 SubjectPublicKeyInfo DER from an
/// uncompressed SEC1 point (65 bytes, `04 || X || Y`).
///
/// Returns `(buffer, length)`; the buffer tail beyond `length` is zero.
pub fn p256_spki_der(sec1_point: &[u8; 65]) -> Option<([u8; 96], usize)> {
    if sec1_point[0] != 0x04 {
        return None;
    }
    let mut alg_oids = [0u8; 32];
    let mut oids_len = put_tlv(&mut alg_oids, 0, TAG_OID, &EC_PUBLIC_KEY_OID);
    oids_len = put_tlv(&mut alg_oids, oids_len, TAG_OID, &PRIME256V1_OID);
    let mut alg = [0u8; 32];
    let mut alg_len =
        put_head(&mut alg, 0, TAG_SEQUENCE, oids_len).expect("fixed algorithm scratch");
    alg_len += copy_into(&mut alg, alg_len, &alg_oids[..oids_len]);
    let mut spki = [0u8; 96];
    let mut spki_len = put_head(&mut spki, 0, TAG_SEQUENCE, alg_len + 68)?;
    spki_len += copy_into(&mut spki, spki_len, &alg[..alg_len]);
    spki_len = put_head(&mut spki, spki_len, TAG_BIT_STRING, 66)?;
    spki[spki_len] = 0x00;
    spki_len += 1;
    spki_len += copy_into(&mut spki, spki_len, sec1_point);
    debug_assert_eq!(spki_len, 91);
    Some((spki, spki_len))
}

/// Canonical RFC 2986 CertificationRequestInfo for the frozen relay profile.
///
/// `SEQUENCE { INTEGER 0, SEQUENCE { SET { SEQ { CN, UTF8String(host) } } },
/// SPKI, [0] IMPLICIT SET OF Attribute (empty) }`.
///
/// The subject is a full `Name ::= SEQUENCE OF RelativeDistinguishedName`,
/// and the `[0]` attributes element is mandatory even when empty (`A0 00`);
/// it is never omitted.
/// Returns `(buffer, length)`; the buffer tail beyond `length` is zero.
pub fn canonical_relay_cri(
    hostname: &[u8],
    sec1_point: &[u8; 65],
) -> Option<([u8; CRI_MAX_LEN], usize)> {
    if !validate_hostname(hostname) {
        return None;
    }
    let (spki, spki_len) = p256_spki_der(sec1_point)?;
    let mut body = [0u8; CRI_MAX_LEN];
    let mut len = put_tlv(&mut body, 0, TAG_INTEGER, &[0x00]);
    // Name: SEQUENCE { SET { SEQUENCE { OID cn, UTF8String hostname } } }.
    let attr_len = 2 + COMMON_NAME_OID.len() + 2 + hostname.len();
    let rdn_content_len = 2 + attr_len;
    let name_content_len = 2 + rdn_content_len;
    len = put_head(&mut body, len, TAG_SEQUENCE, name_content_len)?;
    len = put_head(&mut body, len, TAG_SET, rdn_content_len)?;
    len = put_head(&mut body, len, TAG_SEQUENCE, attr_len)?;
    len = put_tlv(&mut body, len, TAG_OID, &COMMON_NAME_OID);
    len = put_tlv(&mut body, len, TAG_UTF8_STRING, hostname);
    len += copy_into(&mut body, len, &spki[..spki_len]);
    // Mandatory empty [0] IMPLICIT Attributes element.
    len += copy_into(&mut body, len, &[0xA0, 0x00]);
    let mut cri = [0u8; CRI_MAX_LEN];
    let cri_len = put_head(&mut cri, 0, TAG_SEQUENCE, len)?;
    cri_len_checked(cri, cri_len, &body[..len])
}

fn cri_len_checked(
    mut cri: [u8; CRI_MAX_LEN],
    head_len: usize,
    body: &[u8],
) -> Option<([u8; CRI_MAX_LEN], usize)> {
    if head_len + body.len() > CRI_MAX_LEN {
        return None;
    }
    let total = head_len + copy_into(&mut cri, head_len, body);
    Some((cri, total))
}

/// DER-encode an ECDSA-Sig-Value from raw big-endian `r||s` scalars,
/// stripping leading zero octets as required by DER INTEGER rules.
///
/// Returns `(buffer, length)` holding the complete `SEQUENCE { r, s }`.
pub fn der_ecdsa_signature(raw: &[u8; 64]) -> ([u8; 72], usize) {
    let mut body = [0u8; 70];
    let mut len = put_integer(&mut body, 0, &raw[..32]);
    len = put_integer(&mut body, len, &raw[32..]);
    let mut out = [0u8; 72];
    let head = der_seq_head(len as u8);
    out[..head.len()].copy_from_slice(&head);
    let total = head.len() + copy_into(&mut out, head.len(), &body[..len]);
    (out, total)
}

/// Assemble the full canonical PKCS#10 CSR:
/// `SEQUENCE { CRI, SEQ { OID ecdsa-with-SHA256 }, BIT STRING sig }`.
///
/// Returns `(buffer, length)`; the buffer tail beyond `length` is zero.
pub fn assemble_relay_csr(
    cri: &[u8],
    cri_len: usize,
    sig_der: &[u8],
    sig_der_len: usize,
) -> Option<([u8; RELAY_CSR_MAX_LEN], usize)> {
    if cri_len == 0 || cri_len > cri.len() || sig_der_len == 0 || sig_der_len > sig_der.len() {
        return None;
    }
    // AlgorithmIdentifier: SEQUENCE head plus the full OID TLV inside it.
    let alg_len = 4 + ECDSA_WITH_SHA256_OID.len();
    let bit_len = 3 + sig_der_len; // tag+len header, unused-bits byte, scalars
    let body_len = cri_len + alg_len + bit_len;
    if body_len > RELAY_CSR_MAX_LEN {
        return None;
    }
    let mut out = [0u8; RELAY_CSR_MAX_LEN];
    let mut len = put_head(&mut out, 0, TAG_SEQUENCE, body_len)?;
    len += copy_into(&mut out, len, &cri[..cri_len]);
    len = put_head(&mut out, len, TAG_SEQUENCE, 2 + ECDSA_WITH_SHA256_OID.len())?;
    len = put_tlv(&mut out, len, TAG_OID, &ECDSA_WITH_SHA256_OID);
    len = put_head(&mut out, len, TAG_BIT_STRING, sig_der_len + 1)?;
    out[len] = 0x00;
    len += 1;
    len += copy_into(&mut out, len, &sig_der[..sig_der_len]);
    let seq_head_len = if body_len < 0x80 {
        2
    } else if body_len <= 0xFF {
        3
    } else {
        4
    };
    debug_assert_eq!(len, seq_head_len + body_len);
    Some((out, len))
}

fn put_integer(out: &mut [u8], at: usize, value: &[u8]) -> usize {
    let mut first = 0usize;
    while first + 1 < value.len() && value[first] == 0 {
        first += 1;
    }
    // A high bit on the leading octet needs a zero pad to stay positive.
    let pad = (value[first] & 0x80 != 0) as usize;
    let content_len = value.len() - first + pad;
    let mut tmp = [0u8; 34];
    tmp[..pad].fill(0x00);
    tmp[content_len - (value.len() - first)..content_len].copy_from_slice(&value[first..]);
    put_tlv(out, at, TAG_INTEGER, &tmp[..content_len])
}

fn put_tlv(out: &mut [u8], at: usize, tag: u8, content: &[u8]) -> usize {
    let written = put_head(out, at, tag, content.len()).expect("fixed TLV scratch");
    written + copy_into(out, written, content)
}

fn put_head(out: &mut [u8], at: usize, tag: u8, content_len: usize) -> Option<usize> {
    let (head, head_len) = der_head(tag, content_len)?;
    if at + head_len > out.len() {
        return None;
    }
    out[at..at + head_len].copy_from_slice(&head[..head_len]);
    Some(at + head_len)
}

fn der_head(tag: u8, content_len: usize) -> Option<([u8; 5], usize)> {
    let mut head = [0u8; 5];
    head[0] = tag;
    if content_len < 0x80 {
        head[1] = content_len as u8;
        Some((head, 2))
    } else if content_len <= 0xFF {
        head[1] = 0x81;
        head[2] = content_len as u8;
        Some((head, 3))
    } else {
        head[1] = 0x82;
        head[2] = (content_len >> 8) as u8;
        head[3] = content_len as u8;
        Some((head, 4))
    }
}

fn der_seq_head(content_len: u8) -> [u8; 2] {
    [TAG_SEQUENCE, content_len]
}

fn copy_into(out: &mut [u8], at: usize, src: &[u8]) -> usize {
    out[at..at + src.len()].copy_from_slice(src);
    src.len()
}
