# S10-10.0 probe (c): KMS prerequisite chain for an SSH ed25519 key class

Read-only probe, 2026-10-30, HEAD `c319809c8`. Paths cited are repo-relative. `[INFERENCE]` marks anything not observed in a file or command output. No build, test, VM, key generation, network or edit was run. Machine-readable twin: `kms-prerequisites.json`; hashes: `evidence-kms.sha256`.

Scope reminder: user decision D10-2 = E (open a KMS program for SSH key custody as a separate plan; no implementation authorised). This report is input for that program's CP1.

## 0. Bottom line

1. KMS today has **no** SSH leaf and **no** working protected persistence. On x86 it has **no signing provider at all** (`ProviderSlot::Unavailable`); the only provider that ever signs is the AArch64-QEMU-only development Silo.
2. A *session-bound ed25519 userauth signature* fits the 128-byte frame with no chunking; only *import* (and an optional passphrase) needs chunking.
3. The caller-admission list cannot be extended by name: the kernel trailer carries `cell_id/generation/tid/flags`, no path. Admitting `/bin/cellbox-ssh` needs a new kernel-attested caller flag (Law 1 + kernel), exactly like the Phase 01 `CALLER_FLAG_CONFIG_WRITE` precedent.
4. "Production-grade" custody is not reachable by any plan in the repo today. Its real prerequisites (authority hardware, signed time, ADR-0006 GO) are blocked. The honest first deliverable is a labelled dev-posture custody; it must not be described as protected.
5. Contract conflict found: `command-contracts.md:142` forbids an "import command ... added to the CLI", while the S9-5 proposal (`phase-09-s0-proposal.md:108`) offered a `cellbox ssh-key import` builtin. The import surface must be a non-CLI credential-owner path; K0 must record this contract delta.

## 1. What KMS has today

### 1.1 Wire and frame (Law 1 scope `libs/types`)
- Frame is fixed: `KMS_MESSAGE_LEN = 128`, `KMS_PAYLOAD_LEN = 112`, 16-byte header (version, opcode, flags, request_id u32, payload_len, 2 reserved) — `libs/types/src/kms.rs:23-28`, `libs/types/src/kms/frame/request.rs:5-17`. `from_bytes` rejects any other length (`request.rs:41`).
- "Versions and opcodes are append-only": `libs/types/src/kms.rs:23`; `docs/code-standards.md:54-58`.
- Opcodes 1-14 only (`libs/types/src/kms/model.rs:25-57`). The only signing opcode is `SignTls13ClientCertificateVerify = 8`. Key algorithms: `C2cX25519 = 1`, `RelayP256Sha256 = 2` (`model.rs:95-101`). Provider kinds include unimplemented `DiceSealed`, `HardwareSealed`, `HardwareRelay` (`model.rs:62-73`). Error codes 1-26 (`model.rs:121-156`).
- Signing payload precedent: typed fields only, no key id/algorithm/raw message (`libs/types/src/kms/payload/tls.rs:1-40`; 80-byte request, 64-byte reply). Chunk precedent: `RELAY_CSR_CHUNK_LEN = 104` inside the 112-byte payload (`libs/types/src/kms/csr.rs:15-16`), `ReadRelayCsrChunk = 10`.
- Standing rules for any new signer (`docs/code-standards.md:54-70`): typed protocol state only, no generic prehash/raw signer, no caller-selected key or algorithm, authorise live caller generation and TID before provider access, nonzero monotonic replay ids that advance only after all checks, an independent capability leaf (C2C must not authorise or substitute for relay and vice versa), reconstruct the signing input inside KMS, self-verify, no fallback to another key/provider.

