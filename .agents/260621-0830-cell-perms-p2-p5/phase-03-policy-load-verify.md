---
phase: 03
title: Signed policy load + verify at boot (P5b)
tier: thinking
depends: [02]
status: planned
---

# Phase 03 — Signed policy load + verify at boot (P5b)

> **Revised post-red-team (2026-06-21):** fail-CLOSED on absent (C3); `/POLICY.BIN` 8.3-safe path (M);
> bake into ALL embedded images + assert PolicyLoaded (M); verify-then-parse + panic-free parser (M);
> eager-load only — lazy option deleted (M); domain-validate parsed CapSet (M); explicit audit
> discriminants (Minor). Confirmed: VIFS1 is kernel-embedded, no circular dep (POSITIVE).

## Context Links
- Design: [research-cell-security-permissions.md](../../docs/research/research-cell-security-permissions.md) §2.5, §3.4
- Depends: Phase 02 (`ed25519::verify`, or the unsigned-G1 fork).
- VIFS1 = `kernel_fs.img`, `include_bytes!`-embedded FAT16 ([ramdisk.rs:10](../../kernel/src/task/drivers/ramdisk.rs#L10)), mounted by `fs::init()` ([main.rs:420](../../kernel/src/main.rs#L420)) BEFORE init spawn ([main.rs:455](../../kernel/src/main.rs#L455)).

## Overview
At boot the kernel reads a signed operator policy blob from VIFS1, **verifies the signature first**,
then parses the trusted body into a policy table (`cell_path → CapSet`), exposed via `lookup` for
Phase 04. Absent/invalid follows an explicit **fail-safe rule** (fail-closed for P5 builds).

## Key Insights (red-team-corrected)
- **C3 — absence must FAIL-CLOSED in any P5 build.** An attacker can't forge a signature, so they
  **delete the blob** → if absent⇒permit, every cell boots full-caps (signature defeated by deletion).
  Dev convenience (`absent⇒permit`) lives behind `#[cfg(feature = "dev-permissive")]` that a release
  artifact **cannot** enable — NOT a runtime `policy_required` bool defaulting insecure.
- **Verify-then-parse (M).** The trailing 64 bytes are the sig; verify over `blob[..len-64]` using
  only `total_len` (no field parsing) FIRST. Only parse the now-trusted body. A parse-before-verify
  feeds attacker-controlled lengths to the parser at boot → a panic = **no boot** (headless brick).
- **Panic-free parser (M).** Bounds-check every field read (the `CellManifest::from_bytes` discipline
  — [manifest.rs:132](../../libs/api/src/manifest.rs#L132)); never `unwrap`/index-panic; malformed ⇒
  `PolicyInvalid`. Fuzz host-side as a HARD gate, not a "mitigation".
- **8.3-safe path (M).** `fs::read_file_from_vifs1` uppercases ([fs.rs:25](../../kernel/src/fs.rs#L25))
  and it's FAT16 (8.3). Use `/POLICY.BIN` at root (not `/etc/fleet-policy.bin`). Verify the disk-gen
  tool actually places it.
- **No circular dep (POSITIVE).** VIFS1 is kernel-embedded; reading policy needs neither the vfs cell
  nor any policy-gated spawn. Eager load right after `fs::init()` is sound. **Lazy-load is deleted** —
  it would run VIFS1 I/O under spawn-path locks and `VIFS1.lock()` is not in the fault-path
  `force_unlock` list ([task.rs:192](../../kernel/src/task.rs#L192)).

## Requirements
- F1: Read `/POLICY.BIN` from VIFS1 **eagerly**, called from `main.rs` right after `fs::init()` and
  before the init spawn. (Same on all three arches — confirm x86_64 VIFS1 is populated; cap behavior
  must not diverge by arch.)
- F2: `ed25519::verify(&FLEET_ROOT_PUBKEY, &blob[..len-64], &blob[len-64..])` (or the unsigned-G1 fork
  from Phase 02), THEN bounds-checked parse → `static POLICY: Spinlock<Option<PolicyTable>>`.
- F3: **Domain-validate** each parsed entry: mask `mmio_devices` to known `DEV_*`, `block_regions` to
  `0b111`; reject (`PolicyInvalid`) on unknown bits — a signed-but-malformed policy is still rejected.
- F4: `lookup(path) -> PolicyDecision { Permit(CapSet) | DenyAll | NoEntry }`.
- F5: Audit events with **explicit discriminants 16–18**: `PolicyLoaded = 16`, `PolicyInvalid = 17`,
  `PolicyAbsent = 18` (enum tops at `CellMeasure = 15`; ring is a logged wire format).
- F6: `force_unlock_locks()` for the POLICY spinlock, added to the [task.rs:192](../../kernel/src/task.rs#L192) teardown list.

## Architecture
`kernel/src/policy.rs`:
```
struct PolicyEntry { path: String, caps: CapSet }
struct PolicyTable { entries: Vec<PolicyEntry> }   // small N; linear lookup is fine
static POLICY: Spinlock<Option<PolicyTable>> = …;
#[cfg(not(feature="dev-policy-key"))] const FLEET_ROOT_PUBKEY: [u8;32] = include!(prod key);
#[cfg(feature="dev-policy-key")]      const FLEET_ROOT_PUBKEY: [u8;32] = DEV_PUBKEY;
pub fn load_from_vifs1();              // verify-then-parse; sets POLICY
pub fn lookup(path: &str) -> PolicyDecision;
pub unsafe fn force_unlock_locks();
```
Blob (little-endian): `magic u32 "VPOL" | version u8 | flags u8 | entry_count u16 |`
`{ path_len u8, path bytes, block_io u8, network u8, spawn u8, hyp u8, mmio_devices u8, block_regions u8 } × N | sig [u8;64]`.
Signature covers all bytes before the 64-byte sig.

**Fail-safe rule** (Validation decision 3: **G1 ships with `dev-permissive` ON** — the fleet-secure,
fail-closed-on-absent posture is the `dev-permissive` OFF column, enabled by a flag for real fleets.
**Invalid sig/parse ALWAYS fail-closed**, both postures):
| State | Fleet-secure (`dev-permissive` OFF) | G1 default (`dev-permissive` ON) |
|-------|-------------------------------|---------------------------|
| Absent | **fail-closed** (DenyAll-equivalent at lookup) | permit (manifest∩spawner) |
| Invalid sig/parse | **fail-closed** | **fail-closed** (always) |
| `policy_verify_bypass` feature | n/a (release can't enable) | load+parse, skip sig (Phase 02 unsigned-G1 fork / crypto-regression recovery) |

## Related Code Files
**Create**
- `kernel/src/policy.rs` — table, pubkey (cfg-split), verify-then-parse, domain-validate, lookup, force_unlock.
- `scripts/sign-policy.py` (or `tools/policy-sign/`) — host tool: human source (TOML/JSON) → binary blob → Ed25519-sign with dev key; emits dev pubkey to paste into the `dev-policy-key` cfg.
- A dev `/POLICY.BIN` baked into **every** boot-tested VIFS1 image.
**Modify**
- `kernel/src/main.rs` — `pub mod policy;` + `policy::load_from_vifs1()` after `fs::init()`, before init spawn.
- `kernel/src/task.rs` — add `policy::force_unlock_locks()` to teardown list.
- `kernel/src/audit.rs` — `PolicyLoaded=16 / PolicyInvalid=17 / PolicyAbsent=18`.
- **ALL embedded image generators** (`kernel/src/embedded/`, `embedded-aarch64/`, `embedded-test-hooks/`, `embedded-x86_64/` + their build.rs / gen scripts) — include `/POLICY.BIN`. The **test-hooks** image drives `run-tests.ps1 boot` — if the blob is absent there, the boot smoke runs the absent path and tests pass **vacuously**.

## Implementation Steps
1. Blob format + `scripts/sign-policy.py` (dev keypair; emit dev pubkey).
2. `policy.rs`: verify-then-parse (`blob[..len-64]` first), panic-free bounds-checked parse,
   domain-validate, `PolicyTable`, lookup, fail-safe rule, audit (16-18), force_unlock.
3. Bake dev `/POLICY.BIN` into ALL boot-tested images (esp. test-hooks).
4. Wire eager `load_from_vifs1()` into boot ordering (after fs::init, before init spawn).
5. Build both arches.
6. Boot-verify: log shows **`PolicyLoaded`**; `ViCell >`; no faults. Boot test **asserts `PolicyLoaded`**
   (not merely "no fault") so an absent/vacuous blob FAILS the test.
7. Negative: corrupt the blob → `PolicyInvalid` + fail-closed (services capless, kernel does NOT panic
   and does NOT silently permit). Host-fuzz the parser on signed-malformed inputs.

## Todo
- [ ] blob format + host signer + dev keypair
- [ ] `policy.rs` verify-then-parse, panic-free, domain-validate, lookup, fail-safe, audit 16-18, force_unlock
- [ ] dev `/POLICY.BIN` in ALL boot-tested images (incl test-hooks)
- [ ] eager load wired (after fs::init, before init spawn); x86_64 VIFS1 confirmed populated
- [ ] build both arches
- [ ] boot smoke ASSERTS PolicyLoaded + `ViCell >` no faults
- [ ] negative: tampered → PolicyInvalid + fail-closed, no panic; parser fuzzed

## Success Criteria
- Valid dev policy verifies (sig over body) + parses + domain-validates; `lookup` correct.
- Tampered/absent follows fail-safe table (fail-closed on invalid always; absent fail-closed in P5 builds).
- Boot test asserts PolicyLoaded (no vacuous pass). Both arches build; boot clean; parser panic-free.

## Risk Assessment
| Risk | Mitigation |
|------|-----------|
| Blob absent from test-hooks image → vacuous tests | bake into ALL images; assert PolicyLoaded |
| Parse-before-verify panic at boot → brick | verify `blob[..len-64]` first; panic-free parser; fuzz gate |
| `/etc/fleet-policy.bin` mangled by FAT 8.3/uppercase → NotFound | `/POLICY.BIN` root path; verify gen-disk places it |
| Crypto false-negative on valid policy → fail-closed brick | Phase 02 round-trip test; `policy_verify_bypass` dev/maintenance path |
| Dev key trusted in release | `dev-policy-key` feature (not debug_assertions); CI pubkey-compare |
| Arch divergence (x86 no EarlyLoader) | policy is VIFS1 (not bootstrap table); confirm x86 VIFS1 populated |

## Security Considerations
- Verifying key in kernel TCB (cfg-split prod/dev), not in mutable VIFS1 data. Absent⇒fail-closed in
  P5 builds defeats the delete-the-blob downgrade. Domain validation rejects signed-but-malformed caps.

## Next Steps
Phase 04 folds `lookup` into the spawn-time CapSet + adds the recovery hatch.
