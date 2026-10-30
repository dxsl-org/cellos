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
//! - **Missing entry ≠ absent policy:** with a policy loaded, a path the operator
//!   never listed keeps its ordinary caps but LOSES the privileged (P-TRUST) ones,
//!   because those are minted from the install path alone — so a forgotten entry
//!   would otherwise hand out DMA-anywhere authority. With no policy at all,
//!   nothing is tightened. See `PolicyDecision::NoEntry`.
//! - **Domain validation:** parsed cap bytes are masked to known bits; unknown
//!   bits → `Invalid` (a signed-but-malformed policy is still rejected).

use crate::resource_registry::{
    DEV_ADC, DEV_CAN, DEV_DISPLAY, DEV_GPIO, DEV_I2C, DEV_PCIE, DEV_SPI, DEV_UART,
};
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
/// v3: 10 cap bytes; adds usb_driver for USB host authority.
const VERSION_V3: u8 = 3;
/// v4 appends `serial_port` (bit 10) — the x86 16550 port authority.
const VERSION_V4: u8 = 4;
/// v5 appends the four coarse network rights (Phase 04 S2-N): `net_resolve`,
/// `net_connect`, `net_listen`, `net_l2`, each a literal 0|1 byte.
const VERSION_V5: u8 = 5;
/// v6 appends the three S2-T/S2-W rights (Phase 04 annex): `observe_tasks`,
/// `observe_memory`, `vfs_mutate`, each a literal 0|1 byte.
const VERSION_V6: u8 = 6;
/// v7 appends the Q right (Phase 01 CP1): `config_write`, a literal 0|1 byte.
const VERSION_V7: u8 = 7;
/// v8 appends the two KSSH K3 rights (2026-10-30): `kms_ssh_sign`,
/// `kms_ssh_admin`, each a literal 0|1 byte. A v1–v7 entry decodes both as
/// `false` (fail-closed, O-11): the rights were never ambient.
const VERSION_V8: u8 = 8;
const SIG_LEN: usize = 64;
const HEADER_LEN: usize = 8; // magic(4) + version(1) + flags(1) + entry_count(2)
const CAP_BYTES_V1: usize = 6; // block_io, network, spawn, hyp, mmio_devices, block_regions
const CAP_BYTES_V2: usize = 9; // + pcie_driver, platform, supervisor
const CAP_BYTES_V3: usize = 10; // + usb_driver
const CAP_BYTES_V4: usize = 11; // + serial_port
const CAP_BYTES_V5: usize = 15; // + net_resolve, net_connect, net_listen, net_l2
const CAP_BYTES_V6: usize = 18; // + observe_tasks, observe_memory, vfs_mutate
const CAP_BYTES_V7: usize = 19; // + config_write
const CAP_BYTES_V8: usize = 21; // + kms_ssh_sign, kms_ssh_admin
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
        VERSION_V3 => Some(CAP_BYTES_V3),
        VERSION_V4 => Some(CAP_BYTES_V4),
        VERSION_V5 => Some(CAP_BYTES_V5),
        VERSION_V6 => Some(CAP_BYTES_V6),
        VERSION_V7 => Some(CAP_BYTES_V7),
        VERSION_V8 => Some(CAP_BYTES_V8),
        _ => None,
    }
}

/// Valid `mmio_devices` bits and `block_regions` bits (domain-validation masks).
///
/// Both masks must cover every bit the kernel is capable of minting, or a policy
/// that names a legitimate device/partition is rejected as malformed → `DenyAll`
/// for the whole fleet. `CapSet::from_manifest` mints CAN, ADC, I2C, and SPI
/// device classes from the manifest, while path policy mints display authority,
/// and the block-region encoding carries a 4th bit for the cell-store region.
/// Widening a mask cannot grant authority: a `Permit` intersects the request,
/// so a bit the manifest never asked for stays off.
const MMIO_MASK: u8 =
    DEV_GPIO | DEV_UART | DEV_PCIE | DEV_CAN | DEV_ADC | DEV_I2C | DEV_SPI | DEV_DISPLAY;
const REGION_MASK: u8 = 0b1111;

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

/// Result of a policy lookup for a given cell path. Plain data (`Copy`) so a
/// caller can both inspect a decision and pass it to the narrowing rule.
#[derive(Copy, Clone)]
pub enum PolicyDecision {
    /// Policy explicitly grants this path the given caps (ceiling).
    Permit(CapSet),
    /// Policy is present and explicitly denies (or invalid → fail-closed).
    DenyAll,
    /// No caps stated for this path. The caller applies the fail-safe rule:
    /// dev-permissive keeps the spawner-intersected caps; `policy-required`
    /// treats it as deny.
    ///
    /// `policy_loaded` separates the two ways to arrive here, because they carry
    /// different amounts of information:
    /// - `false` — no policy at all (absent, or not yet resolved). The operator
    ///   has said nothing about any path; nothing can be inferred about this one.
    /// - `true` — a signed policy IS loaded and simply does not name this path.
    ///   That is a coverage gap in a document that was supposed to be exhaustive,
    ///   so a path whose install path alone mints privileged authority
    ///   (`CapSet::path_mints_ptrust`) fails closed for that authority.
    ///
    /// Collapsing the two would force one behaviour on both: fail-closed would
    /// strip every driver's caps on a device with no policy blob (no boot), and
    /// permissive lets a forgotten entry hand out DMA-anywhere authority.
    NoEntry { policy_loaded: bool },
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
            let mut policy = POLICY.lock();
            // The verified, parsed containers are now retained by POLICY.
            // Join their public capacities, never the temporary signed blob.
            #[cfg(cellos_p01_trace)]
            {
                use crate::memory::heap::p01_owner::{self, SYSTEM_CONFIG_SHARED};
                p01_owner::shared_vec(&loaded.entries, SYSTEM_CONFIG_SHARED);
                for entry in &loaded.entries {
                    p01_owner::shared_string(&entry.path, SYSTEM_CONFIG_SHARED);
                }
            }
            *policy = Some(PolicyState::Loaded(loaded));
        }
        None => {
            log::warn!("[policy] malformed body — fail-closed");
            mark_invalid(3);
        }
    }
}

