# API Contract — Phase 02B Secure Node Identity / KMS — 2026-08-19

## Verdict

Endpoint count: 5 IPC operations. Biggest compatibility risk: the current Noise path in `cells/services/net-broker/src/transport.rs` constructs `clatter::KeyPair` with the broker-held X25519 private key, so an opaque KMS-owned key requires a transport adapter or handshake-layer change rather than a storage-only swap.

Flagged issue 1 — transport cannot stay byte-for-byte as-is:
- `NoiseSession::new` injects `my_static.inner.secret.clone()` into `clatter`, so the broker currently owns private bytes in-process.
- A KMS that never releases private bytes cannot satisfy this API without changing the transport seam.
- Keeping the existing `StaticKeypair` surface would silently violate the new security goal.
- Migration path: preserve `CellNetId([u8; 32])`, but replace `StaticKeypair` consumption with a KMS-backed `StaticDh` adapter and keep ephemeral/session state in broker RAM only.
- Evidence: `cells/services/net-broker/src/transport.rs:143-179`.

Flagged issue 2 — current Silo is not the Phase 02B API:
- The shipped Silo contract is a 128-byte P-256 mailbox veneer with `Init`, `Sign`, `Ecdh`, and `GetPub`.
- `cells/services/silo/src/main.rs` still depends on a placeholder `silo-guest.bin` and exits if it is empty.
- That makes the current Silo a backend candidate, not the semantic KMS surface for stable X25519 node identity.
- The Phase 02B contract must sit above Silo and hide whether the root provider is `SiloWrappedBlob`, DICE software sealing, or future hardware.
- Evidence: `libs/types/src/silo.rs:23-108`, `cells/services/silo/src/main.rs:12-23`, `cells/services/silo/src/main.rs:74-78`.

Flagged issue 3 — VFS-backed machine-id is not production-safe for this feature:
- Older plans proposed `/etc/cellos/machine-id`, but the current feature requirement is production-first, clone-resistant identity.
- A raw first-boot secret in normal VFS storage cannot meet the clone/fail-closed requirement once images are copied or storage is exposed.
- KMS may persist only a sealed blob whose unwrap policy is rooted in DICE/Silo/hardware state; the leaf private key itself must never live as plaintext in VFS.
- Migration path: retire `machine-id` as the identity root for Cell-to-Cell Anywhere; keep it only if later needed as non-authoritative metadata.
- Evidence: `.agents/260712-1902-dice-attestation-identity/phase-04-k2-per-node-identity.md:17-29`, `cells/services/net-broker/src/identity.rs:24-25`.

Flagged issue 4 — do not copy supervisor's current name-based caller checks:
- The supervisor still authorizes IPC senders by process-name lookup, which is acceptable only for its current narrow shell tool flow.
- Phase 02B must not use task names, labels, or path hints as identity proof for KMS.
- The safe existing primitive is `sys_recv_attested`, which yields kernel-written `CallerIdentity { cell_id, generation, sender_tid }`.
- KMS authorization must bind to live service ownership and attested generation, not strings.
- Evidence: `cells/services/supervisor/src/main.rs:50-87`, `libs/api/src/abi/caller_identity.rs:10-38`, `libs/ostd/src/syscall.rs:952-976`.

## Context

Cell-to-Cell Anywhere needs one stable, non-forgeable node identity across LAN direct and relay paths. The current broker generates a per-run X25519 keypair at startup, which breaks stable addressing and makes remote trust depend on process lifetime rather than machine identity. This contract replaces that with a production-first KMS service that owns the node private key, seals it under a hardware-rooted provider, and authorizes only the live supervised `net-broker` instance to use it.

This checkout does not contain `docs/engineering-standards.md`; the contract therefore follows the injected report contract and `docs/code-standards.md` Law 1 rules directly. Law 1 applies because the design adds new `libs/api` and `libs/types` surface.

Consumers:
- `cells/services/net-broker` — acquires and uses the stable node identity for Noise/static-DH.
- `init` / `supervisor` lifecycle — ensure the live broker instance re-binds cleanly after restart or hotswap.
- Future attestation/enrollment work — may add DICE binding above this contract without changing the node-key handle shape.

