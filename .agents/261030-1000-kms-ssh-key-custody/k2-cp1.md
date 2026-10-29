# KSSH K2 CP1: KMS ABI for ed25519 SSH key custody (final text for Law-1 confirmation 1)

Status: **FINAL TEXT submitted for Law-1 confirmation 1 (design); NOT approved until the maintainer answers §9 and an approval record binds this file's digest.** Documents only: no source, key, network fetch, lock or image change was made to produce it. Derived from `k2-cp1-draft.md` (read at HEAD `a3b9a7d10`) with the user's 2026-10-30 decisions applied (D-K4 own parser, D-K5 per-login unlock, O-1, O-10 = B, O-14); `[INFERENCE]` marks anything not observed in a file or command output. No digest is stored inside this file; the approval record `k2-cp1-approval.md` holds it (§5).

Law 1 applies (`docs/code-standards.md:40-70`): this document is **confirmation 1** (design). Confirmation 2 is the exact ABI delta after implementation. Nothing here is a CP2 delta.

Inputs read: `plan.md`, `contract-delta-import-surface.md`, `lock-adoption-draft.md`, `kms-prerequisites.md`, `libs/types/src/kms.rs`, `kms/{model,frame,payload,csr}.rs`, `kms/frame/*`, `kms/payload/{tls,rotate,relay,enroll,service_net}.rs`, `kms/tests/{frame,authority_compat_vectors}.rs`, `cells/services/kms/src/{main,dispatch,auth,reply}.rs`, `dispatch/relay.rs`, `lifecycle/pending.rs`, `libs/ostd/src/clients/kms.rs`, `libs/api/src/abi/caller_identity.rs`, `kernel/src/task/{cap,syscall}.rs`, `kernel/src/policy.rs`, `kernel/src/loader/launch_profile/{profiles,targets}.rs`, `phase-01-config-right-cp1.md`, `command-contracts.md` lines 132 and 142, `phase-09-s0-proposal.md` S9-5, `third_party/sunset/src/{auth,packets,cliauth}.rs`.

## 0. Summary of the proposal

| Item | Proposal |
|---|---|
| Frame | unchanged: `KMS_MESSAGE_LEN = 128`, 16-byte header, `KMS_PAYLOAD_LEN = 112`, `KMS_ABI_VERSION = 1` (`libs/types/src/kms.rs:23-28`) |
| Algorithm | `KmsKeyAlgorithm::SshEd25519 = 3` (`model.rs:95-100` has 1, 2) |
| Opcodes | 15..24 (ten; table §1.2). Core 15-21 needed under either D-K5 answer; 22-24 exist only under per-login unlock |
| Errors | 27..38 appended (twelve; §1.5). 1..26 untouched |
| Signature request | 104-byte payload: `request_id`, 32-byte public key, 32-byte session id, username. Service, method, algorithm and key blob are **not carried**; KMS reconstructs the 143+n-byte RFC 4252 §7 input (174 B at n=31) and returns a 104-byte reply (§1.4) |
| Import | chunked, 96 bytes of data per frame, strictly ordered, handle-bound, SHA-256 bound; 64 KiB = 683 chunks (§1.3) |
| Caller admission | separate sub-CP for K3; new `CALLER_FLAG_KMS_SSH_SIGN = 1 << 6` (§2). Admin callers are a different class and never `cellbox-ssh` |
| D-K5 | recommend **per-login unlock, bounded to exactly one signature** (keep the key passphrase-wrapped in KMS; passphrase staged once, consumed by one sign, then zeroized). No change to `command-contracts.md:142` (§3) |

## 1. ABI additions (K2)

All integers little-endian. Every payload below is `#[repr(C)]` with a fixed `LEN`, a strict `decode` (exact length, reserved bytes zero, padding after a variable-length field zero, charset validated) and an `encode`, following `payload/tls.rs`, `payload/rotate.rs` and `payload/enroll.rs`. The envelope (`KmsRequestV1`/`KmsResponseV1`) is **not** modified; `flags`, `reserved0`, `reserved1` stay zero and a nonzero value is a frame decode failure (`frame/request.rs`, `validate`).

### 1.1 New constants and enums (`libs/types/src/kms/ssh.rs`, new; `model.rs`)

| Name | Value | Basis |
|---|---|---|
| `KMS_SSH_KEY_NAME_MAX` | 64 | `command-contracts.md:132` "KEY_ID is 1..64 ASCII `[A-Za-z0-9_-]`" |
| `KMS_SSH_USERNAME_MAX` | 31 | same line: "USER is 1..31 ASCII bytes `[A-Za-z0-9._-]`" |
| `KMS_SSH_SESSION_ID_LEN` | 32 | `SessId = heapless::Vec<u8, 32>` (`third_party/sunset/src/kex.rs:38-39`); the profile is curve25519-sha256 only (`command-contracts.md` §4.2) |
| `KMS_SSH_PUBLIC_KEY_LEN` / `SIGNATURE_LEN` | 32 / 64 | ed25519 |
| `KMS_SSH_IMPORT_MAX_LEN` | 65,536 | `command-contracts.md:142` "at most 64 KiB encoded identity" |
| `KMS_SSH_CHUNK_LEN` | 96 | 112 payload - 16 chunk header (§1.3). The CSR precedent is 104 for a response with an 8-byte header (`csr.rs:15-16`, `payload/enroll.rs`, `RelayCsrChunkResponsePayload`) |
| `KMS_SSH_SECRET_MAX` | 1,024 | `command-contracts.md:142` secret-entry bound (only under per-login unlock) |
| `KmsKeyAlgorithm::SshEd25519` | 3 | append to the existing `wire_enum!` |
| `KmsSshKeyCustody` (new) | `None = 0`, `Plain = 1`, `PassphraseWrapped = 2` | derived by KMS from the cipher/KDF names of the imported blob, never caller-selected |
| `KmsCustodyPosture` (new) | `Unassessed = 0`, `DevPostureVolatile = 1` (T0), `DevPosturePersistent = 2` (T1) | D-K1: T0/T1 are `DEV_POSTURE`. **No `ProductionQualified` value is declared**; a later T2 adds one append-only. `RelayProviderAssessment` is relay-specific (`model.rs:112-120` area) and must not be reused (independent capability leaves, `code-standards.md:65-67`) |

`KmsProviderKind` is not extended by K2. T0 reports `None` (no provider) with posture `DevPostureVolatile`; the SSH leaf is `Ready` when the volatile store is serving. Adding the T1 x86 development provider value is K4b's append-only change. [OPEN O-3: whether `None` + `Ready` is acceptable for a volatile in-RAM leaf, or a distinct provider value is added in K2. Decider: user at CP1.]

Name and username validators (`validate_ssh_key_name`, `validate_ssh_username`) live beside `validate_hostname` (`csr.rs`) so KMS, client and tests share one definition.

### 1.2 Opcodes (append-only; none renumbered)

`KmsOpcode` currently ends at `GetRelayActivePublicKey = 14` (`model.rs:55`). **Admin** = a class decided by K7/K3 (O-2); **Login** = holder of the K3 flag.