/// Whether `load_from_vifs1` has already run, in any outcome (loaded, absent, or
/// invalid).
///
/// The boot path needs this to decide whether the root-authority policy exemption
/// still applies: before the policy is resolved there is nothing to apply, and
/// applying it would be circular; afterwards the exemption has no justification.
/// `false` means "policy not yet resolved", never "no policy" — an absent policy
/// resolves to `Absent` and reports `true`.
pub fn is_resolved() -> bool {
    POLICY.lock().is_some()
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
        // cap bytes: per-version stride from `cap_bytes_for` (6/9/10/11/15/18/19/21)
        let caps_raw = body.get(off..off.checked_add(cap_bytes)?)?;
        off += cap_bytes;

        let mmio_devices = caps_raw[4];
        let block_regions = caps_raw[5];
        // The MMIO byte's eight bits are all assigned device classes, so it has
        // no out-of-domain bit pattern. Reject unknown block-region bits.
        if block_regions & !REGION_MASK != 0 {
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
        let (pcie_driver, platform, supervisor, usb_driver, serial_port) =
            if cap_bytes >= CAP_BYTES_V2 {
                let (p, pl, s) = (caps_raw[6], caps_raw[7], caps_raw[8]);
                if p > 1 || pl > 1 || s > 1 {
                    return None;
                }
                let u = if cap_bytes >= CAP_BYTES_V3 {
                    let u_val = caps_raw[9];
                    if u_val > 1 {
                        return None;
                    }
                    u_val == 1
                } else {
                    false
                };
                // v4 only: an older blob grants no serial authority.
                let sp = if cap_bytes >= CAP_BYTES_V4 {
                    let sp_val = caps_raw[10];
                    if sp_val > 1 {
                        return None;
                    }
                    sp_val == 1
                } else {
                    false
                };
                (p == 1, pl == 1, s == 1, u, sp)
            } else {
                (false, false, false, false, false)
            };
        // v5 network rights. Literal 0|1 like the privileged bytes: these decide
        // who may put frames on the NIC, so a stray value is a malformed blob.
        //
        // Compatibility rule (annex S2-N E1), deliberately the OPPOSITE of the
        // privileged fields: a v1–v4 entry predates the rights, which were
        // ambient until this slice, so its Permit must not narrow them. They
        // decode as `true`, leaving request ∩ reviewed ceiling as the bound —
        // that never widens past the edge, and `false` here would silently strip
        // every legacy-listed network client.
        let (net_resolve, net_connect, net_listen, net_l2) = if cap_bytes >= CAP_BYTES_V5 {
            let net = &caps_raw[CAP_BYTES_V4..CAP_BYTES_V5];
            if net.iter().any(|&b| b > 1) {
                return None;
            }
            (net[0] == 1, net[1] == 1, net[2] == 1, net[3] == 1)
        } else {
            (true, true, true, true)
        };
        // v6 S2-T/S2-W rights: literal 0|1 like the network rights. Same
        // compatibility rule as S2-N E1 (annex §4): these rights were ambient
        // until this slice, so a v1–v5 entry decodes them as `true` and its
        // Permit leaves request ∩ reviewed ceiling as the bound; `false` would
        // strip `ps`/`free`/file writes from every legacy-listed path.
        let (observe_tasks, observe_memory, vfs_mutate) = if cap_bytes >= CAP_BYTES_V6 {
            let rights = &caps_raw[CAP_BYTES_V5..CAP_BYTES_V6];
            if rights.iter().any(|&b| b > 1) {
                return None;
            }
            (rights[0] == 1, rights[1] == 1, rights[2] == 1)
        } else {
            (true, true, true)
        };
        // v7 Q right (Phase 01 CP1, D-Q6): literal 0|1 like the rights above,
        // and the same compatibility rule as S2-N E1 and S2-T/S2-W: Config writes
        // were ambient to every Cell until this slice, so a v1–v6 entry decodes
        // it as `true` and its Permit leaves request ∩ reviewed ceiling (only
        // the shell edges carry Q) as the bound.
        let config_write = if cap_bytes >= CAP_BYTES_V7 {
            match caps_raw[CAP_BYTES_V6] {
                0 => false,
                1 => true,
                _ => return None,
            }
        } else {
            true
        };
        // v8 KSSH K3 rights (O-11): literal 0|1 like the rights above, but the
        // compatibility rule is the OPPOSITE of the network, S2-T/S2-W and Q rights
        // and the same as the privileged fields. Those rights were ambient before
        // their slices; signing authority never was, so a v1–v7 entry decodes both
        // as `false` and its Permit STRIPS them until the policy is re-signed at
        // v8. Fail-closed: the cost is "publickey unavailable" for that Cell, never
        // a wrongly granted signing right.
        let (kms_ssh_sign, kms_ssh_admin) = if cap_bytes >= CAP_BYTES_V8 {
            let kms = &caps_raw[CAP_BYTES_V7..CAP_BYTES_V8];
            if kms.iter().any(|&b| b > 1) {
                return None;
            }
            (kms[0] == 1, kms[1] == 1)
        } else {
            (false, false)
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
                serial_port,
                platform,
                supervisor,
                usb_driver,
                net_resolve,
                net_connect,
                net_listen,
                net_l2,
                observe_tasks,
                observe_memory,
                vfs_mutate,
                config_write,
                kms_ssh_sign,
                kms_ssh_admin,
            },
        });
    }
    Some(LoadedPolicy { flags, entries })
}

