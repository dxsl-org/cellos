//! Signed operator policy (roadmap §G.2 P5b) — the headless "consent" mechanism.
//!
//! At boot the kernel reads `/POLICY.BIN` from the kernel-embedded VIFS1, verifies
//! its Ed25519 signature against the fleet root public key, and parses it into a
//! `path → CapSet` table. Phase 04 folds `lookup()` into the spawn-time grant so
//! the effective caps are `manifest ∩ spawner ∩ policy`.
//!
//! Security invariants (red-team-driven):
//! - **Verify-then-parse:** the signature covers `blob[..len-64]`; verify FIRST
//!   (length-only, no field parsing) so the parser never runs on unverified bytes.
//! - **Panic-free parser:** every field read is bounds-checked; malformed →
//!   `Invalid`, never a panic (a boot-path panic = no boot = bricked robot).
//! - **Fail-safe:** an *invalid* signature/parse is ALWAYS fail-closed. An *absent*
//!   policy is dev-permissive in G1 (this build) and fail-closed only when the
//!   `policy-required` feature is set (real-fleet posture). See `lookup`.
//! - **Domain validation:** parsed cap bytes are masked to known bits; unknown
//!   bits → `Invalid` (a signed-but-malformed policy is still rejected).

use crate::resource_registry::{DEV_GPIO, DEV_UART};
use crate::sync::Spinlock;
use crate::task::cap::CapSet;
use alloc::string::String;
use alloc::vec::Vec;

/// Magic "VPOL" as a little-endian u32 (bytes V,P,O,L).
const MAGIC: u32 = u32::from_le_bytes([b'V', b'P', b'O', b'L']);
const VERSION: u8 = 1;
const SIG_LEN: usize = 64;
const HEADER_LEN: usize = 8; // magic(4) + version(1) + flags(1) + entry_count(2)
const CAP_BYTES: usize = 6; // block_io, network, spawn, hyp, mmio_devices, block_regions
/// 8.3-safe, root-level path (VIFS1 uppercases + is FAT16 8.3).
const POLICY_PATH: &str = "/POLICY.BIN";

/// Valid `mmio_devices` bits and `block_regions` bits (domain-validation masks).
const MMIO_MASK: u8 = DEV_GPIO | DEV_UART;
const REGION_MASK: u8 = 0b111;

/// Fleet root Ed25519 **public** key (trust anchor; lives in the kernel TCB, not
/// in mutable VIFS1 data). Replaced by the real provisioned key for production.
///
/// NOTE: still a placeholder (all-zero) until the host signer (`scripts/sign-policy`)
/// is wired and emits the dev key. A zero key fails every verify → any present
/// policy is treated as `Invalid` (fail-closed) until the real key lands, which is
/// the safe direction. Absent policy still works (dev-permissive) for boot.
#[cfg(feature = "dev-policy-key")]
const FLEET_ROOT_PUBKEY: [u8; 32] = [0u8; 32]; // TODO(P5b-signer): dev pubkey
#[cfg(not(feature = "dev-policy-key"))]
const FLEET_ROOT_PUBKEY: [u8; 32] = [0u8; 32]; // TODO(prod): provisioned fleet key

/// Result of a policy lookup for a given cell path.
pub enum PolicyDecision {
    /// Policy explicitly grants this path the given caps (ceiling).
    Permit(CapSet),
    /// Policy is present and explicitly denies (or invalid → fail-closed).
    DenyAll,
    /// No policy entry for this path (or policy absent). Caller applies the
    /// fail-safe rule: dev-permissive keeps the spawner-intersected caps;
    /// `policy-required` treats it as deny.
    NoEntry,
}

struct PolicyEntry {
    path: String,
    caps: CapSet,
}

enum PolicyState {
    Loaded(Vec<PolicyEntry>),
    Absent,
    Invalid,
}

static POLICY: Spinlock<Option<PolicyState>> = Spinlock::new(None);

/// Force-release this module's lock during fault teardown.
///
/// # Safety
/// Single-hart; called only from the fault/panic path with interrupts disabled.
pub unsafe fn force_unlock_locks() {
    POLICY.force_unlock();
}

