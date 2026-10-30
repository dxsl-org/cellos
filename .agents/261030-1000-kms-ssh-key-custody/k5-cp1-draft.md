# KSSH K5 CP1 DRAFT: KMS SSH custody behaviour, T0 volatile (status AWAITING CP1; documents only; nothing applied)

Written 2026-10-30 at HEAD `fad076ee9`. Documents only: no source, `Cargo.toml`/`Cargo.lock`, key, network fetch, VM, lock or image change was made to produce it. Everything cited as `file:line` was read in this session; `[INFERENCE]` marks anything not observed in a file or command output; `OPEN (decider)` marks a decision for the named owner. Numbers marked "computed" were produced in this session by python3 (`cryptography`/OpenSSL, `bcrypt`, `hashlib`) over **public** test fixtures and RFC vectors; no key was generated and no secret is written here.

Inputs read: `plan.md`, `k2-cp1.md` (Law-1 confirmation 1 text), `k2-cp1-approval.md`, `k2-cp2-approval.md`, `reports/k2-cp2-dossier.md`, `k1-cp1-draft.md`, `reports/k1-dependency-evidence.md` (+ `k1-proposed-cargo-lock.diff`, `k1-kms-manifest-own-parser.diff`, `k1-evidence-src/probe-ownmin/main.rs`), `contract-delta-import-surface.md`, `lock-adoption-draft.md`, `lock-handoff-k2-cp2.md` (+ approval), probe `kms-prerequisites.md`; `cells/services/kms/src/{main,lib,auth,dispatch,reply,storage}.rs`, `dispatch/relay.rs`, `lifecycle/pending.rs`, `Cargo.toml`, `build.rs`; `libs/types/src/kms/{model,ssh,payload/ssh}.rs`; `libs/api/src/abi/caller_identity.rs`; `libs/ostd/src/{heap.rs,clients/kms.rs,ipc.rs}`; `kernel/src/task.rs:151-156,675-686`, `kernel/src/task/scheduler.rs:64-81,2300-2360`; registry sources `ed25519-dalek-3.0.0`, `ssh-key-0.7.0-rc.11` (+ `tests/examples`), `cpufeatures-0.3.1`, scratch `bcrypt-pbkdf-0.11.0`; `.agents/260826-1605-phase4-dev-reference-authority/phase-06-*.md`, `cell-contract-0024-locks.md`.

## 0. Summary

| Item | Proposal |
|---|---|
| What | Real behaviour behind opcodes 15-24: chunked import, own OpenSSH-v1 parser with structural pre-KDF allowlist, per-login unlock bounded to one signature, RFC 4252 reconstruction, ed25519 sign + self-verify, replay floors, limiter N = 5, caller classes replacing the K2 `SshKeyUnavailable` arms, zeroization. Volatile (T0, `DevPostureVolatile`), one identity per store (LR-1 = Y) |
| Where | new `cells/services/kms/src/ssh.rs` + `ssh/*.rs` + `dispatch/ssh.rs` + `tests/ssh/**`; shared edits in `Cargo.toml`, `Cargo.lock`, `lib.rs`, `build.rs`, `auth.rs`, `dispatch.rs`, `tests.rs`, `main.rs` (§3) |
| Dependencies | the K1-approved 3 new lock packages (`bcrypt-pbkdf 0.11.0`, `blowfish 0.10.0`, `pbkdf2 0.13.0`), manifest `blowfish{zeroize,bcrypt}`, `aes 0.9{zeroize}`, `ctr 0.10{zeroize}`, `zeroize 1`, `ed25519-dalek 3.0{zeroize}`; **one proposed amendment: drop `base64ct`** (KMS never receives PEM; §4) |
| Gating | opt-in Cargo feature `ssh-dev-custody` (default off, `compile_error!` with `hardware-relay-provider`, `build.rs` refuses `CELLOS_PRODUCTION`): the default KMS build stays byte-identical in behaviour to the K2 baseline (D-5.1) |
| Biggest open measurements | KDF stall on the single-threaded loop (inference only so far, §11); AES backend selection on `x86_64-unknown-none` (not checked, §19) |
| Effort | L, about 3 weeks of focused work for one integrating owner (§17) |
| Entry gates | K3 landed the `CALLER_FLAG_KMS_SSH_SIGN` constant (libs/api); K5 lock handoff approved; K5 CP1 approved; scratch availability of the 3 crates (§2) |

## 1. Scope and non-scope

**In:** all KMS-side behaviour for opcodes 15-24 at T0; the D-K4 parser; the D-K5 unlock; ed25519 signing with reconstruct and self-verify; replay floors (O-4 = 4, evict oldest); limiter (O-5 = 5); admission classes; zeroization; host tests, vectors, fuzz targets, a KDF measurement plan; the K1 manifest/lock application; the K5 lock handoff (§14); a persistence **seam** (`SshPersist`, §5.3) so K4b plugs in without editing K5 logic.

**Out:** persistence (K4b); the kernel flag itself (K3, `libs/api`); client/`cellbox` code (K6); the admin import surface (K7; K5 only needs an admission seam, §6); QEMU evidence (K8); any ABI change (opcodes 1-24, errors 1-38 are final; `libs/types` is not touched); production claims; `command-contracts.md`.

## 2. Entry gates