### 1.2 Caller admission
- Every request arrives through `sys_recv_attested` (`cells/services/kms/src/main.rs:29`); the cell declares `Send, Recv, RecvTimeout, Log, LookupService, VfsMutate` (`main.rs:13`).
- `validated_caller` denies a missing trailer, generation 0, or `sender_tid != sender` (`cells/services/kms/src/auth.rs:132-140`).
- Admitted classes: live net-broker (`register_broker`, `auth.rs:84`), live service-net (`register_service_net`, `auth.rs:102`; used by opcodes 7 and 8: `dispatch/relay.rs:22,50,115`), live supervisor (`authorize_supervisor`, `auth.rs:120`; rotate and enrollment). Liveness is proved with `sys_lookup_service` for NET_BROKER, SUPERVISOR, NET (`main.rs:34-36`). **There is no registry entry for a cellbox ssh client, so the registration-binding pattern cannot be reused for it.** [INFERENCE: no `service::` id for cellbox exists; `libs/api/src/abi/syscall.rs:1383-1408` lists service ids and none is a shell/ssh client.]
- The kernel trailer is `{flags:u32, cell_id, generation, sender_tid}` (`libs/api/src/abi/caller_identity.rs:91-101`); it "carries no install path" (also stated at `phase-01-config-right-cp1.md` §1). Flag bits in use: 0 VFS_MUTATE, 1-4 NET_*, 5 CONFIG_WRITE; 6..31 reserved (`caller_identity.rs:45-75`).
- Process facts: KMS is a single-threaded loop (`main.rs:27-50`), `restart = "always"`, `service_id = 13` (`config/services.toml:89-94`), `/bin/kms` is a legacy-ceiling boot row (`kernel/src/loader/boot_ceiling.rs:195`, `launch_profile/profiles.rs:27`). `/bin/cellbox-ssh` has only the `NET_CLIENT` ceiling edge (`kernel/src/loader/launch_profile/profiles.rs:75`, test `tests.rs:149-160`).
- Image selection: `/bin/kms` is enabled only when the `ui` feature is set and not on `board-rpi3` (`scripts/generate-boot-config.py:49-50`). **Not checked:** whether the S10 canonical x86 image actually ships/launches `/bin/kms`.

### 1.3 Persistence: deliberately fail-closed (file:line)
- Runtime load and persist are stubs that always return `PermissionDenied`: `cells/services/kms/src/storage.rs:75-79` (load) and `:85-88` (persist); doc comment "No runtime sealing key/monotonic provider is wired yet, so the honest result is unavailable and dispatch seals the service" (`storage.rs:73-75`).
- Boot: `boot_lifecycle` (non-test) maps any load error to `RelayLifecycle::sealed()` (`dispatch.rs:261-270`); `persist_protected_lifecycle` seals the service on any persist error (`dispatch.rs:94-109`).
- Provider: `ProviderSlot::development_runtime()` is `Unavailable` unless `development-silo-provider` on AArch64 bare metal (`storage/provider/relay.rs:37-50`); `impl C2cProvider for ProviderSlot` always returns `kind None`, `assess = unavailable()`, `open_or_provision = Unavailable` (`relay.rs:181-199`; `provider.rs:36-42`). Result: node identity never reaches `Ready`; `AcquireNodeIdentity`, `NoiseStaticDh`, `RotateNodeIdentity` end in `SecureRootRequired` (`dispatch/node_identity.rs:44,71,89`).
- Feature gates: `hardware-relay-provider` is a `compile_error!` ("blocked until Phases 6-7") (`lib.rs:38-42`); `development-silo-provider` is `compile_error!` off AArch64 bare metal (`lib.rs:44-50`) and panics in `build.rs:8-15` under `CELLOS_PRODUCTION` or other targets. Cargo deps contain `p256`, `sha2 0.10`, `blake2`, `constant_time_eq` only (`cells/services/kms/Cargo.toml:24-31`): **no ed25519, no KDF, no AEAD/AES**.
- On-disk format that exists (host-tested only): two A/B slot files `/srv/cellos/kms/slot-{a,b}.bin` (`storage/record.rs:10-12`), 186-byte records with a 64-byte `sealed_leaf` and a 32-byte BLAKE2s-MAC (`record.rs:17,49,128-134`). The MAC key is a `JournalKey` supplied by tests only (`tests/storage.rs:14-15`); no runtime source exists. The MAC authenticates but does not encrypt.
- Rollback semantics today: `RollbackDetected` only when both slots are valid but not chained (`storage/journal.rs:49,174-`). **Both slots missing or corrupt load as `Empty`** (`journal.rs:38`; `read_slot` returns `None` on size/MAC failure, `journal.rs:154-172`; test `tests/storage.rs:103-110`). Deleting or reverting the files is therefore indistinguishable from a fresh store: no monotonic floor lives outside the filesystem. `ProviderAssessment.anti_rollback_capable` is `false` in the only runtime provider (`provider.rs:36-42`); `RootAssessment` turns that into `NoAntiRollback` (`storage/root.rs:54-60`).
- A 64-byte `sealed_leaf` cannot hold a sealed ed25519 secret (32-byte seed + AEAD overhead) plus metadata: a new record version or per-key blob is required. [INFERENCE from `record.rs:17`.]
- Store I/O is `ensure_dir/stat/read_file/write_file` over VFS (`storage/backend.rs:17-20`, `storage/runtime.rs`); there is no sync/flush call in the trait. VFS reserves `/srv/cellos/**` for the live KMS only (`cells/services/vfs/src/access/rules.rs:39-52,93-96`, `access/kms.rs:4-56`). `/srv` is CellosFS Native, "existing native V1 volume, not an admitted durable journal" (`cells/services/vfs/src/manager.rs:94-100`).

