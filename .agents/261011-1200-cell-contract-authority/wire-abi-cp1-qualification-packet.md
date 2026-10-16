# Wire ABI Checkpoint 1 (CP1) Qualification Packet

## Decision requested and current status

**APPROVED LAW-1 CHECKPOINT 1 (CP1) DOSSIER.** Recorded 2026-10-16 under explicit user approval instruction: **“Phê duyệt CP1 Wire ABI theo hồ sơ trên và hai digest”**.

This packet establishes the formal Law-1 Checkpoint 1 (CP1) specification and schema approval for the **CellOS Wire ABI**, covering all inter-Cell, transport, and kernel boundary wire encodings across deltas **D01, D04, D05, D07, and D12**.

Following the completion and verification of development tranches N02-A through N07, this dossier transitions the verified internal invariants into the official public and network wire contract baseline.

All 38 scoped execution locks remain formally in effect; Checkpoint 2 (CP2) and production release remain pending.
---

## 1. Scope and Pinned Delta Specifications

The Wire ABI candidate spans five discrete protocol boundaries:

| Delta | Subsystem | Format / Carrier | Wire Size | Governing Spec |
|---|---|---|---|---|
| **D01** | Control Result Header & Handle Carrier | 32-byte header + opaque tickets | 32 B LE | Spec 25 §2, D01 r2 |
| **D04** | Operations & Scoped Multi-Waiter | Submit, Control, Transfer, Reply, Waitset | 48 B, 16 B, 40 B, 32 B, 16+16*N B | Spec 17 §4, Spec 20, D04 r2 |
| **D05** | Service-Owned Directory Attestation & Wire | `ViDirHandleAttestation` + VfsRequest/Response | 112 B LE + Postcard v1 | Spec 09c, Spec 17 §9, D05 r2 |
| **D07** | Lifecycle & Diagnostic Event Records | LifecycleRequest, EventEvidence | 56 B LE, 152 B LE (184 B w/ D01) | Spec 22 §6, Spec 25 §5, D07 r2 |
| **D12** | C2C Cluster & Remote Envelope | `C2cEnvelope` header + framed payload | 112 B LE + payload | Spec 17 §9, ADR-0008, D12 r1 |

---

## 2. Exact Binary Schemas & Layouts

### 2.1 D01: Control Result Header (32 bytes Little-Endian)
Used as the standard return and encapsulation envelope across all control dispatchers:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                          version = 1                          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         status (0..14)                        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                         ticket (u64)                          +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      required_len (u32)                       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      progress_len (u32)                       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        reserved (8 B)                         |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- `version`: Fixed `1u32`.
- `status`: Discrete enum: `Success=0`, `Denied=1`, `Stale=2`, `Timeout=3`, `Capacity=4`, `Exhausted=5`, `Busy=6`, `Small=7`, `Invalid=8`, `WrongType=9`, `Unbound=10`, `AdmissionCut=11`, `Quarantined=12`, `Quiesced=13`, `Pending=14`.
- `ticket`: Opaque monotonic 64-bit ticket. Never encodes a direct table slot index.
- `required_len`: Exact buffer size required when status is `Small` (7). Retains terminal and reservations.
- `progress_len`: Exact payload bytes successfully transferred.

---

### 2.2 D04: Bounded Operation & Scoped Multi-Waiter Wire
Encodes asynchronous operation submission, consumption transfer, and multi-waiter filter registration:

#### A. Bound Submit Request (48 bytes LE)
- `0..4`: `version: u32 = 1`
- `4..8`: `payload_len: u32` ($\le 65536$ bytes)
- `8..16`: `target_tid: u64`
- `16..24`: `target_cell_id: u64`
- `24..32`: `target_generation: u64`
- `32..40`: `payload_addr: u64` (checked user memory address)
- `40..48`: `deadline: u64` (absolute scheduler tick, non-zero, within declared profile)

#### B. Operation Control Base (16 bytes LE)
- `0..4`: `version: u32 = 1`
- `4..8`: `action: u32` (`Take=0, Cancel=1, Reply=2, Abandon=3, Transfer=4, EventTake=5`)
- `8..16`: `op_ticket: u64`

#### C. Transfer Extension (40 bytes LE)
- Base 16 bytes +
- `16..24`: `target_tid: u64`
- `24..32`: `target_cell_id: u64`
- `32..40`: `target_generation: u64`
- *Invariant:* Moving consumption handle preserves immutable original-owner TID and lifetime.

#### D. Reply Extension (32 bytes LE)
- Base 16 bytes +
- `16..24`: `reply_addr: u64`
- `24..32`: `reply_len: u64`
- *Invariant:* Any thread of the accepted provider Cell incarnation is authorized to complete the reply.

#### E. Scoped Wait Header & Filter Entries
- Header (16 bytes): `version: u32 = 1` at 0, `count: u32` at 4, `deadline: u64` at 8.
- Each Filter Entry (16 bytes): `kind: u32` (`Rpc=0, Event=1`) at 0, `source: u32` (`NET_RX=1, TIMER=2`) at 4, `ticket: u64` at 8.
- *Total size:* $16 + 16 \times count$ bytes. Maximum count bounded by admitted filter capacity.

---

### 2.3 D05: Service-Owned Directory Attestation Record (112 bytes LE)
Encapsulates spawner-to-child directory handle inheritance:

