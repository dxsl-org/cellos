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

use crate::resource_registry::{DEV_GPIO, DEV_PCIE, DEV_UART};
use crate::sync::Spinlock;
use crate::task::cap::CapSet;
use alloc::string::String;
use alloc::vec::Vec;

/// Magic "VPOL" as a little-endian u32 (bytes V,P,O,L).
const MAGIC: u32 = u32::from_le_bytes(*b"VPOL");
/// v1: 6 cap bytes. Still parsed — a fleet mid-rollout may hold either.
const VERSION_V1: u8 = 1;
/// v2: 9 cap bytes; adds the three privileged path-caps so a policy can express
/// them at all. Under v1 they parse as `false`, and since `Permit` *intersects*,
/// that means a v1 entry silently STRIPS them — see `parse`.
const VERSION_V2: u8 = 2;
const SIG_LEN: usize = 64;
const HEADER_LEN: usize = 8; // magic(4) + version(1) + flags(1) + entry_count(2)
const CAP_BYTES_V1: usize = 6; // block_io, network, spawn, hyp, mmio_devices, block_regions
const CAP_BYTES_V2: usize = 9; // + pcie_driver, platform, supervisor
/// 8.3-safe, root-level path (VIFS1 uppercases + is FAT16 8.3).
const POLICY_PATH: &str = "/POLICY.BIN";

/// Signed header flags. Unknown bits → `Invalid` (a signed-but-malformed policy
/// is still rejected).
const FLAG_MAINTENANCE_PERMITTED: u8 = 1 << 0;
const FLAGS_MASK: u8 = FLAG_MAINTENANCE_PERMITTED;

/// Cap-byte count for a blob version, or `None` for an unknown version.
const fn cap_bytes_for(version: u8) -> Option<usize> {
    match version {
        VERSION_V1 => Some(CAP_BYTES_V1),
        VERSION_V2 => Some(CAP_BYTES_V2),
        _ => None,
    }
}

/// Valid `mmio_devices` bits and `block_regions` bits (domain-validation masks).
const MMIO_MASK: u8 = DEV_GPIO | DEV_UART | DEV_PCIE;
const REGION_MASK: u8 = 0b111;

/// Dev fleet Ed25519 **public** key — derived from the fixed dev seed in
/// `scripts/sign-policy.py` (reproducible; a dev key, never shipped in release).
const DEV_FLEET_PUBKEY: [u8; 32] = [
    0x21, 0x52, 0xf8, 0xd1, 0x9b, 0x79, 0x1d, 0x24, 0x45, 0x32, 0x42, 0xe1, 0x5f, 0x2e, 0xab, 0x6c,
    0xb7, 0xcf, 0xfa, 0x7b, 0x6a, 0x5e, 0xd3, 0x00, 0x97, 0x96, 0x0e, 0x06, 0x98, 0x81, 0xdb, 0x12,
];

/// Fleet root Ed25519 **public** key (trust anchor; lives in the kernel TCB, not
/// in mutable VIFS1 data). `dev-policy-key` feature → the dev key (so a dev-signed
/// `/POLICY.BIN` verifies); otherwise a placeholder the production provisioning
/// replaces. A zero/placeholder key fails every verify → any present policy is
/// `Invalid` (fail-closed), the safe direction; absent policy still boots
/// (dev-permissive).
#[cfg(feature = "dev-policy-key")]
const FLEET_ROOT_PUBKEY: [u8; 32] = DEV_FLEET_PUBKEY;
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

struct LoadedPolicy {
    /// Signed header flags — see `FLAGS_MASK`. Read only after signature verify,
    /// which is what makes `FLAG_MAINTENANCE_PERMITTED` a second factor rather
    /// than a build-time switch.
    flags: u8,
    entries: Vec<PolicyEntry>,
}