### 1.4 Signing API today
Only `SignTls13ClientCertificateVerify`: service-net only, nonzero monotonic `request_id`, generation and profile checks, normalise low-S and self-verify (`dispatch/relay.rs:43-113`). Provider arm needs a Silo (AArch64) or a test fixture. No ed25519 and no SSH message reconstruction exists.

## 2. What protected persistence needs, and how it overlaps the other plans

### 2.1 Required properties (from `spec.md` PERSIST-001..008, `docs/code-standards.md:122-125`)
Authenticated root (stable non-exportable identity, challenge-bound boot opening), sealed state (confidentiality + integrity, key not derivable from AP-writable storage), rollback protection (monotonic floor that survives power loss and snapshot restore, held outside the AP-writable filesystem), seal-on-failure. `spec.md` LANE-002: "a filesystem-held key ... SHALL NOT satisfy an entry gate".

### 2.2 `260825-1726-kms-silo-production-root` (status in-progress; `executionStatus: blocked`, lock CELL-CONTRACT-0024; `plan.md:3-14`)
| Phase | Status | Relevance to SSH custody |
|---|---|---|
| 1 KMS TLS signing slice | completed | provides the typed-signing pattern only |
| 2 Contain development Silo | completed | AArch64-QEMU DEV_REFERENCE only; unusable on x86 |
| 3 Cert activation/provisioning | completed | relay enrollment; not a key-custody store |
| 4 Service-net mTLS | blocked (entry gates AC-001..011) | **this is where real protected persistence + authenticated time are defined** (`spec.md` Protected persistence; `phase-04-service-net-mutual-tls-integration.md` Next Steps); relay-specific |
| 5 QEMU relay software evidence | pending | DEV_REFERENCE only |
| 6 Production root kill gate | completed, outcome NO-GO (ADR-0006) | no product selected |
| 7 Hardware provider | blocked (ADR-0006) | needs superseding GO ADR |
| 8 Qualify production relay | blocked (ADR-0006) | |
The plan's persistence contract is for the **relay P-256 lifecycle** (`ProtectedRelayState`, 64 bytes, `lifecycle/mod.rs:48-55`), not a general sealed key store.

### 2.3 `260826-1605-phase4-dev-reference-authority` (status blocked; blockedBy hardware-assets, aws-dev-account, cell-contract plan)
P1 admission blocked; P2 private protocol blocked (SOFTWARE_HARNESS complete); P3 VF2 UART boot blocked; P4 STM32/TPM authority blocked (SOFTWARE_HARNESS complete, hardware gates open); P5 signed-time blocked (SOFTWARE_HARNESS cores complete; the public Cloudflare endpoint is not draft-11 conforming); **P6 Frozen-ABI KMS authority integration pending** (the phase that would replace the `PermissionDenied` stubs); P7, P8 pending. The AP-to-authority protocol is closed with 14 operations and says "runtime provisioning, unseal, ... caller-selected keys are intentionally unrepresentable" (`libs/authority-protocol/src/lib.rs:3-6`, `PROTOCOL_VERSION = 2`). [INFERENCE] An SSH leaf cannot be stored by this authority without a protocol revision and a new sealed-record binding.