| # | Name | Class | Purpose | Request LEN | Response LEN | Needed under D-K5 |
|---|---|---|---|---|---|---|
| 15 | `BeginSshKeyImport` | Admin | open one pending import; declare name, length, SHA-256 | 112 | 24 | either |
| 16 | `ImportSshKeyChunk` | Admin | one ordered chunk | 112 | 16 | either |
| 17 | `CommitSshKeyImport` | Admin | verify SHA-256, parse/allowlist, store, destroy staging | 48 | 80 (`SshKeyStatus`) | either |
| 18 | `AbortSshKeyImport` | Admin | destroy the pending import | 8 | 0 | either |
| 19 | `GetSshKeyStatus` | Login or Admin | leaf readiness (empty name) or one key's public facts | 65 | 80 | either |
| 20 | `DeleteSshKey` | Admin | remove one key | 105 | 0 | either |
| 21 | `SignSshUserauth` | Login | sign the session-bound userauth input | 104 | 104 | either |
| 22 | `BeginSshUnlock` | Login | open a one-shot staged secret for one wrapped key | 48 | 24 | per-login only |
| 23 | `SshUnlockChunk` | Login | one ordered chunk of the secret | 112 | 16 | per-login only |
| 24 | `AbortSshUnlock` | Login | destroy the staged secret | 8 | 0 | per-login only |

Each opcode is purpose-specific. There is no generic upload, generic sign or algorithm field. Chunk opcodes 16 and 23 share one payload type but are separate opcodes with separate pending-state slots and disjoint handle spaces; a handle for one never validates for the other.

`KmsRequestV1::from_bytes` rejects unknown opcodes at decode (`frame.rs:13`, `validate_header`) and `KmsService::handle` then returns `None` (`dispatch.rs:163`, `.ok()?`), so a KMS built without these opcodes drops them with **no reply**, while `KmsClient::call` blocks in `sys_recv(self.tid, ..)` (`libs/ostd/src/clients/kms.rs:162`) [INFERENCE: no timeout on that call]. Consequence: a new client against an old KMS hangs. The promoted image has no `/bin/kms` at all (`lock-adoption-draft.md`, K0 resolution 3), so no old KMS exists for this path; K6/K8 must still use matched KMS and client builds. [OPEN O-9: client-side bound (timeout recv) for K6. Decider: Cellbox plan owner.]

### 1.3 Payload layouts

**Convention for variable fields:** a length byte followed by a fixed zero-padded array; bytes after the length must be zero (`RelayEnrollmentBeginRequestPayload::decode`, `payload/enroll.rs`). Names must pass `validate_ssh_key_name` (`[A-Za-z0-9_-]`, 1..64) except in status where `name_len = 0` is the leaf query.

`SshImportBeginRequest` (op 15, LEN 112):

| Off | Size | Field | Rule |
|---|---|---|---|
| 0 | 8 | `request_id` | nonzero, strictly above the caller's floor (§1.6) |
| 8 | 4 | `total_len` | 1..=65,536 else `InvalidRequest` |
| 12 | 1 | `name_len` | 1..=64 |
| 13 | 3 | reserved | zero |
| 16 | 32 | `blob_sha256` | SHA-256 of the complete OpenSSH blob (binary, not armored); not all zero |
| 48 | 64 | `name` | zero after `name_len` |

`SshTransferBeginResponse` (ops 15 and 22, LEN 24): `0 handle:u64` (nonzero), `8 total_len:u32` (import: blob length; unlock: secret length), `12 chunk_count:u32` (= ceil(total/96)), `16 chunk_capacity:u16` (= 96), `18 reserved[6]` zero.

`SshChunkRequest` (ops 16 and 23, LEN 112): `0 handle:u64`, `8 chunk_index:u32`, `12 chunk_len:u16` (1..=96), `14 reserved:u16` zero, `16 chunk[96]` zero after `chunk_len`. Canonical chunking: every chunk except the last has `chunk_len = 96`; the last has `total_len - 96*(chunk_count-1)`. One blob therefore has exactly one valid chunk sequence (non-malleable, golden-testable).

`SshChunkAck` (ops 16 and 23 response, LEN 16): `0 handle:u64`, `8 next_chunk_index:u32`, `12 received_len:u32` (cumulative).

`SshImportCommitRequest` (op 17, LEN 48): `0 handle:u64`, `8 chunk_count:u32`, `12 reserved:u32` zero, `16 blob_sha256[32]`. Both echoed values must equal what Begin recorded and what was received.

`SshAbortRequest` (ops 18 and 24, LEN 8): `0 handle:u64`.

`SshKeyStatus` (ops 17 and 19 response, LEN 80):

| Off | Size | Field | Note |
|---|---|---|---|
| 0 | 1 | `algorithm` | `KmsKeyAlgorithm::SshEd25519` |
| 1 | 1 | `readiness` | `KmsCapabilityReadiness` (existing enum), independent of C2C and Relay |
| 2 | 1 | `provider` | `KmsProviderKind` (T0: `None`, O-3) |
| 3 | 1 | `posture` | `KmsCustodyPosture` |
| 4 | 1 | `key_present` | 0 when `name_len = 0` or key absent (absent key in a named query is `SshKeyNotFound`, not a status) |
| 5 | 1 | `custody` | `KmsSshKeyCustody`; 0 when no key |
| 6 | 2 | `key_count` | stored keys |
| 8 | 8 | `store_revision` | bumps on every import/delete; resets at boot at T0 |
| 16 | 32 | `public_key` | all zero when no key |
| 48 | 32 | `fingerprint_sha256` | SHA-256 of the 51-byte SSH wire public key blob (the value `ssh-keygen -lf` prints, base64) |

`SshStatusRequest` (op 19, LEN 65): `0 name_len:u8` (0 = leaf status, else 1..=64), `1 name[64]`.

`SshDeleteRequest` (op 20, LEN 105): `0 request_id:u64`, `8 public_key[32]`, `40 name_len:u8` (1..=64), `41 name[64]`. The admin names the key **and** supplies the public key it believes is stored; a mismatch is `SshKeyNotFound`. This replaces a revision CAS (there is no existing CAS precedent to copy: `RotateNodeIdentity` is a `SecureRootRequired` stub today) and prevents a delete racing a delete-and-reimport. Success payload is empty.

`SshUnlockBeginRequest` (op 22, LEN 48): `0 request_id:u64`, `8 public_key[32]`, `40 secret_len:u16` (1..=1,024), `42 reserved:u16`, `44 reserved:u32` zero. Valid only for a key whose custody is `PassphraseWrapped`; for `Plain` it is `InvalidRequest`. [INFERENCE: an empty passphrase is not accepted because OpenSSH `bcrypt_pbkdf` rejects a zero-length password; not verified here, K1/K5 must confirm against the chosen crate.]

`SshSignRequest` (op 21, LEN 104): `0 request_id:u64`, `8 public_key[32]`, `40 session_id[32]` (not all zero), `72 username_len:u8` (1..=31), `73 username[31]` (zero after `username_len`; charset `[A-Za-z0-9._-]`).