Non-goals for Phase 02B:
- No generic Wrap/Unwrap API for arbitrary clients.
- No remote attestation token yet.
- No insecure fallback that enables cross-machine mode without a secure root provider.
- No change to `CellNetId` wire size.

## Versioning Strategy

No URL-path or HTTP versioning applies; this is a fixed-size IPC ABI.

Versioning rules:
- All requests and responses carry `abi_version = 1`.
- Opcodes are append-only.
- Public structs are fixed-layout `#[repr(C)]`.
- Unknown opcode or higher `abi_version` fails closed with `UNSUPPORTED_VERSION` or `UNKNOWN_OPCODE`.
- `CellNetId([u8; 32])` stays unchanged, so peer-ticket and route records remain wire-compatible.

Rationale:
- Matches existing Cellos service IPC conventions: small fixed layouts, no stringly version negotiation.
- Avoids widening existing distributed structs for Phase 02B.

## Resource Model

### `NodeIdentitySlot`

Singleton resource owned by KMS.

Fields:
- `handle: NodeIdentityHandle` — opaque, KMS-local, invalidated on KMS restart or rotation.
- `public_key: CellNetId` — stable 32-byte X25519 public key exported to the broker.
- `provider_kind: KmsProviderKind` — `TestHooksDeterministic`, `SiloWrappedBlob`, `DiceSealed`, `HardwareSealed`.
- `state: NodeIdentityState` — `Uninitialized`, `Ready`, `RemoteDisabled`, `CloneDetected`, `ProviderUnavailable`.
- `blob_revision: u64` — monotonic revision of the sealed blob metadata.

### `BrokerBinding`

Authorizes exactly one live broker cell generation to use `NodeIdentitySlot`.

Fields:
- `bound_cell_id: u64`
- `bound_generation: u64`
- `bound_service_tid: u64`
- `binding_epoch: u64` — increments whenever KMS accepts a new broker instance.

Rules:
- Only the task currently registered as `service::NET_BROKER` may create or refresh the binding.
- Subsequent key-use operations accept any sender from the same `cell_id + generation`.
- When `service::NET_BROKER` re-registers to a replacement tid, the old binding becomes stale and only `RegisterBrokerInstance` is accepted until the new broker binds.

### `Root Provider`

Internal backend only; not a public resource.

Responsibilities:
- Generate or unwrap the stable random X25519 leaf key.
- Seal the leaf as an authenticated encrypted blob.
- Refuse unwrap on clone/policy mismatch.

Backends are implementation detail:
- Current `Silo` is only a candidate root provider.
- DICE or hardware-backed sealing can replace or augment it without changing client-visible KMS IPC.

## Wire Types

All request/response frames are fixed-size 128-byte messages to match current small-cell service conventions.

### Request envelope

```c
#[repr(C)]
struct KmsRequestV1 {
  u8  abi_version;     // required = 1
  u8  opcode;          // KmsOpcode
  u16 flags;           // must be 0 in v1
  u32 request_id;      // caller-generated correlation id
  u16 payload_len;     // bytes used inside payload[]
  u16 reserved0;       // must be 0
  u32 reserved1;       // must be 0
  u8  payload[116];
}
```

### Response envelope

```c
#[repr(C)]
struct KmsResponseV1 {
  u8  abi_version;     // = 1
  u8  opcode;          // echoed request opcode
  u8  status;          // 0 = ok, 1 = error
  u8  reserved0;       // 0
  u32 request_id;      // echoed request_id
  u16 code;            // KmsErrorCode when status=1, 0 when ok
  u16 details_len;     // bytes used inside payload[]
  u32 reserved1;       // 0
  u8  payload[116];
}
```

### Shared value types

| Name | Type | Description |
|------|------|-------------|
| `NodeIdentityHandle` | `u32` | Opaque KMS-local handle; `0` is invalid |
| `KmsProviderKind` | `u8` | `1=TestHooksDeterministic`, `2=SiloWrappedBlob`, `3=DiceSealed`, `4=HardwareSealed` |
| `NodeIdentityState` | `u8` | `0=Uninitialized`, `1=Ready`, `2=RemoteDisabled`, `3=CloneDetected`, `4=ProviderUnavailable` |
| `BindingEpoch` | `u64` | Monotonic KMS-side broker-binding generation |

