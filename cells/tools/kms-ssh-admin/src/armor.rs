//! Strict decode of one armored `OPENSSH PRIVATE KEY` block (k7-cp1-draft §3.3).
//!
//! The cell never parses key contents: the `openssh-key-v1\0` magic and every structural
//! check stay in KMS (D-K4). This module only turns pasted text into the binary blob KMS
//! is given, and refuses everything that is not exactly one armored block, before any KMS
//! frame exists.

use base64ct::{Base64, Encoding};

use crate::secret::Secret;

pub const BEGIN: &[u8] = b"-----BEGIN OPENSSH PRIVATE KEY-----";
pub const END: &[u8] = b"-----END OPENSSH PRIVATE KEY-----";
/// Raw text cap: a 65,536-byte blob is 87,384 base64 bytes plus line breaks and the two
/// marker lines (O-K7-4).
pub const TEXT_MAX: usize = 98_304;
/// The ABI cap on one blob (`KMS_SSH_IMPORT_MAX_LEN`).
pub const BLOB_MAX: usize = ostd::clients::kms::KMS_SSH_IMPORT_MAX_LEN;
/// Base64 text of a `BLOB_MAX`-byte blob, canonical padding included.
const BODY_MAX: usize = BLOB_MAX.div_ceil(3) * 4;

/// Why a pasted text is not one armored key block. One fixed message per variant; no
/// message ever carries input bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmorError {
    /// More than [`TEXT_MAX`] bytes.
    TextTooLong,
    /// No bytes at all (or only line breaks).
    Empty,
    /// A bare carriage return not followed by a line feed.
    BareCarriageReturn,
    /// The first line is not the exact BEGIN marker.
    MissingBegin,
    /// No exact END marker line closes the block.
    MissingEnd,
    /// A second block, a second BEGIN or an END marker inside the body.
    SecondBlock,
    /// Bytes other than line breaks after the END line.
    TextAfterBlock,
    /// An empty line, a space, a header line or any byte outside the base64 alphabet
    /// between the markers.
    BadBodyLine,
    /// Nothing between the markers.
    EmptyBody,
    /// Not canonical standard base64 (length, padding or trailing bits).
    BadBase64,
    /// The decoded blob is empty or longer than [`BLOB_MAX`].
    BadLength,
}

impl ArmorError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::TextTooLong => "key text is longer than 98304 bytes",
            Self::Empty => "no key text",
            Self::BareCarriageReturn => "line breaks must be LF or CRLF",
            Self::MissingBegin => "the key text must start with the BEGIN OPENSSH PRIVATE KEY line",
            Self::MissingEnd => "the key text must end with the END OPENSSH PRIVATE KEY line",
            Self::SecondBlock => "exactly one key block is accepted",
            Self::TextAfterBlock => "nothing may follow the END line",
            Self::BadBodyLine => "the key body must be non-empty lines of base64 only",
            Self::EmptyBody => "the key body is empty",
            Self::BadBase64 => "the key body is not canonical base64",
            Self::BadLength => "the decoded key must be 1 to 65536 bytes",
        }
    }
}

/// Split `text` into lines (LF or CRLF terminated; a final unterminated line counts) and
/// call `each` with every line without its terminator. A lone CR is an error.
fn for_each_line<'a>(
    text: &'a [u8],
    mut each: impl FnMut(&'a [u8]) -> Result<(), ArmorError>,
) -> Result<(), ArmorError> {
    let mut start = 0;
    while start < text.len() {
        let rest = &text[start..];
        let (line, advance) = match rest.iter().position(|&b| b == b'\n') {
            Some(at) => {
                let line = &rest[..at];
                (line.strip_suffix(b"\r").unwrap_or(line), at + 1)
            }
            None => (rest, rest.len()),
        };
        if line.contains(&b'\r') {
            return Err(ArmorError::BareCarriageReturn);
        }
        each(line)?;
        start += advance;
    }
    Ok(())
}

/// The standard-alphabet byte set (padding is checked by the decoder).
const fn base64_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='
}

