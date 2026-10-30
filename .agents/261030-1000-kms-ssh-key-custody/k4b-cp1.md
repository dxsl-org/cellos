# KSSH K4b CP1: T1 persistent SSH custody behind the K5 `SshPersist` seam, `DEV_POSTURE` (final text; refreshed against the landed K2/K3/K5/K6/K7 code at HEAD `070f0d0a1`; documents only; nothing applied)

Base: [k4b-cp1-draft.md](k4b-cp1-draft.md) (SHA-256 `bd117f72ca32bd11b9f63aa9c6c816f760c5351bf9e217990d8f20527888e1ff`, written at HEAD `fad076ee9`, before any of K3/K5/K6/K7 existed). That draft is superseded by this text: every K5 name in it came from `k5-cp1-draft.md`; here every K5 name, line and digest was read from the landed tree. Form: as `k5-cp1.md` / `k7-cp1.md` (final CP1 with exact closure and a lock handoff in the form of `k3-cp1.md` section 11).

Written 2026-10-30, HEAD `070f0d0a145bf407ef89f9f8d3d0d43c14e55c43`, `git status --short` empty when read. Documents only: no source, manifest, lock, script, key, network, VM, QEMU, Docker, image or commit action was made to produce it; the only commands run were read-only (`sha256sum`, `git log/status/ls-files`, `grep`, `sed`, file reads; no `cargo`). `file:line` citations were read in this session; `[INFERENCE]` marks anything not observed; "Decisions requested" (section 14) lists only genuinely open choices.

Bound inputs (SHA-256): `k5-cp1.md` `6ff24401d391681b59a21610f6b8e6cb845c48caa21f182f69c960853e4d3d3e` (equals `k5-cp1-approval.md`); `reports/k5-cp2-dossier.md` `18389fb136781c51e76608625aa21ec6563fc63473d5e51e5046dd48b49452f0`; `plan.md` `19c68420da8e435dbbf50a2620d4497f3161d6d52209a6ab59f9404577e4b9ef` (log of 2026-10-30: "K4b design approved (build after K5/T0 lane)"); `k7-cp1.md` `f1c02a8c1493988e4c327423f6579fd9906a50b971837a8e74ffdcd5eab9a108`; `k3-cp1.md` `f3141cf41445921dfc6fc448c7dc231f293e7694a03aaeeec6d6ab764a67519e`; `.agents/cell-contract-0024-locks.md` `012bbe84a9677b7ac501f72f1879471e1a286577c3e9051d690f5bf3af455963`. Digest form: SHA-256 of whole files including the final newline.

## 0. Summary and the honest ceiling

**What K4b delivers.** A second implementation of the landed K5 persistence seam: `FilePersister`, which writes the one passphrase-wrapped SSH identity as a versioned, MAC-framed record (v2) into one of two files `/srv/cellos/kms/ssh-{a,b}.bin` through the VFS service, reads it back in the same boot, and reloads it on the first SSH opcode after the KMS cell starts. It sits behind opt-in feature `ssh-dev-persist` (implies `ssh-dev-custody`, default off, x86_64 bare-metal only). K5's custody logic, ABI (`libs/types`), kernel, client and admin cell are unchanged. T0 (K5) stays the volatile lane and is built and evidenced first.

**What it does not deliver (stated plainly; this paragraph MUST appear in every T1 status report and in K8's dossier).** The label is `DEV_POSTURE` and never `ProductionQualified`.
1. **No rollback protection.** Restoring an older pair of record files (or the whole disk image) loads the older key state with no alarm; deleting both files looks like "never imported". The A/B hash chain is only a tripwire for a half-reverted pair and is forgeable by anyone who can write the files.
2. **No crash or power-loss durability claim.** "Persisted" means: `VfsRequest::Write` returned `Ok` and an immediate read-back in the same boot was byte-equal. CellosFS `sync` is "observed sync is not crash-preserving acknowledgement" (`cells/services/vfs/src/backend_cellosfs.rs:1`); `sync_durable` is a skeleton that always returns `DurabilityUnavailable` (`libs/cellos-fs/src/engine.rs:850-852`); `261025-cellosfs-crash-safety` is `planned` with no lock row (D-K2: "synced, not crash-proof"). A torn tombstone write resurrects a deleted key; an acknowledged import may not survive power loss.
3. **The dev root key is a public constant co-resident with the data.** The record MAC gives format authentication and corruption detection against accidents. It gives no authenticity and no confidentiality against anyone who holds the disk and the repo or the `kms` binary, who can forge a consistent record pair.
4. **Confidentiality at rest is the passphrase and the bcrypt rounds the key was imported with, nothing else.** A disk holder can attack the wrapped ciphertext offline without K5's online limiter. K8's RT1 identity uses `-a 4` (cheap KDF, D8-8), so that lane's at-rest strength is weak by construction. A `Plain` (unwrapped) identity is never written to disk by this slice.
5. **The failure counter, unlock slot, import slot and replay floors are volatile**; a KMS restart resets the limiter.
6. **Shared blast radius:** the volume is the `/srv` CellosFS that also holds the SSH host-trust store; a CellosFS failure triggered or exposed by a KMS write poisons the whole `/srv` backend for every user until reboot (`backend_cellosfs.rs:90-102`).

## 1. Decisions carried, and what the landed code resolved

### 1.1 Carried (approved 2026-10-30; recorded in `plan.md` log and `k3-k8-bundle.md` item 5)
K4b IN, sequenced after K5 **and after K8's T0 lane**; own `SshBlobIo` trait (no reuse of `StoreIo`/`VfsJournalStore`); no `KmsProviderKind` append (`provider = None`, posture `DevPosturePersistent`); no `storage.rs`/`storage/**` edit (the relay `PermissionDenied` stubs stay Phase 06's). K8 uses private fixture images only (route A), one run per set, no rerun; **T0 first, T1 only after K4b CP2** (K8 D8-12).

The draft's other recommendations (D-4b.2 Plain never persisted, D-4b.3 public-constant dev root R1, D-4b.6 sibling checker, D-4b.7 lazy load, D-4b.8 visible corruption, D-4b.9 volatile counter, D-4b.10 pre-declared closure) were part of the design that the owner approved as recommended; this text keeps them and re-derives each from landed facts. They are re-asked only where a landed fact changes the choice (section 14). [INFERENCE: the 2026-10-30 approval of "the K4b design" is read to cover these sub-decisions because the bundle lists them under item 5 and the draft recommended each; the owner may reopen any in section 14.]