enum PolicyState {
    Loaded(LoadedPolicy),
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
            crate::audit::log_event(
                crate::audit::AuditEvent::PolicyAbsent,
                &crate::audit::encode_u32x2(0, 0),
            );
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
        Some(loaded) => {
            let n = loaded.entries.len() as u32;
            log::info!(
                "[policy] loaded + verified ({} entries, flags {:#04x})",
                n,
                loaded.flags
            );
            crate::audit::log_event(
                crate::audit::AuditEvent::PolicyLoaded,
                &crate::audit::encode_u32x2(n, loaded.flags as u32),
            );
            *POLICY.lock() = Some(PolicyState::Loaded(loaded));
        }
        None => {
            log::warn!("[policy] malformed body — fail-closed");
            mark_invalid(3);
        }
    }
}

fn mark_invalid(reason: u32) {
    crate::audit::log_event(
        crate::audit::AuditEvent::PolicyInvalid,
        &crate::audit::encode_u32x2(reason, 0),
    );
    *POLICY.lock() = Some(PolicyState::Invalid);
}

/// Parse the (already signature-verified) body into entries. Panic-free: every
/// read is bounds-checked; any malformation or out-of-domain cap bit → `None`.
fn parse(body: &[u8]) -> Option<LoadedPolicy> {
    if body.len() < HEADER_LEN {
        return None;
    }
    let magic = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    if magic != MAGIC {
        return None;
    }
    // Version selects the per-entry stride; an unknown version is a parse failure,
    // never a guess — misreading the stride would shift every subsequent field.
    let cap_bytes = cap_bytes_for(body[4])?;
    let flags = body[5];
    if flags & !FLAGS_MASK != 0 {
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
        // cap bytes (6 in v1, 9 in v2)
        let caps_raw = body.get(off..off.checked_add(cap_bytes)?)?;
        off += cap_bytes;

        let mmio_devices = caps_raw[4];
        let block_regions = caps_raw[5];
        // Domain validation: reject unknown bits (signed-but-malformed).
        if mmio_devices & !MMIO_MASK != 0 || block_regions & !REGION_MASK != 0 {
            return None;
        }
        // The three privileged path-caps exist only in v2. In v1 they parse as
        // `false`, which — because `Permit` intersects — means a v1 entry STRIPS
        // them from any cell it names. That is the v1 behaviour, preserved
        // deliberately: changing it would widen authority on an old blob.
        //
        // Stricter domain than the four bools above: these are the caps that can
        // DMA anywhere, so anything other than a literal 0/1 is a malformed blob
        // rather than something to coerce with `!= 0`. The older bools keep
        // `!= 0` because tightening them could reject a v1 blob that boots today,
        // and a rejected blob is `DenyAll` — a brick, not a safe default.
        let (pcie_driver, platform, supervisor) = if cap_bytes == CAP_BYTES_V2 {
            let (p, pl, s) = (caps_raw[6], caps_raw[7], caps_raw[8]);
            if p > 1 || pl > 1 || s > 1 {
                return None;
            }
            (p == 1, pl == 1, s == 1)
        } else {
            (false, false, false)
        };
        entries.push(PolicyEntry {
            path: String::from(path),
            caps: CapSet {
                block_io: caps_raw[0] != 0,
                network: caps_raw[1] != 0,
                spawn: caps_raw[2] != 0,
                hypervisor: caps_raw[3] != 0,
                mmio_devices,
                block_regions,
                pcie_driver,
                platform,
                supervisor,
            },
        });
    }
    Some(LoadedPolicy { flags, entries })
}