### 2.4 `261025-cellosfs-crash-safety` (status **planned**; `plan.md:1-5`)
No phase has a detailed plan, CP1 or code. `CellosFs::sync_durable` is a skeleton that always returns `DurabilityUnavailable` (`libs/cellos-fs/src/engine.rs:844-852`, `libs/cellos-fs/src/lib.rs:13-15`); `SaveOutcome::CommittedDurable` exists but nothing returns it (`libs/api/src/services/ipc.rs:346`). Audit findings: no journal, bitmap overwritten before superblock, old-root data reuse, no mount recovery, 512 B tears, FLUSH/VWC unobserved (`plan.md` §"Audit findings"). Phases 1-7 (design, torn-write model, device contract, mount recovery, outcome, evidence, migration) are all pending.
- Hard or soft dependency? [INFERENCE] **Soft** for a labelled dev-posture custody: the KMS A/B+MAC journal already tolerates a torn inactive slot (`tests/storage.rs:91-101,125`), and a lost key is recoverable by re-import; but one torn CellosFS root can make `open()` return `Corrupted` for the whole `/srv` volume (audit finding 3), taking the trust store and the KMS store down together. **Hard** for any statement of the form "after the import reply the key survives power loss" and for a production-grade claim, unless the sealed state is moved into an authority NVRAM (then the FS only carries ciphertext, per PERSIST-004).

### 2.5 Other related plans (not opened)
`260712-1902-dice-attestation-identity` (queued; P00 complete; P01-P05 pending; DICE-sealed provider kind is only an enum name), locks row `260603-1803-phase-h-vfs-hardening` (pending; VFS mutation authority).

## 3. Cellbox / Sunset side