/// Decode the single armored block in `text` to the binary OpenSSH blob.
///
/// Accepted: optional empty lines, the exact BEGIN line, one or more non-empty body lines
/// of the standard base64 alphabet (any width), the exact END line, optional empty lines.
/// Line breaks are LF or CRLF. Padding must be canonical and only at the end. Everything
/// else is refused.
pub fn decode(text: &[u8]) -> Result<Secret, ArmorError> {
    if text.len() > TEXT_MAX {
        return Err(ArmorError::TextTooLong);
    }
    enum At {
        BeforeBegin,
        Body,
        AfterEnd,
    }
    let mut at = At::BeforeBegin;
    let mut saw_text = false;
    let mut saw_marker = false;
    // The compacted base64 text; it cannot exceed what a 65,536-byte blob needs.
    let mut body = Secret::with_capacity(BODY_MAX);
    let mut body_lines = 0usize;
    for_each_line(text, |line| {
        saw_text |= !line.is_empty();
        match at {
            At::BeforeBegin => {
                if line.is_empty() {
                    return Ok(());
                }
                if line != BEGIN {
                    return Err(ArmorError::MissingBegin);
                }
                saw_marker = true;
                at = At::Body;
            }
            At::Body => {
                if line == END {
                    at = At::AfterEnd;
                } else if line == BEGIN {
                    return Err(ArmorError::SecondBlock);
                } else if line.is_empty() || !line.iter().copied().all(base64_byte) {
                    return Err(ArmorError::BadBodyLine);
                } else if !body.push_slice(line) {
                    // More base64 than a maximal blob needs.
                    return Err(ArmorError::BadLength);
                } else {
                    body_lines += 1;
                }
            }
            At::AfterEnd => {
                if line == BEGIN {
                    return Err(ArmorError::SecondBlock);
                }
                if !line.is_empty() {
                    return Err(ArmorError::TextAfterBlock);
                }
            }
        }
        Ok(())
    })?;
    match at {
        At::AfterEnd => {}
        At::Body => return Err(ArmorError::MissingEnd),
        At::BeforeBegin if saw_text || saw_marker => return Err(ArmorError::MissingBegin),
        At::BeforeBegin => return Err(ArmorError::Empty),
    }
    if body_lines == 0 {
        return Err(ArmorError::EmptyBody);
    }
    let encoded = body.as_slice();
    // Canonical standard base64: the length is a multiple of four and `=` pads only the
    // last quantum. `base64ct` refuses a bad length, misplaced or excess padding, bytes
    // outside the alphabet and non-zero trailing bits. A 65,536 and a 65,537 byte blob
    // share one base64 length, so the destination is sized by the upper bound and the
    // real length is checked after the decode.
    if encoded.len() % 4 != 0 {
        return Err(ArmorError::BadBase64);
    }
    let mut blob = Secret::zeroed(encoded.len() / 4 * 3);
    let written = Base64::decode(encoded, blob.as_mut_slice())
        .map_err(|_| ArmorError::BadBase64)?
        .len();
    blob.truncate(written);
    if blob.is_empty() || blob.len() > BLOB_MAX {
        return Err(ArmorError::BadLength);
    }
    Ok(blob)
}

/// Armor `blob` the way `ssh-keygen` writes it (70 columns, LF). Used by the tests to
/// build synthetic fixtures; never by the cell.
#[cfg(test)]
pub fn encode(blob: &[u8], width: usize, newline: &str) -> std::string::String {
    let mut out = std::string::String::new();
    out.push_str(core::str::from_utf8(BEGIN).unwrap());
    out.push_str(newline);
    let mut text = std::vec![0u8; blob.len().div_ceil(3) * 4];
    let encoded = Base64::encode(blob, &mut text).unwrap();
    for chunk in encoded.as_bytes().chunks(width) {
        out.push_str(core::str::from_utf8(chunk).unwrap());
        out.push_str(newline);
    }
    out.push_str(core::str::from_utf8(END).unwrap());
    out.push_str(newline);
    out
}
