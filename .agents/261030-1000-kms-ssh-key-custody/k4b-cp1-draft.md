# KSSH K4b CP1 DRAFT: T1 persistent SSH custody, `DEV_POSTURE` (status AWAITING CP1; documents only; nothing applied)

Written 2026-10-30 at HEAD `fad076ee9`. Documents only: no source, manifest, lock, key, network, VM or image change was made to produce it. `file:line` citations were read in this session; `[INFERENCE]` marks anything not observed; `OPEN (decider)` marks a decision for the named owner. K4a (T0, no storage) has no document: it is K5's volatile store. This draft depends on [`k5-cp1-draft.md`](k5-cp1-draft.md) (it uses K5's `SshPersist` seam, §5.3 there).

Inputs read: `plan.md` (tiers, D-K1/D-K2/D-K7), `k2-cp1.md` §1.1, §3.3, §6, `k2-cp2-approval.md`, `k5-cp1-draft.md`, `lock-adoption-draft.md`, `lock-handoff-k2-cp2.md`, probe `kms-prerequisites.md` §1.3, §2.4, §5; `cells/services/kms/src/{storage.rs,dispatch.rs:94-109,310-325,lib.rs}`, `storage/{backend,record,journal,runtime,provider,provider/relay}.rs`, `Cargo.toml`, `build.rs`; `cells/services/vfs/src/{access.rs,access/kms.rs,access/rules.rs,paths.rs,quota.rs,manager.rs}`; `libs/ostd/src/clients/vfs.rs`; `libs/api/src/services/ipc.rs:23`; `scripts/check-production-relay-image.py`, `scripts/test_check_production_relay_image.py`, `scripts/build-production-relay-image.sh`; `.agents/260826-1605-phase4-dev-reference-authority/{phase-02,phase-04,phase-06}*.md`; `.agents/261025-cellosfs-crash-safety/plan.md`; `cell-contract-0024-locks.md`; `config/services.toml`.

## 0. Recommendation first: in or out of the program now?

**Recommendation: IN, but as the last code slice before K8, sequenced after K5 and K5's T0 lane run; approve the design now, build later, and keep it conflict-free by construction (§9).**

Why in: T0 re-imports the key at every boot through the K7 admin path, so for anything beyond a demonstration the key must survive a reboot; K8's S12/S13 (`k8-cp1-draft.md`, "Reboot persistence", "Rollback shown as unprotected") and `plan.md`'s T1 row exist only with K4b. D-K1 already decided "T0 then T1 labelled `DEV_POSTURE`".

Why the conflicts do not block it (§9): with the three rules below K4b touches **none** of the files that Phase 06 of `260826-1605` plans to rewrite for the `PermissionDenied` stubs (`storage.rs`, `storage/provider*.rs`, `lifecycle/mod.rs`, `dispatch/{enrollment,relay}.rs`), none of the Phase-02-owned production checker, and no `libs/types` file:

1. **New files only** under `cells/services/kms/src/ssh/` plus feature wiring in four files K5 already touches (`Cargo.toml`, `lib.rs`, `build.rs`, `main.rs`) and one line in K5's own `ssh.rs`.
2. **Own tiny I/O trait** (`SshBlobIo`) instead of reusing `StoreIo`/`VfsJournalStore`, so a Phase 06 restructure of `storage/` cannot break K4b and K4b needs no visibility edit in `storage.rs` (private `use runtime::VfsJournalStore`, `storage.rs:30-31`).
3. **Own sibling production-exclusion checker** instead of editing `scripts/check-production-relay-image.py` (Phase 02 is its "sole owner", `phase-02:46,74`; its test freezes the marker list).

