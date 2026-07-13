//! Boot self-test for Manifest v2 (the 8→16 byte ABI bump).
//!
//! Exercises the exact functions the loader runs on every spawn:
//! `CellManifest::from_bytes` (v1-upcast and native-v2 parsing) and
//! `cap::granted_tier` (the tier-floor invariant that drives x86 PKU key
//! selection). Pure logic, no scheduler — runs on all three arches.

use super::cap::{granted_tier, CapSet};
use api::manifest::{
    CellManifest, MANIFEST_FLAG_BLOCK_IO, MANIFEST_MAGIC, MANIFEST_VERSION,
    MANIFEST_VERSION_V1, TIER_LEGACY, TIER_STANDARD, TIER_TIER1B_FFI, TIER_TRUSTED_CORE,
    TIER_UNTRUSTED,
};

fn v1_bytes(flags: u8) -> [u8; 8] {
    let m = MANIFEST_MAGIC.to_le_bytes();
    [m[0], m[1], m[2], m[3], MANIFEST_VERSION_V1, flags, 0, 0]
}

fn v2_bytes(tier: u8, flags: u16) -> [u8; 16] {
    let m = MANIFEST_MAGIC.to_le_bytes();
    let f = flags.to_le_bytes();
    [m[0], m[1], m[2], m[3], MANIFEST_VERSION, tier, f[0], f[1], 0, 0, 0, 0, 0, 0, 0, 0]
}

/// Returns true iff manifest v1-upcast/v2-parse and the tier-floor invariant
/// behave as specified.
pub fn self_test() -> bool {
    let mut ok = true;

    // ── v1 upcast: flags preserved, tier becomes the LEGACY sentinel ──────────
    {
        let bytes = v1_bytes(MANIFEST_FLAG_BLOCK_IO as u8);
        match CellManifest::from_bytes(&bytes) {
            Some(m) if m.has_block_io() && m.tier() == TIER_LEGACY => {}
            other => {
                ok = false;
                log::error!("[selftest] MANIFEST-V2: FAIL — v1 upcast: {:?}", other.map(|m| m.tier()));
            }
        }
    }

    // ── native v2: tier round-trips exactly ────────────────────────────────────
    {
        let bytes = v2_bytes(TIER_TIER1B_FFI, MANIFEST_FLAG_BLOCK_IO);
        match CellManifest::from_bytes(&bytes) {
            Some(m) if m.has_block_io() && m.tier() == TIER_TIER1B_FFI => {}
            other => {
                ok = false;
                log::error!("[selftest] MANIFEST-V2: FAIL — v2 native parse: {:?}", other.map(|m| m.tier()));
            }
        }
    }

    // ── malformed v2 rejected: bad tier, non-zero reserved ─────────────────────
    {
        let bad_tier = v2_bytes(4, 0); // one past TIER_UNTRUSTED, not the LEGACY sentinel
        if CellManifest::from_bytes(&bad_tier).is_some() {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — out-of-range tier accepted");
        }
        let mut bad_reserved = v2_bytes(TIER_STANDARD, 0);
        bad_reserved[12] = 1; // reserved field non-zero
        if CellManifest::from_bytes(&bad_reserved).is_some() {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — non-zero reserved field accepted");
        }
    }

    // ── tier-floor invariant: a cell cannot claim a lower tier than its caps
    //    justify; it CAN self-restrict to a higher one; LEGACY = floor as-is ────
    {
        let untrusted_caps = CapSet::EMPTY; // no block_io/network/spawn/hypervisor
        let trusted_caps = CapSet { block_io: true, ..CapSet::EMPTY };

        // Untrusted caps requesting tier 0 (trusted-core) → floored to STANDARD.
        if granted_tier(&untrusted_caps, TIER_TRUSTED_CORE) != TIER_STANDARD {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — untrusted cell claimed tier 0 (privilege escalation)");
        }
        // Trusted caps requesting tier 0 → granted (floor permits it).
        if granted_tier(&trusted_caps, TIER_TRUSTED_CORE) != TIER_TRUSTED_CORE {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — trusted cell denied its justified tier 0");
        }
        // Trusted caps self-restricting to UNTRUSTED (3) → always allowed (raise is free).
        if granted_tier(&trusted_caps, TIER_UNTRUSTED) != TIER_UNTRUSTED {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — self-restriction to a higher tier was denied");
        }
        // LEGACY (no explicit request) → exactly the floor, both directions.
        if granted_tier(&untrusted_caps, TIER_LEGACY) != TIER_STANDARD
            || granted_tier(&trusted_caps, TIER_LEGACY) != TIER_TRUSTED_CORE {
            ok = false;
            log::error!("[selftest] MANIFEST-V2: FAIL — TIER_LEGACY did not resolve to the floor");
        }
    }

    if ok {
        log::info!("[selftest] MANIFEST-V2: PASS (v1 upcast + v2 parse + tier-floor invariant)");
    } else {
        log::error!("[selftest] MANIFEST-V2: FAIL");
    }
    ok
}