/// Self-test of the full signed-policy path: verify + parse a known dev-signed
/// blob (from `scripts/sign-policy.py`), confirm a known entry parses correctly,
/// confirm a tampered blob is REJECTED, and pin the narrowing rule (including the
/// fail-closed handling of a missing entry). Returns `true` iff every case holds.
/// Run as a boot power-on self-test before trusting the policy path.
pub fn self_test() -> bool {
    // 135-byte dev-signed v1 compatibility blob (4 entries).
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
    if !vfs.caps.block_io {
        return false;
    }
    // The current `/bin/vfs` fold is pinned below in `v2_parse_cases()`. This
    // smaller v1 fixture stays three-region to keep the backward-compat path covered.
    // v1 has no privileged bytes: they parse false, and because Permit intersects
    // that means a v1 entry STRIPS them. Pinned so the compat path cannot silently
    // start granting authority a v1 operator never wrote.
    if vfs.caps.pcie_driver || vfs.caps.platform || vfs.caps.supervisor || vfs.caps.usb_driver {
        return false;
    }
    if !v2_parse_cases() {
        return false;
    }
    if !v3_parse_cases() {
        return false;
    }
    if !v5_parse_cases() {
        return false;
    }
    if !v6_parse_cases() {
        return false;
    }
    if !v7_parse_cases() {
        return false;
    }
    if !v8_parse_cases() {
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
    if decision_to_caps(
        "/bin/app",
        full,
        PolicyDecision::NoEntry {
            policy_loaded: false,
        },
    ) != expect_no_entry
    {
        return false;
    }
    // Trusted core survives NoEntry in BOTH postures — that is the recovery hatch
    // that keeps a fail-closed misfire from bricking a headless device.
    if decision_to_caps(
        "/bin/vfs",
        full,
        PolicyDecision::NoEntry {
            policy_loaded: false,
        },
    ) != full
    {
        return false;
    }
    no_entry_ptrust_cases(full)
}

/// The fail-closed rule for a *loaded* policy with a missing entry, pinned in both
/// directions: privileged authority goes away for a path that mints it, and nothing
/// changes for a path that does not (or when there is no policy at all).
fn no_entry_ptrust_cases(full: CapSet) -> bool {
    let driver = CapSet {
        pcie_driver: true,
        ..full
    };
    let loaded = PolicyDecision::NoEntry {
        policy_loaded: true,
    };
    let absent = PolicyDecision::NoEntry {
        policy_loaded: false,
    };

    // Loaded policy, no entry, path mints P-TRUST → the privileged cap is gone and
    // the ordinary ones survive (the cell runs; the gap is diagnosable).
    #[cfg(not(feature = "policy-required"))]
    let expect_stripped = full;
    #[cfg(feature = "policy-required")]
    let expect_stripped = CapSet::EMPTY;
    let stripped = decision_to_caps("/bin/nvme", driver, loaded);
    if stripped.pcie_driver || stripped != expect_stripped {
        log::error!("[selftest] policy: FAIL — NoEntry kept privileged caps for /bin/nvme");
        return false;
    }

    // Two cases that must NOT tighten: an absent policy (no document, so no gap to
    // infer) and a path the table mints nothing for.
    #[cfg(not(feature = "policy-required"))]
    let expect_kept = driver;
    #[cfg(feature = "policy-required")]
    let expect_kept = CapSet::EMPTY;
    if decision_to_caps("/bin/nvme", driver, absent) != expect_kept {
        log::error!("[selftest] policy: FAIL — absent policy changed behaviour for /bin/nvme");
        return false;
    }
    if decision_to_caps("/bin/app", driver, loaded) != expect_kept {
        log::error!("[selftest] policy: FAIL — NoEntry tightened an ordinary path");
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
        0x66, 0x73, 0x01, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x01, 0x00, 0x00,
    ];
    // Valid v2: the 9-byte stride is read correctly, pcie_driver arrives, and
    // `/bin/vfs` can carry all four block regions through policy.
    let Some(p) = parse(&V2) else {
        return false;
    };
    if p.flags != 0 || p.entries.len() != 1 {
        return false;
    }
    let c = p.entries[0].caps;
    if !c.block_io || c.block_regions != 0b1111 || !c.pcie_driver || c.platform || c.supervisor {
        return false;
    }

    // A privileged byte outside {0,1} → Invalid, not coerced to true.
    let mut bad_priv = V2;
    bad_priv[23] = 2;
    if parse(&bad_priv).is_some() {
        return false;
    }
    // Every u8 device-class bit is assigned, including path-minted display
    // authority, so a policy naming any controller must remain parseable.
    let mut manifest_mmio = V2;
    manifest_mmio[21] = MMIO_MASK;
    match parse(&manifest_mmio)
        .as_ref()
        .and_then(|p| p.entries.first())
    {
        Some(e) if e.caps.mmio_devices == MMIO_MASK => {}
        _ => return false,
    }
    // The 4-bit block-region encoding is inside the domain: the `/bin/vfs`
    // cell-store region lives in bit 3, and a policy able to express it is the
    // precondition for granting it through policy instead of a raw grant.
    let mut regions4 = V2;
    regions4[22] = 0b1111;
    match parse(&regions4).as_ref().and_then(|p| p.entries.first()) {
        Some(e) if e.caps.block_regions == 0b1111 => {}
        _ => return false,
    }
    // …but bit 4 and above are still out of domain — widening the mask must not
    // turn it into "accept anything".
    let mut bad_regions = V2;
    bad_regions[22] = 0b1_0000;
    if parse(&bad_regions).is_some() {
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
    bad_ver[4] = 4;
    if parse(&bad_ver).is_some() {
        return false;
    }
    // Truncated entry (the 9-byte stride does not fit) → None, not a panic.
    if parse(&V2[..V2.len() - 1]).is_some() {
        return false;
    }
    true
}

/// v3 layout coverage for `self_test`: checks that the 10-byte stride arrives,
/// `usb_driver` parses true for `/bin/dwc2-usb`, and other privileged caps stay false.
fn v3_parse_cases() -> bool {
    // magic | version=3 | flags=0 | count=1 | len=13 "/bin/dwc2-usb" | 10 cap bytes
    const V3: [u8; 32] = [
        0x56, 0x50, 0x4f, 0x4c, 0x03, 0x00, 0x01, 0x00, 0x0d, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x64,
        0x77, 0x63, 0x32, 0x2d, 0x75, 0x73, 0x62, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x01,
    ];
    let Some(p) = parse(&V3) else {
        return false;
    };
    if p.flags != 0 || p.entries.len() != 1 {
        return false;
    }
    let c = p.entries[0].caps;
    if !c.usb_driver || c.pcie_driver || c.platform || c.supervisor {
        return false;
    }
    true
}

/// magic | version=5 | flags=0 | count=1 | len=9 "/bin/curl" | 15 cap bytes:
/// network=1, every privileged byte 0, then resolve=1 connect=1 listen=0 l2=0.
const V5_CURL: [u8; 33] = [
    0x56, 0x50, 0x4f, 0x4c, 0x05, 0x00, 0x01, 0x00, 0x09, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x63, 0x75,
    0x72, 0x6c, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00,
    0x00,
];
/// Offset of the first network-right byte in `V5_CURL`.
const V5_CURL_NET: usize = 29;
/// The same `/bin/curl` entry in v4 (11 cap bytes: no network-right bytes).
const V4_CURL: [u8; 29] = [
    0x56, 0x50, 0x4f, 0x4c, 0x04, 0x00, 0x01, 0x00, 0x09, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x63, 0x75,
    0x72, 0x6c, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// v5 layout coverage for `self_test`: the 15-byte stride arrives with the four
/// network rights, a Permit narrows them, every non-0|1 network byte rejects the
/// blob, and a v4 entry (compat rule E1) leaves them unnarrowed.
fn v5_parse_cases() -> bool {
    let Some(p) = parse(&V5_CURL) else {
        return false;
    };
    if p.flags != 0 || p.entries.len() != 1 || p.entries[0].path != "/bin/curl" {
        return false;
    }
    let c = p.entries[0].caps;
    if !(c.network && c.net_resolve && c.net_connect) || c.net_listen || c.net_l2 {
        return false;
    }
    if c.pcie_driver || c.platform || c.supervisor || c.usb_driver || c.serial_port {
        return false;
    }
    // A v5 Permit narrows: a request carrying every right keeps only D + N.
    let all_net = CapSet {
        net_resolve: true,
        net_connect: true,
        net_listen: true,
        net_l2: true,
        ..CapSet::EMPTY
    };
    let narrowed = decision_to_caps("/bin/curl", all_net, PolicyDecision::Permit(c));
    if !(narrowed.net_resolve && narrowed.net_connect) || narrowed.net_listen || narrowed.net_l2 {
        return false;
    }
    // Each network byte outside {0,1} makes the whole blob malformed.
    for i in V5_CURL_NET..V5_CURL.len() {
        let mut bad = V5_CURL;
        bad[i] = 2;
        if parse(&bad).is_some() {
            return false;
        }
    }
    // Truncated v5 entry → None, not a panic.
    if parse(&V5_CURL[..V5_CURL.len() - 1]).is_some() {
        return false;
    }
    // A v5 body relabelled v6 no longer fits the 18-byte stride → Invalid.
    let mut bad_ver = V5_CURL;
    bad_ver[4] = VERSION_V6;
    if parse(&bad_ver).is_some() {
        return false;
    }
    // Compatibility rule (E1): a v4 entry decodes all four rights as true, so its
    // Permit leaves the request ∩ ceiling unnarrowed.
    let Some(v4) = parse(&V4_CURL) else {
        return false;
    };
    let legacy = v4.entries[0].caps;
    if !(legacy.net_resolve && legacy.net_connect && legacy.net_listen && legacy.net_l2) {
        return false;
    }
    decision_to_caps("/bin/curl", all_net, PolicyDecision::Permit(legacy)) == all_net
}

/// magic | version=6 | flags=0 | count=1 | len=9 "/bin/curl" | 18 cap bytes:
/// the `V5_CURL` entry, then observe_tasks=1 observe_memory=0 vfs_mutate=1.
const V6_CURL: [u8; 36] = [
    0x56, 0x50, 0x4f, 0x4c, 0x06, 0x00, 0x01, 0x00, 0x09, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x63, 0x75,
    0x72, 0x6c, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00,
    0x00, 0x01, 0x00, 0x01,
];
/// Offset of the first S2-T/S2-W byte (`observe_tasks`) in `V6_CURL`.
const V6_CURL_RIGHTS: usize = 33;

/// v6 layout coverage for `self_test`: the 18-byte stride arrives with the three
/// S2-T/S2-W rights, a Permit narrows them, every non-0|1 byte rejects the blob,
/// an unknown version rejects, and a v5 entry (annex §4) leaves them unnarrowed.
fn v6_parse_cases() -> bool {
    let Some(p) = parse(&V6_CURL) else {
        return false;
    };
    if p.flags != 0 || p.entries.len() != 1 || p.entries[0].path != "/bin/curl" {
        return false;
    }
    let c = p.entries[0].caps;
    if !(c.observe_tasks && !c.observe_memory && c.vfs_mutate) {
        return false;
    }
    // The v5 prefix of the entry is read exactly as before.
    if !(c.network && c.net_resolve && c.net_connect) || c.net_listen || c.net_l2 {
        return false;
    }
    // A v6 Permit narrows: the request's M is dropped, X and W survive.
    let request = CapSet::LEGACY_OBSERVE_MUTATE;
    let narrowed = decision_to_caps("/bin/curl", request, PolicyDecision::Permit(c));
    if !(narrowed.observe_tasks && !narrowed.observe_memory && narrowed.vfs_mutate) {
        return false;
    }
    for i in V6_CURL_RIGHTS..V6_CURL.len() {
        let mut bad = V6_CURL;
        bad[i] = 2;
        if parse(&bad).is_some() {
            return false;
        }
    }
    if parse(&V6_CURL[..V6_CURL.len() - 1]).is_some() {
        return false;
    }
    // A v6 body relabelled v7 no longer fits the 19-byte stride → Invalid.
    let mut bad_ver = V6_CURL;
    bad_ver[4] = VERSION_V7;
    if parse(&bad_ver).is_some() {
        return false;
    }
    // Compatibility rule (annex §4): a v5 entry decodes all three as true, so
    // its Permit leaves the request ∩ ceiling unnarrowed.
    let Some(v5) = parse(&V5_CURL) else {
        return false;
    };
    let legacy = v5.entries[0].caps;
    if !(legacy.observe_tasks && legacy.observe_memory && legacy.vfs_mutate) {
        return false;
    }
    decision_to_caps("/bin/curl", request, PolicyDecision::Permit(legacy)) == request
}

/// magic | version=7 | flags=0 | count=1 | len=9 "/bin/curl" | 19 cap bytes:
/// the `V6_CURL` entry, then config_write=0 (curl requests no Q).
const V7_CURL: [u8; 37] = [
    0x56, 0x50, 0x4f, 0x4c, 0x07, 0x00, 0x01, 0x00, 0x09, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x63, 0x75,
    0x72, 0x6c, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00,
    0x00, 0x01, 0x00, 0x01, 0x00,
];
/// Offset of the Q byte (`config_write`) in `V7_CURL`.
const V7_CURL_Q: usize = 36;

/// v7 layout coverage for `self_test` (Phase 01 CP1): the 19-byte stride arrives
/// with `config_write`, a v7 Permit narrows it, a non-0|1 byte rejects the blob,
/// an unknown version rejects, and a v6 entry (D-Q6) leaves it unnarrowed.
fn v7_parse_cases() -> bool {
    let Some(p) = parse(&V7_CURL) else {
        return false;
    };
    if p.flags != 0 || p.entries.len() != 1 || p.entries[0].path != "/bin/curl" {
        return false;
    }
    let c = p.entries[0].caps;
    // The v6 prefix of the entry is read exactly as before.
    if c.config_write || !(c.observe_tasks && !c.observe_memory && c.vfs_mutate) {
        return false;
    }
    let shell_request = CapSet {
        config_write: true,
        ..CapSet::LEGACY_OBSERVE_MUTATE
    };
    if decision_to_caps("/bin/curl", shell_request, PolicyDecision::Permit(c)).config_write {
        return false;
    }
    let mut granted = V7_CURL;
    granted[V7_CURL_Q] = 1;
    match parse(&granted) {
        Some(g) if g.entries[0].caps.config_write => {}
        _ => return false,
    }
    let mut bad = V7_CURL;
    bad[V7_CURL_Q] = 2;
    if parse(&bad).is_some() {
        return false;
    }
    if parse(&V7_CURL[..V7_CURL.len() - 1]).is_some() {
        return false;
    }
    // Version 9 is unknown → Invalid.
    let mut bad_ver = V7_CURL;
    bad_ver[4] = 9;
    if parse(&bad_ver).is_some() {
        return false;
    }
    // Compatibility rule (D-Q6): a v6 entry decodes Q as true, so its Permit
    // leaves the request ∩ ceiling unnarrowed.
    let Some(v6) = parse(&V6_CURL) else {
        return false;
    };
    let legacy = v6.entries[0].caps;
    legacy.config_write
        && decision_to_caps("/bin/cellbox", shell_request, PolicyDecision::Permit(legacy))
            .config_write
}

/// magic | version=8 | flags=0 | count=1 | len=9 "/bin/curl" | 21 cap bytes:
/// the `V7_CURL` entry, then kms_ssh_sign=0 kms_ssh_admin=0 (curl asks for neither).
const V8_CURL: [u8; 39] = [
    0x56, 0x50, 0x4f, 0x4c, 0x08, 0x00, 0x01, 0x00, 0x09, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x63, 0x75,
    0x72, 0x6c, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00,
    0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00,
];
/// Offset of the KSSH sign byte (`kms_ssh_sign`) in `V8_CURL`.
const V8_CURL_SIGN: usize = 37;
/// Offset of the KSSH admin byte (`kms_ssh_admin`) in `V8_CURL`.
const V8_CURL_ADMIN: usize = 38;

/// v8 layout coverage for `self_test` (KSSH K3): the 21-byte stride arrives with
/// the two KMS SSH rights, a v8 Permit narrows them, a non-0|1 byte rejects the
/// blob, a v7 body relabelled v8 and an unknown version reject, a pre-v8 entry
/// decodes both as `false` and strips them (O-11), and a loaded policy without a
/// row strips them while an absent policy leaves them alone (H-1).
fn v8_parse_cases() -> bool {
    let Some(p) = parse(&V8_CURL) else {
        return false;
    };
    if p.flags != 0 || p.entries.len() != 1 || p.entries[0].path != "/bin/curl" {
        return false;
    }
    let c = p.entries[0].caps;
    // The v7 prefix of the entry is read exactly as before.
    if c.kms_ssh_sign || c.kms_ssh_admin || c.config_write {
        return false;
    }
    if !(c.observe_tasks && !c.observe_memory && c.vfs_mutate) || !(c.network && c.net_resolve) {
        return false;
    }
    // Each byte alone grants exactly its own right, and a Permit narrows to it.
    let request = CapSet {
        kms_ssh_sign: true,
        kms_ssh_admin: true,
        ..CapSet::LEGACY_OBSERVE_MUTATE
    };
    for (offset, want_sign, want_admin) in
        [(V8_CURL_SIGN, true, false), (V8_CURL_ADMIN, false, true)]
    {
        let mut granted = V8_CURL;
        granted[offset] = 1;
        let Some(g) = parse(&granted) else {
            return false;
        };
        let gc = g.entries[0].caps;
        if gc.kms_ssh_sign != want_sign || gc.kms_ssh_admin != want_admin {
            return false;
        }
        let narrowed = decision_to_caps("/bin/curl", request, PolicyDecision::Permit(gc));
        if narrowed.kms_ssh_sign != want_sign || narrowed.kms_ssh_admin != want_admin {
            return false;
        }
    }
    let narrowed = decision_to_caps("/bin/curl", request, PolicyDecision::Permit(c));
    if narrowed.kms_ssh_sign || narrowed.kms_ssh_admin {
        return false;
    }
    // Each KMS byte outside {0,1} makes the whole blob malformed.
    for i in V8_CURL_SIGN..V8_CURL.len() {
        let mut bad = V8_CURL;
        bad[i] = 2;
        if parse(&bad).is_some() {
            return false;
        }
    }
    // Truncated v8 entry → None, not a panic.
    if parse(&V8_CURL[..V8_CURL.len() - 1]).is_some() {
        return false;
    }
    // A v7 body relabelled v8 no longer fits the 21-byte stride → Invalid.
    let mut v7_as_v8 = V7_CURL;
    v7_as_v8[4] = VERSION_V8;
    if parse(&v7_as_v8).is_some() {
        return false;
    }
    // Version 9 is unknown → Invalid.
    let mut bad_ver = V8_CURL;
    bad_ver[4] = 9;
    if parse(&bad_ver).is_some() {
        return false;
    }
    // O-11: a pre-v8 entry decodes both as false, so its Permit strips them.
    for legacy_body in [&V7_CURL[..], &V6_CURL[..], &V5_CURL[..], &V4_CURL[..]] {
        let Some(legacy) = parse(legacy_body) else {
            return false;
        };
        let legacy = legacy.entries[0].caps;
        if legacy.kms_ssh_sign || legacy.kms_ssh_admin {
            return false;
        }
        let narrowed = decision_to_caps("/bin/cellbox-ssh", request, PolicyDecision::Permit(legacy));
        if narrowed.kms_ssh_sign || narrowed.kms_ssh_admin {
            return false;
        }
    }
    // H-1: a loaded policy without a row strips both rights, whatever else the
    // posture does; an absent policy tightens nothing.
    let loaded = PolicyDecision::NoEntry {
        policy_loaded: true,
    };
    let stripped = decision_to_caps("/bin/cellbox-ssh", request, loaded);
    if stripped.kms_ssh_sign || stripped.kms_ssh_admin {
        return false;
    }
    #[cfg(not(feature = "policy-required"))]
    {
        if stripped != CapSet::LEGACY_OBSERVE_MUTATE {
            return false;
        }
        let absent = PolicyDecision::NoEntry {
            policy_loaded: false,
        };
        if decision_to_caps("/bin/cellbox-ssh", request, absent) != request {
            return false;
        }
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
            PolicyDecision::NoEntry {
                policy_loaded: true,
            }
        }
        // Invalid → fail-closed ALWAYS, regardless of posture.
        Some(PolicyState::Invalid) => PolicyDecision::DenyAll,
        // Absent / not-yet-loaded → NoEntry; the caller's fail-safe rule decides
        // (dev-permissive keeps caps; `policy-required` denies). `policy_loaded:
        // false` keeps this the pre-existing, entirely permissive path: there is no
        // document to have a gap in, so nothing here may tighten.
        Some(PolicyState::Absent) | None => PolicyDecision::NoEntry {
            policy_loaded: false,
        },
    }
}

/// The minimal trusted core that is NEVER reduced to no-caps by policy — so a
/// fail-closed mis-fire (bad/absent policy under `policy-required`) cannot brick
/// a headless robot by stripping the filesystem/shell/network it needs to recover.
///
/// `/bin/platform` belongs here for the same reason, not a lesser one: it is the
/// PCIe ECAM enumeration cell, and on x86_64 the block driver — hence /data and
/// the cell-store — cannot exist without it. A shell and a filesystem that came
/// up with no disk behind them are not a recovery path. This only ever matters
/// on the `DenyAll` branch below (malformed blob, or an explicit deny); a valid
/// policy's own entry for this path is honoured normally through `Permit`.
fn is_trusted_core(path: &str) -> bool {
    matches!(
        path,
        "/bin/vfs" | "/bin/cellbox" | "/bin/net" | "/bin/platform"
    )
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
        PolicyDecision::NoEntry { policy_loaded } => {
            // A loaded policy that does not name a P-TRUST-minting path is a gap in
            // a document meant to be exhaustive — a typo or a forgotten re-bake —
            // and keeping the caps would turn that mistake into a cell holding
            // DMA-anywhere authority no operator approved. Strip only the
            // privileged three: the cell still runs, and the ordinary dev loop for
            // every other path is untouched. `policy_loaded == false` (absent
            // policy) must NOT tighten anything, or a device with no blob loses
            // every driver.
            let caps = if policy_loaded && CapSet::path_mints_ptrust(path) {
                caps.without_ptrust()
            } else {
                caps
            };
            // KSSH K3 (H-1): the signing and admin rights are never ambient, so
            // under a loaded policy a path it does not name holds neither, exactly
            // like an explicit `0` row. Without this a loaded v8 policy that merely
            // omits `/bin/cellbox-ssh` would still leave the right granted
            // (request ∩ edge ceiling) while a row of `0` strips it. An absent
            // policy (`policy_loaded == false`) tightens nothing.
            let caps = if policy_loaded {
                CapSet {
                    kms_ssh_sign: false,
                    kms_ssh_admin: false,
                    ..caps
                }
            } else {
                caps
            };
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

/// The `CapNarrowedByPolicy` payload mask: one bit per capability `narrowed` lost
/// relative to `caps`. Bit layout is documented on `AuditEvent::CapNarrowedByPolicy`.
fn narrowed_bits(caps: CapSet, narrowed: CapSet) -> u32 {
    (caps.block_io && !narrowed.block_io) as u32
        | (((caps.network && !narrowed.network) as u32) << 1)
        | (((caps.spawn && !narrowed.spawn) as u32) << 2)
        | (((caps.hypervisor && !narrowed.hypervisor) as u32) << 3)
        | (((caps.pcie_driver && !narrowed.pcie_driver) as u32) << 4)
        | (((caps.platform && !narrowed.platform) as u32) << 5)
        | (((caps.supervisor && !narrowed.supervisor) as u32) << 6)
        | (((caps.mmio_devices != narrowed.mmio_devices) as u32) << 7)
        | (((caps.block_regions != narrowed.block_regions) as u32) << 8)
        | (((caps.net_resolve && !narrowed.net_resolve) as u32) << 9)
        | (((caps.net_connect && !narrowed.net_connect) as u32) << 10)
        | (((caps.net_listen && !narrowed.net_listen) as u32) << 11)
        | (((caps.net_l2 && !narrowed.net_l2) as u32) << 12)
        | (((caps.observe_tasks && !narrowed.observe_tasks) as u32) << 13)
        | (((caps.observe_memory && !narrowed.observe_memory) as u32) << 14)
        | (((caps.vfs_mutate && !narrowed.vfs_mutate) as u32) << 15)
        | (((caps.config_write && !narrowed.config_write) as u32) << 16)
        | (((caps.kms_ssh_sign && !narrowed.kms_ssh_sign) as u32) << 17)
        | (((caps.kms_ssh_admin && !narrowed.kms_ssh_admin) as u32) << 18)
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
    // The recovery path in that case is `is_trusted_core` — vfs/shell/net/platform
    // keep their caps even under `DenyAll`, which is enough to reach a prompt with
    // a working disk and re-provision.
    #[cfg(feature = "maintenance-mode")]
    if maintenance_permitted() {
        log::warn!("[policy] maintenance bypass ACTIVE for {}", path);
        crate::audit::log_event(
            crate::audit::AuditEvent::PolicyMaintenanceBypass,
            &crate::audit::encode_u32x2(tid as u32, 0),
        );
        return caps;
    }

    let decision = lookup(path);
    // This is the only point that knows *why* the privileged caps disappeared, and
    // the fleet needs that distinct from an ordinary policy narrowing: a missing
    // entry is a bake mistake to fix at the build, not an operator choice. Recorded
    // even when the request carried none of the three, because the coverage gap is
    // the finding.
    let no_entry_strip = matches!(
        decision,
        PolicyDecision::NoEntry {
            policy_loaded: true
        }
    ) && CapSet::path_mints_ptrust(path);
    let narrowed = decision_to_caps(path, caps, decision);
    if no_entry_strip {
        let stripped = caps.pcie_driver as u32
            | ((caps.platform as u32) << 1)
            | ((caps.supervisor as u32) << 2);
        crate::audit::log_event(
            crate::audit::AuditEvent::PolicyNoEntryStripped,
            &crate::audit::encode_u32x2(tid as u32, stripped),
        );
        log::warn!(
            "[policy] no entry for {:?} (tid {}) — privileged caps stripped {:#05b} \
             [sup|plat|pcie]; add the path to the signed policy",
            path,
            tid,
            stripped
        );
    }
    if narrowed != caps {
        let dropped = narrowed_bits(caps, narrowed);
        crate::audit::log_event(
            crate::audit::AuditEvent::CapNarrowedByPolicy,
            &crate::audit::encode_u32x2(tid as u32, dropped),
        );
        // Mirror the audit record into the boot log with its own legend. The audit
        // ring needs a live cell to read it, which is exactly what is missing when
        // policy strips the caps a boot cell needed — leaving the serial log as the
        // only evidence a reader has.
        log::warn!(
            "[policy] narrowed {:?} (tid {}): dropped {:#021b} \
             [kadm|ksgn|cfgw|vfsw|mem|tasks|l2|listen|connect|resolve|regions|mmio|sup|plat|pcie|hyp|spawn|net|bio]",
            path,
            tid,
            dropped
        );
    }
    // Audit the GRANT of privileged authority, not only its removal. Losing a cap
    // is an availability problem and shows up as a broken cell; *keeping* one of
    // these three is the security-relevant event, and until now it left no trace.
    let granted = narrowed.pcie_driver as u32
        | ((narrowed.platform as u32) << 1)
        | ((narrowed.supervisor as u32) << 2)
        | ((narrowed.serial_port as u32) << 3);
    if granted != 0 {
        crate::audit::log_event(
            crate::audit::AuditEvent::PrivilegedCapGranted,
            &crate::audit::encode_u32x2(tid as u32, granted),
        );
    }
    // The network rights are exported to service-net as attested trailer flags;
    // record which ones this Cell leaves spawn holding.
    let net = narrowed.net_resolve as u32
        | ((narrowed.net_connect as u32) << 1)
        | ((narrowed.net_listen as u32) << 2)
        | ((narrowed.net_l2 as u32) << 3);
    if net != 0 {
        crate::audit::log_event(
            crate::audit::AuditEvent::NetRightsGranted,
            &crate::audit::encode_u32x2(tid as u32, net),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn net_rights(c: CapSet) -> (bool, bool, bool, bool) {
        (c.net_resolve, c.net_connect, c.net_listen, c.net_l2)
    }

    fn observe_mutate(c: CapSet) -> (bool, bool, bool) {
        (c.observe_tasks, c.observe_memory, c.vfs_mutate)
    }

    #[test]
    fn boot_self_test_holds_on_host() {
        assert!(v2_parse_cases());
        assert!(v3_parse_cases());
        assert!(v5_parse_cases());
        assert!(v6_parse_cases());
        assert!(v7_parse_cases());
        assert!(v8_parse_cases());
        assert!(self_test());
    }

    #[test]
    fn v7_entry_round_trips_config_write_and_rejects_malformed_bytes() {
        for q in [0u8, 1] {
            let mut body = V7_CURL;
            body[V7_CURL_Q] = q;
            let c = parse(&body).expect("v7 body with a literal 0|1 Q byte parses").entries[0].caps;
            assert_eq!(c.config_write, q == 1);
            // The v6 prefix is unaffected by the appended byte.
            assert_eq!(net_rights(c), (true, true, false, false));
            assert_eq!(observe_mutate(c), (true, false, true));
        }
        for value in [2u8, 0x80, 0xFF] {
            let mut bad = V7_CURL;
            bad[V7_CURL_Q] = value;
            assert!(parse(&bad).is_none(), "Q byte {value:#04x} must reject");
        }
        assert!(parse(&V7_CURL[..V7_CURL.len() - 1]).is_none());
        let mut bad_ver = V7_CURL;
        bad_ver[4] = 9;
        assert!(parse(&bad_ver).is_none());
    }

    /// D-Q6: an entry that predates v7 decodes Q as granted, so its Permit
    /// neither strips the shell's Q nor widens past the request ∩ ceiling.
    #[test]
    fn pre_v7_permit_leaves_config_write_unnarrowed() {
        let shell = CapSet::EMPTY.with_path_caps("/bin/cellbox");
        assert!(shell.config_write);
        for body in [&V6_CURL[..], &V5_CURL[..], &V4_CURL[..]] {
            let legacy = parse(body).expect("legacy body parses").entries[0].caps;
            assert!(legacy.config_write);
            assert!(
                decision_to_caps("/bin/cellbox", shell, PolicyDecision::Permit(legacy)).config_write,
                "a pre-v7 Permit must not strip Q from the shell"
            );
            let curl = CapSet::EMPTY.with_path_caps("/bin/curl");
            assert!(!decision_to_caps("/bin/curl", curl, PolicyDecision::Permit(legacy)).config_write);
        }
        // A v7 Permit with Q=0 does strip it.
        let v7 = parse(&V7_CURL).expect("v7 body parses").entries[0].caps;
        assert!(!decision_to_caps("/bin/cellbox", shell, PolicyDecision::Permit(v7)).config_write);
    }

    #[test]
    fn v6_entry_round_trips_every_observe_mutate_right() {
        for mask in 0u8..8 {
            let mut body = V6_CURL;
            for bit in 0..3 {
                body[V6_CURL_RIGHTS + bit] = (mask >> bit) & 1;
            }
            let p = parse(&body).expect("v6 body with literal 0|1 bytes parses");
            let c = p.entries[0].caps;
            assert_eq!(
                observe_mutate(c),
                (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0),
                "mask {mask:#05b}"
            );
            // The v5 prefix is unaffected by the appended bytes.
            assert_eq!(net_rights(c), (true, true, false, false));
            assert!(c.network && !c.block_io && !c.spawn && !c.serial_port);
            // A Permit narrows the reviewed request to exactly these rights.
            assert_eq!(
                observe_mutate(decision_to_caps(
                    "/bin/curl",
                    CapSet::LEGACY_OBSERVE_MUTATE,
                    PolicyDecision::Permit(c)
                )),
                observe_mutate(c)
            );
        }
    }

    #[test]
    fn pre_v6_permit_leaves_observe_mutate_unnarrowed() {
        for body in [&V5_CURL[..], &V4_CURL[..]] {
            let legacy = parse(body).expect("legacy body parses").entries[0].caps;
            assert_eq!(observe_mutate(legacy), (true, true, true));
            let request = CapSet {
                network: true,
                ..CapSet::EMPTY
            }
            .with_path_caps("/bin/curl");
            assert_eq!(observe_mutate(request), (true, true, true));
            assert_eq!(
                decision_to_caps("/bin/curl", request, PolicyDecision::Permit(legacy)),
                request,
                "a pre-v6 Permit must not strip an S2-T/S2-W right"
            );
            // …and it cannot widen past a sandbox request ∩ ceiling.
            let sandbox = CapSet::EMPTY.with_path_caps("/data/bin/tool");
            assert_eq!(
                decision_to_caps("/data/bin/tool", sandbox, PolicyDecision::Permit(legacy)),
                CapSet::EMPTY
            );
        }
    }

    #[test]
    fn malformed_observe_mutate_byte_rejects_the_policy() {
        for i in V6_CURL_RIGHTS..V6_CURL.len() {
            for value in [2u8, 0x80, 0xFF] {
                let mut bad = V6_CURL;
                bad[i] = value;
                assert!(parse(&bad).is_none(), "byte {i} = {value:#04x} must reject");
            }
        }
        assert!(parse(&V6_CURL[..V6_CURL.len() - 1]).is_none());
        let mut bad_ver = V6_CURL;
        bad_ver[4] = 9;
        assert!(parse(&bad_ver).is_none());
    }

    #[test]
    fn v5_entry_round_trips_every_network_right() {
        for mask in 0u8..16 {
            let mut body = V5_CURL;
            for bit in 0..4 {
                body[V5_CURL_NET + bit] = (mask >> bit) & 1;
            }
            let p = parse(&body).expect("v5 body with literal 0|1 net bytes parses");
            let c = p.entries[0].caps;
            assert_eq!(
                net_rights(c),
                (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0, mask & 8 != 0),
                "mask {mask:#06b}"
            );
            // The rest of the entry is unaffected by the appended bytes.
            assert!(c.network && !c.block_io && !c.spawn && !c.serial_port);
        }
    }

    #[test]
    fn v4_permit_leaves_network_rights_unnarrowed() {
        let legacy = parse(&V4_CURL).expect("v4 body parses").entries[0].caps;
        assert_eq!(net_rights(legacy), (true, true, true, true));
        // P-TRUST fields keep their opposite rule: absent → false.
        assert!(!legacy.serial_port && !legacy.pcie_driver);
        let request = CapSet {
            network: true,
            ..CapSet::EMPTY
        }
        .with_path_caps("/bin/curl");
        assert_eq!(
            decision_to_caps("/bin/curl", request, PolicyDecision::Permit(legacy)),
            request,
            "a v4 Permit must not strip a reviewed network right"
        );
        // …and it cannot widen past the request ∩ ceiling it is applied to.
        assert_eq!(
            decision_to_caps("/bin/curl", CapSet::EMPTY, PolicyDecision::Permit(legacy)),
            CapSet::EMPTY
        );
    }

    #[test]
    fn malformed_network_byte_rejects_the_policy() {
        for i in V5_CURL_NET..V5_CURL.len() {
            for value in [2u8, 0x80, 0xFF] {
                let mut bad = V5_CURL;
                bad[i] = value;
                assert!(parse(&bad).is_none(), "byte {i} = {value:#04x} must reject");
            }
        }
        assert!(parse(&V5_CURL[..V5_CURL.len() - 1]).is_none());
    }

    #[test]
    fn v8_entry_round_trips_both_kms_bytes_and_rejects_malformed_bytes() {
        for (sign, admin) in [(0u8, 0u8), (1, 0), (0, 1), (1, 1)] {
            let mut body = V8_CURL;
            body[V8_CURL_SIGN] = sign;
            body[V8_CURL_ADMIN] = admin;
            let c = parse(&body).expect("v8 body with literal 0|1 KMS bytes parses").entries[0].caps;
            assert_eq!((c.kms_ssh_sign, c.kms_ssh_admin), (sign == 1, admin == 1));
            // The v7 prefix is unaffected by the appended bytes.
            assert_eq!(net_rights(c), (true, true, false, false));
            assert_eq!(observe_mutate(c), (true, false, true));
            assert!(!c.config_write);
        }
        for i in [V8_CURL_SIGN, V8_CURL_ADMIN] {
            for value in [2u8, 0x80, 0xFF] {
                let mut bad = V8_CURL;
                bad[i] = value;
                assert!(parse(&bad).is_none(), "byte {i} = {value:#04x} must reject");
            }
        }
        for len in HEADER_LEN..V8_CURL.len() {
            assert!(parse(&V8_CURL[..len]).is_none(), "truncation to {len} bytes must reject");
        }
        // A v7 body relabelled v8 no longer fits the 21-byte stride.
        let mut v7_as_v8 = V7_CURL;
        v7_as_v8[4] = VERSION_V8;
        assert!(parse(&v7_as_v8).is_none());
        // The entry stride is exactly the two appended bytes.
        assert_eq!(V8_CURL.len() - V7_CURL.len(), CAP_BYTES_V8 - CAP_BYTES_V7);
        assert_eq!(V8_CURL_ADMIN, V8_CURL.len() - 1);
        assert_eq!(V8_CURL_SIGN, HEADER_LEN + 1 + "/bin/curl".len() + CAP_BYTES_V7);
        // Version 9 is unknown.
        let mut bad_ver = V8_CURL;
        bad_ver[4] = 9;
        assert!(parse(&bad_ver).is_none());
        assert_eq!(cap_bytes_for(8), Some(21));
        assert_eq!(cap_bytes_for(9), None);
    }

    /// O-11: an entry that predates v8 decodes both KMS SSH rights as `false`, so
    /// its Permit strips them (the rights were never ambient); a v8 byte of 1 keeps
    /// them and a v8 byte of 0 strips them; no Permit can widen a request.
    #[test]
    fn pre_v8_permit_strips_kms_ssh_rights_and_v8_bytes_decide() {
        let ssh = CapSet::EMPTY.with_path_caps("/bin/cellbox-ssh");
        let admin = CapSet::EMPTY.with_path_caps("/bin/kms-ssh-admin");
        assert!(ssh.kms_ssh_sign && !ssh.kms_ssh_admin);
        assert!(admin.kms_ssh_admin && !admin.kms_ssh_sign);
        for body in [&V7_CURL[..], &V6_CURL[..], &V5_CURL[..], &V4_CURL[..]] {
            let legacy = parse(body).expect("legacy body parses").entries[0].caps;
            assert!(!legacy.kms_ssh_sign && !legacy.kms_ssh_admin);
            assert!(
                !decision_to_caps("/bin/cellbox-ssh", ssh, PolicyDecision::Permit(legacy)).kms_ssh_sign,
                "a pre-v8 Permit must strip the sign right"
            );
            assert!(
                !decision_to_caps("/bin/kms-ssh-admin", admin, PolicyDecision::Permit(legacy))
                    .kms_ssh_admin,
                "a pre-v8 Permit must strip the admin right"
            );
        }
        // The opposite O-11 rule applies to the older ambient rights: unchanged.
        let v4 = parse(&V4_CURL).expect("v4 body parses").entries[0].caps;
        assert_eq!(net_rights(v4), (true, true, true, true));
        let mut granted = V8_CURL;
        granted[V8_CURL_SIGN] = 1;
        granted[V8_CURL_ADMIN] = 1;
        let on = parse(&granted).expect("v8 body parses").entries[0].caps;
        assert!(decision_to_caps("/bin/cellbox-ssh", ssh, PolicyDecision::Permit(on)).kms_ssh_sign);
        assert!(
            decision_to_caps("/bin/kms-ssh-admin", admin, PolicyDecision::Permit(on)).kms_ssh_admin
        );
        let off = parse(&V8_CURL).expect("v8 body parses").entries[0].caps;
        assert!(!decision_to_caps("/bin/cellbox-ssh", ssh, PolicyDecision::Permit(off)).kms_ssh_sign);
        assert!(
            !decision_to_caps("/bin/kms-ssh-admin", admin, PolicyDecision::Permit(off)).kms_ssh_admin
        );
        // A Permit never widens: the shell requests neither right and keeps neither.
        let shell = CapSet::EMPTY.with_path_caps("/bin/cellbox");
        let kept = decision_to_caps("/bin/cellbox", shell, PolicyDecision::Permit(on));
        assert!(!kept.kms_ssh_sign && !kept.kms_ssh_admin);
    }

    /// H-1: with a policy loaded, a path with no row holds neither KMS SSH right
    /// (like an explicit 0 row); with no policy at all nothing is tightened.
    #[test]
    fn loaded_policy_without_a_row_strips_kms_ssh_rights() {
        let request = CapSet {
            kms_ssh_sign: true,
            kms_ssh_admin: true,
            ..CapSet::LEGACY_OBSERVE_MUTATE
        };
        let loaded = PolicyDecision::NoEntry {
            policy_loaded: true,
        };
        let stripped = decision_to_caps("/bin/cellbox-ssh", request, loaded);
        assert!(!stripped.kms_ssh_sign && !stripped.kms_ssh_admin);
        // A fail-closed deny never leaves the rights either.
        assert_eq!(
            decision_to_caps("/bin/cellbox-ssh", request, PolicyDecision::DenyAll),
            CapSet::EMPTY
        );
        #[cfg(not(feature = "policy-required"))]
        {
            // Only the two rights go; every other cap survives the gap.
            assert_eq!(stripped, CapSet::LEGACY_OBSERVE_MUTATE);
            let absent = PolicyDecision::NoEntry {
                policy_loaded: false,
            };
            assert_eq!(decision_to_caps("/bin/cellbox-ssh", request, absent), request);
            // The P-TRUST strip of the same arm is unchanged.
            let driver = CapSet {
                pcie_driver: true,
                ..request
            };
            let s = decision_to_caps("/bin/nvme", driver, loaded);
            assert!(!s.pcie_driver && !s.kms_ssh_sign && !s.kms_ssh_admin);
        }
    }

    /// The `CapNarrowedByPolicy` payload carries the two KMS SSH rights at bits 17
    /// and 18 and leaves every earlier bit where it was.
    #[test]
    fn narrowing_audit_bits_report_the_kms_ssh_rights() {
        assert_eq!(narrowed_bits(CapSet::ALL, CapSet::ALL), 0);
        let sign = CapSet {
            kms_ssh_sign: true,
            ..CapSet::EMPTY
        };
        let admin = CapSet {
            kms_ssh_admin: true,
            ..CapSet::EMPTY
        };
        assert_eq!(narrowed_bits(sign, CapSet::EMPTY), 1 << 17);
        assert_eq!(narrowed_bits(admin, CapSet::EMPTY), 1 << 18);
        assert_eq!(narrowed_bits(sign, sign), 0);
        assert_eq!(narrowed_bits(CapSet::ALL, CapSet::EMPTY) >> 17, 0b11);
        let config = CapSet {
            config_write: true,
            ..CapSet::EMPTY
        };
        assert_eq!(narrowed_bits(config, CapSet::EMPTY), 1 << 16);
        // A right the request never held cannot be reported as dropped.
        assert_eq!(narrowed_bits(CapSet::EMPTY, sign), 0);
    }

    /// `scripts/sign-policy.py` output verifies under the dev key and decodes with
    /// the kernel's own parser, and every listed path's network, S2-T/S2-W, Q and
    /// KMS SSH rights equal what the kernel's `with_path_caps` requests — so the
    /// shipped v8 Permit neither strips a granted right nor names one the kernel
    /// never requests. Only `/bin/cellbox-ssh` carries the sign right and no row
    /// carries the admin right or both.
    #[test]
    fn signer_v8_output_decodes_with_kernel_rule() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let out = std::env::temp_dir().join(format!("cellos-policy-v8-{}.bin", std::process::id()));
        let status = std::process::Command::new("python3")
            .arg(repo.join("scripts/sign-policy.py"))
            .arg("--out")
            .arg(&out)
            .stdout(std::process::Stdio::null())
            .status()
            .expect("python3 must be available to run the signer");
        assert!(
            status.success(),
            "sign-policy.py failed (needs `cryptography`)"
        );
        let blob = std::fs::read(&out).expect("signer wrote the blob");
        let _ = std::fs::remove_file(&out);

        let (body, sig) = blob.split_at(blob.len() - SIG_LEN);
        let mut sig64 = [0u8; SIG_LEN];
        sig64.copy_from_slice(sig);
        assert!(crate::ed25519::verify(&DEV_FLEET_PUBKEY, body, &sig64));
        assert_eq!(body[4], VERSION_V8, "the signer must emit v8");
        let policy = parse(body).expect("kernel parser accepts the signed v8 body");
        assert!(!policy.entries.is_empty());
        for e in &policy.entries {
            let requested = CapSet::EMPTY.with_path_caps(&e.path);
            assert_eq!(
                net_rights(e.caps),
                net_rights(requested),
                "{}: policy network rights drift from the kernel grant table",
                e.path
            );
            assert_eq!(
                observe_mutate(e.caps),
                observe_mutate(requested),
                "{}: policy S2-T/S2-W rights drift from the kernel request rule",
                e.path
            );
            assert_eq!(
                e.caps.config_write,
                requested.config_write,
                "{}: policy Q right drifts from the kernel request rule",
                e.path
            );
            assert_eq!(
                (e.caps.kms_ssh_sign, e.caps.kms_ssh_admin),
                (requested.kms_ssh_sign, requested.kms_ssh_admin),
                "{}: policy KMS SSH rights drift from the kernel request rule",
                e.path
            );
            assert!(
                !(e.caps.kms_ssh_sign && e.caps.kms_ssh_admin),
                "{}: sign and admin must never be held by one Cell",
                e.path
            );
        }
        let holders: Vec<&str> = policy
            .entries
            .iter()
            .filter(|e| e.caps.kms_ssh_sign)
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(holders, ["/bin/cellbox-ssh"]);
        assert!(policy.entries.iter().all(|e| !e.caps.kms_ssh_admin));
        let curl = policy
            .entries
            .iter()
            .find(|e| e.path == "/bin/curl")
            .expect("curl row");
        assert_eq!(net_rights(curl.caps), (true, true, false, false));
        assert!(!curl.caps.config_write && !curl.caps.kms_ssh_sign);
        let shell = policy
            .entries
            .iter()
            .find(|e| e.path == "/bin/cellbox")
            .expect("shell row");
        assert_eq!(net_rights(shell.caps), (false, false, false, false));
        assert_eq!(observe_mutate(shell.caps), (true, true, true));
        assert!(shell.caps.config_write);
        assert!(!shell.caps.kms_ssh_sign);
        let ssh = policy
            .entries
            .iter()
            .find(|e| e.path == "/bin/cellbox-ssh")
            .expect("cellbox-ssh row");
        assert!(!ssh.caps.config_write);
        assert!(ssh.caps.kms_ssh_sign);
        // The signer keeps two literal copies of the stride table (`CAP_BYTES` and
        // the independent decoder's own); both must have gained v8, and its
        // request helpers must name the same paths as `with_path_kms_ssh_rights`.
        let script = std::fs::read_to_string(repo.join("scripts/sign-policy.py"))
            .expect("signer source is readable");
        assert_eq!(script.matches("7: 19, 8: 21}").count(), 2, "both stride tables");
        assert!(script.contains("KMS_SSH_SIGN_PATHS = (\"/bin/cellbox-ssh\",)"));
        assert!(script.contains("KMS_SSH_ADMIN_PATHS = (\"/bin/kms-ssh-admin\",)"));
    }
}