## Endpoints

### `OP 0x01 RegisterBrokerInstance`
**Purpose**: Bind KMS authorization to the currently registered `service::NET_BROKER` task and its attested cell generation.  
**Auth**: sender must be the live provider returned by `LookupService(service::NET_BROKER)` and must arrive via `sys_recv_attested`.

**Request payload**: none

**Response 200 payload**:

```json
{
  "binding_epoch": "u64",
  "bound_cell_id": "u64",
  "bound_generation": "u64",
  "bound_service_tid": "u64"
}
```

**Authorization contract**:
- KMS compares `CallerIdentity.sender_tid` with `sys_lookup_service(service::NET_BROKER)`.
- KMS rejects `generation == 0`.
- On success, KMS stores `cell_id + generation + sender_tid` as the active broker binding and revokes any prior broker handles.

**Error responses**:

| Status | Code | When |
|--------|------|------|
| error | `PERMISSION_DENIED` | Sender is not the current `service::NET_BROKER` provider |
| error | `CALLER_UNATTESTED` | No valid caller trailer or `generation == 0` |
| error | `BINDING_STALE` | Service registry changed during bind; caller must retry |

### `OP 0x02 GetNodeIdentityStatus`
**Purpose**: Report whether cross-machine identity is available and why it is blocked if not.  
**Auth**: active bound broker cell or live `service::SUPERVISOR` provider.

**Request payload**: none

**Response 200 payload**:

```json
{
  "state": "NodeIdentityState",
  "provider_kind": "KmsProviderKind",
  "binding_epoch": "u64",
  "blob_revision": "u64",
  "public_key": "[32]u8, zeroed unless state=Ready",
  "remote_allowed": "bool-as-u8"
}
```

**Error responses**:

| Status | Code | When |
|--------|------|------|
| error | `PERMISSION_DENIED` | Sender is neither the bound broker cell nor the live supervisor |
| error | `CALLER_UNATTESTED` | No valid caller trailer |

### `OP 0x03 AcquireNodeIdentity`
**Purpose**: Open the existing stable node identity or provision it exactly once if it does not exist and the secure root is available.  
**Auth**: active bound broker cell only.

**Request payload**: none

**Response 200 payload**:

```json
{
  "handle": "NodeIdentityHandle",
  "public_key": "[32]u8",
  "provider_kind": "KmsProviderKind",
  "binding_epoch": "u64",
  "blob_revision": "u64"
}
```

**Provisioning rules**:
- If no sealed blob exists, KMS generates a random X25519 leaf, seals it under the active root provider, persists the blob atomically, and returns the new public key.
- If the blob exists and unwraps cleanly, KMS returns the same public key.
- If unwrap fails because the blob belongs to a different device/root state, KMS returns `CLONE_DETECTED` and does not auto-rotate.
- If no secure root is available, KMS returns `SECURE_ROOT_REQUIRED`; the broker must keep cross-machine mode disabled.

**Error responses**:

| Status | Code | When |
|--------|------|------|
| error | `PERMISSION_DENIED` | Sender is not the bound broker cell |
| error | `BINDING_REQUIRED` | Broker has not called `RegisterBrokerInstance` yet |
| error | `SECURE_ROOT_REQUIRED` | Production-capable root provider unavailable |
| error | `CLONE_DETECTED` | Sealed blob failed device/policy unwrap |
| error | `PERSIST_FAILED` | Atomic blob write/replace failed |
| error | `BUSY` | Another acquire/rotation is in progress |

### `OP 0x04 NoiseStaticDh`
**Purpose**: Compute the static X25519 DH for the active node identity without releasing the private key bytes.  
**Auth**: active bound broker cell only.

**Request payload**:

```json
{
  "handle": "NodeIdentityHandle",
  "peer_public_key": "[32]u8"
}
```

**Response 200 payload**:

```json
{
  "handle": "NodeIdentityHandle",
  "shared_secret": "[32]u8"
}
```