/// Self-test of the full signed-policy path: verify + parse a known dev-signed
/// blob (from `scripts/sign-policy.py`), confirm a known entry parses correctly,
/// and confirm a tampered blob is REJECTED. Returns `true` iff both hold. Run as
/// a boot power-on self-test before trusting the policy path.
pub fn self_test() -> bool {
    // 135-byte dev-signed blob (4 entries) emitted by scripts/sign-policy.py.
    const BLOB: [u8; 135] = [
        0x56, 0x50, 0x4f, 0x4c, 0x01, 0x00, 0x04, 0x00, 0x08, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x76,
        0x66, 0x73, 0x01, 0x00, 0x00, 0x00, 0x00, 0x07, 0x08, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x6e,
        0x65, 0x74, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x0a, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x73,
        0x68, 0x65, 0x6c, 0x6c, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x09, 0x2f, 0x62, 0x69, 0x6e,
        0x2f, 0x69, 0x6e, 0x69, 0x74, 0x01, 0x01, 0x01, 0x00, 0x03, 0x07, 0x44, 0x17, 0x69, 0xc2,
        0xc9, 0x40, 0x3a, 0x1f, 0x67, 0xcf, 0xfa, 0x4d, 0xa1, 0x23, 0x15, 0x29, 0xa1, 0xa6, 0x62,
        0x9f, 0xb4, 0xde, 0x48, 0xe1, 0x61, 0x00, 0x0f, 0x83, 0x98, 0x01, 0x00, 0x46, 0x06, 0x6d,
        0x20, 0xa8, 0xa5, 0xff, 0xd9, 0x05, 0x4f, 0x51, 0x12, 0x46, 0xc6, 0x45, 0x59, 0x7b, 0x15,
        0xae, 0x1e, 0x22, 0xb6, 0x33, 0xb4, 0x2b, 0xc8, 0x84, 0x28, 0x2d, 0x83, 0x7f, 0xde, 0x00,
    ];
    if BLOB.len() < HEADER_LEN + SIG_LEN {
        return false;
    }
    let (body, sig) = BLOB.split_at(BLOB.len() - SIG_LEN);
    let mut s = [0u8; SIG_LEN];
    s.copy_from_slice(sig);

    // 1. Valid blob: signature verifies + parses + /bin/vfs has the expected caps.
    if !crate::ed25519::verify(&DEV_FLEET_PUBKEY, body, &s) {
        return false;
    }
    let Some(v1) = parse(body) else {
        return false;
    };
    let Some(vfs) = v1.entries.iter().find(|e| e.path == "/bin/vfs") else {
        return false;
    };
    if !vfs.caps.block_io || vfs.caps.block_regions != 0b111 {
        return false;
    }
    // v1 has no privileged bytes: they parse false, and because Permit intersects
    // that means a v1 entry STRIPS them. Pinned so the compat path cannot silently
    // start granting authority a v1 operator never wrote.
    if vfs.caps.pcie_driver || vfs.caps.platform || vfs.caps.supervisor {
        return false;
    }
    if !v2_parse_cases() {
        return false;
    }
    // 2. Tampered blob: a flipped body byte must FAIL verification.
    let mut bad = BLOB;
    bad[10] ^= 0x01;
    let (bad_body, _) = bad.split_at(bad.len() - SIG_LEN);
    if crate::ed25519::verify(&DEV_FLEET_PUBKEY, bad_body, &s) {
        return false;
    }

    // 3. Phase 04 narrowing rule (decision_to_caps) — default (dev-permissive) posture.
    let full = CapSet {
        block_io: true,
        network: true,
        spawn: true,
        hypervisor: false,
        mmio_devices: 0,
        block_regions: 0,
        ..CapSet::EMPTY
    };
    let net_only = CapSet {
        network: true,
        ..CapSet::EMPTY
    };
    // Permit narrows: full ∩ {network} = {network}.
    if decision_to_caps("/bin/app", full, PolicyDecision::Permit(net_only)) != net_only {
        return false;
    }
    // DenyAll on a non-core path → EMPTY.
    if decision_to_caps("/bin/app", full, PolicyDecision::DenyAll) != CapSet::EMPTY {
        return false;
    }
    // DenyAll on a trusted-core path → keeps caps (headless recovery hatch).
    if decision_to_caps("/bin/vfs", full, PolicyDecision::DenyAll) != full {
        return false;
    }
    // NoEntry is posture-dependent, so the expectation has to be too. This
    // assertion used to hardcode `full`, which meant a `policy-required` build
    // failed its own power-on self-test on every boot — the one posture where
    // the check matters most, and the failure was advisory so nothing stopped.
    #[cfg(not(feature = "policy-required"))]
    let expect_no_entry = full;
    #[cfg(feature = "policy-required")]
    let expect_no_entry = CapSet::EMPTY;
    if decision_to_caps("/bin/app", full, PolicyDecision::NoEntry) != expect_no_entry {
        return false;
    }
    // Trusted core survives NoEntry in BOTH postures — that is the recovery hatch
    // that keeps a fail-closed misfire from bricking a headless device.
    if decision_to_caps("/bin/vfs", full, PolicyDecision::NoEntry) != full {
        return false;
    }
    true
}