| Gate | State today | Needed for |
|---|---|---|
| G-1 K5 CP1 approved (this text after revision) and its Law-1-adjacent readings: none new (no ABI change) | pending | any code |
| G-2 lock handoff §14 approved by the accountable owner; start-of-slice overlap re-check against `260826-1605` Phase 06 file list (§14 C-K5-1) | pending | any edit under `cells/services/kms/**` or `Cargo.lock` |
| G-3 K3 has landed `CALLER_FLAG_KMS_SSH_SIGN` in `libs/api/src/abi/caller_identity.rs` (Law-1 two confirmations are K3's) | not landed: bits 6..31 reserved, `caller_identity.rs:45-75` | K5 CP2 (production Login class). CP1-level host tests may use the literal `1 << 6` |
| G-4 the 3 new crates are available offline. Registry cache today has none (K1 evidence §2); copies with matching digests exist only in the scratch `/tmp/kssh-k1/cargo-home/registry/cache` (`144e5737…`, `62ce3946…`, `112d82ce…` verified this session). A fresh online fetch of the same three `.crate` files, or a digest-verified copy, needs an explicit authorization at K5 CP2 (K1's authorization covered evidence only) | OPEN (decider: user) | `cargo build/test --locked --offline` |
| G-5 baseline capture before edit: `cargo test --offline --locked -p service-kms --target x86_64-unknown-linux-gnu` (65 passed at K2 CP2, `reports/k2-cp2-dossier.md` §3) and `cargo deny check` diagnostic list (red baseline, K1 §5) | pending | acceptance "no new diagnostics" |
| G-6 CI wiring for the feature: `.github/workflows/ci.yml` must run `cargo test -p service-kms --features ssh-dev-custody` (outside the closure; owner follow-up) | pending | CI proof |

## 3. Exact file closure (pre-edit SHA-256 at HEAD `fad076ee9`)

| Path | Action | SHA-256 before |
|---|---|---|
| `cells/services/kms/Cargo.toml` | edit: optional deps, features `ssh-dev-custody`, (K4b: `ssh-dev-persist`) | `3888e6bf8320fb6d69be60f3e26ecd6ae3aab1e664572e602656e8cfa6b2d167` |
| `Cargo.lock` | edit: exactly the amended K1 diff (§4); shared by the whole workspace | `02d35d446b90fe569fab50d721a634258d6bb733ea789c45ddc8534ee8ca555a` |
| `cells/services/kms/build.rs` | edit: refuse `ssh-dev-custody` under `CELLOS_PRODUCTION` | `3faaa0787f994857bfeb7221e9f7e88e6b99e923ab3e41889ff7e16b4669af14` |
| `cells/services/kms/src/lib.rs` | edit: `mod ssh` (cfg), two `compile_error!` | `3f5c7f0c554a8949c41b81465c908111345d9b3cc9afb111b1bf80708609d49f` |
| `cells/services/kms/src/auth.rs` | edit: public `attested_caller`, SSH class helpers | `174690fb9da5c2b5c061e6a776d70644ca4418bc01aadac6cb1d0f76b412ebe9` |
| `cells/services/kms/src/dispatch.rs` | edit: `ssh: SshLeaf` field, arm bodies call `dispatch/ssh.rs`; K2 stub arm kept only for the feature-off build | `2c19b290c252b3d850eae71035aac35a05dedc6ce605897bd8f9a8a38b586b06` |
| `cells/services/kms/src/tests.rs` | edit: `mod ssh;` | `c21a6ab725c3afd436165f1c7d0853b0bfa85681402f202e2b05ed9d630b487b` |
| `cells/services/kms/src/main.rs` | edit: one boot banner line under the feature (marker for K8/production scan; no key data) | `3cce19d7fefee434367293498e316c9a840bdea7bee8487273c9f507efc22010` |
| `cells/services/kms/src/dispatch/ssh.rs` | new: ten handlers | ABSENT |
| `cells/services/kms/src/ssh.rs` | new: module root, `SshLeaf`, `SshPersist` seam | ABSENT |
| `cells/services/kms/src/ssh/{limits,parser,unwrap,store,import,unlock,sign,floor}.rs` | new | ABSENT |
| `cells/services/kms/src/tests/ssh.rs` + `tests/ssh/{fixtures,admission,parser,import,unlock_sign,store,vectors,zeroize,mutation,stack}.rs` | new | ABSENT |
| `fuzz/Cargo.toml` | edit: one `[[bin]]` row (own `[workspace]` at `fuzz/Cargo.toml:28`, own lock; precedent `fuzz/src/vfs_path.rs`). Optional (D-5.12) | `611124b5c94c3011220198adebc63a08b11311ee56466f719ff7cbdcc6325b1d` |
| `fuzz/Cargo.lock` | unchanged if `libfuzzer-sys` is already locked (it is: `fuzz/Cargo.toml:23` dependency) | `8becea3f6fe3d9458da1f45fa21f9a398e2ca529e33c61f9eb33b28ec252bfd2` |
| `fuzz/src/kms_ssh_parser.rs` | new (optional) | ABSENT |

All "before" digests above were printed by `sha256sum` in this session (HEAD `fad076ee9`, clean tree); re-take them in the baseline step (G-5) because other plans may edit `plan.md`-class files but must not touch these.

Not in the closure: `libs/types/**` (ABI final), `libs/api/**` and `kernel/**` (K3), `libs/ostd/**` and `cells/tools/cellbox/**` (K6), `scripts/**`, `config/services.toml`, image/signing paths, `storage*`, `provider*`, `lifecycle/**`, `dispatch/{relay,enrollment,node_identity}.rs` (Phase 06 surface: untouched on purpose). Any other path discovered mid-slice stops the slice (Phase 02 A1 pattern).

## 4. Dependencies, manifest and lock

Approved by K1 CP1 (`plan.md` decisions log): D-K4 = own parser; lock diff = three packages; **applied only with K5**.

Manifest (K1 `k1-kms-manifest-own-parser.diff`, with the changes marked):

```
bcrypt-pbkdf = { version = "0.11", default-features = false, features = ["zeroize"], optional = true }
blowfish     = { version = "0.10", default-features = false, features = ["zeroize", "bcrypt"], optional = true }
aes          = { version = "0.9",  default-features = false, features = ["zeroize"], optional = true }
ctr          = { version = "0.10", default-features = false, features = ["zeroize"], optional = true }
zeroize      = { version = "1",    default-features = false, optional = true }
ed25519-dalek= { version = "3.0",  default-features = false, features = ["zeroize"], optional = true }
# base64ct: DROPPED (K1 diff had it): import carries binary blobs (k2-cp1.md section 1.3 "binary, not armored"); an unused crate is dead weight
[features]
ssh-dev-custody = ["dep:bcrypt-pbkdf","dep:blowfish","dep:aes","dep:ctr","dep:zeroize","dep:ed25519-dalek"]
```

* Optional dependencies are still recorded in `Cargo.lock`, so the lock diff is independent of gating.
* **Lock diff = `reports/k1-proposed-cargo-lock.diff` minus the single line `+ "base64ct",`** in the `service-kms` stanza (the three `[[package]]` blocks are unchanged). It applies cleanly to today's `Cargo.lock` (`patch --dry-run` on a scratch copy this session). K1 acceptance 1 said "three packages, no other line changes"; the dropped line only removes an edge, but it is a **deviation from the approved wording** (D-5.6). To be re-derived with `cargo update --workspace --offline` in a scratch copy at CP2 and compared byte for byte.
* `sha2` stays the existing direct `sha2 0.10` for SHA-256 (blob digest, `message_sha256`, fingerprint). `bcrypt-pbkdf` brings `sha2 0.11.1` itself; both versions already coexist in the workspace (K1 §1).
* **ed25519-dalek 3.0.0 vs 2.2.0.** Choose **3.0.0**: it is the version in the K1-approved diff, Sunset (the verifying counterpart in `cellbox`) already links it (`third_party/sunset/Cargo.toml:34`), and `default-features = false` drops `precomputed-tables` (`Cargo.toml [features] fast`). 2.2.0 would save nothing (bcrypt-pbkdf already needs `sha2 0.11`) and would add a third dalek-era surface to KMS. API facts used (registry source, read): `SigningKey::from_bytes(&[u8;32])` (`signing.rs:115`, stores the seed, derives and keeps the verifying key), `Signer::sign` (re-exported, `lib.rs:294`), `VerifyingKey::verify_strict` (`verifying.rs:367`, rejects small-order R/A), `Drop` zeroizes `SigningKey` (`signing.rs:719-726`) and `ExpandedSecretKey` (`hazmat.rs:68-76`), signing is deterministic (no RNG on this path).
* No `ed25519-compact` or `ssh-key` dev-dependency (D-5.4): either would add lock edge lines beyond the approved diff; `ssh-key` with `encryption` adds `bcrypt-pbkdf`/`rand_core 0.10.1` edges to `ssh-key` and `aead 0.6.1`/`ctutils` edges to `ssh-cipher` (`reports/k1-lock-diff-sshkey-encryption.diff`) and `deny.toml [graph]` has no `exclude-dev`, so dev edges are in the deny graph [INFERENCE: the 5 `duplicate` warnings K1 measured for the normal-dependency variant would then appear].
* Acceptance (K1): lock diff as above; `cargo deny check` shows no new diagnostics against the red baseline (7 advisories, 2 license rejections, 4 wildcards); licenses MIT OR Apache-2.0; `x86_64-unknown-none` build with `-Z build-std=core,alloc` succeeds and `.text` growth is recorded; no `ssh-key` encryption, no chacha20-poly1305/AES-GCM compiled into KMS.

## 5. Module layout

```
cells/services/kms/src/
  ssh.rs                 SshLeaf (state), LeafState, SshPersist seam, re-exports      [cfg(feature="ssh-dev-custody")]
  ssh/limits.rs          constants (§5.2)
  ssh/parser.rs          pure OpenSSH-v1 outer/inner parser; depends on core only
  ssh/unwrap.rs          KDF + AES-256-CTR + inner validation -> Seed (Zeroizing)
  ssh/store.rs           StoredIdentity (one), custody, status facts, store_revision
  ssh/import.rs          ImportSlot state machine + staging arena
  ssh/unlock.rs          UnlockSlot state machine + failure limiter
  ssh/sign.rs            RFC 4252 reconstruction, sign, self-verify
  ssh/floor.rs           FloorTable (per-principal replay ids)
  dispatch/ssh.rs        ten handlers: admission -> decode -> floor -> state -> reply
  auth.rs                + attested_caller(), SshClass, classify_ssh()
```

### 5.1 Visibility and fuzzability

`parser.rs` and `limits.rs` depend only on `core` (no `types`, no `KmsErrorCode`): the parser returns its own `Reject` enum (reasons exist for tests only and never reach a reply; the handler maps every reason to `SshKeyFormatRejected`, or to `SshStoreFull` for the one size-budget reason). This lets the fuzz crate include the file with `#[path = "../../cells/services/kms/src/ssh/parser.rs"] mod parser;` without exposing any API from the cell or a `test-hooks` export (that feature is production-forbidden: `scripts/check-production-relay-image.py` `KMS_FORBIDDEN`). `parser.rs` carries `#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]`: every access goes through `get`/`checked_*`; no allocation is ever sized from a declared length.

### 5.2 Constants (`limits.rs`; all but the last two are already frozen in `types::kms`)

`KMS_SSH_IMPORT_MAX_LEN 65,536`, `KMS_SSH_CHUNK_LEN 96`, `KMS_SSH_SECRET_MAX 1,024`, name 64, username 31, session 32, key 32, signature 64 (`libs/types/src/kms/ssh.rs:11-26`); K5-local: `KDF_ROUNDS 1..=64`, `KDF_SALT 16..=64`, `STORE_WRAPPED_CT_MAX 2,048` (O-6, D-5.2), `FAIL_LIMIT 5`, `FLOOR_SLOTS 4`.

### 5.3 State and the persistence seam

```
struct SshLeaf {
    state: LeafState,            // Serving | Unavailable | NotLoaded | Repairable(ProviderError|PolicyMismatch). T0 uses only Serving (and Unavailable when construction/arena fails); the other variants exist so K4b adds no K5 logic (row 5)
    identity: Option<StoredIdentity>,   // exactly one (LR-1 = Y)
    revision: u64,               // = status store_revision; +1 on import and delete; 0 at boot (T0)
    import: ImportSlot,          // Idle | Receiving{..} | Complete{..}
    unlock: UnlockSlot,          // Empty | Collecting{..} | Staged{..}
    floors: FloorTable,          // 4 x {cell_id, generation, floor, seq}
    handle_counter: u64,         // never wraps: checked_add else LeafState::Unavailable
    failures: u8,                // consecutive SshUnlockFailed for the one key
    persist: Persister,          // cfg: NoPersist (T0) | K4b's file persister
}
enum Stored { Written, VolatileOnly }          // VolatileOnly: this persister did not (or may not) write the record (T0; K4b for a Plain key)
trait SshPersist {
    fn load(&mut self) -> Loaded;                                   // Empty | Loaded(record) | Corrupt | RollbackDetected | IoError
    fn store(&mut self, rec: &RecordView) -> Result<Stored, PersistError>;   // next revision is rec.revision
    fn tombstone(&mut self, next_revision: u64) -> Result<Stored, PersistError>;
}
```


Lazy load: handler step (4) (§6) calls `persist.load()` once while the state is `NotLoaded` and maps the result to `Serving` (Empty/Loaded), `Repairable(..)` (Corrupt, RollbackDetected) or stays `NotLoaded` (IoError: the opcode answers `SshKeyUnavailable`, the next opcode retries). `NoPersist` is constructed `Serving`, so T0 never observes `NotLoaded`.
`NoPersist` returns `Loaded::Empty` and `Ok(Stored::VolatileOnly)`. A test fake can fail at any call, which lets K5 prove **all-or-nothing ordering now** (§8: persist before the RAM swap; failure leaves store, revision and floor unchanged and replies `PersistFailed` = existing code 11) so K4b only supplies the real persister (`k4b-cp1-draft.md`). The reported `posture` is derived from `Stored` (`Written` -> `DevPosturePersistent`, else `DevPostureVolatile`). `Persister` is a `cfg`-selected type alias, not a `dyn` (no `Box<dyn>` in a heap-constrained cell).

## 6. Caller admission (replaces the K2 arms)

K2's arms called `authorize_supervisor(sender, caller, Some(sender))` (vacuous supervisor comparison) plus a `cell_id == 0` refusal because `validated_caller` is private to `auth.rs` (`dispatch.rs:294-305`, `k2-cp2-approval.md` provisional point 1). `auth.rs` is inside K5's closure, so:

* `auth.rs` gains `pub(crate) fn attested_caller(sender, caller) -> Result<CallerIdentity, KmsErrorCode>` = `validated_caller` + `cell_id != 0` (`CallerUnattested` otherwise). The K2 provisional point is resolved: **`CallerIdentity::from_trailer` already returns `None` for `cell_id == 0`** (`caller_identity.rs:148-150`, doc: "kernel/unattributable — never a cell"), so a real Cell can never be `0`; the extra check stays only as defence in depth for synthetic callers, and the vacuous supervisor call is deleted. Existing opcodes 1-14 are untouched.
* Classes (`auth.rs`): `Login` = `flags & CALLER_FLAG_KMS_SSH_SIGN != 0` (K3 constant, `1 << 6`; G-3); `Admin` = the K7 seam. **Seam in K5:** one function `classify_ssh(caller, sender, registry) -> SshClass` with exactly one production Admin rule, the **live supervisor** (`registry.supervisor_tid == Some(sender)`, the only privileged class KMS already has: `auth.rs:120-130`, `main.rs:35`); K7 CP1 (O-2) may replace or extend it append-only. OPEN (decider: user, D-5.3): alternative is fail-closed `PermissionDenied` for every Admin op until K7, which makes K5 untestable end to end without K7.
* Defence in depth: a caller that is **both** supervisor and flagged is refused on every SSH opcode (`PermissionDenied`): "the admin flag and the sign flag are never held by one Cell" (`k2-cp1.md` §2.1).
* Handler order (`code-standards.md:62-64`; precedent `dispatch/relay.rs:50-58`): (1) envelope decode (existing: noncanonical frames are dropped, no reply) (2) `attested_caller` -> `CallerUnattested` (3) class check -> `PermissionDenied` (4) leaf state: `Serving` proceeds; `Repairable` proceeds only for ops 15-18; anything else -> `SshKeyUnavailable` (**exception:** op 19's leaf query reports the true readiness instead, §9.5) (5) payload decode -> `InvalidRequest` (6) floor check -> `InvalidRequest` (7) state machine (8) **floor advance only after success** (9) reply. Nothing is read, parsed or written before step (3) passes; garbage payloads from a wrong class never reach decode.
* Matrix: ops 15-18, 20 Admin; 19 Admin or Login; 21-24 Login (k2-cp1.md §1.9).

Principal = `(cell_id, generation)`; transfer slots additionally bind `sender_tid` (`k2-cp1.md` §1.6).

## 7. OpenSSH-v1 parser (byte level)

Input: the complete binary blob (<= 65,536 B, SHA-256 already verified). Reader: `u32_be`; `string = u32_be len + bytes` with `len <= remaining` checked before slicing; sub-readers must be fully consumed.

### 7.1 Outer container: structural pre-KDF allowlist (runs for Plain and Wrapped; **no KDF, no AES, no hashing of secrets**)

| Step | Check | Reject class |
|---|---|---|
| P1 | first 15 bytes == `openssh-key-v1\0` | format |
| P2 | `ciphername` string in {`none`, `aes256-ctr`} (exact, case-sensitive) | format |
| P3 | `kdfname` string in {`none`, `bcrypt`} | format |
| P4 | (`none`,`none`) or (`aes256-ctr`,`bcrypt`); any mix refused | format |
| P5 | `kdfoptions` string: for `none` length 0; for `bcrypt` a sub-reader `{string salt; u32 rounds}` fully consumed, **salt 16..=64 bytes, rounds 1..=64** | format |
| P6 | `u32 nkeys == 1` | format |
| P7 | `string publickey`: sub-reader `{string "ssh-ed25519"; string key (32 B)}` fully consumed (51 B total) | format |
| P8 | `string private`: cipher `none`: `L % 8 == 0` and `L >= 136`; cipher `aes256-ctr`: `L % 16 == 0` and `L >= 144` (minimum inner = 8 + 15 + 36 + 68 + 4 = 131 B, padded); **wrapped `L > 2,048` -> `SshStoreFull`** (not a format error: store budget, D-5.2) | format / store-full |
| P9 | no bytes after the private string | format |

Every rejection answers `SshKeyFormatRejected` and stores nothing; one reply code, no reason text. The KDF counter test seam (`#[cfg(test)] KDF_CALLS`) must read 0 after every P1-P9 reject (matrix row 17). Rationale for each bound: contract `command-contracts.md:142`, `k1-dependency-evidence.md` §6 (crate accepts rounds 65 and `0x7fffffff`, any non-empty salt: the bounds must be ours, before the KDF), `k2-cp1.md` §4.5 row 17.

### 7.2 Inner section (cipher `none`: at import, no oracle concern; Wrapped: after decrypt, at unlock)

`bs = 8` (none) or `16` (aes), `max_pad = 16` (none) or `15` (aes) — **mirrors `ssh-key` exactly** (`private.rs:680-712`, `:881-891`, comment "PuTTYgen uses a non-standard block size of 16 ... for unencrypted keys"). Measured this session on the public fixtures: `puttygen_overpadded` is a plain ed25519 key whose private section is 176 B with **16** padding bytes `1..16` (comment 29 B), so a strict `< 8` rule would reject a real PuTTYgen export (D-5.5; recommend mirror `ssh-key`).

| Step | Check |
|---|---|
| I1 | `u32 checkint1 == u32 checkint2` |
| I2 | `string keytype == "ssh-ed25519"` |
| I3 | `string pub` is 32 B and equals the outer public key |
| I4 | `string keypair` is 64 B and `keypair[32..64] == pub` |
| I5 | `string comment` (any bytes; length <= remaining; never stored for Plain, never returned) |
| I6 | remaining bytes `r <= max_pad` and equal `1,2,...,r` exactly |
| I7 | `SigningKey::from_bytes(keypair[0..32])`: derived public key == `pub` (without the `ed25519` feature even `ssh-key` skips this: `k1-dependency-evidence.md` §7) |

Plain: any I-failure -> `SshKeyFormatRejected`. Wrapped: any I-failure (including a wrong passphrase, which shows as I1 failing with probability 1 - 2^-32) -> the **same** `SshUnlockFailed` (`k2-cp1.md` §1.5, row 28). Computed this session: wrong passphrase on `id_ed25519.aes256-ctr.enc` gives `checkint1 != checkint2`.

## 8. Import state machine (ops 15-18)

`ImportSlot`: `Idle | Receiving{handle, principal{cell,gen,tid}, name, total_len, chunk_count, blob_sha256, next_index, received} | Complete{..}`. **Staging arena:** one 65,536-byte buffer, allocated lazily on the first Begin with `Vec::try_reserve_exact` (failure -> `Busy`, nothing changed), **retained for the life of the cell and wiped, not freed** (no heap fragmentation in the 1 MiB `ostd` heap `heap.rs:28`; observable `is_zeroed()` for tests).

| Op | Checks, in order | On success | Violation handling |
|---|---|---|---|
| 15 Begin | Admin; leaf `Serving`; decode (`total_len` 1..=65,536, name charset, digest nonzero; by `types`); `request_id` > floor; identity absent else (`name == stored name` -> `SshKeyExists`, else `SshStoreFull`); same principal's import open -> `SshImportPending`; another principal's open import is **zeroized and replaced** (O-7) | `Receiving`; handle derived (below); reply `SshTransferBeginResponse{handle,total_len,chunk_count,96}`; floor advances | errors leave state unchanged except the O-7 replacement (which happens only after every other check passed) |
| 16 Chunk | Admin; decode; handle == slot handle, principal and tid equal, state `Receiving` else `SshHandleInvalid`; `chunk_index == next_index`; `chunk_len == ssh_chunk_len(total, idx)` (canonical: 96 except last); `received + len <= total` | copy into arena, `next_index += 1`; last chunk -> `Complete`; reply `SshChunkAck` | any violation -> **wipe arena, `Idle`**; `SshHandleInvalid` for handle/principal/state, `SshOrderInvalid` for index/length |
| 17 Commit | Admin; decode; handle/principal/tid/`Complete` (else `SshOrderInvalid`/`SshHandleInvalid`, destroyed); `chunk_count` and `received == total` (else `SshOrderInvalid`); `SHA-256(arena[..total])` equals Begin's **and** Commit's (else `SshImportIntegrity`) — before any parsing; §7.1 outer allowlist; Plain: §7.2 -> keep only `seed[32]` + public; Wrapped: keep `{salt, rounds, ct}`; store budget; re-check identity absent (defence: `SshKeyExists` if same public key else `SshStoreFull`); **persist (seam) before the RAM swap** (failure -> `PersistFailed`, nothing stored); install; `revision += 1`; wipe arena | reply `SshKeyStatus` of the new key | every failure above wipes arena and returns to `Idle` |
| 18 Abort | Admin; decode; handle matches slot of this principal else `SshHandleInvalid` | wipe, `Idle` | |

**Handles.** Domain-separated BLAKE2s-256 over `("cellos-kssh-handle-v1", kind, handle_counter, cell_id, generation, tid)` truncated to u64 and `| 0x8000_0000_0000_0001` (nonzero), following `lifecycle/pending.rs:113-136`; `kind` is 1 for import and 2 for unlock so the two spaces are disjoint and a handle for one never validates for the other (`k2-cp1.md` §1.2). Handles are identifiers, not secrets. KMS has no entropy source and no clock (`main.rs` blocks in `sys_recv_attested`): the counter restarts at KMS restart, so a stale pre-restart handle of the same principal can equal a fresh one; the consequence is only an order/handle violation that destroys the new transfer (same principal, no cross-principal effect) [INFERENCE: acceptable; the K2 "restart-bound" property is satisfied by state loss, `k2-cp1.md` §1.7 "KMS restart: Idle"]. Adding `GetTime` to the cell's `declare_syscalls!` would mix a boot-time value in but needs a kernel-ceiling check (not done); not proposed.

**Row 18 under LR-1 = Y.** With at most one identity, "same public key under another name" is unreachable through the ABI (Begin already answered `SshStoreFull`); the Commit re-check exists for defence and is exercised with a test-only direct insert.

## 9. Unlock, sign, limiter, replay (ops 21-24)

### 9.1 Unlock slot (per-login, D-K5)

`UnlockSlot`: `Empty | Collecting{handle, principal, tid, public_key, secret_len, next_index, received} | Staged{principal, public_key, secret_len}`; secret buffer: one fixed `[u8; 1024]` inside `SshLeaf` (no heap), wiped on every exit.

* 22 Begin: Login; `Serving`; decode; `request_id` > floor; stored public key == request key else `SshKeyNotFound`; custody `PassphraseWrapped` else `InvalidRequest` (row 25); `failures < 5` else `SshUnlockLocked` (**before any work**); any existing slot is zeroized and replaced, **whichever principal owns it** (K5 reading: `k2-cp1.md` §1.7 states only the same-principal replacement; replacing another principal's slot mirrors O-7 so a dead client cannot wedge unlock); reply 24-byte transfer response; floor advances.
* 23 Chunk / 24 Abort: same rules and violation handling as import (destroy slot on any violation, `SshHandleInvalid`/`SshOrderInvalid`). Reaching `secret_len` -> `Staged`.
* The slot exits on: sign (success **or** failure), abort, violation, replacement, key delete, KMS restart.

### 9.2 Sign (op 21)

1. Login class; `Serving`; decode (session id nonzero, username `1..=31` `[A-Za-z0-9._-]`, request id nonzero) else `InvalidRequest`; `request_id` > floor.
2. Request public key == the single stored key else `SshKeyNotFound`.
3. **Plain:** seed from the store. **Wrapped:** slot must be `Staged` for this exact principal and key else `SshUnlockRequired` (empty slot: no state change; a `Collecting` slot, or a slot of another principal or key: destroyed, row 27, as approved); `failures >= 5` -> destroy slot, `SshUnlockLocked`, **no KDF**; move the secret into a stack `Zeroizing`, wipe the slot, then `bcrypt_pbkdf_with_memory(secret, salt, rounds, &mut okm[..48], &mut mem[..64])`, AES-256-CTR (`Ctr128BE<Aes256>::new_from_slices(okm[..32], okm[32..48])`) into a `Zeroizing<[u8; 2048]>` stack buffer, §7.2. Any failure: `failures += 1`, wipe everything, `SshUnlockFailed` (one code, byte-identical reply); floor unchanged. Success: `failures = 0`.
4. Reconstruct the RFC 4252 §7 input (below), `Signer::sign`, **self-verify with `verify_strict` against the stored public key**; mismatch -> `InvalidSignature`, no signature bytes in the reply, seed zeroized, `failures` unchanged (it is not a passphrase failure).
5. `message_sha256 = SHA-256(input)` (O-8 kept); reply `SshSignResponse{request_id echo, signature[64], message_sha256}`; zero the input buffer, seed, okm, plaintext; floor advances.

**Reconstruction** (`sign.rs`; stack `[u8; 174]`, length `143 + n`):

```
off 0       00 00 00 20 | session_id[32]                      36
36          32                                                   1   (SSH_MSG_USERAUTH_REQUEST)
37          u32 n | username[n]                                  4+n
41+n        00 00 00 0e "ssh-connection"                         18
59+n        00 00 00 09 "publickey"                              13
72+n        01                                                   1
73+n        00 00 00 0b "ssh-ed25519"                            15
88+n        00 00 00 33 | 00 00 00 0b "ssh-ed25519" | 00 00 00 20 | pk[32]   55
end         143+n   (n=1: 144, n=4: 147, n=31: 174)
```

Matches `k2-cp1.md` §1.4 and the K2 test that assembles the 147-byte vector from typed parts (`reports/k2-cp2-dossier.md` §4, `signing_input_sizing_proof_holds_for_every_username_length`).

### 9.3 Failure limiter (O-5)

One `failures: u8` for the one key; `SshUnlockFailed` +1; sign success resets to 0; `>= 5` -> `SshUnlockLocked` at Begin and at Sign before any KDF; reset only by key delete, re-import or KMS restart (volatile at T0). Stated cost: a compromised Login caller can lock the key out (denial of service) and cannot exceed 5 guesses per boot; offline guessing is out of its reach at T0 (nothing leaves KMS).

### 9.4 Replay floors (O-4)

`FloorTable`: 4 entries `{cell_id, generation, floor, seq}`, evict the lowest `seq`. One floor per principal covers ops 15, 20, 21, 22 (an unlock id n is followed by a sign id above n). `request_id` must be nonzero and `> floor`; the floor is set to `request_id` **only after the whole operation succeeded** (precedent `dispatch/relay.rs:58,111`). Incoming note from the K6/K7 drafter: a fresh `cellbox-ssh` instance is a new principal with floor 0 and starts ids at 1: satisfied. Eviction resets the evicted principal's floor to 0; the only frames that can be replayed under an attested trailer come from that principal itself (the kernel writes the trailer, `main.rs:32`), so this lowers defence in depth, not an access boundary [INFERENCE: stated risk of O-4 in `k2-cp1.md` §1.6]; test pins the behaviour.

### 9.5 Status (19) and Delete (20)

* 19: Admin or Login; `name_len == 0` -> leaf status, **answered even when the leaf is not serving** (the one exception to handler step 4, §6): `algorithm SshEd25519`, `readiness` = `Ready` (T0 always, once the cell is up) or the true non-ready value (K4b: `ProviderError`, `PolicyMismatch`), `provider None`, `posture DevPostureVolatile`, `key_present 0`, `custody None`, `key_count` 0 or 1, `store_revision`, public key and fingerprint all zero (`SshKeyStatusPayload` doc: `key_present` is "1 only when the named key is stored; 0 for the leaf query"; decode invariants in `reports/k2-cp2-dossier.md` D-3). Named query while not serving -> `SshKeyUnavailable`; named query serving: stored name equals -> full status with `custody`, else `SshKeyNotFound`. Fingerprint = SHA-256 of the 51-byte wire public key blob. For a Wrapped key the public key is the outer-blob claim until the first successful unlock (the inner check is impossible earlier, O-15): a spoofed outer key only makes the first unlock fail.
* 20: Admin; decode; `request_id`; name **and** public key must equal the stored ones else `SshKeyNotFound` (key intact); persist tombstone via the seam (failure -> `PersistFailed`, key intact); zeroize the identity, destroy the unlock slot, reset `failures`, `revision += 1`; empty success payload; floor advances.

## 10. Reply path and the K6 note

`main.rs:41-45` replies with `ostd::ipc::reply` when an async operation exists, else a blocking `sys_send(sender, ..)`; `KmsClient::call` is `sys_send` then `sys_recv(tid)` with no timeout (`libs/ostd/src/clients/kms.rs:153-163`). The K6/K7 drafter points out that a client that times out but stays alive outside `recv` (or in its own blocking `send` to KMS) can wedge the single-threaded KMS in the reply `send` (kernel `Send` arm: `reply_current` for an async operation, otherwise `ipc_send` returns "blocked" until the receiver takes the message, `kernel/src/task/syscall.rs:4463-4496`). This risk exists today for relay opcodes; the 64-round KDF widens the window. K5 leaves `main.rs` reply logic unchanged: `TrySend` is "non-blocking: deliver if target in Recv, else drop" (`syscall.rs:4503-4508`) and would lose replies to a client that has not yet re-entered `recv`; no bounded-send primitive exists in `ostd` [INFERENCE: not found by name]. Mitigation is on the client side (K6 exits the login at once on timeout). OPEN (decider: user, D-5.9): accept; or ask the kernel owner for a bounded send (out of this program).

## 11. Memory, stack and KDF stall

| Item | Budget | Basis |
|---|---|---|
| Blowfish state | 4,168 B (4 S-boxes 4 x 1,024 + P-array 72) per `Blowfish`, created on the stack inside each `Bhash` PRF call; allow 3 copies for moves = about 12.5 KB peak; **budget the whole unlock call at <= 16 KiB of stack** | `k1-dependency-evidence.md` §0; `bcrypt-pbkdf` source `lib.rs:91-124` (`bhash` builds `Blowfish::bc_init_state()` per call) |
| KDF scratch | `okm` 64 B + `mem` 64 B (48-byte output needs `ceil(48/32) * 32 = 64`) | `lib.rs:100-110`, K1 §6 |
| AES-256-CTR | key schedule 240 B + counter state | [INFERENCE: `aes 0.9`] |
| Plaintext private section | `Zeroizing<[u8; 2048]>` on stack | §5.2 |
| Sign | input 174 B, hash states, dalek expanded key 64 B | |
| Heap | staging 64 KiB retained once; identity <= about 2.2 KB; unlock secret 1 KiB (inline) | `heap.rs:28` 1 MiB arena |
| Stack available | `kms` is not named in `stack_pages_for`, so it takes `STACK_PAGES = 64` pages of 4 KiB = 256 KiB | `kernel/src/task.rs:154,675-686`, `paging.rs:16` [INFERENCE: the cell name resolves to `kms`] |
| `.text` | probe `ownmin` +76,968 B on x86 (synthetic, no p256) | K1 §4 |

**Stack proof (K5 test `stack.rs`):** run the full wrapped sign on a host thread built with `std::thread::Builder::stack_size(64 * 1024)` in `--release`; plus record the per-function frame sizes from the `x86_64-unknown-none` release build (`-Z emit-stack-sizes`, `llvm-readobj --stack-sizes`) for the unlock call chain. A pass means the 16 KiB budget holds with margin against the 256 KiB cell stack; **not** a measurement on Cellos.

**KDF stall.** The loop is single-threaded (`main.rs:27-50`) and no other request is served during an unlock KDF. Measured this session: Rust host (K1) 4.70 / 75.25 / 301.26 ms at rounds 1 / 16 / 64; OpenBSD-C `bcrypt_pbkdf` (python `bcrypt.kdf`) **4.5 / 72.9 / 288.5 ms**: the cost is inherent to the algorithm. Target cost unmeasured: [INFERENCE] same order on bare metal x86, several times slower under QEMU TCG (seconds at 64). The kernel CPU watchdog kills only `RealTime`-priority tasks that run 500 ticks (5 s) without blocking (`scheduler.rs:2318-2327`); `kms` has no priority in `config/services.toml:89-93` [INFERENCE: default priority], so a long KDF is not a watchdog kill. On x86 the relay and node identity are already `Unavailable` (`kms-prerequisites.md` §1.3), so the stall costs little there.

**Proposed measurement (required before any acceptance claim, `k2-cp1.md` §3.5):**
1. Host, repeat at CP2 with the final crate versions (rounds 1, 16, 64; 5 samples).
2. Target: K8's lane records the client-observed unlock time (S02/S03 in `k8-cp1-draft.md`) at rounds 16 (default `ssh-keygen -a 16`) and 64 on QEMU/TCG.
3. If an in-KMS split is needed: a `test-hooks`-only boot bench that prints `rounds` and elapsed ms with synthetic input (needs `GetTime` in a `cfg`-split `declare_syscalls!`, [INFERENCE: not verified]); never compiled into non-test-hooks builds.
4. Acceptance threshold (OPEN, decider: user, D-5.8): proposal **<= 10 s per unlock at rounds 64 on QEMU/TCG**; if exceeded, options are a lower rounds cap (narrows the contract line 142 set: needs user acceptance), or a KDF cell separate from the KMS loop (new design, out of this program).

## 12. Zeroization policy

| Material | Holder | Wiped when | Residual / test |
|---|---|---|---|
| Staging blob (<= 64 KiB; may hold a plain seed) | retained arena | after commit, abort, violation, replacement | `is_zeroed()` after each exit (row 32) |
| Unlock secret (<= 1,024 B) | fixed inline array | consumed by sign (success or failure), abort, violation, replacement, delete | `is_zeroed()` |
| Plain seed at rest | `Zeroizing<[u8; 32]>` inside `StoredIdentity` | delete, replacement | drop test through a probe wrapper |
| Wrapped `{salt, rounds, ct}` | `StoredIdentity` | delete | not secret (passphrase-protected) |
| KDF output `okm`, `mem` | stack `Zeroizing<[u8; 64]>` | function exit | |
| Blowfish state | `blowfish/zeroize` (direct dep unifies the feature, K1 §2) | `Drop` | |
| `Sha512::digest(passphrase)` temporary inside `bcrypt_pbkdf_with_memory` | inside the crate | **not wiped** (a temporary; `zeroize` feature only wipes `Bhash.sha2_pass`, K1 §2) | residual risk; mitigation below |
| AES key schedule, CTR state | `aes`/`ctr` `zeroize` features | `Drop` | |
| Plaintext inner section | stack `Zeroizing<[u8; 2048]>` | function exit | |
| Dalek expanded key, `SigningKey` | `ZeroizeOnDrop` (`signing.rs:719`, `hazmat.rs:68`) | `Drop` | SHA-512 block buffers inside dalek/sha2 are not wiped (`sha2 0.11` `zeroize` feature is not enabled; enabling it adds one edge line to the lock) |

Best-effort scrub: `fn scrub_stack()` (`#[inline(never)]`, a zero-filled `[u8; 16 KiB]` through `core::hint::black_box`) is called after every unlock and sign. The crate forbids `unsafe`, so this is the only available technique and is **best effort, not proven** (the compiler may elide it; a QMP residue scan is deferred, K8 D8-6). The honest statement for evidence: key bytes are wiped on all owned buffers by construction and test; third-party temporaries and stack remnants are a stated residual risk.
`Debug` for every secret holder is a manual impl printing `<redacted>`.

## 13. Test and evidence plan

All host tests run `cargo test --offline --locked -p service-kms --features ssh-dev-custody --target x86_64-unknown-linux-gnu`. No VM.

### 13.1 Vectors (computed or transcribed; none generated)

**RFC 8032 §7.1 pure Ed25519.** Transcribed from the RFC and cross-checked this session by (a) `ed25519-compact-2.6.0/src/ed25519.rs:1036-1066` (registry text) and (b) python `cryptography`/OpenSSL (seed -> public -> signature all equal). My first recollection of TEST 3's public key was wrong; the committed value must come from the RFC text with the source and the cross-check recorded in the test file.

| Test | seed (secret key) | public key | message | signature |
|---|---|---|---|---|
| 1 | `9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60` | `d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a` | empty | `e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b` |
| 2 | `4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb` | `3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c` | `72` | `92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00` |
| 3 | `c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7` | `fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025` | `af82` | `6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a` |

TEST 1024 and TEST SHA(abc) are to be transcribed from the RFC text at CP2 (not recalled here) and cross-checked with python `cryptography` the same way.

**Signing-input vectors for the userauth reconstruction** (computed: fixture key = public test key `ssh-key-0.7.0-rc.11/tests/examples/id_ed25519`, public `b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62`; `session_id = 00 01 .. 1f`; signatures by OpenSSL via python `cryptography`, verified against the same key; public-key-blob SHA-256 `5025222ebecf8ecf7014524c0c1c8b81cdcdaed754df8e0e814338e7064f7084`):

| username | n | input length | SHA-256 of input (`message_sha256`) | signature |
|---|---|---|---|---|
| `u` | 1 | 144 | `7559ff7d7bd10978808bab2447a6bc20efea91ae91e51404d24e172316916e93` | `85be1a68483e3393c8adee3f034af8bb4458decff846d59e43046a655f16ae850d557f5b20e56a00b06140cd60567609e16e9302e3e1db0a10a1c9322503e70e` |
| `user` | 4 | 147 | `fa3246abe7b89d40c24cd8b330fc0eba243f06d4835cbc88181be36bb4e6de65` (equals `k2-cp1.md` §4.3) | `38e041cad337b43040aeea295dfeb41e3ca1302b1705919a8a2746f263d162fa6e9a6a5c57fdaf71d85f77b77b3ae088efa722c991aef28ef843208084215803` |
| `a` x 31 | 31 | 174 | `a5eb9ca6afe088ec3bc8c6a1ee82ba7e74794771e44b88f36fd402ad994f8c4c` | `f4b2f8f238344ef3009b9ba8d5e0359d1cd4715285f88707de575f485a069ee5403298f54bfe656f1f8118f0736db49e868487049b906bd6ccea5f490007cf0e` |

KMS must reproduce these bytes exactly (deterministic signing), and a signature for session B / user B must **not** verify against input A (row 31).

**OpenSSH-generated reference containers** (public fixtures from the registry crate; bytes to be embedded as hex with attribution, D-5.10; whole-file SHA-256 `ssh-key-0.7.0-rc.11/tests/examples/...`):

| Fixture | Cipher / KDF | Result expected | Facts (computed) |
|---|---|---|---|
| `id_ed25519` | none / none | Plain accepted; public `b33eaef3…ab62`; padding `1..5`; comment 16 B | sha `1d3eb512…858b`, blob 250 B |
| `id_ed25519.aes256-ctr.enc` | aes256-ctr / bcrypt, rounds 16, salt `4a1fdeae8d6ba607afd69d334f8d379a` | Wrapped accepted; passphrase `hunter42` -> same seed/public; padding `1..13`; independently decrypted with python `bcrypt.kdf` + `cryptography` AES-CTR this session | sha `b5fd6cdd…963d`, blob 290 B, ciphertext 160 B |
| `puttygen_overpadded` | none / none | Plain accepted with 16 padding bytes (D-5.5) | blob 274 B |
| `id_ed25519.aes128-ctr.enc`, `.aes192-ctr.enc`, `.aes128-cbc.enc`, `.aes192-cbc.enc`, `.aes256-cbc.enc`, `.3des-cbc.enc`, `.chacha20-poly1305.enc`, `.aes128-gcm.enc`, `.aes256-gcm.enc` | other ciphers | `SshKeyFormatRejected` at P2, `KDF_CALLS == 0` | gcm/chacha blobs carry 16 trailing tag bytes (P9 also fails) |
| `id_ecdsa_p256`, `id_rsa_3072`, `id_dsa_1024`, `id_opaque` | other key types | P7/P2 reject | |
| hand-crafted (cannot come from `ssh-keygen`: `-a` is 1.., salt 16, `-Z` limited) | rounds 0, 1, 64, 65, `0x7fffffff`; salt 15, 16, 64, 65; kdf `none` with aes256-ctr and the converse; `nkeys` 0/2; trailing byte; length fields `0xFFFFFFFF`, `0x80000000`; wrapped ciphertext 2,048 / 2,064 / not multiple of 16; plain padding 16 / 17 / wrong sequence | boundary accept, rest reject | all refused before any KDF |

`ssh-keygen -y` over a 0600 copy of each public fixture is an optional third oracle at CP2 (OpenSSH 9.6 is installed; run refused on a 0644 file this session, as designed).

### 13.2 Differential and fuzz

* **Differential (D-5.4, recommended = golden vectors):** at CP2 run the K1 `hosttest` pattern once in a scratch directory outside the repo (`ssh-key 0.7.0-rc.11` with `encryption`, offline from the scratch cache) over the fixtures above, plus OpenSSL signatures; commit only the resulting hex goldens. K1 already observed identical signatures from the own-minimal parser and `ssh-key` (`k1-dependency-evidence.md` §4). Alternatives: (B) `ssh-key` as a dev-dependency (D-K4 wording) — adds the lock-edge lines and probable deny warnings above, contradicts K1 acceptance 1/2; (C) an out-of-workspace host tool with its own lock (precedent `tools/cellos-fs-provision`, `Cargo.toml:10`) — needs a root `Cargo.toml` `exclude` edit (outside the closure).
* **Parser fuzz target** `fuzz/src/kms_ssh_parser.rs` (D-5.12): `#[path]`-includes `ssh/parser.rs` (§5.1); properties: never panics; `Ok` implies every §7.1 bound holds on the input (re-checked independently in the target); seed corpus = the fixtures above. Run `cargo fuzz run kms_ssh_parser -- -max_total_time=300` (needs cargo-fuzz and libfuzzer-sys already in `fuzz/Cargo.lock`; not run in this draft).
* **Always-on deterministic mutation test** (`tests/ssh/mutation.rs`, runs in `cargo test`): xorshift64, fixed seed, 200,000 iterations over bit flip / byte set / truncate / extend / length-field splice (`0`, `0xFFFFFFFF`, `0x7FFFFFFF`, `0x80000000`) / chunk duplicate; asserts no panic, outcome in {Ok, Reject}, `KDF_CALLS == 0` for every reject that is not a wrapped inner failure, and that `Ok` for a Wrapped container still requires no KDF at import.
* Chunking property tests reuse `types::kms::{ssh_chunk_count, ssh_chunk_len}` (K2).

### 13.3 Refusal matrix rows 2..34 (from `k2-cp1.md` §4.5) -> tests

Row 1 (unattested) is already covered for the stub arms in `dispatch.rs` tests; K5 re-runs it against the real handlers. Every row asserts the typed error, **no state change** (identity, revision, import/unlock slots, floors, `failures`), and no persister call before admission. `T` = `cells/services/kms/src/tests/ssh/`.

| Row | Test (file::fn) |
|---|---|
| 2, 3, 4 | `admission::non_login_refused_on_21_24`, `login_refused_on_admin_ops`, `admin_refused_on_sign_ops`, `supervisor_with_flag_refused_everywhere` |
| 5 | `admission::leaf_and_relay_readiness_independent` (fixture relay `Ready` + leaf `Unavailable` -> `SshKeyUnavailable`; leaf `Serving` + relay `Unavailable` -> relay ops unchanged) |
| 6 | `admission::noncanonical_envelopes_dropped` (nonzero flags/reserved/tail, opcode 25, version 2: no reply) |
| 7 | `admission::malformed_payloads_invalid_request` (every op: wrong length, nonzero reserved/padding, charset) |
| 8 | `store::request_id_floor_rules` (0, equal, lower, reuse after success, unchanged on failure, 4-slot eviction, fresh principal floor 0) |
| 9-12 | `import::begin_bounds`, `begin_name_exists`, `begin_same_principal_pending`, `begin_other_principal_replaces_and_zeroizes` |
| 13-16 | `import::chunk_handle_violations_destroy`, `chunk_order_violations_destroy`, `commit_early_or_count_mismatch`, `commit_integrity` |
| 17 | `parser::every_row17_case_rejected_before_kdf` (fixtures + hand-crafted; `KDF_CALLS == 0`) |
| 18, 19 | `import::same_public_key_defence_recheck`, `store_budget_wrapped_ct` |
| 20, 21 | `store::status_unknown_name`, `delete_wrong_name_or_key_leaves_key` |
| 22, 23 | `unlock_sign::sign_unknown_public_key`, `sign_bad_session_or_username` |
| 24-27 | `unlock_sign::wrapped_without_secret`, `unlock_begin_bounds_and_plain`, `unlock_chunk_violations`, `foreign_staged_secret_destroyed` |
| 28, 29 | `unlock_sign::wrong_passphrase_uniform_failure` (byte-identical replies for wrong pass / corrupt blob / pubkey mismatch; counter +1), `limiter_locks_without_kdf` (`KDF_CALLS` unchanged at 6th try; reset by delete/re-import/new service) |
| 30 | `unlock_sign::self_verify_fault_injection` (`#[cfg(test)]` seam flips one signature byte -> `InvalidSignature`, reply has no signature bytes, seed zeroized) |
| 31 | `vectors::signature_bound_to_session_and_user` + the three §13.1 input vectors byte for byte |
| 32 | `zeroize::buffers_zero_after_every_exit` (staging, unlock secret, plain seed holder) |
| 33 | `store::restart_clears_state` (new `KmsService`: store empty, slots Idle, floors 0, revision 0) |
| 34 | `zeroize::no_secret_in_replies_debug_or_logs` (scan every reply frame and `format!("{:?}")` of the service for the seed, passphrase and ciphertext) |

Additional K5 rows: all-or-nothing persist (`store::persist_failure_leaves_everything_unchanged` with the failing fake); Plain/Wrapped import->status->sign end to end with the §13.1 fixtures and `hunter42`; wrong, empty (refused by ABI), 1,024-byte passphrases; `KMS` default build (feature off) still passes the 65 K2 tests unchanged.

### 13.4 Build evidence

`cargo build --release --locked --offline -p service-kms` for riscv64gc / x86_64 / aarch64 bare-metal targets as in `reports/k2-cp2-dossier.md` §3, with and without `--features ssh-dev-custody`; `compile_error!` demonstrations for `hardware-relay-provider + ssh-dev-custody`; `.text` growth; clippy with `-D warnings` on the crate (CI lints riscv64). Not run in this draft.

## 14. Lock handoff packet content (CELL-CONTRACT-0024; form of `lock-handoff-k2-cp2.md`; draft for the accountable owner, not submitted)

### 14.1 Slice identity and what it serves (step 1)
**Slice:** KSSH K5: KMS behaviour for opcodes 15-24 at T0 (volatile, `DEV_POSTURE`): parser, import, unlock, sign, limiter, replay floors, caller classes, zeroization; the `Cargo.lock` application of the K1-approved dependency diff. No ABI change, no kernel change, no persistence. **Serves:** Cellbox Phase 09 S9-5 / CBX-24 publickey (not an Intel C2C outcome; the Phase 02 bounded-slice basis, as for K2). **Controlling plan:** `.agents/261011-1200-cell-contract-authority/plan.md` (complete; unlocks nothing). **Evidence reconciled:** `k2-cp1.md` (approved), `k2-cp2-approval.md`, `k1-cp1-draft.md` (approved), this draft.

### 14.2 Contract delta against ADR-0024 principles (step 2)
| Principle | Effect of K5 |
|---|---|
| Cell principal distinct from Task | principal = kernel-attested `(cell_id, generation)` with `sender_tid` bound on transfers; no identity concept added (LR-2 = P) |
| Shared per-Cell capability space; delegation/attenuation | none: no capability or handle crosses Cells; handles are per-principal, destroyed on violation; the sign right is K3's non-delegable kernel flag |
| Revocation generations | live generation validated by `attested_caller` before any state access; floors keyed by generation |
| Kernel substrate vs policy Cells | no kernel file touched |
| Durable state vs execution | T0 stores nothing durable; persistence seam only (K4b) |
| Caller/test/doc migration | callers: none change in K5 (`libs/ostd` is K6). Tests: new `tests/ssh/**`; K2 stub-arm tests stay valid for the feature-off build. Docs: `docs/code-standards.md` "KMS SSH Custody Boundary" gets one line when K5 lands (the "Until KMS behaviour lands" bullet); `CHANGELOG.md` entry |
| No ambient credential / remote pointer | no raw-message signer, no caller-selected key/algorithm, no private-key export; passphrase only enters through the Login-class staged slot |

### 14.3 Exact file closure requested
§3 (new files and the eight edited shared files + `Cargo.lock`; `fuzz/**` optional), nothing else. `Cargo.lock` is **shared by the whole workspace**: the exception admits exactly the amended K1 diff and any plan with an uncommitted lock edit must rebase first (re-checked at slice start; `git status` was clean and the last lock commit is `96207b5ad` at draft time).

### 14.4 Locked rows and overlapping plans (step 3)
| Plan / row | Overlap | Needed |
|---|---|---|
| `260825-1726-kms-silo-production-root` (`cell-contract-0024-locks.md:147`; blocked scope: mTLS, relay, protected signer, production root) | K5 edits files under `cells/services/kms/**` but none of the blocked scope; relay/enrollment/node-identity files untouched | file-bounded exception for §3 |
| `260826-1605-phase4-dev-reference-authority` (`:149`) Phase 06 "Related Code Files" (`phase-06-frozen-abi-kms-authority-integration.md:58-77`) lists `Cargo.toml`, `build.rs`, `lib.rs`, `dispatch.rs`, `tests.rs` (and `storage.rs`, `provider*.rs`, `lifecycle/mod.rs`, `dispatch/{enrollment,relay}.rs` which K5 does not touch; its "Verify unchanged" list at `:78-84` includes `model.rs` and `tests/frame.rs`, already ruled historical by C-2) | **C-K5-1 textual overlap on five shared files.** All Phase 06 edits are pending/blocked on hardware (`status: pending`). K5's edits there are additive (a cfg'd `mod ssh`, optional deps and features, one field and arm bodies, `mod ssh;`). Request: ruling that K5 lands first and Phase 06 rebases; the post-K5 hashes of those five files become Phase 06's anchor (same device as the K2 ruling C-2 for `model.rs`/`tests/frame.rs`) |
| same, `phase-06:163` ruling | **C-K5-2** the K2 ruling says later provider/stub replacement must keep opcodes 15-24 unchanged: K5 touches no storage/provider code, so it is preserved | reciprocal note |
| same, Phase 02 ownership of `scripts/check-production-relay-image.py` (`phase-02:46,74`) | K5 does not edit it; production exclusion is structural (feature + `compile_error!` + `build.rs`); marker hand-off is K4b's | none |
| `Cargo.lock` | **C-K5-3** shared file, see §14.3 | named exception |
| `code-standards.md` "volatile state is never a substitute" (`:122-125`) | relay-lifecycle wording; the SSH leaf's own posture rules were added by K2 | no ruling needed; stated so it is not misread |
| `260825-sdk-delivery` P02, `260712-1902-dice-attestation-identity` P03, `260819-1409-cell-to-cell-anywhere-core` P02, `migration-ledger.md` E12 | "not examined" at K2 (grep matches only) | owner confirms no overlap, or names one; integrating owner re-checks at slice start |

### 14.5 Retained gates (step 4)
Law 1: **not applicable** (no `libs/api`/`libs/types` edit in K5; the K3 constant is K3's two confirmations). Protected identity/non-rollback time/state: not applicable (no storage, no time, no identity store; `DEV_POSTURE`, never `ProductionQualified`). Independent human decisions: D-K1..D-K7, LR-1/LR-2, D-5.x. No physical/hardware/AWS/production gate is touched. A green host test, a fuzz run or this documentation does not substitute for these gates.

### 14.6 Proposed scheduling change (step 5; text only, not applied)
Record "KSSH K5" as a bounded adopted slice with ceiling "host tests, feature `ssh-dev-custody` opt-in, volatile, `DEV_POSTURE`" in the controlling plan, `plan-portfolio.md` and `cell-contract-0024-locks.md` next to the K2 paragraph (`:115`), naming the §3 closure and this packet's digest; narrow only the applicable relation of `260825-1726`; keep the whole-plan relations; WIP stays one implementation slice (K3's and K5's may not be simultaneous: K5 CP2 starts after K3's constant lands).

### 14.7 Execution boundary once adopted
Single integrating owner. Baseline first (G-5), with logs. Edits only inside the closure; K2's 65 `service-kms` and 74 `types` tests stay green. No key generation, no network except the authorized crate copy (G-4), no VM, no image/signing, no `libs/**` or `kernel/**`. **Rollback:** restore the pre-edit hashes of §3 (and the pre-edit `Cargo.lock`); T0 has no persistent effect. Dossier with post-edit digests, `git diff --stat`, test counts, build evidence for the three targets, `cargo deny` before/after, measurement per §11; then K5 hands off to K6/K8. K4b's files are a separate handoff.

### 14.8 Decisions requested from the accountable owner
1. Adopt K5 as a bounded slice with the §3 file-bounded exception (serves Cellbox publickey, not C2C).
2. Rule C-K5-1 (Phase 06 rebases over K5 on the five shared files, post-K5 hashes as its anchor) and approve reciprocal notes in `260826-1605` and `260825-1726`; confirm C-K5-2.
3. Admit the `Cargo.lock` edit as a named, bounded exception (C-K5-3).
4. Confirm no overlapping owner for the §3 paths, or name one.
5. Approve recording the adoption as one scheduling change.

## 15. Risks

1. **KDF stall** of the single-threaded loop: unmeasured on target (§11). 2. **Own parser in a locked service** (about 250 lines of untrusted-input code): mitigated by the structural allowlist, `deny(indexing_slicing, arithmetic_side_effects)`, fixtures, mutation test, fuzz target, `ssh-key` golden differential. 3. **Residual secret remnants** in third-party temporaries and stack (§12), best-effort scrub only. 4. **Reply wedge** with an abandoned client (§10). 5. **AES backend on `x86_64-unknown-none`:** `aes 0.9` selects AES-NI by runtime `cpuid` on x86 (`aes-0.9.3/src/lib.rs:151-154`) and by compile-time features only on bare-metal aarch64 (`cpufeatures-0.3.1/src/aarch64.rs:176`); whether an `x86_64-unknown-none` cell may use the SIMD register file safely, or must be built with `--cfg aes_backend="soft"` (a RUSTFLAGS change outside the closure), is **not checked** (Sunset already runs AES in `cellbox` on x86 [INFERENCE: not verified for this reason]). 6. **Wrapped keys are unverified at import** (O-15): corrupt blobs surface at first login. 7. **Brute force limited to a count** (no clock) and lockable by a compromised Login caller. 8. **Cap narrowing:** wrapped containers with a ciphertext above 2,048 B (comment above about 1.9 KB) are refused although the contract allows 64 KiB (D-5.2). 9. **Lock diff deviation** (base64ct line) from K1's wording. 10. Default build unchanged but CI must add the feature leg (G-6) or the code is untested in CI.

## 16. Explicitly out of scope

Persistence, the dev root key, any provider (K4b); the kernel flag (K3); `KmsClient`, `cellbox`, `vissh`, `--identity`, passphrase lease, timeouts (K6); the import surface and Admin class beyond the seam (K7); QEMU/TCG evidence, image deltas, fixtures (K8); T2; ABI additions; PEM/armor handling in KMS; key generation or export; host-trust enforcement before signing (cannot be done from the session id alone, `k2-cp1.md` §2.1).

## 17. Effort

L: parser and unwrap 3 days; import/unlock/sign state machines 4 days; admission, dispatch, store and floors 2 days; tests, vectors, mutation, fuzz target 4 days; lock handoff, dependency application, deny/baselines, three-target builds, measurement plan 3 days; slack for review: about 3 weeks, one integrating owner. K3's constant is a prerequisite elsewhere.

## 18. Decisions requested from the user (at CP1)

| ID | Decision | Recommendation |
|---|---|---|
| D-5.1 | Gating: opt-in feature `ssh-dev-custody` (default off) vs always compiled vs default on | opt-in; default build = K2 baseline; CI adds a feature leg |
| D-5.2 | Stored budget (O-6): wrapped ciphertext <= 2,048 B; Plain stores only the seed (comment dropped, so any comment length up to the 64 KiB blob limit is accepted for Plain); a larger wrapped container -> `SshStoreFull` | accept (narrows contract's 64 KiB for wrapped keys only) |
| D-5.3 | Admin class seam: live supervisor now, K7 may extend, vs fail-closed until K7 | live supervisor |
| D-5.4 | Differential test placement: golden vectors / `ssh-key` dev-dependency / out-of-workspace tool | golden vectors (D-K4 said "dev-dependency": this is a recorded deviation to avoid lock edges and deny warnings) |
| D-5.5 | Inner padding policy mirrors `ssh-key` (none <= 16, aes <= 15) so PuTTYgen exports import | mirror |
| D-5.6 | Drop `base64ct` from the manifest; lock diff is the K1 diff minus one line | drop |
| D-5.7 | K5 CP2 waits for the K3 constant in `libs/api` (no local duplicate of a Law-1 constant) | wait |
| D-5.8 | KDF acceptance threshold and fallback if exceeded (§11) | <= 10 s at rounds 64 on QEMU/TCG; fallback decided only after measurement |
| D-5.9 | Reply wedge: accept pre-existing blocking reply, rely on K6 exit-on-timeout | accept |
| D-5.10 | Embed the public `ssh-key` test blobs as hex with attribution (their crate is MIT OR Apache-2.0; this repo's SPDX is MPL-2.0 on `types`) | accept after a licence glance |
| D-5.11 | `ed25519-dalek` 3.0.0 (not 2.2.0) | 3.0.0 |
| D-5.12 | Add `fuzz/src/kms_ssh_parser.rs` and a `[[bin]]` row to `fuzz/Cargo.toml` (outside `cells/services/kms/**`) | include; run only when authorized |

## 19. Not checked

No compile, test, lint or VM run; no `cargo deny` run; no KDF or stack measurement on Cellos; `kernel/src/loader/boot_ceiling.rs` legacy ceiling for `/bin/kms` and whether `GetTime`/other syscalls could be added; the AES backend question (§15.5); whether `kms` has default task priority; that the cell name resolves to `kms` in `stack_pages_for`; `ssh-keygen -y` output; the K3 constant's final name and value (the K3 draft is a peer's); `cargo-fuzz` availability; license text of the `ssh-key` fixtures; RFC 8032 TEST 1024 / SHA(abc) text; whether the K7 admin surface needs a list opcode (none exists, `k2-cp1.md` §6).