**Contract**:
- KMS validates that `handle` belongs to the currently bound broker binding.
- Returned `shared_secret` is ephemeral transport material; it may exist in broker RAM, but the node private key never does.
- Peer key validation failure is explicit; KMS does not coerce malformed inputs.

**Error responses**:

| Status | Code | When |
|--------|------|------|
| error | `PERMISSION_DENIED` | Sender is not the bound broker cell |
| error | `INVALID_HANDLE` | Handle unknown, stale, or revoked |
| error | `INVALID_PEER_KEY` | Peer public key is not a valid X25519 input |
| error | `PROVIDER_FAILURE` | Root provider failed the DH operation or unwrap |
| error | `BUSY` | Rotation or provider recovery is in progress |

### `OP 0x05 RotateNodeIdentity`
**Purpose**: Replace the sealed node identity after clone detection, lost-key recovery, or operator-forced rekey.  
**Auth**: live `service::SUPERVISOR` provider only.

**Request payload**:

```json
{
  "reason": "u8 enum: 1=CloneRecovery, 2=LostKeyRecovery, 3=OperatorRekey",
  "flags": "u16 reserved, must be 0 in v1"
}
```

**Response 200 payload**:

```json
{
  "new_public_key": "[32]u8",
  "blob_revision": "u64",
  "re_enroll_required": "bool-as-u8"
}
```

**Contract**:
- KMS generates a fresh X25519 leaf, seals it, increments `blob_revision`, revokes all existing handles, and clears the broker binding.
- The broker must call `RegisterBrokerInstance` and `AcquireNodeIdentity` again before cross-machine mode resumes.
- This operation is destructive by design and must fail closed if the secure root is unavailable.

**Error responses**:

| Status | Code | When |
|--------|------|------|
| error | `PERMISSION_DENIED` | Sender is not the live supervisor provider |
| error | `CALLER_UNATTESTED` | No valid caller trailer |
| error | `SECURE_ROOT_REQUIRED` | No production-capable root provider available |
| error | `PERSIST_FAILED` | New sealed blob could not be committed atomically |
| error | `BUSY` | Another acquire/rotation is in progress |

## Error Shape (standard across all operations)

Low-level Cellos IPC should stay numeric and fixed-size, so the wire carries `code + details`; the client helper expands that deterministically into the standard logical shape below without putting variable-length strings into the Law 1 surface.

```json
{
  "code": "SCREAMING_SNAKE_CASE",
  "message": "Stable client-side string mapped from code",
  "details": {
    "provider_kind": "optional u8",
    "binding_epoch": "optional u64",
    "handle": "optional u32",
    "expected_service_tid": "optional u64",
    "observed_cell_id": "optional u64",
    "observed_generation": "optional u64"
  }
}
```

### Error codes

| Code | Meaning |
|------|---------|
| `CALLER_UNATTESTED` | Missing or invalid kernel-written caller trailer |
| `PERMISSION_DENIED` | Caller is not authorized for this operation |
| `BINDING_REQUIRED` | Broker must register itself before key use |
| `BINDING_STALE` | Service registry/provider changed; caller must re-bind |
| `SECURE_ROOT_REQUIRED` | No secure root provider is available, so cross-machine mode must stay off |
| `CLONE_DETECTED` | Sealed blob exists but cannot be unwrapped on this device/root state |
| `INVALID_HANDLE` | Handle is zero, unknown, stale, or revoked |
| `INVALID_PEER_KEY` | Peer public key input failed validation |
| `UNKNOWN_OPCODE` | Opcode not defined in this ABI version |
| `UNSUPPORTED_VERSION` | `abi_version` not supported |
| `PERSIST_FAILED` | Atomic sealed-blob write/replace failed |
| `PROVIDER_FAILURE` | Underlying root provider returned an internal error |
| `BUSY` | A mutually exclusive KMS operation is already in progress |

## Broker Authorization Model

The core authorization rule is: KMS trusts only kernel-attested caller identity plus live service ownership; it never trusts names, labels, or request-embedded claims.