- **Sunset path as vendored** (`third_party/sunset`, 0BSD, commit `62b98f39…`, PROVENANCE): `SignKey::AgentEd25519(VerifyingKey)` (`src/sign.rs:339-340`); `from_agent_pubkey` builds it from a 32-byte key (`sign.rs:430-436`); `CliEvent::Pubkey` carries the key, then on `UserauthPkOk` the library raises `CliEvent::AgentSign` (`src/cliauth.rs:132-134`); `RequestSign::{key, message, signed(&OwnedSig), skip}` (`src/event.rs:129-146`); `OwnedSig::Ed25519([u8;64])` is a public variant (`sign.rs:268-270`); `sshwire::write_ssh` is public (`src/sshwire.rs:196`), so the embedder can serialise `AuthSigMsg`. `AuthSigMsg` fields are `pub(crate)` (`src/auth.rs:14-17`): without a Sunset patch the embedder must serialise then parse the blob to get session id and username. [INFERENCE: not compiled; a tiny reviewed patch 0007 adding accessors is the alternative.] Patches 0001-0006 do not touch this path (`PROVENANCE`).
- **vissh today**: `Pubkey` and `AgentSign` events are skipped (`libs/vissh/src/session.rs:367-368`); `Auth::PublicKey{key_id}` and `--identity` already parse (`libs/vissh/src/args.rs:12-15,143-148,183-187`); `ssh` refuses publickey with `publickey_unavailable` (`cells/tools/cellbox/src/ssh.rs:760-761`). Trust precedes auth in the session state machine (`session.rs:339`), satisfying contract "trust before any signature" on the cellbox side only; KMS cannot enforce it.
- **Signing blob size**: RFC 4252 §7 data = `string session_id(32) | 50 | string user(≤31) | "ssh-connection" | "publickey" | TRUE | "ssh-ed25519" | string pubkey(51)` ≈ 175 bytes [INFERENCE from the RFC], which exceeds 112. The Law-1-compliant design (KMS reconstructs) needs only `{key id, session_id[32], user ≤31 B, request id}` ≈ 70-80 bytes and returns 64 bytes: **no chunking for signing**. Session id is 32 bytes because the profile is curve25519-sha256 only (`PROVENANCE` patch 0002).
- **Import over 128-byte frames**: payload 112 B minus chunk header ≈ 100 B per frame (CSR precedent: 104). Contract cap is 64 KiB (`command-contracts.md:142`) ⇒ up to ~650 frames worst case; a real unencrypted ed25519 OpenSSH key is a few hundred bytes (≈ 5 frames) [INFERENCE, format sizes, not measured]. The passphrase (≤1,024 B, `command-contracts.md:142`) is ≤ 11 more frames. Needs an ordered, handle-bound, restart-bound one-shot session like the CSR handle (`docs/code-standards.md:108-121`).
- **OpenSSH v1 parsing**: `ssh-key 0.7.0-rc.11` is already in the lock (`Cargo.lock:3729-3740`: deps `ctutils, sha2 0.11.1, signature 3.0.0, ssh-cipher, ssh-encoding, zeroize`) with **no** `encryption` feature. That feature is `["dep:bcrypt-pbkdf", "alloc", "cipher/aes", "cipher/chacha20poly1305", "rand_core"]` (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ssh-key-0.7.0-rc.11/Cargo.toml:83-89`), requiring `bcrypt-pbkdf 0.11` with `alloc` (`:181-185`). It would also enable chacha20-poly1305, while the contract allows AES-256-CTR only (`command-contracts.md:142`); an own minimal v1 container parser with a pre-KDF allowlist is a design choice for the program (CP1 sub-decision).
- **bcrypt-pbkdf status**: `grep -c bcrypt Cargo.lock` = 0; no `bcrypt-pbkdf`, `blowfish`, `pbkdf2` directory in `~/.cargo/registry/src` and no `.crate` for them in the cache (only `ssh-key-0.7.0-rc.11.crate` matched). There is no `vendor/` directory; registry crates are pinned by lock checksums and the local cache (`phase-09-s0-proposal.md` S9-1 deviation 1). So adding it needs **one online fetch** plus `cargo deny` (`deny.toml:26-39` allows MIT, Apache-2.0, 0BSD, etc.). Its license and exact dependency set are not verifiable offline; [INFERENCE] MIT OR Apache-2.0 as RustCrypto crates. AES-CTR (`aes 0.9.3`, `ctr 0.10.1`) and `ed25519-dalek 3.0.0`, `sha2 0.11.1` are already in the lock. KMS currently links `sha2 0.10` (`kms/Cargo.toml:30`); both versions are in the lock.
- **KDF cost risk** [INFERENCE]: bcrypt rounds up to 64 inside the single-threaded KMS loop (`main.rs:27-50`) would stall relay and node-identity service during import, and is slow under TCG (`phase-09-s0-proposal.md:139`). The contract places parse/decrypt in the "credential owner", which may or may not be KMS (CP1 sub-decision).
- **Contract text that binds the design** (`command-contracts.md:132,138,142`): KEY_ID is 1..64 `[A-Za-z0-9_-]` resolved only by an independently issued KMS credential ref; rights `A` (credential/auth op) and `Vt` (trust) are independent; "never a KMS signing right" inside Vt; passphrase captured by the foreground client under its own no-echo lease and passed only to the authenticated owner; "No ... agent ... key generation, import command or secret persistence is added to the CLI".
- **Dev-posture side effect of publickey**: removes the "pipeline stage has no terminal for the password" blocker, so pipeline exec and exact non-UTF-8 stdin (S9-3 deviation 3, `phase-09-s0-proposal.md:303`) can be re-opened after the key path works.

## 4. Locked relations and protected paths each step touches

Lock inventory: `.agents/cell-contract-0024-locks.md` (38 relations; unfreeze procedure `:293-300`; the Cellbox Phase 02 file-bounded exception `:113` is the precedent form). New plan dirs are not automatically in the inventory; any KMS file edit sits in lock scope of rows below. All owners are "P01 rebase + explicit per-slice handoff" per the table.

| Step | Relation / protected surface | Owner per inventory |
|---|---|---|
| Any edit under `cells/services/kms/**` | row `260825-1726-kms-silo-production-root` (`locks.md:145`, "protected signer and production-root integration") and shared files with Phase 4 spec | controlling plan P01 then P08/P10 |
| Real root/authority/sealed state/time | rows `260825-1726…` and `260826-1605-phase4-dev-reference-authority` (`:147`, "protected authority, time, enrollment and KMS handoff"); hardware/AWS gates; ADR-0006 | P01 then P08/P10 |
| New opcodes, `KmsKeyAlgorithm`, payloads, errors in `libs/types/src/kms/**` | **Law 1** (`docs/code-standards.md:40-70`): two confirmations (design, then exact-delta implementation), append-only | user as accountable maintainer; new CP1/CP2 |
| New caller flag in `libs/api/src/abi/caller_identity.rs` (+ `libs/api/src/services/ipc.rs` if needed) | **Law 1**; plus kernel permission: `kernel/src/task/{cap,syscall,tcb}.rs`, `kernel/src/policy.rs`, `kernel/src/loader/boot_ceiling.rs`, `launch_profile/profiles.rs`, `scripts/sign-policy.py` (file list of the Q-right precedent, `phase-01-config-right-cp1.md` table) | row `260603-1803-phase-h-vfs-hardening` (`locks.md:118`, kernel permission) and controlling P05 (owner/delegation); Cellbox adopted-slice precedent `:113` |
| New persistence blobs under `/srv/cellos/kms/**` | no VFS rule change needed (path already KMS-only, `access/rules.rs:39-52`); if the store moves, VFS mutation authority row (`:118`) | P05/P06 |
| Durability claims | `261025-cellosfs-crash-safety` (planned; not in the 2026-10-11 inventory) maps to controlling P06 (durable logical state, crash evidence) [INFERENCE: ownership of this plan under the lock is unrecorded; K0 must state it] | P06 |
| bcrypt-pbkdf in `Cargo.lock` | not a locked relation; `deny.toml` license/advisory gate; network authorisation | maintainer |
| `cells/tools/cellbox/**`, `libs/vissh/**`, `libs/ostd/src/clients/kms.rs` | Cellbox plan (single integrating owner); `libs/ostd` is not Law 1 | Cellbox |
| Image/signing/CI | `kernel/src/main.rs`, `build/`, the 546-path census: KMS source files are listed in `build/*manifest*.json` (e.g. `build/exact-delta-package-manifest-initial.json`); **not checked** which manifest is the S10 census | S10 preservation rules |
Nothing in this chain lifts a production/physical/remote/crash-durability gate. Documentation completion unlocks nothing (`locks.md:300`).

## 5. Minimal dev-posture subset vs production-grade custody

| Tier | Content | Honestly proves | Does **not** prove |
|---|---|---|---|
| T0 volatile dev custody | SSH leaf in RAM only; import at every boot through an admin path; new opcodes; new caller flag; `AgentSign` wiring; refusal matrix | key bytes never in cellbox/UART/logs; signature bound to session id, user, key; caller admission by kernel-attested flag; unknown KEY_ID/wrong passphrase/format/other-session refusals; real login to a controlled other host | persistence, confidentiality at rest, rollback, crash safety; protection against a compromised admitted caller beyond session binding |
| T1 dev persistent | T0 + sealed blob under `/srv/cellos/kms/` with a development root key; new x86 development provider; record version bump | key survives QEMU reboot; torn inactive blob tolerated | rollback resistance (delete/revert = `Empty`, §1.3); crash durability ("synced, not crash-proof", D78-2); confidentiality against anyone holding the disk and the image (dev root key is co-resident, `LANE-002` rejects this as a gate); any production claim |
| T2 production-grade | authority-held root + sealed state + monotonic floor outside AP storage + signed time + qualified provider + crash-safe store | the properties `spec.md` PERSIST-001..008 describe | n/a; currently **blocked**: hardware assets, AWS account, ADR-0006 NO-GO, lock handoff, no x86 provider or authority exists [INFERENCE: no x86 TPM/authority code found by name; not searched exhaustively] |

Labelling rule proposed: T0/T1 are `DEV_POSTURE` and must never be reported as `ProductionQualified`; CBX-24's publickey half could be "met at dev-posture ceiling" only by explicit user disposition. `code-standards.md:122-125` ("volatile state is never a substitute") is written for the relay lifecycle; an ADR must state that the SSH leaf has its own posture rules [INFERENCE].

## 6. Proposed slice DAG (program "KSSH")

Effort: S ≤ days, M ≈ 1-2 weeks, L ≈ weeks plus external dependency. Every code slice has its own CP1/CP2; ABI slices follow the Law-1 two-step.

| ID | Slice | Entry gate | Effort | Result |
|---|---|---|---|---|
| **K0** | Program charter (docs only): new plan; acceptance classes T0/T1/T2; lock adoption packet (file-bounded, like `locks.md:113`); contract delta for the import surface; decision set D-K1..D-K7 | user approval of this CP1 | S | plan + scoped authorization; no code |
| K1 | Dependency admission: `bcrypt-pbkdf` (+ transitive) into lock, `cargo deny`; choose `ssh-key` vs own parser | K0; **explicit network authorisation** | S | lock diff, license record |
| K2 | KMS ABI CP1 (design) then impl: `KmsKeyAlgorithm::SshEd25519`, opcodes 15+ (begin/chunk/commit/abort import, sign userauth, status, delete), error codes, byte fixtures | K0; Law-1 confirmation 1, then 2 | M | frozen-layout fixtures |
| K3 | Caller admission right (new `CALLER_FLAG_*`, kernel `CapSet` bit, launch row for `/bin/cellbox-ssh`, `sign-policy.py` row) | K0, K2 CP1, lock handoff for kernel permission (row `:118`/P05) | M-L | kernel-attested admission with host+QEMU lane denials |
| K4 | Storage tier: K4a T0 (none); K4b T1 (x86 dev provider, record v2, dev root, production exclusion like `scripts/check-production-relay-image.py`) | K0, D-K1; K4b needs D-K2 | M | persistence evidence labelled DEV_POSTURE |
| K5 | KMS implementation: chunked import, OpenSSH v1 (none; bcrypt+AES-256-CTR), ed25519, sign with reconstruct+self-verify, replay ids, zeroize, caller checks | K1, K2 (CP2), K3 or test-hook flag, K4 | L | host tests, refusal matrix |
| K6 | Cellbox/vissh: `KmsClient` additions, `AgentSign` handling, `--identity`, passphrase lease, lift `publickey_unavailable`, re-open pipeline exec and non-UTF-8 stdin | K5, K3 | M | CLI behaviour |
| K7 | Admin import surface (non-CLI credential owner; candidates: supervisor-authorised op or a dedicated admin cell) | K2, K5, D-K3 | S-M | import path |
| K8 | QEMU evidence: login to controlled OpenSSH with KMS key; negatives; no key bytes in serial/logs; reboot persistence (T1); rollback shown as unprotected | K6, K7 | M | private evidence, CBX-24 publickey half at stated ceiling |
| K9 | T2: adopt `260826-1605` P1-P6, ADR-0006 GO, SSH leaf in the authority protocol, crash-safe store | external; separate program | L, not estimated | production-grade claim |
| X | `261025-cellosfs-crash-safety` P1-P7 | own CP1s | L | parallel track: soft for K4b, hard for K9 and "survives power loss" claims |

Edges: K0 → {K1, K2, K4}; K2 → {K3, K5, K7}; {K1, K4} → K5; {K3, K5} → K6; K5 → K7; {K6, K7} → K8; K9 and X feed a later re-run of K8; K9 requires X.

Order of value: K0 → K2 CP1 → K1 → K3 → K5 is the critical path; T0 can ship without K4b and without X.

## 7. First concrete CP1 to approve: "KSSH-CP1-A, charter and ceiling (docs only)"

Request the user to approve, with no code, no key, no network:
1. Open the plan `.agents/<date>-kms-ssh-key-custody/` with slices K0-K9 and the acceptance tiers T0/T1/T2.
2. D-K1 posture ceiling: **recommend** deliver T0 then T1 labelled `DEV_POSTURE`; T2 stays a separate program. Alternative: do nothing until T2.
3. D-K2: CellosFS crash safety is soft for T0/T1 and hard for T2 and any power-loss claim (alternative: make it hard for everything).
4. D-K3: import is a non-CLI credential-owner path; record the contract delta against `command-contracts.md:142` and the S9-5 proposal `:108`.
5. D-K4: parser choice (`ssh-key` + `encryption`, or own parser with an AES-256-CTR-only allowlist before the KDF).
6. D-K5: decrypt once and re-seal under the KMS root, or keep the key passphrase-wrapped and unlock per session (S9-5 proposal implies the former, contract §4.1 says "unlock").
7. D-K6: caller right as a new kernel-attested flag (the only precedent that works, K3) versus a registration handshake (not available for an unregistered client).
8. D-K7: a new x86 development provider, not the AArch64 Silo, for T1.
9. Authorise the lock-adoption packet (file list per §4) to be drafted for later approval; authorise K1's single online fetch only at K1's own CP1.

## 8. Not checked
No compile/test/lane run; canonical-image presence of `/bin/kms` and its launch order; whether VFS `write_file` flushes; `kernel/src/task/{cap,syscall,tcb}.rs` and `policy.rs` (relied on the Q-right CP1 file list); `lifecycle/mod.rs` and enrollment dispatch beyond the cited lines; phase 01/03/05 bodies and ADR-0008/0009 bodies; bcrypt-pbkdf/blowfish/pbkdf2 licenses and versions (offline); whether `ssh-key` `encryption` builds with `sha2 0.11`; x86 TPM or authority code beyond a by-name search of `libs/`; which `build/*.json` is the S10 546-path census; `.agents/261011-1200-cell-contract-authority` body (only the lock inventory was read).