/// v2 layout coverage for `self_test`, exercised at the `parse` level: these check
/// stride and domain handling, which a signature cannot. Bodies are hand-built so
/// the test does not have to be regenerated every time the shipped policy changes.
fn v2_parse_cases() -> bool {
    // magic | version=2 | flags=0 | count=1 | len=8 "/bin/vfs" | 9 cap bytes
    const V2: [u8; 26] = [
        0x56, 0x50, 0x4f, 0x4c, 0x02, 0x00, 0x01, 0x00, 0x08, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x76,
        0x66, 0x73, 0x01, 0x00, 0x00, 0x00, 0x00, 0x07, 0x01, 0x00, 0x00,
    ];
    // Valid v2: the 9-byte stride is read correctly, and pcie_driver arrives.
    let Some(p) = parse(&V2) else {
        return false;
    };
    if p.flags != 0 || p.entries.len() != 1 {
        return false;
    }
    let c = p.entries[0].caps;
    if !c.block_io || c.block_regions != 0b111 || !c.pcie_driver || c.platform || c.supervisor {
        return false;
    }

    // A privileged byte outside {0,1} → Invalid, not coerced to true.
    let mut bad_priv = V2;
    bad_priv[23] = 2;
    if parse(&bad_priv).is_some() {
        return false;
    }
    // mmio outside MMIO_MASK → Invalid (pre-existing rule, re-checked under v2).
    let mut bad_mmio = V2;
    bad_mmio[21] = 0xF0;
    if parse(&bad_mmio).is_some() {
        return false;
    }
    // Unknown flag bit → Invalid: flags gate the maintenance bypass, so an
    // unrecognised one must not be ignored.
    let mut bad_flags = V2;
    bad_flags[5] = 0x02;
    if parse(&bad_flags).is_some() {
        return false;
    }
    // Unknown version → Invalid. Guessing the stride would misread every field.
    let mut bad_ver = V2;
    bad_ver[4] = 3;
    if parse(&bad_ver).is_some() {
        return false;
    }
    // Truncated entry (the 9-byte stride does not fit) → None, not a panic.
    if parse(&V2[..V2.len() - 1]).is_some() {
        return false;
    }
    true
}