What "OUT" would mean (acceptable, user's call): the program stops at T0; `plan.md` T1 row, K8 S12-S14 and the "dev-posture ceiling" wording would be T0-only. Honest cost: every KMS restart (crash `restart = "always"`, `config/services.toml:89-93`, or reboot) loses the key until an admin re-imports it.

## 1. Scope

**In:** the T1 persister behind K5's seam; record v2; dev root key; production exclusion; torn-slot behaviour; tests.
**Out (§12):** rollback protection, crash durability, any production claim, authority-held root, signed time, `ProductionQualified`, CellosFS changes (plan X), VFS changes, relay lifecycle changes, `libs/types`, kernel, client, admin surface, QEMU evidence (K8).

## 2. Entry gates

| Gate | State |
|---|---|
| K5 CP2 landed (the `ssh/` module and `SshPersist` seam exist) | pending (K5 CP1 not yet approved) |
| K5 T0 lane evidence (K8 RT0) shows the volatile path works; otherwise persistence adds nothing | pending |
| K4b lock handoff §9 approved (can ride in K5's packet as a pre-declared amendment, §9.4) | pending |
| D-K2 respected: label "synced, not crash-proof" (D78-2); `261025-cellosfs-crash-safety` stays `planned`, no lock row, no dependency (`lock-adoption-draft.md` K0 resolution 1) | decided |
| User decisions D-4b.1..D-4b.10 (§14) | pending |
| Image delta for the K8 lane: KMS needs VFS at runtime; `config/services.toml` has no `after = ["vfs"]` on `kms` (`:89-93`), see §5.5 for why K4b loads lazily | K8 |

## 3. Exact file closure (pre-edit SHA-256 at HEAD `fad076ee9`; K5-created files have no digest yet and are anchored by K5's dossier)

| Path | Action | SHA-256 before |
|---|---|---|
| `cells/services/kms/src/ssh/persist.rs` | new: `FilePersister` (`SshPersist` impl), A/B writer, loader | ABSENT |
| `cells/services/kms/src/ssh/record.rs` | new: record v2 codec, MAC, chain digest | ABSENT |
| `cells/services/kms/src/ssh/devroot.rs` | new: dev root constant and MAC key derivation | ABSENT |
| `cells/services/kms/src/ssh/blob_io.rs` | new: `SshBlobIo` trait; `VfsBlobIo` (`cfg(target_os = "none")`, wraps `ostd::clients::vfs::VfsClient`); `MemBlobIo` (`cfg(test)`, fault-injecting) | ABSENT |
| `cells/services/kms/src/tests/ssh/{persist,torn,exclusion}.rs` | new (registered in K5's `tests/ssh.rs`: one `mod` line in a K5-created file) | ABSENT |
| `cells/services/kms/src/ssh.rs` | K5 file: one `cfg` line selecting `type Persister = FilePersister` under `ssh-dev-persist` | (K5 post-edit anchor) |
| `cells/services/kms/Cargo.toml` | edit: `ssh-dev-persist = ["ssh-dev-custody"]` | `3888e6bf8320fb6d69be60f3e26ecd6ae3aab1e664572e602656e8cfa6b2d167` (pre-K5) |
| `cells/services/kms/src/lib.rs` | edit: `compile_error!` rules (§7) | `3f5c7f0c554a8949c41b81465c908111345d9b3cc9afb111b1bf80708609d49f` (pre-K5) |
| `cells/services/kms/build.rs` | edit: refuse `ssh-dev-persist` under `CELLOS_PRODUCTION` | `3faaa0787f994857bfeb7221e9f7e88e6b99e923ab3e41889ff7e16b4669af14` (pre-K5) |
| `cells/services/kms/src/main.rs` | edit: banner line with the marker under the feature | `3cce19d7fefee434367293498e316c9a840bdea7bee8487273c9f507efc22010` (pre-K5) |
| `scripts/check-ssh-dev-persist-exclusion.py` | new sibling checker | ABSENT |
| `scripts/test_check_ssh_dev_persist_exclusion.py` | new | ABSENT |

`Cargo.lock`: **no change** (no new dependency: BLAKE2s/`blake2`, `constant_time_eq`, `sha2`, `aes`, `ctr` are already KMS dependencies after K5). **No `libs/types/**` change** if D-4b.4 is accepted (§5.6). Untouched on purpose: `storage.rs`, `storage/**`, `lifecycle/**`, `dispatch.rs`, `dispatch/**`, `auth.rs`, `scripts/check-production-relay-image.py`, `scripts/test_check_production_relay_image.py`, `scripts/build-production-relay-image.sh`, every VFS file.

## 4. What exists today (observed)

* Stubs: `load_runtime_protected_relay_state` and `persist_runtime_protected_relay_state` return `Err(StoreError::PermissionDenied)` unconditionally (`storage.rs:76-89`); boot maps any load error to `RelayLifecycle::sealed()` (`dispatch.rs:310-325`), persist failure seals (`dispatch.rs:94-109`). Phase 06 will replace the stubs with an STM32 authority client (`phase-06:24,92`; its "Related Code Files" `:58-77` lists `storage.rs`, `storage/provider.rs`, `storage/provider/relay.rs`, `lifecycle/mod.rs`, `dispatch/{enrollment,relay}.rs`, `Cargo.toml`, `build.rs`, `lib.rs`, `dispatch.rs`, `tests.rs`) and states "VFS remains untrusted opaque transport ... never regains authority freshness, integrity, key, counter, or recovery responsibility" (`phase-06:36`).
* The existing relay store: `/srv/cellos/kms/slot-{a,b}.bin`, 186-byte records, magic `CKMS`, **version 1**, `slot` byte checked, BLAKE2s-256 MAC keyed by a test-only `JournalKey` (`record.rs:10-18,86-134`); A/B chain by `previous_slot_digest` (`journal.rs:134,174-184`); invalid files load as `None`, so both-invalid = `Empty` (`journal.rs:154-172`; probe §1.3); `RollbackDetected` only when both slots are valid and unchained (`journal.rs:49`). `StoreIo` has `ensure_dir/stat/read_file/write_file`, no delete, no sync (`backend.rs:14-20`).
* VFS: `/srv/cellos` and `/srv/cellos/` are owned by the live KMS (`rules.rs:40,93`, `access.rs:131-134`, `access/kms.rs:6-12,48-56`: caller cell, generation and tid nonzero and `lookup(service::KMS) == sender_tid`); writes also need `caller.may_mutate()` (KMS declares `VfsMutate`, `main.rs:13`); `can_remove_tree`/`can_remove_dir` exclude the KMS namespace (`access.rs:98-113`), single-file `Delete` goes through `can_write` (`paths.rs:115-127`). `write_file` is whole-file overwrite, non-atomic, limited to about 3.9 KB per call by the 4,096-byte IPC buffer (`libs/ostd/src/clients/vfs.rs:107-108`, `ipc.rs:23`); per-cell quota 32 MiB (`quota.rs:27`) but **1,100 bytes in a `test-hooks` VFS** (`manager.rs:108-109`: the K8 lane must not use a test-hooks VFS, or every write is `ERR_QUOTA`). `/srv` is the CellosFS Native volume "not an admitted durable journal" (`manager.rs:94-100`); `CellosFs::sync_durable` always reports unavailable (probe §2.4). One torn CellosFS root can make `open()` fail for the whole `/srv` volume (audit finding 3 in `261025-cellosfs-crash-safety/plan.md`).

## 5. Design

### 5.1 The provider (D-K7): a small x86-only development persister, not a `ProviderSlot` variant

`ProviderSlot` (`storage/provider/relay.rs:21-34`) is the relay/C2C provider boundary, and `code-standards.md` asks for "one protected-root provider boundary with independent, typed capability leaves". A new SSH variant would force edits to `relay.rs`'s five `match` blocks, exactly the file and enum Phase 06 extends with `Stm32`. K4b therefore adds **no** variant: the "new x86 development provider" is `FilePersister` plus `devroot.rs`, owned by the SSH leaf, reporting `assessment { anti_rollback_capable: false, production_capable: false }` semantics through the posture (`DevPosturePersistent`) and readiness only. `cfg`: `feature = "ssh-dev-persist"` and `any(target_arch = "x86_64", test)` (so host tests run on any x86_64 host; an aarch64/riscv64 cell build with the feature is a `compile_error!`).

### 5.2 Dev root key (D-4b.3)

Recommended **R1: a public build-time constant**, not a secret, not generated: `DEV_ROOT_LABEL = b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET"`; `mac_key = BLAKE2s-256(DEV_ROOT_LABEL)` (the same hash family as `record.rs`). The label doubles as the artifact marker (§7). Honest reading: it gives **format authentication and corruption detection**, not secrecy; anyone with the binary or the repo can compute it, and a disk holder can forge records (the same is true of any key stored on the same disk: R2 below does not change that).

Alternative **R2**: first-boot random 32-byte root in `/srv/cellos/kms/ssh-root.bin`. Needs a kernel RNG syscall the cell does not declare (`main.rs:13` has none; the kernel has a `dev-weak-rng` feature that the production checker forbids, `check-production-relay-image.py` `KERNEL_FORBIDDEN`), a second file and a lost-root failure mode, for no confidentiality gain against a disk holder. Not recommended.

What carries confidentiality at rest at T1 is the user's passphrase on the OpenSSH container (§5.4 policy), never the dev root.

### 5.3 Record v2 (`ssh-a.bin`, `ssh-b.bin`)

Files `/srv/cellos/kms/ssh-a.bin`, `/srv/cellos/kms/ssh-b.bin`, **disjoint from `slot-{a,b}.bin`**. Magic `CKMS` (same as v1), **version 2**: `JournalRecord::decode` rejects any version other than 1 (`record.rs:86-92`), so a v2 file can never be parsed as a relay slot, and the v2 decoder rejects v1 and wrong `kind`/`slot`. Integers little-endian. Variable length, maximum `217 + 2,048 + 32 = 2,297` bytes (within the about 3.9 KB single write).

| Off | Size | Field | Rule |
|---|---|---|---|
| 0 | 4 | magic | `CKMS` |
| 4 | 1 | version | `2` |
| 5 | 1 | slot | `0` = `ssh-a.bin`, `1` = `ssh-b.bin`; must match the file |
| 6 | 1 | kind | `1` = SSH identity |
| 7 | 1 | state | `0` tombstone (no key), `1` key present; other values invalid |
| 8 | 8 | revision | `>= 1`; equals the leaf's `store_revision` |
| 16 | 32 | previous_slot_digest | BLAKE2s-256 of the entire previous record (including its MAC); zeros for the first record ever written |
| 48 | 1 | name_len | `0` iff tombstone, else `1..=64` |
| 49 | 64 | name | `[A-Za-z0-9_-]`, zero after `name_len` |
| 113 | 1 | custody | `0` iff tombstone; `2` PassphraseWrapped; `1` Plain is **refused by the decoder** in this build (D-4b.2) |
| 114 | 32 | public_key | zero iff tombstone |
| 146 | 4 | kdf_rounds | `0` iff tombstone, else `1..=64` |
| 150 | 1 | salt_len | `0` iff tombstone, else `16..=64` |
| 151 | 64 | salt | zero after `salt_len` |
| 215 | 2 | ct_len | `0` iff tombstone, else multiple of 16 in `16..=2048` |
| 217 | `ct_len` | ciphertext | the OpenSSH wrapped private section, verbatim (already passphrase-encrypted by AES-256-CTR/bcrypt) |
| 217+`ct_len` | 32 | mac | BLAKE2s-MAC-256 (`Blake2sMac256`, as `record.rs:128-134`) keyed by `mac_key` over bytes `[0, 217+ct_len)`, compared with `constant_time_eq` |

Decoder strictness: exact file length, all padding and tombstone fields zero, every enum known, MAC checked **before** any field is trusted. Any failure returns `None` (slot invalid).

If D-4b.2 is overridden to persist Plain keys: `custody = 1`, `ct_len = 48` carrying `AES-256-CTR(enc_key, iv, seed || pub-check)` with `enc_key = BLAKE2s-256(DEV_ROOT_LABEL || "enc")`, `iv = BLAKE2s-256("iv" || revision || slot)[..16]` (unique per revision/slot), encrypt-then-MAC; no new crate (K5's `aes`/`ctr`). **Not designed further**: it makes the seed readable by anyone holding the disk and the binary.

### 5.4 Persistence policy (D-4b.2): T1 persists only passphrase-wrapped identities

A `Plain` identity (no passphrase) is kept RAM-only: `FilePersister::store` returns `Stored::VolatileOnly` and the named status reports `DevPostureVolatile` for it. **No plaintext private seed ever touches the disk at T1.** The only secret at rest is an OpenSSH `aes256-ctr`/`bcrypt` ciphertext (rounds 1..64) which an attacker with the disk can attack **offline**, without the limiter (K5's N = 5 is an online control): the strength is the passphrase's. K8's at-rest scan (S14) checks this.

### 5.5 Load, repair and the relay lifecycle

**Lazy load.** KMS starts without an ordering dependency on VFS (`config/services.toml:89-93`; `boot_probe_store` ignores errors, `storage.rs:33-39`). K4b therefore loads on the **first SSH opcode**, not at boot: `NotLoaded` -> one `SshBlobIo` read pass. An I/O error keeps `NotLoaded` (the opcode answers `SshKeyUnavailable`; the next SSH opcode retries once); no relay state, no `RelayLifecycle`, no `boot_lifecycle()` call is involved. The relay lifecycle (`dispatch.rs:310-325`, still `sealed` at runtime) is **not read, written or disturbed**; an SSH load/persist failure never seals the relay and a relay failure never affects SSH (independent leaves, row 5 of the K2 matrix).

**Load algorithm** (both slots read with `stat` + bounded `read_file(path, 2,297)`; decode each):

| Observation | Result | Leaf state / `readiness` | SSH opcodes |
|---|---|---|---|
| neither file exists | `Empty` | `Ready`, no key | all normal |
| exactly one valid record | `Loaded` (tombstone -> empty store) | `Ready` | normal |
| both valid and chained (`newer.previous == digest(older)`, revisions differ) | the newer | `Ready` | normal |
| both valid, not chained or equal revisions | `RollbackDetected` | `PolicyMismatch` (4) | 19 leaf query answers; 15-18 allowed (repair); 20-24 and named 19 -> `SshKeyUnavailable` |
| file(s) exist but none valid | `Corrupt` | `ProviderError` (3) | same as above |
| I/O or permission error reading | `IoError` | `NotLoaded`, retried next op | `SshKeyUnavailable` |

Difference from the relay journal on purpose: the relay journal maps "files present but invalid" to `Empty` (`journal.rs:154-172`); for SSH that would make a key silently vanish, so corruption is visible as `ProviderError`. **Repair path:** an admin import (op 15-17) is allowed in `PolicyMismatch`/`ProviderError`; it writes `revision = max(valid revisions, 0) + 1` into the slot that is not the newer valid one, chained to the newer valid record's digest (or revision 1, previous zeros, slot A when none is valid), after which the next load is `Loaded`.

**Status mapping (no ABI change):** `provider = None` (D-4b.4), `posture = DevPosturePersistent` when the key (or, for the leaf query, the build) is persisted and `DevPostureVolatile` for a RAM-only Plain key, `readiness` as above, `store_revision = record revision`.

### 5.6 Replacing the stubs without disturbing the relay (D-4b.4, D-4b.5)

The `PermissionDenied` stubs are **not replaced and not edited** by K4b. They are the relay lifecycle's seam and belong to Phase 06. K4b's persister is a second, SSH-only store beside them, reached through the SSH seam, so:

* `storage.rs`, `storage/**` stay byte-identical (their digests are listed in §3 "untouched"); Phase 06 replaces the stubs on its own schedule.
* `KmsProviderKind` is **not** extended (`libs/types/src/kms/model.rs:85-93`: values 0-5). K2 CP1 §1.1 said the T1 provider value "is K4b's append-only change"; that would be a Law-1 two-confirmation change plus a lock handoff for `libs/types` (`model.rs`, `tests/frame.rs`, `tests/ssh.rs`), the same files Phase 06 treats as historical-hash. `provider = None` with `posture = DevPosturePersistent` is already unambiguous and is what the approved ABI reserved. If the user prefers an explicit provider value (D-4b.4) it is a separate Law-1 slice that this draft does not scope.

### 5.7 Write protocol (all-or-nothing; K5 owns the ordering)

`store(rec)` / `tombstone(rev)` run **before** K5 swaps RAM and replies:

1. `ensure_dir("/srv/cellos")`, `ensure_dir("/srv/cellos/kms")` (once per boot).
2. Target slot = the inactive one (A when nothing is loaded); `revision = active.revision + 1`; `previous = digest(active)`.
3. Encode, `write_file(path, bytes)`, `read_file(path, len)`, byte-compare (`ReadbackMismatch` on any difference; same pattern as `journal.rs:76-89`).
4. Only then does K5 install the identity, bump `store_revision` and reply.

Failure at steps 3: reply `PersistFailed` (existing code 11), store, revision, floor and identity unchanged (K5 tests this with `MemBlobIo` fault injection before K4b exists). The failed write may leave the **inactive** slot torn; the active slot is untouched, and the next attempt rewrites the same inactive slot.

Delete writes a **tombstone** record (state 0) the same way; files are never deleted (`StoreIo`/`SshBlobIo` have no delete and tree/dir removal is denied under the KMS namespace anyway). A RAM-only Plain identity needs no tombstone: a tombstone is written iff the latest persisted state is "present".

### 5.8 Torn-slot and failure behaviour (what each case yields)

| Case | Outcome |
|---|---|
| Crash or error during the write of a new record (inactive slot) | slot fails length/MAC/magic: invalid; the older active slot loads; **the import or delete that was in flight is lost** (never acknowledged, or acknowledged only if the reply raced the crash: not proven) |
| Torn **tombstone** write | the previous "key present" record loads again: **a deleted key reappears** (documented non-guarantee, S13-style evidence) |
| Both slots valid, chained | newer wins |
| One slot reverted to an older valid copy, or two valid unchained slots | `RollbackDetected` -> `PolicyMismatch` (a dev-grade tripwire only: the chain is keyed by the public dev root, so a forger can produce a consistent pair) |
| Both files removed or reverted to before the first record | `Empty`: indistinguishable from "never imported" (not detectable) |
| Both present, none valid | `Corrupt` -> `ProviderError`, repair by re-import |
| v1 relay record copied into `ssh-*.bin`, or v2 into `slot-*.bin` | invalid in both directions (version/kind/slot/magic) |
| A bit flip anywhere in the file | MAC failure: invalid slot |
| CellosFS root corruption at `/srv` | VFS reads fail: `IoError` (retry) or `ERR_IO` writes: `PersistFailed`; **also affects every other `/srv` user (SSH host-trust store)** |

## 6. VFS path and authority needs

* Paths: `/srv/cellos/kms/ssh-a.bin`, `/srv/cellos/kms/ssh-b.bin` under the existing KMS-only prefix `/srv/cellos/` (`rules.rs:63-68`). **No VFS rule change** (`lock-adoption-draft.md` row: "new blobs under `/srv/cellos/kms/**` ... no VFS rule change").
* Authority: the live KMS service instance (`access/kms.rs:48-56`) with the `VfsMutate` syscall declared (`main.rs:13`). No other cell can read or write them (`can_read_fast` denies the namespace, `access.rs:90-95`).
* Sizes: file <= 2,297 B < about 3.9 KB per `write_file`; read with `read_file_bounded(path, 2,297)`.
* Quota: counted to the KMS cell (32 MiB default); K8 must use a VFS without the 1,100-byte test quota (`manager.rs:105-109`).
* Needed image facts (K8): a formatted `/srv` volume mounted (`manager.rs:94-100`), and KMS able to reach VFS (lazy load removes any boot-order dependency).

## 7. Production exclusion

Structural (no new checker dependency):

1. Feature `ssh-dev-persist = ["ssh-dev-custody"]`, default off.
2. `lib.rs`: `compile_error!` when `ssh-dev-persist` or `ssh-dev-custody` is combined with `hardware-relay-provider` (K5 adds the custody half), and when `ssh-dev-persist` is selected on any target that is neither `x86_64` nor `test`.
3. `build.rs`: panic when `CARGO_FEATURE_SSH_DEV_PERSIST` (or `..._CUSTODY`) is set together with `CELLOS_PRODUCTION`, exactly as `build.rs:7-10` does for the Silo provider.
4. Marker: the dev root label (`CELLOS-KSSH-T1-DEV-ROOT-v1`, §5.2) is linked into the binary only under the feature, plus a banner `[kms] DEV_POSTURE ssh persist (dev root key co-resident)` in `main.rs`.

Checker (new sibling, **not** an edit of `scripts/check-production-relay-image.py`): `scripts/check-ssh-dev-persist-exclusion.py --kms-features F [--artifact P ...] [--expect-dev]`. Default mode (production-like): exit 1 if `F` contains `ssh-dev-custody` or `ssh-dev-persist`, or any artifact contains the closed ASCII markers `CELLOS-KSSH-T1-DEV-ROOT-v1`, `ssh-dev-persist`, `ssh-dev-custody`, `DEV_POSTURE ssh` (chunked scan with overlap, copying the structure of `scan_artifact`, `check-production-relay-image.py:91-113`, including the chunk-boundary case that its test covers at `test_check_production_relay_image.py:118`). `--expect-dev` (K8 lane): inverse, requires the marker so the lane proves it booted the T1 build. Tests: features rejected by exact name; marker in artifact; marker split across a 64 KiB boundary; clean artifact passes; `--expect-dev` positive and negative. **Hand-off, not edit:** the three marker names are offered to the Phase 02 owner as a closed list (`phase-02:46`, `phase-04:245` pattern "hand off closed ASCII marker names"); until adopted there, both checkers run side by side wherever the production relay check runs. The existing checker already requires KMS features to select exactly `hardware-relay-provider` among providers (`check-production-relay-image.py:71`), which the `compile_error!` makes incompatible with the SSH features anyway.

## 8. Tests and evidence (host only; K8 owns QEMU)

Run with `cargo test --offline --locked -p service-kms --features ssh-dev-persist --target x86_64-unknown-linux-gnu`.

* `record`: golden hex for a present record (fixture wrapped container) and a tombstone; encode/decode round trip; every field's boundary (`ct_len` 16/2048/2064/not multiple of 16, rounds 0/1/64/65, salt 15/16/64/65, custody 0/1/2/3, state 0/1/2, name charset/length, nonzero padding); wrong magic/version/slot/kind (including a v1 relay record: `JournalRecord::ENCODED_LEN` 186 bytes); MAC covers every byte (flip each of the first 2,297 bytes of a record: decode is `None`).
* `torn`: truncate a record at every length 0..=len-1; write failure after k bytes (`MemBlobIo` fault points: before write, mid-write truncation, readback mismatch, read error); expectation per §5.8 table row, including "deleted key reappears" asserted as the documented outcome, both-valid-unchained -> `PolicyMismatch`, none-valid -> `ProviderError`, repair import -> `Loaded`.
* `persist`: import persists before reply; persist failure -> `PersistFailed` with identity/revision/floor unchanged; delete writes a tombstone and a failing tombstone leaves the key intact; reload in a **new** `KmsService` over the same `MemBlobIo` restores identity, `store_revision`, posture `DevPosturePersistent`, custody; Plain key: `VolatileOnly`, reboot -> `Empty`, and **no byte of any file written during import/sign/delete contains the seed, passphrase or the Plain private section** (byte scan of the fake's files); limiter counter and unlock slot are **not** persisted (reset at reboot, stated).
* `relay_independence`: a failing/corrupt SSH store changes no relay status (`RelayLifecycle::sealed()` unchanged at runtime builds, fixture relay `Ready` unaffected); relay seal does not affect SSH.
* `exclusion`: `compile_error!` demonstrations by `cargo check --features hardware-relay-provider,ssh-dev-persist` (expected failure with the diagnostic) and for a non-x86 target; `CELLOS_PRODUCTION=1 cargo check --features ssh-dev-persist` panics in `build.rs`; the Python checker tests (§7).
* Evidence artifacts for K8 S12-S14: file names, record sizes, marker presence in `/bin/kms`.

## 9. Lock rows and conflicts (handoff content, step form of `lock-handoff-k2-cp2.md`)

### 9.1 Slice identity
KSSH K4b: T1 persistent SSH custody (`DEV_POSTURE`): new files under `cells/services/kms/src/ssh/`, four feature-wiring edits, one sibling checker. Serves Cellbox S9-5 / CBX-24 publickey (not an Intel C2C outcome). No ABI change, no kernel change, no dependency change.

### 9.2 Contract delta against ADR-0024 principles
None beyond K5's: no capability, handle, delegation or revocation concept; **durable logical state** gets a dev-only SSH record that is explicitly not protected (no rollback, no crash durability) and is not claimed as trusted state; caller/test/doc migration: K5's tests and `docs/code-standards.md` gain one sentence ("T1 persists passphrase-wrapped identities only, `DEV_POSTURE`"); no remote impact.

### 9.3 Locked rows and conflicts
| Plan / row | Overlap | Resolution |
|---|---|---|
| `260825-1726-kms-silo-production-root` (`cell-contract-0024-locks.md:147`) | the new files and four edits sit under `cells/services/kms/**` | file-bounded exception (§3) |
| `260826-1605-phase4-dev-reference-authority` Phase 06 (`:149` row; stub replacement `phase-06:24,92`; "Related Code Files" `:58-77`) | **C-K4b-1**: Phase 06 will replace `load_/persist_runtime_protected_relay_state`; K4b must not do it or collide with it. **Avoidance: K4b never edits `storage.rs`, `storage/**`, `lifecycle/**`, `dispatch/{relay,enrollment}.rs`, `dispatch.rs`**; textual overlap is limited to `Cargo.toml`, `lib.rs`, `build.rs` (shared with K5, pre-declared in K5's ruling C-K5-1) and `main.rs` (not in Phase 06's list) | request the ruling "Phase 06 rebases over K5/K4b on those shared files; the SSH persister is not trusted state in the `phase-06:36` sense (it holds no freshness, counter, key or recovery responsibility and is labelled `DEV_POSTURE`); Phase 06's stub replacement does not read or write `ssh-*.bin`" and a reciprocal note |
| same, **C-K4b-2** `phase-06:36` "VFS never regains authority ... responsibility" and `code-standards.md:122-125` ("volatile state is never a substitute"; relay wording) | the SSH leaf stores wrapped key material on VFS | stated so it is not misread: the SSH record is neither authority state nor a relay floor; no `ProductionQualified` |
| same, Phase 02 sole ownership of `scripts/check-production-relay-image.py` and its frozen-list test (`phase-02:46,74`; `test_check_production_relay_image.py:52`) | production exclusion for K4b | **C-K4b-3**: K4b adds a sibling script and hands off marker names; it never edits the Phase-02 files (their digests: `check-production-relay-image.py` `5cb07d35ee48434e54aa6210590c74e473a2ea6da54c7640136af33f29af7d65`, test `b13e1c298fb6581fbd7dc33840fc08d241b74f1ed9f3608c16be3eadf8e74304`, `build-production-relay-image.sh` `3ee7736b91a8a9478e03257c488082ebfe3fda2ffe5288dbcbb88f96fa560cda`; Phase 08 later modifies all three, `phase-08:73`) |
| VFS mutation authority row `260603-1803-phase-h-vfs-hardening` (`:120`) | none: path already KMS-only, no VFS file touched | none |
| `261025-cellosfs-crash-safety` (planned; no lock row, `lock-adoption-draft.md` K0 res. 1) | soft dependency only (D-K2) | no relation requested; any "survives power loss" claim stays forbidden until its own CP1s |
| `libs/types/src/kms/**` (Law 1) | none if D-4b.4 = no `KmsProviderKind` append | otherwise a separate Law-1 two-confirmation slice |

### 9.4 Packaging of the handoff
Preferred: K4b's closure and the C-K4b rulings are **pre-declared as an amendment inside K5's handoff** (`k5-cp1-draft.md` §14) so the owner rules on the full `cells/services/kms/src/ssh/**` closure once and K4b CP2 needs only a go-ahead; alternative: a standalone packet of the same shape after K5 CP2. Retained gates and execution boundary: as K5 §14.5/§14.7; rollback = remove the new files and revert the four wiring edits (no persistent effect in the repo; on a lane disk the `ssh-*.bin` files are inert for every other component).

## 10. What T1 does NOT prove (must appear in every T1 status report and in K8's dossier)

1. **No rollback resistance:** restoring an older valid record pair, or deleting both files, yields the older key or `Empty` with no alarm (the chain is a tripwire keyed by a public constant).
2. **No crash durability:** `write_file` + readback is "synced, not crash-proof" (D78-2); a torn tombstone resurrects a deleted key; an acknowledged import may not survive power loss (`CellosFs::sync_durable` is a skeleton).
3. **Dev root key is co-resident and public** (R1): it provides integrity against accidents, no confidentiality or authenticity against anyone holding the disk or the repo.
4. **Confidentiality at rest = passphrase strength only,** attackable offline up to bcrypt rounds <= 64 without the online limiter; a Plain key is never written (policy), but the wrapped ciphertext and public key are readable by a disk holder.
5. **Limiter and unlock state are volatile:** a reboot resets the failure counter.
6. **No production claim:** never `ProductionQualified`; no authority, time, or hardware binding; ADR-0006 NO-GO unchanged.
7. **Shared-volume blast radius:** a CellosFS corruption caused or exposed by SSH writes can take down the SSH host-trust store and other `/srv` users.

## 11. Risks

Phase 06 later restructuring `StoreIo` (neutralized by the own `SshBlobIo`); the lazy-load retry loop under a permanently failing VFS (one extra VFS call per SSH opcode; bounded); a corrupt-but-present store showing `ProviderError` until an admin re-imports (by design); Phase 02 owner declining the marker hand-off (sibling checker still guards); the 1,100-byte test-hooks VFS quota breaking a lane silently (called out for K8); CellosFS torn-root coupling (§10.7); the K5 seam signature (`Stored`) being changed by K5 review (tracked: both drafts name it).

## 12. Out of scope

Rollback floor, monotonic counter, signed time, authority/TPM binding, qualified provider, crash-safe store (T2/K9, plan X); a `KmsProviderKind` value (unless D-4b.4); VFS changes, `StoreIo` changes, `storage/**` edits, relay lifecycle changes; Plain-key persistence (unless D-4b.2); persisted limiter; image or `services.toml` changes (K8); client, admin surface, kernel flag; any edit to Phase-02-owned scripts.

## 13. Effort

M, about 1 week for one owner after K5: record codec and strict decoder 1 day; persister, `SshBlobIo` + VFS glue, lazy load and repair 1.5 days; tests including the torn matrix 2 days; sibling checker and tests 0.5 day; wiring, handoff amendment, dossier 1 day.

## 14. Decisions requested from the user (at CP1)

| ID | Decision | Recommendation |
|---|---|---|
| D-4b.1 | K4b in or out of the program now | **IN, sequenced after K5 and the T0 lane; design approved now, build later** |
| D-4b.2 | Persist Plain keys? | **No**: persist only passphrase-wrapped identities; Plain stays RAM-only |
| D-4b.3 | Dev root source: R1 public constant vs R2 first-boot random file | **R1** |
| D-4b.4 | Append a `KmsProviderKind` value (Law-1 slice) vs `provider = None` + `DevPosturePersistent` | **no append** |
| D-4b.5 | Own `SshBlobIo` vs reuse `StoreIo`/`VfsJournalStore` (needs a visibility edit in `storage.rs`) | **own trait** |
| D-4b.6 | Sibling checker + marker hand-off vs editing the Phase-02 checker | **sibling + hand-off** |
| D-4b.7 | Lazy load on first SSH opcode vs load at boot | **lazy** (no VFS ordering dependency) |
| D-4b.8 | Corrupt/unchained -> visible `ProviderError`/`PolicyMismatch` with import as repair, vs silently `Empty` like the relay journal | **visible** |
| D-4b.9 | Persist the failure counter | **no** (volatile, stated) |
| D-4b.10 | Pre-declare K4b's closure in K5's handoff (one owner ruling) vs a second packet | **pre-declare** |

## 15. Not checked

No compile, test or VM run; the K5 files do not exist yet (every K5 name here comes from `k5-cp1-draft.md`); `StoreIo`/VFS behaviour under a torn write (no fault injection on CellosFS was observed); whether CellosFS `write_file` replaces in place or via a new file (only the VFS-level overwrite semantics were read); boot ordering of `kms` against the VFS cell and whether a blocked `VfsClient` call can stall KMS at first use; KMS cell memory impact of the VFS client; `kernel/src/loader/boot_ceiling.rs` rows; the `GetRandom` syscall allowlist (R2); the K3 flag; `ssh-keygen` behaviour on the produced wrapped blobs; whether the Phase 02 owner accepts the marker hand-off; `.github/workflows/ci.yml` wiring for the new Python test file.