- `0..4`: `magic: u32 = 0x56494441` (ASCII "VIDA")
- `4..8`: `version: u32 = 1`
- `8..16`: `child_cell_id: u64`
- `16..24`: `child_generation: u64`
- `24..32`: `spawner_cell_id: u64`
- `32..40`: `spawner_generation: u64`
- `40..44`: `count: u32` (maximum 8)
- `44..48`: `reserved: u32 = 0`
- `48..112`: `handles: [u64; 8]` (array of 8 directory handles; unused slots must be 0)

*Behavior:* All-or-nothing verification in VFS. If any handle was not held by the attested spawner, none are bound.

---

### 2.4 D07: Lifecycle & Diagnostic Event Records

#### A. Lifecycle Request (56 bytes LE)
- `0..4`: `version: u32 = 1`
- `4..8`: `action: u32` (`Watch=0, Cancel=1, StopCell=2, Query=3, Acknowledge=4`)
- `8..12`: `subject_kind: u32` (`Thread=0, Cell=1`)
- `12..16`: `stage_mask: u32` (`Cutoff=1, Gone=2, Both=3, StateOnlyQuery=0`)
- `16..24`: `subject_cell_id: u64`
- `24..32`: `subject_generation: u64`
- `32..40`: `subject_tid: u64`
- `40..48`: `watch_or_query_ticket: u64`
- `48..56`: `lifecycle_cap_ticket: u64`

#### B. Event / Evidence Diagnostic Record (152 bytes LE, 184 bytes total with D01 header)
- `0..4`: `version: u32 = 1`
- `4..8`: `subject_kind: u32` (`Thread=0, Cell=1`)
- `8..12`: `stage: u32` (`Cutoff=1, Gone=2`)
- `12..16`: `reason: u32` (`CleanThread=0, CleanRoot=1, UnhandledUserFault=2, AuthorizedCellStop=3`)
- `16..24`: `cell_id: u64`
- `24..32`: `generation: u64`
- `32..40`: `root_tid: u64`
- `40..48`: `trigger_tid: u64`
- `48..56`: `sequence: u64`
- `56..64`: `exit_code: u64`
- `64..72`: `raw_cause: u64`
- `72..80`: `pc: u64`
- `80..88`: `fault_addr: u64`
- `88..96`: `operation_ticket: u64`
- `96..104`: `validity_mask: u64` (bit 0: cause, bit 1: PC, bit 2: addr, bit 3: op, bit 4: artifact, bit 5: sticky dropped overflow)
- `104..112`: `dropped_records: u64`
- `112..144`: `artifact_sha256: [u8; 32]`
- `144..148`: `architecture: u32` (`X86_64=0, Aarch64=1, Riscv64=2, Riscv32=3`)
- `148..152`: `native_address_bits: u32` (32 or 64)

*Redaction Rule:* Without `ReadEvidence` permission in caller's capability, PC, fault address, cause, operation ticket, and artifact hash are zeroed and their validity bits cleared.

---

### 2.5 D12: C2C Cluster & Remote Wire Envelope (112 bytes LE + payload)
Standard transport envelope for inter-node and broker routing:

- `0`: `version: u8 = 1`
- `1`: `kind: u8` (`Lookup=1, Request=2, Response=3, Busy=4, Indeterminate=5, Cancel=6, Heartbeat=7`)
- `2`: `retry_class: u8` (`NonIdempotent=1, Idempotent=2, Cacheable=3`)
- `3`: `reserved0: u8 = 0`
- `4..6`: `payload_len: u16` (LE)
- `6..8`: `reserved1: [u8; 2] = [0; 2]`
- `8..16`: `request_id: u64` (LE)
- `16..48`: `source_node: [u8; 32]` (ed25519 node public key identity)
- `48..80`: `destination_node: [u8; 32]`
- `80..88`: `source_boot_epoch: u64` (LE)
- `88..96`: `source_server_epoch: u64` (LE)
- `96..104`: `cluster_id: u64` (LE, FNV-1a cluster identity hash)
- `104..106`: `service_id: u16` (LE)
- `106..108`: `export_id: u16` (LE)
- `108..112`: `deadline_ms: u32` (LE)
- `112..`: Payload bytes (length `payload_len`, maximum 65536)

---

## 3. Migration Responsibilities & Non-Regression Ledger

1. **Clean Cutover, No Shims:** Wire decoders reject unknown versions and uncanonical fields with `Invalid` (8). No heuristic fallback to unversioned postcard or legacy raw pointers.
2. **Deterministic Error Mapping:**
   - Undersized take buffer returns `Small` (7) and leaves the message in terminal storage.
   - Stale/unmatched generation returns `Stale` (2).
   - Receiver at capacity returns `Busy` (6) or `Capacity` (4).
3. **Producer & Consumer Alignment:**
   - Kernel IPC (`async_ipc.rs`, `completion.rs`, `completion_wait.rs`) aligns with D01 and D04.
   - VFS Service (`dirs.rs`, `bind.rs`) aligns with D05.
   - Kernel fault capture (`hart_local.rs`, `scheduler.rs`) aligns with D07.
   - Network Broker (`c2c_envelope.rs`, `c2c_ingress.rs`) aligns with D12.

---

## 4. Required Evidence for Subsequent Checkpoint 2 (CP2)

Approval of CP1 admits these byte schemas as the development baseline. Subsequent CP2 promotion and lock release require:
- Complete end-to-end multi-node C2C serialization round-trip witnesses.
- Native VMware / bare-metal execution of all wire structures under fault injection.
- Zero raw pointer references in public SDK bindings.