Binding flow:
1. `init` or supervisor spawns and registers `service::KMS`.
2. `init` or supervisor spawns and registers `service::NET_BROKER`.
3. The broker immediately calls `RegisterBrokerInstance`.
4. KMS verifies `sender_tid == LookupService(service::NET_BROKER)` and records the attested `cell_id + generation`.
5. Later `AcquireNodeIdentity` and `NoiseStaticDh` accept any sender from that same `cell_id + generation`.
6. On broker restart/hotswap, service lookup moves to a new tid; the replacement broker must re-run step 3. Old handles and bindings fail closed.

Why this is sufficient:
- `LookupService` is fed by the kernel service registry, not by caller strings.
- Service registration is SpawnCap-gated in the kernel, so an arbitrary cell cannot mint `service::NET_BROKER`.
- `generation` distinguishes a replacement broker from a dead predecessor even if task ids are later recycled.

Why this does not depend on current supervisor sender-name checks:
- KMS does not examine process names.
- KMS does not trust `path_hint`, `service_name`, or request fields for identity.
- All security decisions are made from `sys_recv_attested` and live service registry state.

## Persistence Contract

Persistence is intentionally not a public API, but the client-visible guarantees are part of the contract:

- The node private key is generated as random X25519 leaf material exactly once per device identity lifecycle.
- KMS persists only a sealed blob plus non-secret metadata such as `blob_revision` and provider kind.
- Blob updates must use atomic replace semantics; partial writes leave the previous committed blob intact.
- Copying the blob to another device must yield `CLONE_DETECTED`, not silent success.
- If the root provider is unavailable or downgraded, KMS returns `SECURE_ROOT_REQUIRED`; the broker must not enable LAN direct or relay mode.
- Local single-machine IPC remains available; only cross-machine mode is disabled.

## Backward Compatibility Analysis

### Additive Law 1 touches required

1. `libs/api/src/abi/syscall.rs`
- Add `service::KMS = 13`.
- Pure append-only constant addition; existing numeric assignments remain unchanged.

2. `libs/types/src/kms.rs` (new file)
- Add fixed-layout `KmsRequestV1`, `KmsResponseV1`, opcodes, enums, and helper payload structs.
- Export from `libs/types/src/lib.rs`.

3. Optional doc-only clarification
- Update `libs/api/src/services/cluster.rs` comments to reflect that stable node identity is still `CellNetId([u8; 32])`, but the private key is now KMS-owned rather than broker-generated.
- This is not ABI-visible if only comments/docs change.

### Explicitly avoided in Phase 02B

- No change to `CellNetId` size or layout.
- No new syscall.
- No change to `LookupService` or service registry semantics.
- No change to existing peer-ticket encoding in this phase.

### Migration notes

- Existing per-run broker identities are not preserved; they were never stable enough to be a supported address.
- Existing cross-machine dev setups must re-enroll peers against the first production-grade stable `CellNetId`.
- The broker-side transport implementation changes, but the node-id wire shape does not.

## Law 1 Inventory

This design requires fresh explicit confirmation before any implementation touching `libs/api` or `libs/types`.

Files expected to change:
- `libs/api/src/abi/syscall.rs` — add `service::KMS = 13`
- `libs/types/src/kms.rs` — new ABI types
- `libs/types/src/lib.rs` — export `kms`

Why Law 1 is triggered:
- `docs/code-standards.md` defines `libs/api/` and `libs/types/` as stable ABI surface.
- New service constants and public request/response structs are ABI-visible even when append-only.

## Open Questions

1. `clatter` integration seam:
- Either extend the transport layer to accept a callback/trait for static DH, or replace the current handshake wrapper.
- This is the main implementation risk but does not change the public KMS contract.

2. Root-provider rollout matrix:
- The API is production-first, but current backends are not equally mature on every architecture.
- Implementation must define which targets expose `SiloWrappedBlob`, which expose test-hooks only, and which remain local-only.

3. Blob storage path and authority:
- The public ABI does not expose a path, but implementation still needs a supervisor-provisioned write location and atomic replace rules.
- The location must not widen ordinary VFS authority.

4. Supervisor admin surface:
- `RotateNodeIdentity` is supervisor-only in v1.
- If operator tooling later needs direct access, add a separate authenticated admin flow rather than widening broker privileges.