/// Policy decision for a cell path. See `PolicyDecision`; the caller (Phase 04)
/// applies the dev-permissive vs `policy-required` fail-safe rule to `NoEntry`.
pub fn lookup(path: &str) -> PolicyDecision {
    let guard = POLICY.lock();
    match guard.as_ref() {
        Some(PolicyState::Loaded(loaded)) => {
            for e in &loaded.entries {
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

/// The minimal trusted core that is NEVER reduced to no-caps by policy — so a
/// fail-closed mis-fire (bad/absent policy under `policy-required`) cannot brick
/// a headless robot by stripping the filesystem/shell/network it needs to recover.
fn is_trusted_core(path: &str) -> bool {
    matches!(path, "/bin/vfs" | "/bin/shell" | "/bin/net")
}

/// Pure narrowing rule: combine spawner-intersected `caps` with a policy
/// `decision`. Recovery: trusted-core cells keep their caps even under DenyAll /
/// fail-closed. NoEntry is dev-permissive unless the `policy-required` feature.
fn decision_to_caps(path: &str, caps: CapSet, decision: PolicyDecision) -> CapSet {
    match decision {
        PolicyDecision::Permit(p) => caps.intersect(p),
        PolicyDecision::DenyAll => {
            if is_trusted_core(path) {
                caps
            } else {
                CapSet::EMPTY
            }
        }
        PolicyDecision::NoEntry => {
            #[cfg(feature = "policy-required")]
            {
                if is_trusted_core(path) {
                    caps
                } else {
                    CapSet::EMPTY
                }
            }
            #[cfg(not(feature = "policy-required"))]
            {
                caps
            }
        }
    }
}

/// Apply operator policy to a cell's spawn-time caps (Phase 04): the final grant
/// is `caps ∩ policy(path)` with trusted-core recovery + fail-safe. Audits when
/// policy actually narrows the grant. `init` (Spawner::Root) is exempt and must
/// NOT call this (subjecting the policy loader to the loaded policy is circular).
pub fn apply(path: &str, tid: usize, caps: CapSet) -> CapSet {
    // Maintenance bypass needs TWO factors: the build feature AND a signed
    // `MAINTENANCE_PERMITTED` flag in the policy. A feature flag alone used to be
    // enough, so one image built with the wrong flag granted every cell every cap
    // with nothing on the device to say so.
    //
    // Consequence accepted: an absent or `Invalid` policy no longer permits the
    // bypass, so maintenance mode cannot recover a device *from* a bad policy.
    // The recovery path in that case is `is_trusted_core` — vfs/shell/net keep
    // their caps even under `DenyAll`, which is enough to reach a prompt and
    // re-provision.
    #[cfg(feature = "maintenance-mode")]
    if maintenance_permitted() {
        log::warn!("[policy] maintenance bypass ACTIVE for {}", path);
        crate::audit::log_event(
            crate::audit::AuditEvent::PolicyMaintenanceBypass,
            &crate::audit::encode_u32x2(tid as u32, 0),
        );
        return caps;
    }

    let narrowed = decision_to_caps(path, caps, lookup(path));
    if narrowed != caps {
        let dropped = (caps.block_io && !narrowed.block_io) as u32
            | (((caps.network && !narrowed.network) as u32) << 1)
            | (((caps.spawn && !narrowed.spawn) as u32) << 2)
            | (((caps.hypervisor && !narrowed.hypervisor) as u32) << 3)
            | (((caps.pcie_driver && !narrowed.pcie_driver) as u32) << 4)
            | (((caps.platform && !narrowed.platform) as u32) << 5)
            | (((caps.supervisor && !narrowed.supervisor) as u32) << 6)
            | (((caps.mmio_devices != narrowed.mmio_devices) as u32) << 7)
            | (((caps.block_regions != narrowed.block_regions) as u32) << 8);
        crate::audit::log_event(
            crate::audit::AuditEvent::CapNarrowedByPolicy,
            &crate::audit::encode_u32x2(tid as u32, dropped),
        );
    }
    // Audit the GRANT of privileged authority, not only its removal. Losing a cap
    // is an availability problem and shows up as a broken cell; *keeping* one of
    // these three is the security-relevant event, and until now it left no trace.
    let granted = narrowed.pcie_driver as u32
        | ((narrowed.platform as u32) << 1)
        | ((narrowed.supervisor as u32) << 2);
    if granted != 0 {
        crate::audit::log_event(
            crate::audit::AuditEvent::PrivilegedCapGranted,
            &crate::audit::encode_u32x2(tid as u32, granted),
        );
    }
    narrowed
}

/// Whether the loaded policy carries the signed maintenance-bypass flag.
/// Absent / `Invalid` / not-yet-loaded → `false` (the bypass is opt-in and signed).
#[cfg(feature = "maintenance-mode")]
fn maintenance_permitted() -> bool {
    matches!(
        POLICY.lock().as_ref(),
        Some(PolicyState::Loaded(p)) if p.flags & FLAG_MAINTENANCE_PERMITTED != 0
    )
}