### 1.2 Delta against the draft (everything below was verified in the tree; each row changes the design or the closure)
| # | Draft said | Landed fact | Consequence in this text |
|---|---|---|---|
| 1 | K5 has a cfg-selected `Persister` type alias; K4b adds "one cfg line in K5's `ssh.rs`" | There is no alias: `KmsService.ssh` is typed `SshLeaf<NoPersist>` (`dispatch.rs:29`) and built `SshLeaf::serving(NoPersist)` (`dispatch.rs:56`); `NoPersist` is also named in tests (`tests/ssh.rs:106-108`, `:415-416`) | K4b adds the alias and a constructor in `ssh.rs` **and a two-line edit in `dispatch.rs`** (a K5 shared file, C-K5-1). Section 4.1 keeps every K5 test valid by making the alias `NoPersist` under `cfg(test)` |
| 2 | `Loaded::Loaded(record)` | `Loaded::Found { revision, identity: StoredIdentity }`, `Empty`, `Corrupt`, `RollbackDetected`, `IoError` (`ssh.rs:60-69`); `Corrupt`, `Empty` and `RollbackDetected` carry **no revision** | Revision consequences in 4.6: after a tombstone boot or a repair import the leaf counts from 0/1 again. Accepted without a seam change (D-4b.14) |
| 3 | Absent file: `stat` returns `NotFound`, "neither file exists => `Empty`" | The VFS answers a **missing path with `ERR_IO`** (`cells/services/vfs/src/dispatch_paths.rs:29-35`: `Stat` of an absent path is `ERR_IO`; `dispatch_file_handles.rs:27-30` `OpenFileAt` and `dispatch_dirs.rs` read-at the same), and the ostd client maps wire code 1 to `ViError::IO` (`libs/ostd/src/clients/vfs/read_file/wire.rs:4,12`); only handle errors map to `NotFound`. `VfsJournalStore::stat` (`storage/runtime.rs:28-31`) maps `NotFound` to `None`, so it would report a missing slot as an I/O error | The draft's load pass cannot be built as written. 4.3 distinguishes absent from failed with `ensure_dirs` + `stat` + a directory listing; this is a further, independent reason for the own `SshBlobIo` |
| 4 | "the next attempt rewrites the same inactive slot" after a failed write | A failed or non-syncing **VFS mutation poisons the entire `/srv` CellosFS backend**: `mutate` sets `NativeState::Poisoned` on any error and every later read/write then fails (`backend_cellosfs.rs:90-102`, `:77-88`); `ERR_IO` follows from `vfs.healthy` | After a real CellosFS write failure, retry is futile until reboot: `PersistFailed` now, `IoError`/`SshKeyUnavailable` afterwards. Retry logic is not built; tests use a `MemBlobIo` that models the poison |
| 5 | wrapped `ct_len` 16..=2048 | The parser admits a wrapped private section only if `len % 16 == 0` **and `len >= 144`** (`ssh/parser.rs:179-180`, `padded_min(16)` = 144) and `<= 2,048` (`:182-184`) | Decoder bounds are 144..=2,048, multiple of 16 |
| 6 | `build.rs` refuses `ssh-dev-persist` under `CELLOS_PRODUCTION` | `build.rs` already panics on `CARGO_FEATURE_SSH_DEV_CUSTODY` + `CELLOS_PRODUCTION` (K5), and `ssh-dev-persist = ["ssh-dev-custody"]` sets both | **`build.rs` stays untouched** (one fewer Phase 06 shared file); a test proves the transitive refusal |
| 7 | "`lib.rs`: compile_error with hardware relay (K5 adds the custody half)" | `lib.rs:70-74` already has it | `lib.rs` gets one new rule only: the target restriction (section 5, item 2) |
| 8 | boot banner under the feature | K5 already prints `[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf` (`main.rs:19-20`), which is false for a T1 build | `main.rs`: the custody banner is cfg'd to `not(persist)` and a persist banner added |
| 9 | syscall allowlist `[INFERENCE]` | Verified: no manifest change (section 3.4) | none |
| 10 | lock rows `:147` / `:149` | At HEAD the rows are `cell-contract-0024-locks.md:153` (`260825-1726-kms-silo-production-root`) and `:155` (`260826-1605-phase4-dev-reference-authority`); the K5 slice paragraph is `:119` | handoff uses the current lines |
| 11 | file digests "pre-K5" | K5 landed in `236b79915`; every K5 "after" digest equals the file now (section 7) | closure re-anchored |
| 12 | Phase 02 owns the checker; sibling script | Still true and now stated more strongly: `phase-02-private-protocol-and-dev-separation.md:46` "Phases 3-7 hand off closed ASCII marker names only ... never checker edits" (status `blocked`) | sibling checker + marker hand-off retained (section 5, items 4-5) |
| 13 | K5 left docs for later | `docs/code-standards.md` still says "Until KMS behaviour lands (KSSH K5) every SSH opcode answers `SshKeyUnavailable`" (line 87-90); `k5-cp2-dossier.md` section 9 lists it as owner debt | closure includes that one bullet (K5 landed, T1 wrapped only) and the `CHANGELOG.md` entry |

## 2. Entry gates

| Gate | State at HEAD `070f0d0a1` |
|---|---|
| K5 CP2 landed (`ssh/` module, `SshPersist` seam) | **met** (`236b79915`) |
| K3 / K6 / K7 landed (flags, client, admin cell) | **met** (`47a9d5bda`, `297a89a9e`, `070f0d0a1`) |
| K8 T0 lane (RT0) run and accepted: persistence adds nothing if the volatile path was never shown to work end to end | **pending** (K8 CP1 in progress; user decision: T0 first) |
| This CP1 + the section 11 handoff approved and recorded | pending |
| Baseline capture at CP2 start (G-5): `-p types` and `-p service-kms` (default, `ssh-dev-custody`) counts, builds, closure digests | pending. Reference only (K5 dossier section 4, HEAD `13d6e0273`): `types` 74, `service-kms` 65, `--features ssh-dev-custody` 122 passed / 0 failed / 1 ignored. K6/K7 did not touch `cells/services/kms/**`, so these should hold; recapture, do not assume |
| Start-of-slice overlap re-check of the four plans "not examined" at K2/K5 (`260825-sdk-delivery` P02, `260712-1902-dice-attestation-identity` P03, `260819-1409-cell-to-cell-anywhere-core` P02, `migration-ledger.md` E12) | pending |
| D-K2 respected: label "synced, not crash-proof"; no lock row or dependency on `261025-cellosfs-crash-safety` | decided |
| K8 image delta: formatted `/srv` volume mounted in the lane image; VFS **not** built with `test-hooks` (1.1 KiB quota, `cells/services/vfs/src/manager.rs:105-110`) | K8 |

## 3. Landed facts the design stands on (all read this session)

### 3.1 The K5 seam (`cells/services/kms/src/ssh.rs`, 327 lines)
* `trait SshPersist { fn load(&mut self) -> Loaded; fn store(&mut self, &RecordView<'_>) -> Result<Stored, PersistError>; fn tombstone(&mut self, next_revision: u64) -> Result<Stored, PersistError>; }` (`ssh.rs:73-77`); `Stored { Written, VolatileOnly }` (`:45-50`); `PersistError` is a unit struct (`:55`); `Loaded { Empty, Found { revision, identity }, Corrupt, RollbackDetected, IoError }` (`:60-69`); `LeafState { Serving, Unavailable, NotLoaded, Repairable(KmsCapabilityReadiness) }` (`:100-107`).
* `SshLeaf<P: SshPersist>` is generic; `new(p)` starts `NotLoaded`, `serving(p)` starts `Serving` (`:134-141`). `ensure_loaded` (`:161-182`): `Empty` -> `Serving`; `Found` -> identity + revision + posture `DevPosturePersistent` + `Serving`; `Corrupt` -> `Repairable(ProviderError)`; `RollbackDetected` -> `Repairable(PolicyMismatch)`; `IoError` -> stays `NotLoaded`.
* Handlers (`dispatch/ssh.rs`): admission first (`:66-69`), then the leaf query (op 19, empty name) answers in any state (`:74-77`, `:155-170`), otherwise `ensure_loaded()` (`:79`) and a state gate: `Serving` only, except `Repairable` admits ops 15-18 (`:80-84`, `is_import_op` `:43-51`); everything else is `SshKeyUnavailable`. **No persister call happens before admission** (K5 test `admission::no_persister_call_before_admission_passes`).
* Import commit: `next_revision()` (`import.rs:270`, `:313`), `persist.store(&identity.record_view(revision))` (`:273-274`) **before** any RAM change, `PersistFailed` on error, posture from `Stored` (`:276-279`). Delete: `persist.tombstone(revision)` (`dispatch/ssh.rs:206-209`) before the RAM swap, posture from `Stored` (`:211`, `posture_of` `:312-317`). Import into an existing identity is refused earlier (`SshKeyExists`/`SshStoreFull`, `import.rs:116-122`), so replacing a key is delete then import.
* `RecordView { revision, name, public_key, secret: SecretView }`, `SecretView::{Plain(&[u8;32]), Wrapped{salt, rounds, ciphertext}}` (`ssh/store.rs:39-54`); `StoredIdentity::{plain, wrapped}` constructors return `Option` (`:62-98`); bounds in `ssh/parser.rs:19-25` (`KDF_SALT_MIN 16`, `KDF_SALT_MAX 64`, `KDF_ROUNDS_MAX 64`, `STORE_WRAPPED_CT_MAX 2,048`, all `pub(crate)`).
* The leaf's initial posture is `DevPostureVolatile` and nothing resets it on `Loaded::Empty` (`ssh.rs:148` area, `with_state`): **with an empty T1 store the leaf query reports `DevPostureVolatile`**; the posture describes the stored key and flips to `DevPosturePersistent` when a wrapped key is written or loaded. K8 must read the status that way (section 9).

### 3.2 Existing KMS provider/stub structure (not touched)
`load_runtime_protected_relay_state` / `persist_runtime_protected_relay_state` return `Err(StoreError::PermissionDenied)` unconditionally (`storage.rs:76-89`); boot maps a load error to `RelayLifecycle::sealed()`. `ProviderSlot` is the relay/C2C provider boundary (`storage/provider/relay.rs`); Phase 06 extends it with a hardware authority provider. The v1 relay journal uses `slot-{a,b}.bin`, magic `CKMS`, version 1, 186-byte records, BLAKE2s-256 MAC keyed by a test-only `JournalKey` (`storage/record.rs:9-18,86-134`), and `StoreIo` has `ensure_dir/stat/read_file/write_file` only (`storage/backend.rs:14-20`). `VfsJournalStore` exists behind `cfg(target_os = "none")` (`storage.rs:13-14,30-31`) and `boot_probe_store` only `stat`s and ignores errors (`storage.rs:33-39`). K4b adds **no** `ProviderSlot` variant and does not call, wrap or edit any of this: the SSH leaf stays an independent leaf (K2 matrix row 5).