`SshSignResponse` (op 21, LEN 104): `0 request_id:u64` (echo), `8 signature[64]` (raw ed25519 `R || S`; the SSH `string "ssh-ed25519" || string sig` wrapping is the client's job, `OwnedSig::Ed25519([u8;64])`, `third_party/sunset/src/sign.rs:268-270`), `72 message_sha256[32]` (SHA-256 of the exact signed input).

`message_sha256` lets the client compare against the SHA-256 of the message Sunset hands it (`RequestSign::message`, `third_party/sunset/src/event.rs:129-146`) before using the signature, so a KMS/Sunset reconstruction disagreement is a local error instead of a server rejection. It costs no extra frame and the response still fits (104 ≤ 112). [OPEN O-8: keep or drop `message_sha256`. Decider: user at CP1; recommend keep.]

### 1.4 What a signature request contains, and the sizing proof

The signed data is RFC 4252 §7, as serialized by Sunset's `AuthSigMsg` (`third_party/sunset/src/auth.rs:14-29`, `packets.rs:254-291`): `string session_id | byte 50 | string user | string "ssh-connection" | string "publickey" | boolean TRUE | string "ssh-ed25519" | string pubkey_blob`.

| Component | Bytes | Who supplies it |
|---|---|---|
| `string session_id` | 4 + 32 = 36 | **caller** (request, 32 B) |
| `byte 50` | 1 | KMS constant |
| `string user` | 4 + n, n = 1..31 | **caller** (request) |
| `string "ssh-connection"` | 4 + 14 = 18 | KMS constant; not carried |
| `string "publickey"` | 4 + 9 = 13 | KMS constant; not carried |
| `boolean TRUE` | 1 | KMS constant |
| `string "ssh-ed25519"` (signature algorithm) | 4 + 11 = 15 | KMS constant; not carried, not selectable |
| `string pubkey_blob` | 4 + (4 + 11 + 4 + 32) = 55 | **KMS** from the stored key; the caller supplies only the 32-byte key as a selector, and it must equal a stored key |
| Total | **143 + n** | n = 31 gives 174; n = 1 gives 144 |

Sizing: the reconstructed input is 144..174 bytes, which does **not** fit a 112-byte payload, so the full message is never sent (and sending it would make KMS a raw-message signer, forbidden by `code-standards.md:59-61`). The request is 8 + 32 + 32 + 1 + 31 = **104 ≤ 112**; the response is 8 + 64 + 32 = **104 ≤ 112**. Both exact sums are asserted at compile time in the fixtures (`size_of` asserts as in `payload/tls.rs`). Arithmetic check run in this session (python3, not committed): 143 + 4 = 147 for the vector in §6.

This matches the standing rules (`code-standards.md:54-70`) except one clause that needs the maintainer's explicit reading (LR-1 and LR-2 in §9.1): "no caller-selected key". The contract itself requires `--identity KEY_ID` (`command-contracts.md:128-129,132`), so the caller must be able to choose among stored keys. The proposal restricts that choice to a purpose-specific class (`SshEd25519` keys only, reachable only with the SSH flag), never an algorithm, and never a raw message. The public key is the selector rather than a KMS handle so it survives KMS restart and a T1 reload; two names for one public key are refused at import (`SshKeyExists`) so the selector is unambiguous.

### 1.5 Error codes (append-only; `model.rs:121-156` ends at 26)

| Code | Name | Meaning |
|---|---|---|
| 27 | `SshKeyUnavailable` | the SSH leaf is not serving (not provisioned, provider failed, sealed) |
| 28 | `SshKeyNotFound` | name or public key not stored, or delete public key mismatch |
| 29 | `SshKeyExists` | name taken or public key already stored |
| 30 | `SshStoreFull` | key-count or byte budget reached (O-6) |
| 31 | `SshImportPending` | an import by the same principal is still open |
| 32 | `SshHandleInvalid` | stale, foreign, consumed or unknown transfer handle |
| 33 | `SshOrderInvalid` | chunk index, length, total or commit state out of order |
| 34 | `SshImportIntegrity` | SHA-256 or count mismatch at commit |
| 35 | `SshKeyFormatRejected` | content refused by the pre-KDF allowlist/structure checks (any reason; no free text) |
| 36 | `SshUnlockRequired` | sign on a wrapped key with no complete staged secret |
| 37 | `SshUnlockFailed` | wrong passphrase, checkint mismatch or derived public key mismatch (one code: no oracle between them) |
| 38 | `SshUnlockLocked` | failure limit reached (O-5); refused before any KDF work |

Existing codes reused: 1 `CallerUnattested`, 2 `PermissionDenied` (wrong class), 19 `InvalidSignature` (self-verify failed), 21 `InvalidRequest` (field-level violation and non-monotonic `request_id`, as `relay.rs:58`). Codes 36-38 exist only under per-login unlock; if D-K5 is decrypt-once they are dropped and 27-35 stay (numbers are not frozen until CP2, so they renumber densely). `KmsErrorCode::try_from` rejects unknown codes (`model.rs`, `wire_enum!`), so an old decoder refuses a new error response with `UnknownErrorCode`: an additive, fail-closed behavior.

### 1.6 Replay and sequence ids

Three distinct counters:

1. **Frame `request_id: u32`** (header offset 4): client-chosen correlation, echoed by KMS and checked by `KmsClient::call` (`kms.rs`, `call`). Not a security control; unchanged.
2. **Payload `request_id: u64`** (ops 15, 20, 21, 22): nonzero and strictly greater than the **per-principal floor**, where the principal is the attested `(cell_id, generation)`. The floor advances **only after all checks and the operation succeeded** (`code-standards.md:62-64`; precedent `relay.rs:58,111`, one global `last_tls_request_id` reset at registration, `dispatch/relay.rs`). A failed attempt leaves the floor unchanged, so a retry needs a fresh higher id only if the earlier attempt succeeded or the client cannot tell. One floor covers ops 15/20/21/22 so an unlock id n is followed by a sign id above n.
3. **`chunk_index: u32` + handle** (ops 16, 23): strictly sequential from 0; the handle is bound to the opening principal and its TID. Any violation (wrong index, repeated chunk, foreign principal, early/late commit, wrong length) **destroys the pending transfer and zeroizes it**, following the CSR rule "stale, foreign, repeated, or out-of-order handle use must invalidate the flow" (`code-standards.md:113-116`; `lifecycle/pending.rs`, `chunk`).

The floor table is bounded (proposed 4 principals, oldest evicted). Eviction resets that principal's floor to zero [INFERENCE: harmless because a replayed frame can only come from the live attested principal itself]. KMS has no registration for these callers (the registry holds only NET_BROKER, SUPERVISOR, NET: `main.rs:34-36`), so the `register_*` binding pattern (`auth.rs:84,102`) is not reusable. [OPEN O-4: table size and eviction. Decider: user at CP1.]

Handles: nonzero, unique per transfer within a KMS instance, bound to the principal; the derivation is a K5 detail (the CSR precedent mixes counters with `blake2s`, `lifecycle/pending.rs`, `derive_csr_handle`, and notes uniqueness is by bound facts, not entropy). Handles are not secrets; the checks enforce access.

### 1.7 State machines

**Import (ops 15-18)** `Idle -> Receiving(next_index, received) -> Complete -> Idle`:
- Begin: admin class; leaf serving; `request_id` ok; name valid and unused; `total_len` in range; store not full. Another principal's pending import is replaced after zeroizing it so a dead admin instance cannot wedge the leaf; the same principal gets `SshImportPending` and must Abort (O-7).
- Chunk: handle, principal and TID match; `chunk_index == next`; canonical `chunk_len`; `received + chunk_len <= total_len`. Reaching `total_len` moves to Complete.
- Commit: Complete only, count and SHA-256 equal; **then** structural parse and allowlist (the K1/D-K4 parser); public-key uniqueness; insert; zeroize the 64 KiB staging buffer (this is why SHA-256 binds before parsing: no expensive work on a corrupt transfer). Response is `SshKeyStatus` of the new key.
- Abort or any violation: zeroize, `Idle`. KMS restart: `Idle` and (T0) an empty store.

**Unlock (ops 22-24, per-login only)** `Idle -> Collecting -> Staged -> consumed`:
- Begin: Login class; key exists and is wrapped; not locked out; `secret_len` 1..=1,024; opens one staged-secret slot bound to `(principal, tid, public_key)` (one slot; a second Begin from the same principal replaces and zeroizes).
- Chunk: same rules and violation handling as import; reaching `secret_len` makes it Staged.
- Sign: consumes the slot (success **or** failure), zeroizes the secret and every KDF/decrypt intermediate before replying; the unlocked seed lives only on the stack inside the signing call. There is **no persistent unlocked state** and so no timer is needed (KMS has no clock: `main.rs` blocks in `sys_recv_attested(0, ..)`).
- Abort, violation, restart, key delete: zeroize.

**Sign (op 21):** admission; `request_id`; session id nonzero; username valid; key found by public key; if wrapped, consume a Staged slot for the same principal and key else `SshUnlockRequired`; decrypt, check `checkint` and that the derived public key equals the stored one (else `SshUnlockFailed`); reconstruct the §1.4 input; sign; **self-verify with the stored public key** (else `InvalidSignature`, no output); zeroize; advance the floor; reply. Deterministic ed25519 means no RNG on this path.

### 1.8 Frozen maximum sizes

| Quantity | Max | Frames |
|---|---|---|
| OpenSSH v1 blob (ABI) | 65,536 B | 1 Begin + 683 chunks + 1 Commit = 685 round trips |
| Chunk math | `chunk_count = ceil(total/96)`; last chunk `total - 96*(chunk_count-1)`; 65,536 = 682*96 + 64, so 683 chunks, last = 64 B. At 100 B/chunk (no explicit length field) it would be 656; the explicit `chunk_len` and 16-byte header were kept to follow the CSR convention (`RelayCsrChunkResponsePayload`) |
| Staging buffer | 64 KiB in KMS | one pending import |
| Secret (per-login) | 1,024 B | Begin + ceil(1024/96) = 11 chunks (10*96 = 960; last = 64) + Sign |
| Name / username / session id / key / signature | 64 / 31 / 32 / 32 / 64 B | single frame |
| Reference blobs from OpenSSH-generated files | plain 250 B = 3 chunks; AES-256-CTR 290 B = 4 chunks (computed in this session from `ssh-key-0.7.0-rc.11/tests/examples`, base64-decoded) | |

Whether KMS must *store* up to 64 KiB per key is a separate budget (O-6). Lowering the stored cap below the contract's 64 KiB narrows the contract's accepted set and needs user acceptance.

### 1.9 Admission classes inside KMS (what K2 fixes, K3 supplies)

| Opcode | Admitted caller | Otherwise |
|---|---|---|
| 15-18, 20 | Admin class | `PermissionDenied` |
| 19 | Admin or Login | `PermissionDenied` |
| 21-24 | Login (K3 flag) | `PermissionDenied`; `CallerUnattested` for a missing or invalid trailer (`auth.rs:132-140`) |

The wire format does not depend on how the Admin class is realised (O-2); `validated_caller` (`auth.rs:132`) runs first for every opcode, and no provider or store is touched before admission (`code-standards.md:62-64`). The only Law-1 clause that cannot be met literally is "registered service TID": the Login class has no registry entry, and liveness is instead proven by the kernel on every receive (`attested_identity_of` returns `None` unless the cell owner is live and the generation matches, `kernel/src/task/syscall.rs:2889-2910`). [OPEN O-1: maintainer accepts this reading at CP1.]

### 1.10 Exact K2 file closure (Law-1 preview, not an edit)

`libs/types/src/kms/model.rs` (opcodes 15-24, `SshEd25519`, two enums, errors 27-38), `kms/ssh.rs` (new: constants, validators), `kms/payload/ssh.rs` (new), `kms/payload.rs` (mod and re-exports), `libs/types/src/kms.rs` (`mod ssh`, re-exports), `kms/tests/ssh.rs` (new), `kms/tests/mod.rs`, `kms/tests/frame.rs` (extend `opcode_values_are_append_only` and `fixed_layouts_match_the_frozen_wire_contract`). Followers outside Law 1: `cells/services/kms/src/dispatch.rs` has an exhaustive `match opcode` with no wildcard arm (`dispatch.rs:165-199`), so adding variants breaks the KMS build until it handles them. Two ways: (A) land K2 CP2 and K5 as one integration, or (B) K2 CP2 includes arms that admit then answer `SshKeyUnavailable`, which is the honest status of a build without the leaf and is exercised by the refusal matrix; K5 replaces them. Recommend B (it keeps K2 independently testable and is not a no-op: it is the unavailable-leaf behavior). [OPEN O-10: A or B. Decider: user at CP1.] `docs/code-standards.md` gains a "KMS SSH Custody Boundary" subsection beside "Relay Certificate Provisioning Boundary" (text proposed at CP2, not written here), plus `CHANGELOG.md`. `libs/ostd/src/clients/kms.rs` additions are K6 and not Law 1.

## 2. Caller admission: separate sub-CP for K3

K2 fixes only the KMS-side expectation: a `u32` flag in `CallerIdentity.flags` (a plain field, so K5 host tests can synthesize it without the kernel). Everything below is **K3's CP1**, which needs this K2 design approved first and the kernel-permission lock handoff (`lock-adoption-draft.md`, row `260603-1803-phase-h-vfs-hardening` / controlling P05).

### 2.1 Right and what the kernel attests

| | Proposal |
|---|---|
| Flag | `CALLER_FLAG_KMS_SSH_SIGN = 1 << 6` (first reserved bit; bits 0-5 are in use, `caller_identity.rs:45-75`) |
| Kernel field | `CapSet::ssh_sign: bool` of the sender's **Cell root**, exported by `attested_identity_of` exactly like `config_caller_flags` (`syscall.rs:2889-2910`) |
| Requested by | exact install path `/bin/cellbox-ssh` only, in a new `with_path_ssh_rights` helper kept out of `with_path_caps`' body (that body is read by `sign-policy.py` as the P-TRUST list, `cap.rs:481-483`), bounded by launch ceiling and signed policy |
| Attested | the sender's live Cell was started at install path `/bin/cellbox-ssh` through the reviewed launch edge, and its root `CapSet` still holds the right |
| **Not** attested | that the code is unmodified `cellbox-ssh`, that the host key was trusted, that the user authorised this login, that the session id is real (KMS cannot enforce "trust before signature"; `kms-prerequisites.md` §3) |

Admin class: if K7 chooses a supervisor-authorised operation, the existing `authorize_supervisor` (`auth.rs:120`) is used and **no second flag** is needed. If K7 chooses a dedicated admin cell, a second flag `CALLER_FLAG_KMS_SSH_ADMIN = 1 << 7` repeats this whole sub-CP for that cell's path. The admin flag and the sign flag are never held by one Cell. [OPEN O-2: admin class. Decider: user at K7 CP1; K2 only requires that it is distinct from Login.]

### 2.2 Files the Q-right precedent changed, and whether this right needs them

Source: `phase-01-config-right-cp1.md` §4.2 and §8, cross-checked with a repository grep for `config_write|CONFIG_WRITE|CAP_BYTES_V7` this session.

| File (Q precedent) | Q change | Needed for `ssh_sign` |
|---|---|---|
| `libs/api/src/abi/caller_identity.rs` | flag `1<<5`; reserved-bit doc and test (`:61`, `:207` "Bits 6..31 are reserved") | **Yes**: new flag; reserved range becomes 7..31; update test |
| `libs/api/src/services/ipc.rs` | `CONFIG_ERR_DENIED` constant | **No**: KMS errors live in `types::kms` |
| `kernel/src/task/cap.rs` | field in `CapSet`, `EMPTY`, `ALL`, `of_task`, `from_manifest`, path helper, `intersect`, `apply_to`, `from_task`, flag helper | **Yes** (all of it) |
| `kernel/src/task/tcb.rs` | per-task field | **Yes** |
| `kernel/src/task/syscall.rs` (`attested_identity_of`) | OR the flag | **Yes** |
| `kernel/src/policy.rs` | v7, `CAP_BYTES_V7 = 19`, legacy decode, audit bit | **Yes**: v8, `CAP_BYTES_V8 = 20`. **Legacy decode differs**: Q decoded v1-v6 as `true` because the write was ambient before (`policy.rs:361-367`); SSH signing was never ambient, so older entries should decode `false` (fail-closed). [OPEN O-11: user decides D-Q6 analogue] |
| `kernel/src/audit.rs` | doc for narrowing bit 16 | **Yes**: next narrowing bit (17) [INFERENCE from the precedent] |
| `kernel/src/loader/boot_ceiling.rs`, `boot_ceiling/selftest.rs` | `/bin/cellbox` boot row and self-test | **Compile-follow only**: `/bin/cellbox-ssh` is not a boot row (shell launches it); `CapSet` literals in these files need the new field if they enumerate fields [INFERENCE: not opened] |
| `kernel/src/loader/launch_profile/profiles.rs` | `SHELL_EDGE` rows | **Yes**: the `/bin/cellbox-ssh` edge (`profiles.rs:75`) |
| `kernel/src/loader/launch_profile/targets.rs` | not changed by Q | **Yes (new vs precedent)**: `profiles.rs:75` uses `NET_CLIENT`, which `targets.rs:79` also assigns to curl, wget and others, so the right cannot be added to `NET_CLIENT`; a new ceiling constant (`NET_CLIENT` + `ssh_sign`) is needed |
| `kernel/src/loader/launch_profile/tests.rs` | row tests | **Yes**: `:149-160` already tests the `cellbox-ssh` edge; add "only this edge carries the right" |
| `scripts/sign-policy.py` | `VERSION = 7`, `CAP_BYTES[7]`, request helper | **Yes**: v8 and `ssh_sign_request(path)` mirroring the kernel helper |
| `cells/services/config/src/{main,rights}.rs` | service-side admission | **Equivalent is K5**: KMS `auth.rs`/dispatch check `flags & CALLER_FLAG_KMS_SSH_SIGN` (K5, `cells/services/kms/**`, KMS lock row) |
| `cells/tools/cellbox/**` | client mapping, `export` | **K6**: `cellbox-ssh` handling of `PermissionDenied` |
| `cells/tests/phase03-filter`, `phase04-terminal-driver`, `scripts/qemu-phase04-terminal-test.sh` | private lane proof | **Lane for K3/K8** (OPEN O-12: the promoted image has no `/bin/kms`, so a QEMU proof needs an image delta first) |
| `command-catalog.md`, `acceptance.md`, `CHANGELOG.md` | evidence text | `CHANGELOG.md` yes; catalog unchanged (no new command, `contract-delta-import-surface.md` §3) |

Compile-follow: wherever `CapSet { .. }` is built with every field spelled out, the new field is required (`policy.rs`, `cap.rs`, test fixtures `syscall.rs:9975-10014`). Not run.

## 3. D-K5: per-login unlock versus decrypt-once

### 3.1 Contract constraint (verbatim anchors)

`command-contracts.md:142`: "Unlock asks at most once under the same secret-entry bound: the actual foreground client captures the secret under its separate no-echo lease and passes it only to the authenticated credential owner for bounded unlock; parsing/decryption/private-key custody remain inside that owner. Failed auth/unlock returns failure without switching methods or retries." Also "No ... import command or secret persistence is added to the CLI." The S9-5 proposal (`phase-09-s0-proposal.md:106`) says the passphrase "is read under a no-echo lease. KMS keeps the key ... sealed as existing KMS records are, and returns a `KEY_ID`" (decrypt at import). K0 delta item 3 forwards "at most one secret" at login for a wrapped key.

### 3.2 The two models

- **A. Per-login unlock, bounded to one signature** (recommended). KMS stores the OpenSSH blob wrapped (cipher/KDF fields and ciphertext verbatim) or plain. For a wrapped key the foreground client captures the passphrase once and stages it through ops 22-23; the next sign (op 21) consumes it: KDF, AES-256-CTR decrypt, `checkint`, public-key equality, sign, self-verify, zeroize. No unlocked seed persists between frames.
- **B. Decrypt once at import.** The admin path supplies the passphrase at import (secret chunks added to the import ops); KMS decrypts, keeps only the seed, and (T1) seals it under the KMS root. Login needs no secret. Ops 22-24 are not needed.

### 3.3 Comparison

| Property | A. per-login | B. decrypt-once |
|---|---|---|
| Confidentiality at rest, T1 | wrapped by the user's passphrase (bcrypt up to 64 rounds); a disk + image holder does not get the key without the passphrase | wrapped only by the KMS root, which at T1 is a **development key co-resident with the disk** (D-K7; `LANE-002` rejects such a root as a gate: `kms-prerequisites.md` §5). A disk + image holder gets the key. This is an **at-rest downgrade of an encrypted key** |
| RAM exposure, T0 | key wrapped in RAM; seed exists only inside one sign call | seed in RAM for the whole boot |
| Compromised admitted `cellbox-ssh` | sees the passphrase it captures; signing still requires that passphrase each time | can sign any session id for any host with no secret [INFERENCE: KMS cannot verify trust or host, §2.1] |
| Where the passphrase flows | foreground client -> KMS only, per login | the import tool/admin path must handle it on every boot at T0 (keys are re-imported every boot, `plan.md` T0), and K7 is non-CLI with no foreground no-echo lease of its own: **a second secret-capture surface to design** |
| Headless use (pipeline stage, non-UTF-8 stdin) | encrypted key: refused (no terminal; consistent with the password path, `command-contracts.md` rights `T`). **Plain imported key**: works | works for every imported key |
| KDF cost | per login, on the single-threaded KMS loop (`main.rs:27-50`). Rounds are contract-bounded (1..64) but the time is **unmeasured** [INFERENCE: seconds natively; far worse under QEMU TCG]. On x86 the relay and node-identity paths are already `Unavailable` (`kms-prerequisites.md` §1.3) so the stall costs little there; on AArch64 DEV_REFERENCE it blocks relay signing | once, at import |
| KMS-side attack surface | +3 opcodes, one staged-secret slot, a failure counter | secret chunks on import; no login-time secret path |
| Wrong passphrase / bad key found | at login (generic `SshUnlockFailed`) | at import |
| Key corrupt but passphrase unknown at import | not detectable at import (no passphrase); the first unlock verifies | detected at import |
| Failure modes added | stale staged secret after a client crash (destroyed by abort, replace, key delete, restart); brute-force by an admitted caller (each try costs one KDF; no clock, so only a count limit is possible, O-5) | none new at login |
| Contract impact | none: this is the contract's own shape. One clarification worth recording: "bounded unlock" = valid for exactly one `SignSshUserauth` | line 142 must change: the unlock moves from the foreground client at login to the import surface; "forwards at most one secret" in K0 delta item 3 is withdrawn; the S9-5 import line stays consistent. A contract delta with its own approval (`contract-delta-import-surface.md` §3) |
| Reversibility | B can be added later with new opcodes (append-only) | A cannot be added for already-imported keys: the passphrase is gone, so they must be re-imported from the original file |

### 3.4 Recommendation

Choose **A, per-login unlock bounded to exactly one signature**, and support plain (unencrypted) keys through the same ABI for headless use. Reasons grounded above: it matches line 142 without a contract delta; at the development posture (T1) the user's passphrase is the only real confidentiality at rest because the dev root is co-resident; the passphrase never enters the admin path (no second secret-capture surface); and the seed never outlives a single call. The cost is stated honestly: an encrypted key cannot be used by a non-foreground pipeline stage, KDF stalls the KMS loop, and a brute-force limit is count-based only.

Conditions that would flip it to B (user decides, O-13): the user needs passphrase-protected keys usable headlessly, or measured KDF time under the K8 environment is unacceptable and a lower rounds cap is refused. B then needs the line 142 delta, an explicit "decrypt-once loses passphrase protection" label at import, and the T0 admin path must be given a secret-capture design.

### 3.5 Required K5 measurement and the failure limiter (per-login)

K5 must measure `bcrypt-pbkdf` at rounds 16 and 64 on the QEMU target before any acceptance claim (not done here). Failure limit: count consecutive `SshUnlockFailed` per key; at N (proposed 5) answer `SshUnlockLocked` without KDF until restart or key delete/re-import. A compromised caller can lock a key out (denial of service), traded against unlimited guessing; volatile at T0 so a restart resets it. [OPEN O-5: N and reset rule. Decider: user at CP1.]

## 4. Test and fixture plan (K2 CP2 and K5)

K2 CP2 tests are host tests in `libs/types` (and the KMS refusal matrix for option B). No VM.

### 4.1 Frozen layouts

- `size_of` asserts for every payload in §1.3 (as `payload/tls.rs`, `tests/frame.rs`) and `KmsRequestV1`/`KmsResponseV1` still 128.
- Append-only tests extend `tests/frame.rs`: opcode 15..24 values, `KmsKeyAlgorithm::SshEd25519 as u8 == 3`, error codes 27..38 (as `:44-50` does for 22..26), and that 1..14 / 1..26 are unchanged.
- Golden hex per payload plus full 128-byte request, success and error frames per opcode, following `tests/authority_compat_vectors.rs` (`BEGIN_REQ`, `FRAME_BEGIN_SUCCESS`, ... with frame `request_id` `0x11223344`), then `encode`/`decode` round trip and exact re-encode.
- Decode rejection per payload: wrong length, nonzero reserved, nonzero padding after a variable field, name/username charset, `name_len` 0/65 (op 19 allows 0), `total_len` 0/65,537, `chunk_len` 0/97, `secret_len` 0/1,025, all-zero session id, all-zero SHA-256, request_id 0.

### 4.2 Chunking vectors

| `total_len` | `chunk_count` | last `chunk_len` |
|---|---|---|
| 1 | 1 | 1 |
| 96 | 1 | 96 |
| 97 | 2 | 1 |
| 250 (OpenSSH plain ed25519 reference) | 3 | 58 |
| 290 (OpenSSH AES-256-CTR reference) | 4 | 2 |
| 65,535 | 683 | 63 |
| 65,536 | 683 | 64 |
| secret 1,024 | 11 | 64 |

(Computed in this session; the 250 and 290 totals are base64-decoded lengths of the files in §4.4.) Also: a transfer driven by a property test (random sizes) reassembles to the original and a re-chunked sequence is byte-identical.

### 4.3 Signature-input vector (computed, no key used)

Public key = the 32 bytes of `ssh-key-0.7.0-rc.11/tests/examples/id_ed25519.pub` (`b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62`), `session_id = 00 01 02 ... 1f`, username `user` (n = 4). Reconstructed input (147 bytes = 143 + 4):

```
00000020 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f
32 00000004 75736572 0000000e 7373682d636f6e6e656374696f6e
00000009 7075626c69636b6579 01 0000000b 7373682d65643235353139
00000033 0000000b 7373682d65643235353139 00000020 b33eaef3...ab62
```
(the spaces are for reading; the fixture is one 147-byte string). SHA-256 of these 147 bytes: `fa3246abe7b89d40c24cd8b330fc0eba243f06d4835cbc88181be36bb4e6de65`. SHA-256 of the 51-byte public key blob: `5025222ebecf8ecf7014524c0c1c8b81cdcdaed754df8e0e814338e7064f7084` (base64 `UCUiLr7Pjs9wFFJMDByLgc3NrtdU344OgUM45wZPcIQ`, which `ssh-keygen -lf` should print as `SHA256:...`; **not run here**). Expected signature: produced at CP2 by an independent implementation over these exact bytes and verified by KMS and by Sunset's verify path; it is not computed now.

### 4.4 Cryptographic and OpenSSH vectors (K5; described, **not generated here**)

- **RFC 8032 §7.1 pure Ed25519** (TEST 1, TEST 2, TEST 3, TEST 1024, TEST SHA(abc)): proves seed -> public key -> signature in KMS independent of SSH framing. The local `ed25519-dalek-3.0.0` ships only an **Ed25519ph** vector (`tests/ed25519.rs:101-`, RFC 8032 §7.3), and no pure vectors file; the pure vectors must be transcribed from the RFC text at CP2 with the source recorded. They are not recalled from memory in this document.
- **OpenSSH-generated reference keys already on disk** (read-only registry copy, not generated here): `ssh-key-0.7.0-rc.11/tests/examples/id_ed25519` (411 B armored, blob 250 B, sha256 `1d3eb512fd04aef12599c2540b08c95a3319170652a8c07c226c3aa3d431858b`, comment `user@example.com`, produced by `ssh-keygen -t ed25519 -f id_ed25519 -C user@example.com`, `examples/generate.sh:12`) and `id_ed25519.aes256-ctr.enc` (464 B armored, blob 290 B, sha256 `b5fd6cdd4a7f204bf0de748a68f8c87903941c8c27dde62eed90f99963ff963d`; bcrypt rounds 16, salt `4a1fdeae8d6ba607afd69d334f8d379a`, passphrase `hunter42`, plaintext is the first file: `tests/encrypted_private_key.rs:69-71,74-86,183`). Copying either into this repository is a licensing and K1/K5 decision (the crate is MIT OR Apache-2.0 [INFERENCE from `LICENSE-*` file names]).
- **Vectors still to be produced at CP2 on a host, with throwaway keys**: `ssh-keygen -t ed25519 -Z aes256-ctr -a N` for rounds 1 and 64; salt 64 B is not producible by `ssh-keygen` (it uses 16) so the extremes and the rejects (rounds 0/65, salt 15/65, `aes128-ctr`, `chacha20-poly1305@openssh.com`, `aes256-gcm@openssh.com`, kdf `none` with a cipher, two keys, `ecdsa`/`rsa`, trailing bytes, bad `checkint`, bad padding) are **hand-crafted containers**, which is valid because every one of those is refused before the KDF.
- **End-to-end** acceptance (K8, not K2): login to a controlled OpenSSH server; a signature for another session id is refused by the server; no key or passphrase bytes in serial/logs.

### 4.5 Refusal matrix (K2 under option B for ABI/admission rows; K5 for the rest)

Every row asserts: the typed error, **no state change** (store revision, pending slot, floor, failure counter), and no provider/store access before admission.

| # | Case | Expected |
|---|---|---|
| 1 | missing, garbage or `cell_id = 0` trailer; `generation = 0`; `sender_tid != sender` | `CallerUnattested`, any opcode 15-24 |
| 2 | attested caller without the sign flag (shell, net, net-broker, supervisor-as-Login) on 21-24 | `PermissionDenied` |
| 3 | Login caller on 15-18, 20 | `PermissionDenied` |
| 4 | Admin caller on 21-24 | `PermissionDenied` (admin never signs) |
| 5 | NET/relay readiness `Ready` while the SSH leaf is not | `SshKeyUnavailable` (capability leaves independent, both directions tested) |
| 6 | frame with nonzero `flags`/`reserved`, nonzero bytes after `payload_len`, opcode 25+, version != 1 | decode error, **no reply** (`dispatch.rs:163`) |
| 7 | wrong payload length, nonzero reserved, nonzero padding, bad charset | `InvalidRequest` |
| 8 | `request_id` 0 or <= floor; reuse after success | `InvalidRequest`; floor unchanged |
| 9 | Begin: `total_len` 0 or > 65,536; bad name | `InvalidRequest` |
| 10 | Begin: name exists | `SshKeyExists` |
| 11 | Begin while the same principal's import is open | `SshImportPending` |
| 12 | Begin by another admin principal while one is open | replaces and zeroizes (O-7); old handle then `SshHandleInvalid` |
| 13 | chunk: wrong handle, foreign principal, consumed handle | `SshHandleInvalid`, pending destroyed |
| 14 | chunk: index skipped, repeated or backwards; `chunk_len` 0 / > 96 / short non-final / overflow past `total_len` | `SshOrderInvalid`, pending destroyed |
| 15 | commit before all bytes; count mismatch | `SshOrderInvalid`, destroyed |
| 16 | commit SHA-256 differs (from Begin or from received bytes) | `SshImportIntegrity`, destroyed |
| 17 | content: not `openssh-key-v1\0`, `nkeys != 1`, type != `ssh-ed25519`, cipher not in {none, aes256-ctr}, KDF not in {none, bcrypt}, cipher/KDF inconsistent, rounds outside 1..64, salt outside 16..64, trailing bytes, bad padding/`checkint` (plain), public != derived (plain) | `SshKeyFormatRejected`, nothing stored, **no KDF run** |
| 18 | same public key under another name | `SshKeyExists` |
| 19 | store budget reached | `SshStoreFull` |
| 20 | status for an unknown name | `SshKeyNotFound` |
| 21 | delete with unknown name or wrong public key | `SshKeyNotFound`; key intact |
| 22 | sign: unknown public key | `SshKeyNotFound` |
| 23 | sign: all-zero session id; bad username | `InvalidRequest` |
| 24 | sign: wrapped key, nothing staged | `SshUnlockRequired`; no state change |
| 25 | unlock Begin on a plain key; `secret_len` 0 / > 1,024 | `InvalidRequest` |
| 26 | unlock chunk violations (as rows 13-14) | destroyed secret; next sign `SshUnlockRequired` |
| 27 | sign with a secret staged for a different key or principal | `SshUnlockRequired`/`PermissionDenied`; secret destroyed |
| 28 | wrong passphrase; right passphrase but corrupt blob; derived public key != stored | the **same** `SshUnlockFailed`; secret consumed; counter +1 |
| 29 | failure limit reached | `SshUnlockLocked`, no KDF work |
| 30 | self-verify fails (fault injection) | `InvalidSignature`, no signature bytes in the reply, seed zeroized |
| 31 | signature produced for session A does not verify for session B or username B | verify fails (KMS-side fixture of "a signature for another session is refused") |
| 32 | zeroization: staging, secret, seed and KDF buffers are zero after commit, abort, violation, sign, delete | unit tests over the KMS buffers (K5) |
| 33 | restart (fixture re-creation): store empty (T0), pending cleared, floors reset | as stated |
| 34 | no key, passphrase or seed bytes in any reply, `Debug` output or log line | grep-style test over reply frames and `Debug` impls |

## 5. Digest-bound form used by the precedent, and what K2 binds

Observed forms:

1. **Phase 01 CP1** (`phase-01-config-right-cp1.md:3`): the approval is recorded **inside the same file**: "Approved text SHA-256 `26d5b6a6808fb5989684a21b349a30ea6c6fc2730e30c35eab738b6d5e4783b4` (before this status line was amended)"; the later CP2 amendment records a second digest the same way. Weakness: the approved bytes are not recoverable from the current file, so the digest cannot be re-derived by `sha256sum` (observed: the file was edited after approval).
2. **`contract-deltas.md`**: a digest table that lives **outside** the hashed documents, "SHA-256 of whole UTF-8 file, including final newline", no normalization, no self-hash, a runnable `sha256sum` command. Re-verified this session: `command-contracts.md` still hashes to `4ff1c3be46a39312d400877e0655a715b024563f2d84db7aecf27947efb2090d`.
3. **Phase 02** (`phase-02-cp1-approval.md`): approval recorded in a **separate immutable-subject record**: annex digest + a bound source snapshot JSON digest + source HEAD + exact path closure with pre-edit hashes; "the approved annex remains byte-for-byte immutable".
4. **S2-N / S4 dossiers**: per-file pre-edit hashes tables (`phase-04-s2n-cp2-dossier.md`, "current disk SHA-256").

**K2 proposal (form 2 + 3):** at approval, create `k2-cp1-approval.md` (outside this file) recording (a) the whole-file SHA-256 of the then-final `k2-cp1.md` (UTF-8 bytes including the final newline, no normalization, no digest inside the draft); (b) the same for the input documents it relies on (`plan.md`, `contract-delta-import-surface.md`, `lock-adoption-draft.md`, `kms-prerequisites.md`) and `command-contracts.md` (`4ff1c3be…`, above); (c) source HEAD and the pre-edit SHA-256 of the exact closure in §1.10 (`libs/types/src/kms/**` files, `dispatch.rs`) with a runnable `sha256sum` command; (d) the user's exact decision text and the chosen O-items. CP2 then binds the post-edit digests, the exact diff and the host evidence. **No digest is computed here for the unfinished draft.** This file will change when O-items are decided, so the digest is taken only after that edit.

## 6. Explicitly out of scope for K2

- **T1 persistence**, the sealed record v2, the x86 development provider, the dev root key, rollback/crash claims (K4b, D-K2; the T1 provider value for `KmsProviderKind` is K4b's append). K2 only reserves `DevPosturePersistent`.
- **Dependency admission and parser choice** (`bcrypt-pbkdf`, `ssh-key` vs own parser, D-K4): K1. K2 specifies only what the parser must accept and the error it returns.
- **The caller flag implementation, kernel/policy/launch/signer edits** (K3); **KMS implementation, parser, KDF, ed25519, store** (K5); **`KmsClient` additions, vissh/cellbox, `--identity`, passphrase lease, `publickey_unavailable`** (K6); **admin import surface and Admin class** (K7); **QEMU evidence** (K8); T2 production custody (K9).
- Any change to `command-contracts.md`, `command-catalog.md` (only if D-K5 = B, a separate contract delta), the authority protocol (`libs/authority-protocol`), CellosFS, or image/signing.
- KMS-side enforcement of host trust before signature (cannot be done with the session id alone).
- Listing keys (`GetSshKeyStatus` is by name only). A list opcode is a later append-only addition if K7 needs it.

## 7. Risks

- KDF stall of the single-threaded KMS loop (§3.3), unmeasured.
- A compromised admitted `cellbox-ssh` can request signatures for sessions of its choosing; the ABI binds session id and username only, not the host. Stated limit of T0/T1 (`plan.md` tiers table).
- Brute-force limiter is count-based; no clock in KMS.
- Eviction of the replay-floor table resets a principal's floor (O-4).
- Option B of §1.10 lands handlers that answer `SshKeyUnavailable` until K5; they must not be mistaken for implementation in status reports.

## 8. Not checked

No compile, test or lane run; no KDF timing; `bcrypt-pbkdf` empty-password behavior; the RFC 8032 text (no fetch; K1's network authorization covers dependency evidence only); `kernel/src/loader/boot_ceiling*.rs` and `kernel/src/audit.rs` bodies (listed from the Q-precedent and a grep, not opened); whether supervisor can carry an import (O-2); `KmsClient::call` timeout behavior beyond the blocking `sys_recv` line cited; lifecycle/enrollment code beyond `pending.rs`; `ssh-keygen` output (`-lf`) for the example key; license text of the `ssh-key` example files; whether `/bin/kms` ships in any non-promoted image.

## 9. Law-1 readings and decisions to confirm (replaces the draft's OPEN register)

### 9.1 Law-1 clause readings that need the maintainer's explicit confirmation
`docs/code-standards.md:54-70` is stricter than this design in two places. Confirmation 1 must either accept these readings or choose the Law-literal alternative.

| ID | Law-1 clause | What this design does | Alternatives for the maintainer |
|---|---|---|---|
| **LR-1** | "Do not expose ... caller-selected key or algorithm" (`code-standards.md:59-61`) | The caller supplies a 32-byte public key that selects among stored `SshEd25519` keys (never algorithm, never raw message). Required by `command-contracts.md:128-132` (`--identity KEY_ID`). **The contract is internally inconsistent here:** line 142 says publickey uses "**one** KMS-owned Ed25519 identity". | **X** (recommended earlier, user decision O-1): allow up to the key-count cap with public-key selection; needs the maintainer's reading that selection within one purpose-specific class is not "caller-selected key". **Y** (Law-literal): exactly one SSH identity per KMS store (cap = 1); the request's public key is an expected-key confirmation that must equal the stored key, so the caller selects nothing; `--identity KEY_ID` then names that one identity. Y needs no Law-1 reading but gives up multiple identities and slightly changes the reading of line 128-132. |
| **LR-2** | "Authorize the live caller generation and **registered service TID** before any provider access" (`code-standards.md:62-64`) | `cellbox-ssh` is an unregistered client, not a registered service; liveness and identity come from the kernel-attested caller trailer (`cell_id`, `generation`, `tid`, flag bit) checked by `validated_caller` (`auth.rs:132`) before any store/provider access. Replay ids stay nonzero and monotonic per principal and advance only after a successful signature. | **P** (proposed): accept kernel attestation as the liveness proof for this purpose-specific right (precedent: Q-right path in Phase 01); **Q**: require a service-registry registration handshake for `cellbox-ssh`, which does not exist and would need a separate Law-1 delta. |

### 9.2 Decisions recorded by the user (2026-10-30)
D-K5 = **per-login unlock bounded to one signature** (O-13), O-1 as per LR-1 option X **unless the maintainer chooses Y here** (this is the one place the earlier decision is re-opened, because the Law-1 text was not shown to the user when O-1 was approved), O-10 = **B** (K2 CP2 lands `SshKeyUnavailable` arms so `dispatch.rs`'s exhaustive match keeps building; they are not implementation), O-14 = **admin-chosen KEY_ID name (1..64 bytes)**.

### 9.3 Values this text fixes (confirm with the text)
O-3 volatile T0 leaf reports `provider = None` + `Ready` with a posture field (K2 reserves `DevPosturePersistent`); O-4 replay-floor table size 4, evict oldest (stated risk: eviction resets a principal's floor); O-5 unlock failure limiter N = 5, reset by KMS restart or key delete; O-7 another admin principal's Begin replaces a pending import, the same principal must Abort first; O-8 the sign reply keeps the 32-byte `message_sha256`; O-15 wrapped keys are not decryptability-checked at import (no passphrase at that moment), the first login verifies.

### 9.4 Deferred, with owners (not decided here)
O-2 Admin class realisation (K7 CP1); O-6 stored bytes per key and key-count cap (K5 CP1; under LR-1 option Y the cap is 1); O-9 client-side receive bound for a dropped frame (Cellbox owner, K6); O-11 legacy policy decode of `ssh_sign` for v1-v7 entries, proposed `false` (K3 CP1); O-12 the QEMU proof of the flag needs an image delta because the promoted image has no `/bin/kms` (K3/K8).

## 10. What approval of this text authorizes
Only: Law-1 confirmation 1 for the design above in `libs/types/src/kms/**` and the O-10 = B dispatch follow-up. It authorizes no code, no key, no network fetch, no lock release, no image change, no K3 kernel work, no `Cargo.lock` edit. Before any K2 code: (1) the approval record with digests (§5), (2) the lock handoff for `libs/types/src/kms/**` and `cells/services/kms/**`, (3) after implementation, confirmation 2 on the exact ABI delta with evidence.
