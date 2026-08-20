//! Boot self-test for Manifest v2 (the 8→16 byte ABI bump).
//!
//! Exercises the exact functions the loader runs on every spawn:
//! `CellManifest::from_bytes` (v1-upcast and native-v2 parsing) and
//! `cap::granted_protection_class` (the protection-class floor invariant that drives x86 PKU key
//! selection). Pure logic, no scheduler — runs on all three arches.

use super::cap::{granted_protection_class, CapSet};
use api::manifest::{
    CellManifest, MANIFEST_FLAG_BLOCK_IO, MANIFEST_MAGIC, MANIFEST_VERSION, MANIFEST_VERSION_V1,
    PROTECTION_CLASS_FFI, PROTECTION_CLASS_LEGACY, PROTECTION_CLASS_STANDARD,
    PROTECTION_CLASS_TRUSTED_CORE, PROTECTION_CLASS_UNTRUSTED,
};

fn v1_bytes(flags: u8) -> [u8; 8] {
    let m = MANIFEST_MAGIC.to_le_bytes();
    [m[0], m[1], m[2], m[3], MANIFEST_VERSION_V1, flags, 0, 0]
}

fn v2_bytes(tier: u8, flags: u16) -> [u8; 16] {
    let m = MANIFEST_MAGIC.to_le_bytes();
    let f = flags.to_le_bytes();
    [
        m[0],
        m[1],
        m[2],
        m[3],
        MANIFEST_VERSION,
        tier,
        f[0],
        f[1],
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
}

/// Returns true iff manifest v1-upcast/v2-parse and the protection-class floor invariant
/// behave as specified.
pub fn self_test() -> bool {
    let mut ok = true;

    // ── v1 upcast: flags preserved, accessor sees the LEGACY sentinel ─────────
    {
        let bytes = v1_bytes(MANIFEST_FLAG_BLOCK_IO as u8);
        match CellManifest::from_bytes(&bytes) {
            Some(m)
                if m.has_block_io()
                    && m.protection_class() == PROTECTION_CLASS_LEGACY
                    && m.protection_class() == m.tier() => {}
            other => {
                ok = false;
                log::error!(
                    "[selftest] MANIFEST-V2: FAIL — v1 upcast: {:?}",
                    other.map(|m| m.protection_class())
                );
            }
        }
    }

    // ── native v2: protection class round-trips exactly ───────────────────────
    {
        let bytes = v2_bytes(PROTECTION_CLASS_FFI, MANIFEST_FLAG_BLOCK_IO);
        match CellManifest::from_bytes(&bytes) {
            Some(m)
                if m.has_block_io()
                    && m.protection_class() == PROTECTION_CLASS_FFI
                    && m.protection_class() == m.tier() => {}
            other => {
                ok = false;
                log::error!(
                    "[selftest] MANIFEST-V2: FAIL — v2 native parse: {:?}",
                    other.map(|m| m.protection_class())
                );
            }
        }
    }

    // ── malformed v2 rejected: bad tier, non-zero reserved ─────────────────────
    {
        let bad_tier = v2_bytes(4, 0); // one past PROTECTION_CLASS_UNTRUSTED, not the LEGACY sentinel
        if CellManifest::from_bytes(&bad_tier).is_some() {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — out-of-range protection class accepted");
        }
        let mut bad_reserved = v2_bytes(PROTECTION_CLASS_STANDARD, 0);
        bad_reserved[12] = 1; // reserved field non-zero
        if CellManifest::from_bytes(&bad_reserved).is_some() {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — non-zero reserved field accepted");
        }
    }

    // ── floor invariant: a cell cannot claim a lower protection class than its
    //    caps justify; it CAN self-restrict to a higher one; LEGACY = floor ────
    {
        let untrusted_caps = CapSet::EMPTY; // no block_io/network/spawn/hypervisor
        let trusted_caps = CapSet {
            block_io: true,
            ..CapSet::EMPTY
        };

        // Untrusted caps requesting class 0 (trusted-core) → floored to STANDARD.
        if granted_protection_class(&untrusted_caps, PROTECTION_CLASS_TRUSTED_CORE)
            != PROTECTION_CLASS_STANDARD
        {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — untrusted cell claimed protection class 0 (privilege escalation)");
        }
        // Trusted caps requesting class 0 → granted (floor permits it).
        if granted_protection_class(&trusted_caps, PROTECTION_CLASS_TRUSTED_CORE)
            != PROTECTION_CLASS_TRUSTED_CORE
        {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — trusted cell denied its justified protection class 0");
        }
        // Trusted caps self-restricting to UNTRUSTED (3) → always allowed.
        if granted_protection_class(&trusted_caps, PROTECTION_CLASS_UNTRUSTED)
            != PROTECTION_CLASS_UNTRUSTED
        {
            ok = false;
            log::error!(
                "[selftest] MANIFEST-V2: FAIL — self-restriction to a higher protection class was denied"
            );
        }
        // LEGACY (no explicit request) → exactly the floor, both directions.
        if granted_protection_class(&untrusted_caps, PROTECTION_CLASS_LEGACY)
            != PROTECTION_CLASS_STANDARD
            || granted_protection_class(&trusted_caps, PROTECTION_CLASS_LEGACY)
                != PROTECTION_CLASS_TRUSTED_CORE
        {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — PROTECTION_CLASS_LEGACY did not resolve to the floor");
        }
    }

    if ok {
        log::info!(
            "[selftest] MANIFEST-V2: PASS (v1 upcast + v2 parse + protection-class floor invariant)"
        );
    } else {
        log::error!("[selftest] MANIFEST-V2: FAIL");
    }
    ok
}
