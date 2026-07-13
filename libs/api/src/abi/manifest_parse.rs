//! `CellManifest::from_bytes` — the security-critical ELF-section parser.
//!
//! Split out of `manifest.rs` to keep that file under the 200-LOC law; this is
//! an `impl CellManifest` block in a sibling file (Rust allows an `impl` to span
//! multiple files within a crate), not a separate type.

use super::manifest::CellManifest;
use super::manifest_flags::{
    MANIFEST_FLAGS_MASK, MANIFEST_MAGIC, MANIFEST_VERSION, MANIFEST_VERSION_V1,
    TIER_LEGACY, TIER_UNTRUSTED,
};

impl CellManifest {
    /// Parse a manifest from raw ELF section bytes.  Field-by-field — never casts
    /// the slice to `&Self` (alignment hazard in `no_std`).  Accepts both the v2
    /// 16-byte record and the legacy v1 8-byte record (upcast).
    ///
    /// # Returns
    /// `None` if the bytes are too short, the magic mismatches, the version is
    /// unsupported, a reserved field is non-zero, the tier is out of range, or a
    /// flag bit outside `MANIFEST_FLAGS_MASK` is set.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != MANIFEST_MAGIC {
            return None;
        }
        match bytes[4] {
            // ── v1 upcast: 8-byte record, u8 flags, no tier ──────────────────
            MANIFEST_VERSION_V1 => {
                let flags = bytes[5] as u16;
                if flags & !MANIFEST_FLAGS_MASK != 0 {
                    return None;
                }
                // _pad (bytes[6..8]) was "must be [0,0]" in v1; tolerate it either
                // way (v1 kernels never validated it) but do not import it.
                Some(Self {
                    magic,
                    version: MANIFEST_VERSION,
                    tier: TIER_LEGACY, // → loader keeps the v1 is_trusted heuristic
                    flags,
                    cap_args_off: 0,
                    reserved: 0,
                })
            }
            // ── native v2: 16-byte record ────────────────────────────────────
            MANIFEST_VERSION => {
                if bytes.len() < 16 {
                    return None;
                }
                let tier = bytes[5];
                // A real domain (0..=TIER_UNTRUSTED), OR the TIER_LEGACY sentinel —
                // which is valid here too: it is what `CellManifest::new`/`with_parts`
                // (the tier-less constructors `declare_manifest!` uses by default)
                // bake into a NATIVE v2 record, meaning "no explicit tier requested,
                // apply the caller's floor policy." Anything else is malformed.
                if tier > TIER_UNTRUSTED && tier != TIER_LEGACY {
                    return None;
                }
                let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
                if flags & !MANIFEST_FLAGS_MASK != 0 {
                    return None;
                }
                let cap_args_off = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
                let reserved = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
                // Reserved fields MUST be zero in v2 (forward-compat: a future field
                // must not be silently ignored by a kernel that predates it).
                if cap_args_off != 0 || reserved != 0 {
                    return None;
                }
                Some(Self { magic, version: MANIFEST_VERSION, tier, flags, cap_args_off, reserved })
            }
            _ => None,
        }
    }
}