/// Load + verify the operator policy from VIFS1. Call once at boot AFTER
/// `fs::init()` and BEFORE the first cap-bearing cell spawns. Eager-only (no
/// lazy path — VIFS1 is kernel-embedded and available this early).
pub fn load_from_vifs1() {
    let blob = match crate::fs::read_file_from_vifs1(POLICY_PATH) {
        Ok(b) if !b.is_empty() => b,
        _ => {
            log::info!("[policy] no {} in VIFS1 — absent", POLICY_PATH);
            crate::audit::log_event(crate::audit::AuditEvent::PolicyAbsent, &crate::audit::encode_u32x2(0, 0));
            *POLICY.lock() = Some(PolicyState::Absent);
            return;
        }
    };

    // Verify-then-parse: the trailing SIG_LEN bytes are the signature over the body.
    if blob.len() < HEADER_LEN + SIG_LEN {
        return mark_invalid(1);
    }
    let split = blob.len() - SIG_LEN;
    let (body, sig) = blob.split_at(split);
    let mut sig64 = [0u8; SIG_LEN];
    sig64.copy_from_slice(sig);
    if !crate::ed25519::verify(&FLEET_ROOT_PUBKEY, body, &sig64) {
        log::warn!("[policy] signature verification FAILED — fail-closed");
        return mark_invalid(2);
    }

    match parse(body) {
        Some(entries) => {
            let n = entries.len() as u32;
            log::info!("[policy] loaded + verified ({} entries)", n);
            crate::audit::log_event(crate::audit::AuditEvent::PolicyLoaded, &crate::audit::encode_u32x2(n, 0));
            *POLICY.lock() = Some(PolicyState::Loaded(entries));
        }
        None => {
            log::warn!("[policy] malformed body — fail-closed");
            mark_invalid(3);
        }
    }
}

fn mark_invalid(reason: u32) {
    crate::audit::log_event(crate::audit::AuditEvent::PolicyInvalid, &crate::audit::encode_u32x2(reason, 0));
    *POLICY.lock() = Some(PolicyState::Invalid);
}

/// Parse the (already signature-verified) body into entries. Panic-free: every
/// read is bounds-checked; any malformation or out-of-domain cap bit → `None`.
fn parse(body: &[u8]) -> Option<Vec<PolicyEntry>> {
    if body.len() < HEADER_LEN {
        return None;
    }
    let magic = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    if magic != MAGIC || body[4] != VERSION {
        return None;
    }
    let count = u16::from_le_bytes([body[6], body[7]]) as usize;

    let mut entries = Vec::new();
    let mut off = HEADER_LEN;
    for _ in 0..count {
        // path_len
        let path_len = *body.get(off)? as usize;
        off += 1;
        // path bytes
        let path_bytes = body.get(off..off.checked_add(path_len)?)?;
        let path = core::str::from_utf8(path_bytes).ok()?;
        off += path_len;
        // 6 cap bytes
        let caps_raw = body.get(off..off.checked_add(CAP_BYTES)?)?;
        off += CAP_BYTES;

        let mmio_devices = caps_raw[4];
        let block_regions = caps_raw[5];
        // Domain validation: reject unknown bits (signed-but-malformed).
        if mmio_devices & !MMIO_MASK != 0 || block_regions & !REGION_MASK != 0 {
            return None;
        }
        entries.push(PolicyEntry {
            path: String::from(path),
            caps: CapSet {
                block_io: caps_raw[0] != 0,
                network: caps_raw[1] != 0,
                spawn: caps_raw[2] != 0,
                hypervisor: caps_raw[3] != 0,
                mmio_devices,
                block_regions,
            },
        });
    }
    Some(entries)
}

/// Policy decision for a cell path. See `PolicyDecision`; the caller (Phase 04)
/// applies the dev-permissive vs `policy-required` fail-safe rule to `NoEntry`.
pub fn lookup(path: &str) -> PolicyDecision {
    let guard = POLICY.lock();
    match guard.as_ref() {
        Some(PolicyState::Loaded(entries)) => {
            for e in entries {
                if e.path == path {
                    return PolicyDecision::Permit(e.caps);
                }
            }
            PolicyDecision::NoEntry
        }
        // Invalid → fail-closed ALWAYS, regardless of posture.
        Some(PolicyState::Invalid) => PolicyDecision::DenyAll,
        // Absent / not-yet-loaded → NoEntry; the caller's fail-safe rule decides
        // (dev-permissive keeps caps; `policy-required` denies).
        Some(PolicyState::Absent) | None => PolicyDecision::NoEntry,
    }
}
