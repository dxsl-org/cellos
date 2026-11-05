//! Kernel-measured image identity of one live Cell incarnation (syscall 441
//! `ResolveCellImage`).
//!
//! The caller-identity trailer says *which* Cell generation sent a message, not
//! *what program* it runs: a Cell's only other name is the path its spawner
//! chose. This record carries what the kernel measured at admission: the
//! SHA-256 of the exact bytes, the anchor that verified the signature (if any),
//! and the program name declared in [`IMAGE_NAME_SECTION`].
//!
//! The signature covers every ELF byte except its own 64
//! (`kernel/src/signing.rs`), so a signed image cannot change its name bytes
//! without failing verification. The kernel reports a name only for a verified
//! image, and a name is as trustworthy as its anchor: the dev seed is public,
//! so under [`ANCHOR_DEV`] anyone can sign any name.
//!
//! Invariants: recorded once at publication and immutable for the incarnation;
//! resolved only for the exact live `(cell_id, generation)`.

/// Exact byte size written by `ResolveCellImage`.
pub const CELL_IMAGE_LEN: usize = 80;
/// The one record layout defined so far.
pub const CELL_IMAGE_VERSION: u16 = 1;
/// Longest program name an image may declare.
pub const CELL_IMAGE_NAME_MAX: usize = 24;
/// ELF section holding the program name. The signing tool emits it (the
/// artifact name without its `service-`/`app-`/`driver-` crate prefix) into
/// the bytes it signs; an image without it declares no name and still loads.
pub const IMAGE_NAME_SECTION: &str = "__ViCell_image";

/// No signature verified: the digest is all the kernel can state.
pub const ANCHOR_UNSIGNED: u8 = 0;
/// Verified by the reproducible development key (its seed is public).
pub const ANCHOR_DEV: u8 = 1;
/// Verified by the provisioned release key (`release-signing-key`).
pub const ANCHOR_RELEASE: u8 = 2;
/// Verified by a private profile key (`phase02-terminal-profile`).
pub const ANCHOR_PRIVATE: u8 = 3;

/// Whether `name` is a well-formed program name: 1..=24 bytes of `a-z`,
/// `0-9`, `-` or `_`. Lowercase-only so two spellings never name one program.
pub const fn is_valid_image_name(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > CELL_IMAGE_NAME_MAX {
        return false;
    }
    let mut i = 0;
    while i < name.len() {
        let b = name[i];
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

/// What the kernel measured for one admitted image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageIdentity {
    digest: [u8; 32],
    anchor: u8,
    name_len: u8,
    name: [u8; CELL_IMAGE_NAME_MAX],
}

impl ImageIdentity {
    /// An image admitted without a verified signature: digest only.
    pub const fn unsigned(digest: [u8; 32]) -> Self {
        Self {
            digest,
            anchor: ANCHOR_UNSIGNED,
            name_len: 0,
            name: [0; CELL_IMAGE_NAME_MAX],
        }
    }

    /// A signature-verified image. `name` is empty when the image declares
    /// none. `None` for an unknown or unsigned anchor or an invalid name.
    pub fn signed(digest: [u8; 32], anchor: u8, name: &[u8]) -> Option<Self> {
        if !matches!(anchor, ANCHOR_DEV | ANCHOR_RELEASE | ANCHOR_PRIVATE)
            || (!name.is_empty() && !is_valid_image_name(name))
        {
            return None;
        }
        let mut out = Self::unsigned(digest);
        out.anchor = anchor;
        out.name_len = name.len() as u8;
        out.name[..name.len()].copy_from_slice(name);
        Some(out)
    }

    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    pub const fn anchor(&self) -> u8 {
        self.anchor
    }

    pub const fn is_signed(&self) -> bool {
        self.anchor != ANCHOR_UNSIGNED
    }

    /// Declared program name; empty for an unsigned image.
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }

    /// The binding a path rule matches: a verified signature over an image
    /// declaring exactly `name`. Unsigned or unnamed images never match.
    pub fn is_signed_program(&self, name: &str) -> bool {
        self.is_signed() && is_valid_image_name(name.as_bytes()) && self.name() == name.as_bytes()
    }
}

/// Fixed record written by `ResolveCellImage` for one live Cell generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellImage {
    pub cell_id: u64,
    pub generation: u64,
    pub identity: ImageIdentity,
}

impl CellImage {
    /// Layout: version u16 | anchor u8 | name_len u8 | reserved u32 (zero) |
    /// cell_id u64 | generation u64 | digest [32] | name [24], zero past
    /// `name_len`. All integers little-endian.
    pub fn to_bytes(&self) -> [u8; CELL_IMAGE_LEN] {
        let id = &self.identity;
        let mut out = [0u8; CELL_IMAGE_LEN];
        out[0..2].copy_from_slice(&CELL_IMAGE_VERSION.to_le_bytes());
        out[2] = id.anchor;
        out[3] = id.name_len;
        out[8..16].copy_from_slice(&self.cell_id.to_le_bytes());
        out[16..24].copy_from_slice(&self.generation.to_le_bytes());
        out[24..56].copy_from_slice(&id.digest);
        out[56..80].copy_from_slice(&id.name);
        out
    }

    /// Strict decode: exact length, known version and anchor, zero reserved
    /// and padding, a name only under a signing anchor, non-zero identity.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CELL_IMAGE_LEN
            || u16::from_le_bytes([bytes[0], bytes[1]]) != CELL_IMAGE_VERSION
            || bytes[4..8] != [0; 4]
        {
            return None;
        }
        let name_len = usize::from(bytes[3]);
        let name_field = &bytes[56..80];
        if name_len > CELL_IMAGE_NAME_MAX || name_field[name_len..].iter().any(|b| *b != 0) {
            return None;
        }
        let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or([0; 8]));
        let (cell_id, generation) = (word(8), word(16));
        if cell_id == 0 || generation == 0 {
            return None;
        }
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&bytes[24..56]);
        let identity = match bytes[2] {
            ANCHOR_UNSIGNED if name_len == 0 => ImageIdentity::unsigned(digest),
            ANCHOR_UNSIGNED => return None,
            anchor => ImageIdentity::signed(digest, anchor, &name_field[..name_len])?,
        };
        Some(Self {
            cell_id,
            generation,
            identity,
        })
    }
}

#[cfg(test)]
#[path = "cell-image-tests.rs"]
mod tests;