### 3.3 VFS and CellosFS as seen from the KMS service cell
* **Authority.** `/srv/cellos` (exact rule) and `/srv/cellos/` (prefix rule) are owned by the live KMS only (`access/rules.rs:40,93`; `access.rs:131-134`; `access/kms.rs`: caller cell, generation and tid nonzero and `lookup(service::KMS) == sender_tid`). Writes also need `caller.may_mutate()` (`access.rs:69`); KMS declares `VfsMutate` (`main.rs:13`). `can_read_fast` denies the namespace (`access.rs:90-95`), so every KMS read is served on the ecall path that proves "live KMS". Tree/dir removal under the namespace is denied (`access.rs:98-113`). Nothing else can read or write the two files.
* **Reads are handle-addressed.** The only client read API is `VfsClient::read_file_bounded`: `OpenRootDir("/")`, `OpenDir` per component, `OpenFileAt`, `StatAt`, chunked `ReadFileHandle`, `CloseFile`/`CloseDir` (`libs/ostd/src/clients/vfs/read_file/session.rs:44-62`). Each step is authorised with `can_read` (`dispatch_dirs.rs:128,153,179`, `dispatch_file_handles.rs:24,59,128`): `/` and `/srv` fall to the readable root rule, `/srv/cellos` and below to the KMS rule. [INFERENCE: no VFS test opens `/srv/cellos/kms/**` through handles as the live KMS (`read_path_tests.rs:350` only proves denial for another reader); K8's RT1 is the first execution.]
* **Writes.** `VfsClient::write_file` is one `VfsRequest::Write`, whole-file create-or-overwrite, limited by the 4,096-byte IPC buffer (`libs/api/src/services/ipc.rs:23`; client doc "about 3.9 KB"). A record is at most 2,297 bytes. `write_file` takes a namespace transient lease (`paths.rs:63-77`), charges the quota to the KMS cell (32 MiB default, `quota.rs`; overwrite refunds) and calls the backend `write` = `ensure_file` + `replace_file` + `sync` (`backend_cellosfs.rs:179-185`), where `replace_file` is "functional in-place mutation, not a crash-safe transaction" (`libs/cellos-fs/src/engine.rs:371-377`).
* **Poisoning.** Any `mutate` error, including a `sync` failure, moves the backend to `Poisoned` (`backend_cellosfs.rs:90-102`); after that every operation reports `ERR_IO` (`vfs.healthy`). `Unavailable` (unformatted `/srv`) is the same from the client side.
* **Directories.** `Mkdir` of `/srv/cellos` and `/srv/cellos/kms` is allowed to KMS (`dispatch_paths.rs:69-90`); `mkdir` of an existing path is `ERR_IO` (`backend_cellosfs.rs:210-218`), so the client must `stat` first (as `VfsJournalStore::ensure_one` does, `runtime.rs:56-62`).
* **Listing.** `ListDir` answers lines `f:NAME\n` / `d:NAME\n` on CellosFS (`backend_cellosfs.rs:129-142`), empty for an empty directory, `ERR_IO` when unhealthy or the directory is missing (`dispatch.rs:86-93`). The `VfsClient::list_dir` doc says "newline-separated entry names" without the tag; K4b accepts both forms (4.3). [INFERENCE for non-CellosFS backends.]

### 3.4 Syscall allowlist, manifest, ceiling: no change needed
`declare_manifest!(block_io = false, network = false, spawn = false)` and `declare_syscalls![Send, Recv, RecvTimeout, Log, LookupService, VfsMutate]` (`main.rs:12-13`). `VfsClient` calls go through `ServiceRef::call` = `sys_lookup_service_bound` + `sys_ipc_submit` / `sys_ipc_take` / `sys_ipc_wait` / `sys_ipc_cancel` (`libs/ostd/src/service.rs:117-206`, `ipc.rs:318-392`). Allowlist bits: `IpcSubmit`/`IpcReply` share bit 0 with `Send`, `IpcTake`/`IpcWait`/`IpcCancel`/`IpcCurrent` bit 1 with `Recv` (`libs/api/src/abi/syscall.rs:991-992`), and `LookupServiceBound` shares bit 37 with `LookupService` (`:1052-1056`); `VfsMutate` is the declared bit 63. So the already declared set covers every call K4b makes (`stat`, `mkdir`, `write_file`, `read_file_bounded`, `list_dir`); `/bin/kms` has the `LEGACY_OBSERVE_MUTATE` ceiling row (`kernel/src/loader/boot_ceiling.rs:201`) and K3 gave it no new right. **K4b changes no manifest, no syscall list, no ceiling and no kernel file.** `VfsClient::new()` resolves lazily on the first call (`vfs.rs:33-36`), so constructing the persister at boot does no IPC. The IPC wait is unbounded (`service_call` -> `exchange(.., None)`): a wedged VFS stalls the single-threaded KMS loop (section 12 risk 3).
Startup order: `config/services.toml:89-93` gives `kms` no ordering after `vfs`; K4b therefore loads lazily (4.4), as the draft decided.

## 4. Design

### 4.1 Shape: a persister behind the existing seam, selected by an alias
New module tree under the SSH leaf, all compiled only with `ssh-dev-persist`:

```
cells/services/kms/src/ssh/
  persist.rs            FilePersister<IO: SshBlobIo> : SshPersist (slot choice, load classification, write protocol)
  persist/record.rs     record v2 codec, MAC, digest
  persist/devroot.rs    DEV_ROOT_LABEL constant, mac_key()
  persist/blob_io.rs    trait SshBlobIo, Slot, listing parser; VfsBlobIo (cfg(target_os = "none"))
```
`ssh.rs` gains (cfg `ssh-dev-persist`) `pub(crate) mod persist;` and

```
#[cfg(not(all(feature = "ssh-dev-persist", not(test))))]
pub(crate) type Persister = NoPersist;
#[cfg(all(feature = "ssh-dev-persist", not(test)))]
pub(crate) type Persister = persist::FilePersister<persist::blob_io::VfsBlobIo>;
pub(crate) fn new_leaf() -> SshLeaf<Persister>   // serving(NoPersist) | new(FilePersister::new(VfsBlobIo::new()))
```
`dispatch.rs:29,56` become `SshLeaf<crate::ssh::Persister>` and `crate::ssh::new_leaf()` (two lines inside the existing `cfg(feature = "ssh-dev-custody")` regions).

**Why the alias is `NoPersist` under `cfg(test)`:** K5's 122 tests build `KmsService::new()` and assert volatile behaviour (`store::restart_clears_state`, posture asserts at `import.rs:485`, `store.rs:273`); `tests/ssh.rs:106-108` pins `type P = NoPersist` for the `KmsService` device. Keeping `NoPersist` under test leaves all of them valid and **unedited** under `--features ssh-dev-persist`. The persister is tested at the leaf level (`SshLeaf<FilePersister<MemBlobIo>>`), which is already a supported device (`impl<P: SshPersist> Dut for SshLeaf<P>`, `tests/ssh.rs:135-153`), through the same ten handlers. The one thing host tests cannot cover is the two-line alias/constructor wiring; it is compile-checked by the `x86_64-unknown-none` feature build and executed by K8 RT1 (stated in section 15). Outside `test`, `lib.rs` forbids the feature on anything but x86_64 bare metal (section 5, item 2), so there is no silent volatile fallback.

`FilePersister` is `no_std`, holds no secret (the only secret that reaches it is a `Plain` seed it never reads) and does not import `crate::storage`: a source-scan test asserts this (section 6). It is not a `ProviderSlot` variant: `ProviderSlot` is the relay boundary and its `match` blocks are exactly what Phase 06 extends.

### 4.2 `SshBlobIo`: two files, no paths in the persister
```
pub(crate) const RECORD_MAX: usize = 217 + 2_048 + 32;          // 2,297
pub(crate) enum Slot { A, B }                                    // ssh-a.bin, ssh-b.bin; paths live only in blob_io.rs
pub(crate) struct BlobError;                                     // any I/O failure, no detail reaches a reply
pub(crate) enum Read { Absent, Present(usize), Oversize }
pub(crate) trait SshBlobIo {
    fn ensure_dirs(&mut self) -> Result<(), BlobError>;                              // /srv/cellos, /srv/cellos/kms
    fn read(&mut self, slot: Slot, out: &mut [u8; RECORD_MAX]) -> Result<Read, BlobError>;
    fn write(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), BlobError>;
}
```
Structural property: through this trait the persister can touch exactly two files; there is no delete, no rename, no path argument, no directory removal. The persister never needs `stat` or `list`: those are implementation details of `VfsBlobIo::read`.

### 4.3 `VfsBlobIo` (x86_64 bare metal) and the absent-versus-failed rule
Wraps one `ostd::clients::vfs::VfsClient`.
* `ensure_dirs`: for `"/srv/cellos"` then `"/srv/cellos/kms"`: `stat` -> `Ok((_, true))` continue; `Ok((_, false))` -> error; `Err(_)` -> `mkdir`, success continues, failure -> error. Done once per boot after the first success. **Load is therefore not read-only on a blank volume**: it creates the two directories (what `VfsJournalStore::ensure_dir` does). A failed `mkdir` means VFS unavailable, `/srv` unformatted or poisoned, or denied -> `BlobError`.
* `read(slot)`: `stat(path)`: `Ok((size, false))` -> `size > RECORD_MAX` is `Oversize`, else `read_file_bounded(path, RECORD_MAX)` (`OutOfMemory` = grew meanwhile = `Oversize`; any other error = `BlobError`) and copy into `out` -> `Present(len)`; `Ok((_, true))` (a directory in the way) -> `BlobError`; `Err(ViError::PermissionDenied | NotFound)` -> `BlobError`; **`Err(ViError::IO)` is ambiguous on this wire** (3.3 and row 3 of 1.2: absent file and failing backend both answer `ERR_IO`), so the implementation resolves it with `list_dir("/srv/cellos/kms")`: a successful listing with **no entry for the file name** -> `Absent`; a listing that names the file (as `f:NAME`, or bare `NAME`; a `d:NAME` entry, or a near miss such as `xssh-a.bin`, `ssh-a.bin.tmp`, does not match as a file) or any listing failure -> `BlobError`. The directory's existence was proven by `ensure_dirs` in the same pass, and `ListDir` itself answers `ERR_IO` for a missing directory or an unhealthy backend, so "absent" is reported only for a healthy backend whose directory lists without the file. The listing parser is a pure function (host-tested). Direction of every uncertainty is toward `BlobError`/`IoError`, never toward `Absent`: a wrong guess about a foreign listing format can only make the key unavailable, never make it silently vanish. [INFERENCE: format on other backends; CellosFS format read at `backend_cellosfs.rs:129-142`.]
* `write(slot)`: one `write_file`; any error -> `BlobError`.
* Cost: a cold load is about 6 IPC round trips for `ensure_dirs` plus per slot `stat` (+ `list_dir` when absent) or about 10 for a handle-based read; a store is one write plus one read-back. KMS blocks for these calls (single-threaded loop, unbounded wait; section 12 risk 3).

### 4.4 Lazy load and the persister's memory
KMS starts without a VFS ordering dependency; `boot_probe_store` already ignores errors. The load runs from `ensure_loaded()` on the first **admitted** SSH opcode (K5's lazy path; an unadmitted caller can never trigger I/O). An I/O failure keeps `NotLoaded`: that opcode answers `SshKeyUnavailable`, the next one retries the whole pass (bounded: a few VFS calls per attempt). The relay lifecycle (`boot_lifecycle`, `RelayLifecycle::sealed()` at runtime) is neither read, written nor disturbed; an SSH load/persist failure never seals the relay and a relay failure never touches SSH.

`FilePersister` state: `io`, `dirs_ready: bool`, `head: Option<Head { slot, revision, digest: [u8;32], present: bool }>` (the newest valid record seen or written) and `repair_target: Option<Slot>` set by a load that found an unusable pair. Nothing in it is secret. Stack: encode and decode buffers are 2,297 bytes each plus `Loaded::Found`'s inline identity (about 2.2 KiB, K5 accepted it, `ssh.rs:59`); `VfsClient` methods place two 4 KiB buffers on the stack per call (`vfs.rs:25-26`, `service.rs:177`). [INFERENCE: total depth added is under about 30 KiB against the 256 KiB cell stack assumed in K5 CP1 section 11; K5 measured `main` at 19,304 B. K8 observes; a host stack test on a 64 KiB thread covers the persister frames.]

### 4.5 Record v2 (`ssh-a.bin`, `ssh-b.bin`), little-endian, variable length
Magic `CKMS` (same family as v1) and **version 2**: the v1 decoder rejects any version other than 1 (`storage/record.rs:86-92`), so a v2 file can never parse as a relay slot, and the v2 decoder rejects v1, a wrong `kind`, or a wrong `slot`. Files are disjoint from `slot-{a,b}.bin`. Maximum 2,297 bytes (within one write).

| Off | Size | Field | Rule |
|---|---|---|---|
| 0 | 4 | magic | `CKMS` |
| 4 | 1 | version | `2` |
| 5 | 1 | slot | `0` = `ssh-a.bin`, `1` = `ssh-b.bin`; must equal the file read |
| 6 | 1 | kind | `1` = SSH identity |
| 7 | 1 | state | `0` tombstone, `1` key present; else invalid |
| 8 | 8 | revision | `>= 1`; informational (4.6) |
| 16 | 32 | previous | BLAKE2s-256 of the whole previous record (including its MAC); zeros for the first record ever written or after a repair with no valid predecessor |
| 48 | 1 | name_len | tombstone: `0`; else `1..=64` |
| 49 | 64 | name | `types::kms::validate_ssh_key_name` charset `[A-Za-z0-9_-]`; zero after `name_len` |
| 113 | 1 | custody | tombstone: `0`; present: `2` PassphraseWrapped. `1` Plain is **refused by the decoder** |
| 114 | 32 | public_key | tombstone: zeros |
| 146 | 4 | kdf_rounds | tombstone `0`; present `1..=64` (`KDF_ROUNDS_MAX`) |
| 150 | 1 | salt_len | tombstone `0`; present `16..=64` (`KDF_SALT_MIN/MAX`) |
| 151 | 64 | salt | zero after `salt_len` |
| 215 | 2 | ct_len | tombstone `0`; present: multiple of 16 in `144..=2,048` (`parser.rs:179-184`) |
| 217 | `ct_len` | ciphertext | the OpenSSH wrapped private section verbatim (already AES-256-CTR/bcrypt-encrypted by the user's passphrase) |
| 217+`ct_len` | 32 | mac | `Blake2sMac256` keyed by `mac_key()` over `[0, 217+ct_len)`, compared with `constant_time_eq`, MAC checked **before** any field is trusted |

Decoder strictness: exact file length, every padding and tombstone field zero, every enum known, bounds above; any failure yields "slot invalid". The decoder builds the identity with `StoredIdentity::wrapped(name, public, salt, rounds, ct)` (K5 constructor, `None` => invalid). Dependencies: `blake2 0.10` (`Blake2sMac256`, as `storage/record.rs:1-6`), `constant_time_eq 0.4` and `types` are already unconditional KMS dependencies: **no `Cargo.toml` dependency and no `Cargo.lock` change**. The v1 `authenticator` is `cfg(test)`-only (`storage.rs:105-107`) and the storage tree is untouched, so `persist/record.rs` carries its own 8-line MAC helper.

### 4.6 Slot rules, revisions, chain
* Newest = the record that **chains**: record X is the successor of Y iff `X.previous == digest(Y)`. Revision numbers are never compared for order. This makes a repair import (revision restarts at 1, below the old pair) and a restart after a tombstone (leaf counter 0) order correctly.
* Target slot for a write: the slot that is not `head.slot`; slot A when there is no head; after a load that found both slots valid but unchained, the slot not holding the higher-revision record (tie: B); after a load with files present but none valid, A.
* `store` writes `revision = rec.revision` (K5's `next_revision()`), `previous = head.digest` (zeros without a head). `tombstone(next_revision)` the same with state 0.
* Leaf `store_revision` equals the record revision after a normal import/delete/reload. Two seam-bound, accepted discrepancies (D-4b.14): (a) after reboot on a **tombstone** the leaf loads `Empty` and counts from 0 again; (b) a **repair import** from `Corrupt`/`RollbackDetected` produces revision 1. Both are informational only (the revision gates nothing).

### 4.7 Persistence policy and the write protocol
**Only passphrase-wrapped identities are persisted (D-4b.2).** For `SecretView::Plain` the persister returns `Ok(Stored::VolatileOnly)` **without reading the seed and without any I/O**; the leaf then reports `DevPostureVolatile` for that key and a restart yields `Empty`. A tombstone is written iff `head.present`; otherwise `tombstone` returns `VolatileOnly` with no I/O (a RAM-only key has nothing persisted to delete). No plaintext private seed ever touches the disk.

`store(rec)` / `tombstone(rev)` run before K5 swaps RAM and replies (K5 owns the ordering and tests it with a fake persister):
1. `ensure_dirs` if not yet done.
2. Encode into the stack buffer; `write(target, bytes)`.
3. `read(target)` and byte-compare with what was written (`Read::Present(len)` and equal; anything else is failure; the same pattern as `storage/journal.rs:76-89`).
4. Only then update `head` and return `Stored::Written`.
Any failure returns `PersistError` (K5 replies `PersistFailed`, code 11) with `head`, leaf identity, revision and floor unchanged. The failed write may have torn the **target** slot; the active slot was never touched. Files are never deleted (and the VFS would refuse tree/dir removal under the namespace anyway). If the write succeeded but the read-back failed or differed, the new record may nevertheless be on disk and valid: an unacknowledged import can then appear after a restart (a documented non-guarantee, torn matrix 4.9).

### 4.8 Dev root key co-residency (R1, carried)
`DEV_ROOT_LABEL = b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET"`; `mac_key() = BLAKE2s-256(DEV_ROOT_LABEL)`. A public build-time constant, not generated and not a secret. **Co-resident** means: the key is in the `kms` binary, the data is on the disk image that ships with that binary, and both are in the repo/artifacts; anyone holding the disk plus the binary (or the repo) can recompute it and forge a consistent record pair. It is a format/corruption check with domain separation, not authentication. The alternative R2 (first-boot random root file) needs a kernel RNG syscall the cell does not declare (`main.rs:13`; the kernel RNG for development is `dev-weak-rng`, forbidden by the production checker) and a second file with a lost-root failure mode, for no gain against a disk holder, so it is not recommended and not designed. The label is linked only under the feature and doubles as an artifact marker (section 5). What carries confidentiality at rest is the user's passphrase (policy 4.7), never the dev root.

### 4.9 Load classification and failure matrix
Both slots are read (`ensure_dirs`, then `read(A)`, `read(B)`), each decoded for its own slot id.

| Observation | `Loaded` | Leaf state / readiness | SSH opcodes |
|---|---|---|---|
| neither file exists | `Empty` (head none) | `Serving`, no key | normal |
| exactly one valid record; the other absent **or invalid** (torn newer write) | `Found` if present, `Empty` if tombstone (head set either way) | `Serving` | normal |
| both valid and one chains onto the other (`X.previous == digest(Y)`) | the successor; `Found` or `Empty` by its state | `Serving` | normal |
| both valid, neither chains onto the other | `RollbackDetected` (repair target set) | `Repairable(PolicyMismatch)` (4) | op 19 leaf query answers; 15-18 allowed (repair); 20-24 and named 19 -> `SshKeyUnavailable` |
| file(s) present but none valid (or `Oversize`) | `Corrupt` (repair target A) | `Repairable(ProviderError)` (3) | same as above |
| any `BlobError` (VFS down, `/srv` unformatted, poisoned, listing failure, directory in the way) | `IoError` | stays `NotLoaded` (retry on next admitted opcode) | `SshKeyUnavailable`; leaf query reports `Unavailable` |

Difference from the relay journal on purpose: that journal maps "files present but invalid" to `Empty` (`storage/journal.rs:34-60`; `read_slot` `:154-172` yields `None` for any invalid file); for SSH that would make a key silently vanish, so corruption is visible as `ProviderError`. **Repair path:** an admin import (ops 15-18 are the only admitted ops in `Repairable`, `dispatch/ssh.rs:80-84`) writes a record chained to the newest valid predecessor (or none) into the repair target slot; the next load is `Found`. A repair import of a **Plain** key persists nothing (policy), so a `Corrupt`/`Rollback` store stays unusable across the next boot; repairing it durably needs a wrapped key.

Torn-slot behaviour (what each case yields):
| Case | Outcome |
|---|---|
| crash or error during the write of a new record (target slot) | target fails length/MAC/magic: invalid; the older active slot loads; **the in-flight import or delete is lost** (acknowledged to the caller only if the reply raced the crash: not proven) |
| torn **tombstone** write | the previous "present" record loads again: **a deleted key reappears** (documented non-guarantee; asserted by a test as the expected outcome) |
| write ok, read-back failed | `PersistFailed` now; the new record may still be valid on disk and load after restart (unacknowledged write appears) |
| both valid, chained | the successor wins |
| one slot reverted to an older valid copy, or two valid unchained slots | `RollbackDetected` (a dev-grade tripwire only) |
| both files restored to an older consistent pair, or both removed | `Found` (old state) or `Empty`: **not detectable** |
| none valid | `Corrupt` -> `ProviderError`; repair by import |
| v1 relay record copied into `ssh-*.bin`, or a v2 record into `slot-*.bin` | invalid in both directions (version, kind, slot, magic) |
| a bit flip anywhere | MAC failure: slot invalid |
| CellosFS poisoned at `/srv` | reads and writes fail: `IoError` / `PersistFailed`; **also affects every other `/srv` user** (host-trust store) |

## 5. Production exclusion

Guards, in order of strength (K5-landed ones are verified in the tree):
1. **Feature graph.** `ssh-dev-persist = ["ssh-dev-custody"]`, default off. Every guard on `ssh-dev-custody` therefore applies to a persist build: `lib.rs:70-74` `compile_error!` with `hardware-relay-provider`, and `build.rs` panic "ssh-dev-custody is forbidden in production builds" under `CELLOS_PRODUCTION` (`build.rs` K5 edit). The Phase 02 checker already requires a production KMS to select exactly `hardware-relay-provider` (`check-production-relay-image.py:65-73`), which cannot compile with custody. **K4b does not edit `build.rs`**; a demonstration proves the transitive refusal (section 8).
2. **Target restriction (one new `lib.rs` rule).** `#[cfg(all(feature = "ssh-dev-persist", not(test), not(all(target_arch = "x86_64", target_os = "none"))))] compile_error!(...)`: D-K7 (x86 development provider); also forbids a host or other-arch build from silently resolving `Persister = NoPersist`.
3. **Banner and marker.** `main.rs`: the K5 custody banner is cfg'd to `all(ssh-dev-custody, not(ssh-dev-persist))`; under persist: `[kms] DEV_POSTURE ssh-dev-persist: wrapped identity persisted to /srv, dev root key co-resident, no rollback or crash-durability claim`. The dev root label is linked only under the feature (used at runtime by `mac_key()`, so it stays in `.rodata`) [INFERENCE until an artifact scan observes it].
4. **Sibling artifact checker (new; the Phase 02 files are not edited).** `scripts/check-ssh-dev-persist-exclusion.py --kms-features F --kms-artifact P [--expect none|custody|persist]`. Exit 0 pass, 1 violation, 2 usage. Markers (closed ASCII): `CELLOS-KSSH-T1-DEV-ROOT-v1`, `DEV_POSTURE ssh-dev-persist`, `DEV_POSTURE ssh-dev-custody`, `ssh-dev-persist`, `ssh-dev-custody`. `--expect none` (default, production-like): fail if `F` names `ssh-dev-custody` or `ssh-dev-persist`, or the artifact contains any marker. `--expect custody` (K8 I-T0 lane): the custody banner must be present, the root label and persist banner absent. `--expect persist` (K8 I-T1 lane): persist banner and root label present, custody banner absent. Chunked scan with overlap, copying `scan_artifact` (`check-production-relay-image.py:91-113`) including the chunk-boundary case its test covers (`test_check_production_relay_image.py:118`). Its tests: features by exact name; each marker; marker split across a 64 KiB boundary; missing/empty artifact; each `--expect` mode both ways. The lane proves it booted the intended variant; the script cannot prove anything about a different binary.
5. **Marker hand-off to Phase 02 (text only, C-K4b-3).** Phase 02 may later add these five names to `KMS_FORBIDDEN`/`FROZEN_DEV_MARKERS` through its reviewed manifest (`phase-02:46`); until then the sibling script is the guard. K5's dossier section 9 asked for the same hand-off for the custody banner.

## 6. Tests (host only; QEMU is K8)

Command at CP2: `cargo test --offline --locked -p service-kms --features ssh-dev-persist --target x86_64-unknown-linux-gnu` (K5's 122 + 1 ignored run unchanged, plus the new groups below), plus the unchanged default and `--features ssh-dev-custody` runs, plus `python3 scripts/test_check_ssh_dev_persist_exclusion.py`. `MemBlobIo` (`tests/ssh/persist/mem_io.rs`, test-only, never in the cell) = two in-memory files, a `dirs` flag, fault points (fail before write, tear after k bytes then error, wrong read-back, read error, `ensure_dirs` error) and a **poison mode** that fails every later call after the first failure (models `backend_cellosfs.rs:90-102`); it can be reopened over its files to model a reboot.

| Group | Content |
|---|---|
| `record` | golden hex for a present record (fixture wrapped container: the public OpenSSH `aes256-ctr` fixture already embedded by K5, rounds 16) and a tombstone, computed independently in Python at CP2 from the public constant (no key generation); round trip; every bound (`ct_len` 128/144/160/2,048/2,064/not multiple of 16, rounds 0/1/64/65, salt 15/16/64/65, custody 0/1/2/3, state 0/1/2, name charset/length, nonzero padding); wrong magic/version/slot/kind including a 186-byte v1 relay record; flip each byte of a record: decode is "invalid" (MAC covers every byte) |
| `devroot` | label pinned byte for byte; `mac_key()` golden |
| `listing` | `f:ssh-a.bin\n`, `d:ssh-a.bin\n`, bare name, near misses (`xssh-a.bin`, `ssh-a.bin.tmp`, `ssh-a.binx`), both files, empty, no trailing newline: the pure matcher; absent only on a listing that names no file |
| `load` | every row of the 4.9 table (neither / one valid + torn other / both chained both orders / unchained / none valid / `Oversize` / `BlobError` at `ensure_dirs`, `read(A)`, `read(B)`), leaf states and readiness through `SshLeaf::new(FilePersister<MemBlobIo>)`; truncate a record at every length `0..len` |
| `torn` | each fault point on import and on delete; "deleted key reappears" asserted as the documented outcome; write-ok/read-back-fail leaves the record that a new leaf then loads (documented); poison mode: `PersistFailed` then `IoError` for every later call, leaf `NotLoaded` after restart |
| `leaf` | import persists before reply and before RAM swap; `PersistFailed` leaves identity, revision, floors, unlock slot unchanged (reuses K5 snapshot helpers); a new leaf over the same files restores identity, `store_revision`, `custody = PassphraseWrapped`, posture `DevPosturePersistent`, then **unlock + sign byte-equal to the K5 wrapped-fixture signature**; delete writes a tombstone, failing tombstone keeps the key, reload after tombstone is `Empty` (revision 0, 4.6a); repair import after `Corrupt` and after `RollbackDetected` -> `Serving`, next reload `Found` (revision 1, 4.6b); second-import-after-delete chains to the tombstone; `IoError` then success on the next opcode; limiter counter, unlock slot, floors reset on a new leaf |
| `plain` | Plain import: **zero `write` calls** on the fake, posture `DevPostureVolatile`, reload `Empty`; Plain delete: zero writes; wrapped persisted then Plain imported after delete: no resurrection |
| `secrecy` | byte scan of every byte the fake ever received (and of the `Debug` output of the leaf and persister) for the seed, passphrase, plain private section and the decrypted inner section across an import/unlock/sign/delete cycle: none occurs (only the verbatim ciphertext does, by design) |
| `boundary` | admission before I/O: unadmitted callers cause **zero** fake calls (the K5 property, repeated with the real persister); `include_str!` scan: `persist.rs`/`persist/*.rs` contain no `storage::`, `lifecycle`, `RelayLifecycle` or `ProviderSlot`; relay readiness is not an input |
| `stack` | persister frames (load, store, tombstone) on a 64 KiB thread, `--release`, as K5's stack test |
| python | the checker tests (section 5, item 4) |

K5's existing tests, K2's six stub-arm tests (feature off) and the default build stay unchanged. Not testable on host: `VfsBlobIo` (needs the VFS cell), the two-line wiring (4.1), and the VFS-side effects (K8).

## 7. Exact file closure (SHA-256 re-taken at HEAD `070f0d0a1`; any other path stops the slice)

Edited (digests are the files now; they equal the K5 CP2 dossier "after" values):
| Path | Action | SHA-256 now |
|---|---|---|
| `cells/services/kms/Cargo.toml` | edit: feature `ssh-dev-persist = ["ssh-dev-custody"]`, comment; no dependency | `116fb261e7fb5115a1bbe655b0628681b3feaa9bae1900cb3455a2f858d77206` |
| `cells/services/kms/src/lib.rs` | edit: the one target-restriction `compile_error!` (section 5, item 2) | `b59323a873b79f22429367c6f4c63caefbfc4aeeae436f1af747a37b822522b1` |
| `cells/services/kms/src/main.rs` | edit: persist banner; custody banner cfg'd `not(persist)` (section 5, item 3) | `8089fe1d9c143ce06f9f78b74d66f296ee65ccc7e1a1f1bea23362ea5e07d0c2` |
| `cells/services/kms/src/ssh.rs` | edit (K5 file): `mod persist` (cfg), `Persister` alias, `new_leaf()`, `allow(dead_code)` on `NoPersist`/`serving` only as needed for lint-clean builds of both feature sets | `de8b408714ad76da504c4b219c5b14095c98d5f61e5b37ed362c4e8ce173da5e` |
| `cells/services/kms/src/dispatch.rs` | edit (K5 shared file, C-K4b-1): `dispatch.rs:29` field type and `:56` constructor only | `e84f8633774419ca036ffe73a0cfac0326c4d4786db94c4421c02b42e400cd7c` |
| `cells/services/kms/src/tests/ssh.rs` | edit (K5 file): `#[cfg(feature = "ssh-dev-persist")] mod persist;` only | `bce808d45e196e63de8d8b80683c093f2cd5d3fa3d1754409a174c494ec61c51` |
| `docs/code-standards.md` | edit: the "Until KMS behaviour lands" bullet (`:87-90`): K5 landed; T1 persists passphrase-wrapped identities only, `DEV_POSTURE`, no rollback/crash claim | `f7081cc1b41a417913f0f6111767b10410690f4c999ff3cab475f5db90c0769e` |
| `CHANGELOG.md` | edit: one entry at CP2 | `29b6356dfb424f66ff5013e9cf6bbbdb2e6b8bc676d5585a1cf388153ecb28b3` |

New (all ABSENT now):
| Path | Content |
|---|---|
| `cells/services/kms/src/ssh/persist.rs` | `FilePersister`, load classification, slot rules, write protocol |
| `cells/services/kms/src/ssh/persist/{record,devroot,blob_io}.rs` | codec + MAC + digest; label + `mac_key`; `SshBlobIo`, `Slot`, listing parser, `VfsBlobIo` (`cfg(target_os = "none")`) |
| `cells/services/kms/src/tests/ssh/persist.rs` + `tests/ssh/persist/{mem_io,record,devroot,listing,load,torn,leaf,plain,secrecy,boundary,stack}.rs` | section 6 |
| `scripts/check-ssh-dev-persist-exclusion.py` | sibling checker (section 5, item 4) |
| `scripts/test_check_ssh_dev_persist_exclusion.py` | its unit tests |

Byte-identical on purpose (hashes at HEAD; a closure-external edit stops the slice): `cells/services/kms/build.rs` `792505f3f152be9c322eb27666105777a61a1b0e3604254e882d76711373e900`; `src/auth.rs` `c0e43e3b02a1b17401ae51572781c88325625323c66fdbacd97ff616518cda71`; `src/tests.rs` `337295452c62694f318af705ac5aa40eb3e72897a7884bcd0ef03301bd8356f6`; `src/storage.rs` `d3fa592c54782bced2a09d58912b42eb43054e253126c9eda8f27751dd89c0d2`; `src/storage/{backend,runtime,record}.rs` `a44591d8…8b92` / `f12a8807…30b8` / `36860893…f254` (full: `a44591d80324fe52707a75fedc46eac9c41e7e79e23ef06b6af90a9d171f8b92`, `f12a8807d0fcfc22aea25eb9eab078ae41a71e5abf4f3be2c2446278be2530b8`, `36860893efe7acc32e18af468192520941836956e087ed9476d55ca98607f254`); `src/storage/**`, `src/lifecycle/**`, `src/dispatch/**` (including `dispatch/ssh.rs` `307717a22244a3e0cd6af1ccf31fddbebebc711756a2055af09534dbdf4b6fac`), the other `src/ssh/*.rs` (`floor` `7eb92a00…`, `import` `0b204cab…`, `limits` `b644e896…`, `parser` `eb6d0623…`, `sign` `ae89f5f3…`, `store` `55250449…`, `unlock` `9323798e…`, `unwrap` `718fefc0…`, as in the K5 dossier); `Cargo.lock` `5181251ccc712f27f71db9861b1b4feab0a8fec910e43716a7b744a4f8c81130` (changed since K5 by K7's local package stanza; **K4b adds no dependency edge, so it must not change**); `libs/api/**`, `libs/types/**`, `libs/ostd/**` (`clients/vfs.rs` `dff8f69fd56b70eb1ac98f0fa3a80dd5410e44b23ab61a137ddbe9aa6b900f97`), `kernel/**`, `cells/services/vfs/**` (cited read-only: `access.rs` `33e16f3f…`, `access/kms.rs` `ae0646b7…`, `access/rules.rs` `ed2e7c99…`, `paths.rs` `e64685b1…`, `dispatch_paths.rs` `4f229e80…`, `backend_cellosfs.rs` `2b11d930…`, `manager.rs` `57e6c5a4…`, `quota.rs` `379d6b88…`), `libs/cellos-fs/**` (`engine.rs` `954f9437…`), `config/services.toml` `188d4d1966a91ad0bb4fb0336548a27969f86f349bd2b05041704ed8d5deecdb`, `.github/workflows/ci.yml` `0d77f752b62a2c92e1b89bdde366caf911ca553ca7bb41638942e7fc0def9a79`, `scripts/check-production-relay-image.py` `5cb07d35ee48434e54aa6210590c74e473a2ea6da54c7640136af33f29af7d65`, `scripts/test_check_production_relay_image.py` `b13e1c298fb6581fbd7dc33840fc08d241b74f1ed9f3608c16be3eadf8e74304`, `scripts/build-production-relay-image.sh` `3ee7736b91a8a9478e03257c488082ebfe3fda2ffe5288dbcbb88f96fa560cda`, `cells/tools/**`, images, signing, promoted `POLICY.BIN`.

Law 1: **neither `libs/api` nor `libs/types` is touched** (no `KmsProviderKind` value, no error code, no payload: `PersistFailed = 11` and `DevPosturePersistent = 2` already exist, `libs/types/src/kms/model.rs:180,164`). No Law-1 confirmation text is needed or offered. If the owner prefers an explicit T1 provider value instead of `provider = None` (D-4b.4, not recommended), that is a separate Law-1 slice needing its own confirmation 1 on that exact delta; this CP1 does not scope it.

## 8. Build and evidence plan (CP2; nothing run here)

Baseline first (section 2). After edits, private `CARGO_TARGET_DIR` (the S10 census includes `target/`), x86_64 flag set as `reports/k2-cp2-dossier.md` section 3 / K5 dossier section 6 (`CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C no-redzone=yes -Z cf-protection=full -C relocation-model=pic"`):
* `cargo build --release --locked --offline -Z build-std=core,alloc -p service-kms` for `x86_64-unknown-none`: default, `--features ssh-dev-custody`, `--features ssh-dev-persist`; record sections and `.text` growth of persist over custody. **Acceptance: the default and the custody-only x86_64 binaries are byte-identical to a build of a `git archive HEAD` copy** (K5 observed byte identity for the default; the custody-only build should also hold because the alias resolves to the same type, `new_leaf()` to the same call, and the banner string is unchanged) [INFERENCE until built; fall back to identical section sizes and an instruction-identical diff].
* Guards: `--features "ssh-dev-persist hardware-relay-provider"` -> K5's `compile_error!` fires; `--features ssh-dev-persist` for `riscv64gc-unknown-none-elf` and `aarch64-unknown-none-softfloat` -> the new target rule fires; `CELLOS_PRODUCTION=1 ... --features ssh-dev-persist` -> `build.rs` panic naming `ssh-dev-custody`; `CELLOS_PRODUCTION=1` default build passes.
* `cargo clippy --no-deps -p service-kms --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` (the CI form) clean for the default feature set; host `--lib --tests -D warnings` clean with `ssh-dev-custody` and with `ssh-dev-persist`; `rustfmt --check` on every closure path.
* `cargo metadata --offline --locked` succeeds; `git diff Cargo.lock` is empty.
* Run `scripts/check-ssh-dev-persist-exclusion.py` against the three x86_64 artifacts with `--expect none|custody|persist` and record the exit codes (the persist artifact must contain the root label: first observation of that [INFERENCE]).
* Dossier (as K5's): digests of every closure path before/after, `git diff --stat`, test counts, builds, the Python test run.

## 9. Evidence handed to K8 (RT1, T1 set; K8 owns QEMU)

Disk facts for S12-S14: files `/srv/cellos/kms/ssh-a.bin` and `ssh-b.bin` on the P5 CellosFS volume; first import writes slot A at revision 1, the next record B, alternating; a record is 217 + ct_len + 32 bytes (for the K5 `-a 4`/`-a 16` fixtures a few hundred bytes); `slot-*.bin` are never written. Status semantics K8 must use: after a wrapped import `posture = DevPosturePersistent`, `provider = None`; **with an empty store the leaf query reads `DevPostureVolatile`** (3.1); `store_revision` equals the record revision after reload. S12: boot 2 loads lazily on the first admitted SSH opcode (the K7 status call or the first login), so "status shows the key" is evidence of the load. S13 (rollback unprotected): restoring a consistent older pair loads the older state silently; a half-reverted pair reports `PolicyMismatch`, and K8 records either. S14: the verbatim wrapped ciphertext **will** appear in the disk image (by design); the seed, the decrypted inner section and the passphrases must not. A Plain identity is RAM-only at T1, so a `Plain` import in RT1 shows `DevPostureVolatile` and does not survive reboot (a negative scenario worth one row). K8 images must not use a `test-hooks` VFS, must mount a formatted `/srv`, and should run `--expect custody` on I-T0 and `--expect persist` on I-T1. The lane evidence of the section 0 ceiling is rollback shown unprotected and S12c informational only.

## 10. What T1 does NOT prove (must appear in every T1 status report and in K8's dossier)
The six points of section 0, restated as claims never made: rollback resistance; crash/power-loss durability or atomic delete; authenticity or secrecy against a disk holder (public dev root); confidentiality beyond passphrase strength and the imported rounds; persistence of the failure counter; production, authority, signed-time or hardware binding (`ProductionQualified`, ADR-0006 NO-GO unchanged); isolation of the `/srv` volume from other users' failures.

## 11. Lock handoff packet content (CELL-CONTRACT-0024; form of `k3-cp1.md` section 11; DRAFT for the accountable owner, not submitted)

**11.1 Slice identity.** KSSH K4b: T1 persistent SSH custody for the single wrapped ed25519 identity behind the K5 `SshPersist` seam, `DEV_POSTURE`, opt-in feature `ssh-dev-persist`, host tests only (QEMU evidence is K8's separate gate). Serves Cellbox Phase 09 S9-5 / CBX-24 publickey **and not an Intel C2C outcome**; the owner is asked to confirm the same bounded-slice basis as K2/K3/K5/K7. Controlling plan `.agents/261011-1200-cell-contract-authority/plan.md` (status complete; completion unlocks nothing). Evidence reconciled: `k5-cp1.md` + `k5-cp1-approval.md` (C-K5-1..3), `k5-cp2-dossier.md`, `k2-cp1.md`, this document; no history erased.

**11.2 ADR-0024 contract delta.**
| Principle | Effect of K4b |
|---|---|
| Cell principal distinct from Task | none: principal/admission are K5's, I/O happens only after admission |
| Shared per-Cell capability space; delegation/attenuation | none: no capability, handle or right is added; the VFS namespace right is the existing live-KMS rule; no manifest/syscall change (3.4) |
| Revocation generations | none; the store is not generation-keyed state (replay floors stay volatile) |
| Kernel substrate vs policy Cells | no kernel file touched; no VFS file touched |
| Durable logical state vs execution | **adds durable logical state**: one dev-only record pair. It is explicitly **not protected state**: no rollback floor, no authority, no crash-durability, public dev root; it is never reported as `ProductionQualified` and holds no freshness/counter/key/recovery responsibility |
| Caller/test/doc migration, concurrency, remote | callers: none change (`KmsClient`, admin cell, ABI untouched); tests: new `tests/ssh/persist/**`, K5 tests unedited; docs: `docs/code-standards.md` bullet + `CHANGELOG.md`; concurrency: single-threaded KMS loop, blocking VFS calls (section 12 risk 3); no remote impact |
| No ambient credential / remote shortcut | no raw signer, no export; the record holds only ciphertext and the public key; the passphrase never reaches the persister |

**11.3 Locked rows and overlapping plans (read-only; `git status --short` empty at HEAD `070f0d0a1`; no K4b edit in flight).**
| Plan / row (`cell-contract-0024-locks.md`) | Overlap | Needed |
|---|---|---|
| `260825-1726-kms-silo-production-root` (`:153`; K5 slice paragraph `:119`) | files under `cells/services/kms/**`; blocked scope (mTLS, relay, protected signer, production root) untouched | file-bounded exception for section 7 |
| `260826-1605-phase4-dev-reference-authority` (`:155`) Phase 06 "Related Code Files" (`phase-06-...:58-77`, status `pending`) | **C-K4b-1** textual overlap on `Cargo.toml`, `lib.rs`, `dispatch.rs` (shared with K5, already ruled C-K5-1: K5 lands first, Phase 06 rebases on post-K5 hashes). K4b adds one feature line, one `cfg` rule, and **two lines in `dispatch.rs:29,56`** inside K5's own `cfg(ssh-dev-custody)` regions; it touches **none** of `storage.rs`, `storage/**`, `lifecycle/**`, `dispatch/{relay,enrollment}.rs`, `build.rs`, `tests.rs`. `main.rs` and `ssh.rs` are not in Phase 06's list | ruling: Phase 06 rebases over K4b's additions on those three files, anchor = the post-K4b hashes recorded in the K4b dossier |
| same, **C-K4b-2** `phase-06:36` ("VFS remains untrusted opaque transport ... never regains authority freshness, integrity, key, counter, or recovery responsibility") and `code-standards.md` relay wording ("volatile state is never a substitute") | the SSH leaf stores wrapped key material on VFS | stated so it is not misread: the SSH record is neither relay authority state nor a floor and takes none of those responsibilities; the relay lifecycle and stubs are not read, written or edited. Request: note, no ruling beyond C-K4b-1 |
| same, **C-K4b-3** Phase 02 sole ownership of `scripts/check-production-relay-image.py` and its test (`phase-02:46,74`; status `blocked`) | production exclusion | K4b adds a sibling script and hands off five closed marker names; it never edits the Phase 02 files; confirm |
| `260603-1803-phase-h-vfs-hardening` (`:120`) and controlling P05 | none: no kernel, VFS or `libs/**` file in the closure; VFS is used as an existing client | none (note) |
| `261025-cellosfs-crash-safety` (planned, no lock row) | soft dependency only (D-K2) | none; every "survives power loss" claim stays forbidden |
| `libs/types/src/kms/**`, `libs/api/**` (Law 1) | none (7) | none |
| `260825-sdk-delivery` P02, `260712-1902-dice-attestation-identity` P03, `260819-1409-cell-to-cell-anywhere-core` P02, `migration-ledger.md` E12 | "not examined" at K2/K5 | owner confirms no overlap or names one; integrating owner re-checks at slice start |

**11.4 Retained gates.** Law 1: **not applicable** (no `libs/api`/`libs/types` edit; avoided by design, 7). Protected identity / non-rollback time/state: **not applicable and not claimed**: the T1 record is unprotected dev state and the owner is asked to confirm that this reading holds (no storage-protection gate is satisfied or bypassed). Independent human decisions: D-K1/D-K2/D-K7 recorded; section 14. No physical/hardware/AWS/production gate is touched. A green host test, a clean build or this document does not substitute for these gates.

**11.5 Scheduling change (text only, not applied).** Record "KSSH K4b" as a bounded adopted slice with ceiling "host tests, feature `ssh-dev-persist` opt-in, x86_64, `DEV_POSTURE`, no rollback/crash-durability claim" in the controlling plan, `plan-portfolio.md` and `cell-contract-0024-locks.md` next to the K5 paragraph (`:119`), naming the section 7 closure and this packet's digest; narrow only the applicable relation of `260825-1726`; keep whole-plan relations; WIP stays one implementation slice (K4b starts after K8's T0 lane).

**11.6 Execution boundary once adopted.** Single integrating owner. Baseline first, with logs. Edits only inside section 7. No key generation (public fixtures and test seeds only), no network, no QEMU/VM/Docker, no image, signing or `config/` change, no `libs/**` or `kernel/**`, no `Cargo.lock` change, no CI edit. **Rollback:** restore the pre-edit digests of the eight edited paths and remove the new files (source reversal; no persistent effect in the repo; on a lane disk the `ssh-*.bin` files are inert for every other component). Dossier as section 8.

**11.7 Decisions requested from the accountable owner.** (1) Adopt K4b as a bounded slice with the section 7 file-bounded exception (serves Cellbox publickey, not C2C). (2) Rule C-K4b-1 (including the two-line `dispatch.rs` edit) and note C-K4b-2, confirm C-K4b-3. (3) Confirm no overlapping owner for the section 7 paths or name one, and the four unexamined plans. (4) Confirm the 11.4 reading (unprotected dev state, no protected-state gate). (5) Approve the one scheduling record. Nothing here authorises a source edit.

## 12. Risks
1. **Absent-versus-failed on the VFS wire** (1.2 row 3): mitigated by listing and by erring toward `IoError`; first real execution is K8 RT1. 2. **A write failure poisons `/srv`** for all users until reboot (1.2 row 4): `PersistFailed` then a dead SSH leaf; host-trust store shares the fate. 3. **Single-threaded blocking I/O:** an unbounded VFS wait stalls every KMS opcode (relay ones too); only admitted SSH callers can trigger it; accepted at `DEV_POSTURE`, observed in K8. 4. **Two-line `dispatch.rs` edit** on a Phase 06 shared file (C-K4b-1). 5. **Wiring untested on host** (4.1): compile-checked and exercised by K8 only. 6. **Phase 06 later restructures `StoreIo`:** neutralised by the own trait. 7. **Weak rounds in lane keys** (`-a 4`): not a bug, a stated ceiling. 8. **Inherited from K5 and unresolved:** +1 MiB bss in every custody build (`k5-cp2-dossier.md` section 6, finding 1: whether the loader accepts it for `/bin/kms`), AES-NI/SIMD use in an x86_64 cell, D-5.8 KDF time on TCG (K8). 9. **CellosFS in-place replace tearing** is unobserved: the on-disk result of a torn `replace_file` (truncate then write) is [INFERENCE]; the two-slot design assumes only that the *other* slot stays intact. 10. **Dev-root forgery:** a disk holder can substitute a record, e.g. their own wrapped key, making KMS sign as the attacker's identity (the server would then reject it unless authorised): an availability/substitution issue, stated.

## 13. Out of scope
Rollback floor, monotonic counter, signed time, authority/TPM binding, qualified provider, crash-safe store (T2 / `261025-cellosfs-crash-safety`); a `KmsProviderKind` value; VFS, CellosFS, `StoreIo`, `storage/**`, relay-lifecycle or stub changes; Plain-key persistence; persisted limiter; image, `config/services.toml` or CI changes; client, admin cell, kernel flag; bounded/timeout VFS calls; any edit to Phase 02-owned scripts; QEMU evidence (K8).

## 14. Decisions requested from the user (only choices still open after the landed facts)

| ID | Decision | Recommendation |
|---|---|---|
| D-4b.11 | A persist build whose VFS or `/srv` is unavailable: **unavailable** (`SshKeyUnavailable`, retry per opcode) or a silent volatile fallback | **Unavailable.** A persist build must never report or behave as T0; a lane that lost `/srv` should fail loudly |
| D-4b.12 | Test strategy for wiring: leaf-level persister tests + unedited K5 suite (alias is `NoPersist` under `cfg(test)`) vs editing K5's `Dut`/`service()` helpers so `KmsService` itself runs the persister in tests | **Leaf-level, K5 suite unedited** (smaller closure, no change to approved K5 tests); wiring proven by the bare-metal build and K8 |
| D-4b.13 | Production exclusion beyond the structural guards: add the sibling checker + test and the five-name marker hand-off, vs rely on the feature graph only | **Add the sibling checker** (it also gives K8 the `--expect` variant proof); Phase 02 files untouched |
| D-4b.14 | Revision discontinuities (4.6 a/b): accept, vs change the K5 seam (`Loaded::Empty{revision}`, `Corrupt{max_revision}`), which edits closed K5 logic in `ssh.rs`/`ssh/store.rs` tests | **Accept** (revision gates nothing; no K5 logic change) |
| D-4b.15 | CI: leave `ci.yml` alone (K5's G-6 leg for `--features ssh-dev-custody` is still the owner's), vs add `.github/workflows/ci.yml` to this closure for `ssh-dev-custody` and `ssh-dev-persist` test legs and the Python test | **Leave CI out of the closure**; the owner wires one combined leg with G-6 |
| D-4b.16 | Whether K4b updates the stale `docs/code-standards.md` K5 bullet (section 1.2 row 13) | **Yes, one bullet** (T0 landed; T1 wrapped-only, `DEV_POSTURE`), in the closure |

Carried and not re-asked: D-4b.1 IN after K5 and the T0 lane; D-4b.2 Plain never persisted; D-4b.3 R1 public-constant dev root; D-4b.4 no `KmsProviderKind` append; D-4b.5 own `SshBlobIo`; D-4b.7 lazy load; D-4b.8 visible corruption/rollback with import as repair; D-4b.9 volatile counter; D-4b.10 closure pre-declared in its own handoff (K5 already landed, so a standalone packet, not an amendment).

## 15. Not checked
No compile, test, lint, `cargo deny`, VM or QEMU run for this document; the K5 test counts are the K5 dossier's, not re-measured at `070f0d0a1`. Not observed: that `VfsBlobIo` works against the live VFS (handle-based reads of `/srv/cellos/kms/**` as the live KMS have no existing test; the `ERR_IO`-for-absent behaviour was read in `dispatch_paths.rs:29-35`, not executed); the exact listing format on non-CellosFS backends; what a torn CellosFS `replace_file` leaves on disk; whether a blocked VFS call can stall KMS at first use in the lane image; boot ordering of `kms` against `vfs` in the lane (lazy load makes it irrelevant for correctness, not for first-use latency); whether the loader accepts the +1 MiB bss of custody builds for `/bin/kms`; whether the persist build's `.rodata` contains the root label (marker visibility); byte identity of the custody-only x86_64 build before and after (section 5 item 3, section 8); the `ssh-keygen`/OpenSSH behaviour on the stored wrapped blob; whether the Phase 02 owner accepts the five-name marker hand-off; the four plans "not examined" at K2/K5; that `NamespaceKey::parse` accepts `ssh-a.bin` under `/srv/cellos/kms` (read: only rejects empty/`.`/`..`/NUL components, `namespace.rs:26-40`); VFS `healthy()` behaviour for a `/srv` partition that is formatted but whose CellosFS needs recovery.

## 16. Effort
M, about 4-5 working days for one owner after K8's T0 lane (the seam exists): record codec and strict decoder 1 day; persister, `SshBlobIo`, `VfsBlobIo` and listing rule 1.5 days; tests including the load/torn matrices 1.5 days; sibling checker and tests 0.5 day; wiring, docs, handoff text and dossier 0.5-1 day.
