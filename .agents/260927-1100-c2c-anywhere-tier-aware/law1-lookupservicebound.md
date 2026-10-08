# Law 1 record — `LookupServiceBound` (C2C local service binding)

**Status**: **2 of 2 confirmations recorded. FROZEN (2026-10-08).**
**Owner**: sole accountable maintainer (ADR-0013 decision #8).
**Design revision**: ADR-0023 §2.3 sha256 `3f1696dfc96b307a…` (whole file
`docs/decisions/0023-local-service-generation-binding.md`).
**Implementation revision**: the digests in §2.2, asserted on every run by
`scripts/check-lookupservicebound-law1-digests.sh`.
**Law-1 scope**: `libs/api/` (`CONTRIBUTING.md:70-71`, `docs/code-standards.md:40-45`).

Law 1 requires **two explicit confirmations from the accountable maintainer at separate
checkpoints** — design approval *before* editing, then implementation approval after reviewing the
exact ABI delta and evidence. One message cannot satisfy both. This file exists so both
confirmations bind to one item list and one set of digests rather than to a memory of them.

Both checkpoints are recorded (§3), so the surface in §1 is **FROZEN**: removal, rename,
layout/discriminant change, or addition now requires the ABI process again, including two fresh
explicit confirmations. A later commit that changes a confirmed item without that process leaves
this record stale by construction, and the check in §5 fails.

## 1. What is being confirmed

| # | Item | Location after implementation |
|---|---|---|
| 1 | `ViSyscall::LookupServiceBound = 429` — the next free opcode after `SerialConfigure = 428`; `400` stays unmapped and `500-503` stay reserved | `libs/api/src/abi/syscall.rs` |
| 2 | Argument encoding: `a0 = service_id: u16`, `a1 = out_ptr: usize`, `a2 = out_len: usize` | same |
| 3 | Success return `SERVICE_BINDING_LEN` (24) = bytes written, matching the bytes-written convention `QueryDirHandles = 241` uses | same |
| 4 | `0` = no live provider, shared with `LookupService`'s absence sentinel; a `Paused` provider also answers `0`, preserving the hot-swap quiesce barrier | same |
| 5 | Short buffer → `SyscallError::BufferTooSmall` (see §2.1 deviation); never a partial write | same |
| 6 | Decode arm `429 => Self::LookupServiceBound` in `impl From<usize> for ViSyscall` | same |
| 7 | Allowlist arm `Self::LookupServiceBound => Some(37)` — **shares bit 37 with `LookupService`**, the open-syscall bit; the `u64` allowlist is full (bits 0–62 syscalls, 63 VFS-mutate) so no fresh bit exists | same |
| 8 | `pub const SERVICE_BINDING_LEN: usize = 24;` | new `libs/api/src/abi/service_binding.rs` |
| 9 | `#[repr(C)] pub struct ServiceBinding { pub tid: u64, pub cell_id: u64, pub generation: u64 }` | same |
| 10 | Little-endian `to_bytes() -> [u8; SERVICE_BINDING_LEN]` and `from_bytes(&[u8]) -> Option<Self>` (rejecting a short slice) | same |
| 11 | `is_live()` requires `tid != 0 && cell_id != 0 && generation != 0` | same |
| 12 | **No reserved field.** A future need is a new opcode, which is this repository's append-only precedent (`GetProcs2 = 239` added while `GetProcs = 30` kept serving) | same |
| 13 | The record is written under one `SCHEDULER → registry` section, so `tid`/`cell_id`/`generation` are one instant | `kernel/src/task/syscall.rs` |
| 14 | `CASES` entry pinning `429` in the syscall table test | `libs/api/src/abi/syscall_tests.rs:21` |

Semantics that are part of the confirmation, not just signatures:

- **`LookupService = 206` is untouched.** Its bare-tid return with `0 = absent` is documented
  stable ABI (`libs/api/src/abi/syscall.rs:1302-1305`). Reinterpreting it is a frozen change; the
  additive sibling is the escape hatch this repository already used for `GetProcs2`.
- **No caller-supplied identity is accepted.** The kernel resolves and states the binding; there is
  no `tier`, `epoch`, or `tid` argument, and no variant that trusts a peer's self-report.
- **`tid` is a transport detail, not identity.** It is never re-issued within one boot, so it is
  safe to carry; `(cell_id, generation)` is the identity because `CellId` slots *are* reused.
- **The binding is boot-local.** It is not, and must not be presented as, a remote replay epoch or
  a cross-reboot incarnation (`docs/specs/20-unified-ipc-contract.md:35`).
- **`SERVICE_BINDING_LEN` = 24 = 3 × `u64`.** The three fields fully specify the record, so there is
  no reserved byte to preserve and no canonical-zero rule to satisfy.

## 2. Compatibility consequences reviewed at this checkpoint

| Consumer | Consequence |
|---|---|
| Existing cells and every `ViSyscall` discriminant | None. A new variant is appended; `From<usize>` keeps failing closed for unmapped numbers, and `Unknown = 9999` is unchanged. |
| Cells that declared `__ViCell_syscalls` before this opcode existed | None. Bit 37 is already granted by declaring `LookupService`, so no cell is silently denied a syscall it never had — the failure mode that the always-permitted list documents for other additions (`libs/api/src/abi/syscall.rs:1104-1113`). |
| `LookupService = 206` callers | None. Unchanged code path and return values. |
| Allowlist budget | No widening. Bits 0–62 are full and bit 63 is the VFS-mutate declaration; this addition shares an existing bit rather than reclaiming 63. |
| `ViSyscall::Unknown` sentinel / retired opcode rules | Not affected: `400` remains unmapped and no retired number is reused. |
| Kernel memory | +16 bytes per registry entry, bounded by `MAX_SERVICES = 32` (`kernel/src/cell/service_registry.rs:19-23`). |
| Wire (Spec 17) | Untouched. This is a syscall, not a frame; no byte-0 discriminant, no envelope layout, no framing change. |
| Re-signing of existing cells | **Not required.** No existing cell's ELF section, manifest, or bytes change. |

## 2.1 Deviations from the confirmed design — ACCEPTED at checkpoint 2

Both are implementation-level refinements discovered while writing the code, and both were
presented for checkpoint 2 and accepted with it:

1. **Short buffer returns `SyscallError::BufferTooSmall`, not `InvalidInput`** (item 5). The two
   existing fixed-record writers, `QueryDirHandles = 241` and `ResolveCellOwner = 244`, both use
   `BufferTooSmall` for exactly this case; matching them is more precise and keeps one convention
   for "your buffer is too small for this kernel-written record". `InvalidInput` in the confirmed
   design was drafted before that convention was checked.
2. **A provider whose recorded identity is not live answers `0`** (item 4). `cell_id == 0` is the
   kernel's own allocation rather than a Cell, and a zero generation is not an epoch. The kernel
   therefore reports "no live provider" instead of writing a record it cannot stand behind.
   `LookupService`'s tid answer is unaffected: the legacy path keeps working for such an entry,
   which is why a registration whose identity could not be read records `0/0` rather than being
   refused (`kernel/src/task/syscall.rs`, self-registration arms).

## 2.2 Implemented ABI delta (the frozen surface)

| Item | Implemented at |
|---|---|
| `LookupServiceBound = 429`, docs, `From<usize>` arm, allowlist bit 37 | `libs/api/src/abi/syscall.rs` |
| `SERVICE_BINDING_LEN`, `ServiceBinding`, `to_bytes`/`from_bytes`/`is_live` + 3 unit tests | `libs/api/src/abi/service_binding.rs` |
| Module registration | `libs/api/src/abi.rs` |
| `CASES` entry, discriminant pin `429`, allowlist-sharing test | `libs/api/src/abi/syscall_tests.rs` |
| Client wrapper `sys_lookup_service_bound` | `libs/ostd/src/syscall.rs` |
| Kernel enum variant, `From<ViSyscall>` arm, `syscall_to_vi` arm, dispatch | `kernel/src/task/syscall.rs` |
| Registry records and returns `(tid, cell_id, generation)`; `lookup_bound` | `kernel/src/cell/service_registry.rs` |
| Provider identity captured at every `register` call site | `kernel/src/task/syscall.rs`, `kernel/src/cell/hotswap.rs`, `kernel/src/loader/atomic_publication_tests/baseline.rs` |
| `checked_add` fail-closed id advance + boot no-re-issue guard (ADR-0023 §2.5) | `kernel/src/task/scheduler.rs`, `kernel/src/task/task_id_selftest.rs`, `kernel/src/main.rs` |

Digests of the revision checkpoint 2 confirmed (provenance for *which* revision; the item check
in §5 is the gate):

| File | sha256 (first 16) |
|---|---|
| `libs/api/src/abi/syscall.rs` | `7cb9c3ff34f9b354…` |
| `libs/api/src/abi/service_binding.rs` | `fe6bd6298c2ccc1f…` |
| `libs/api/src/abi/syscall_tests.rs` | `e4ab0e072d4b5a48…` |
| `libs/api/src/abi.rs` | `6ba9984caf39263b…` |
| `libs/ostd/src/syscall.rs` | `a559e70788475e62…` |
| `kernel/src/task/syscall.rs` | `669726835412ca65…` |
| `kernel/src/cell/service_registry.rs` | `27ff723bf9d0d049…` |

Evidence proofread at checkpoint 2: `docs/evidence/local-service-lifecycle-x86-qemu.{txt,log}`
(end-to-end opcode reachability on Intel x86_64 QEMU, plus the synchronous-vs-bounded contrast) and
`docs/evidence/task-id-reuse-guard-aarch64-qemu.{txt,log}` (the §2.5 guard).


## 3. Confirmation log

| # | Date (UTC) | Statement | Binds to |
|---|---|---|---|
| 1 | 2026-10-08 | Owner instruction: "xác nhận law-1", confirming the design item list in §1 as presented for `LookupServiceBound = 429` | ADR-0023 whole file sha256 `3f1696dfc96b307a…`; `libs/api/src/abi/syscall.rs` sha256 `799c3151e2b22d6f…`; `libs/api/src/abi/syscall_tests.rs` sha256 `787e75b09ca07589…`; layout precedent `libs/api/src/abi/cell_owner.rs` sha256 `d6d14a5c11585826…` |
| 2 | 2026-10-08 | Second explicit owner confirmation of the **same** item list, the implemented delta in §2.2 (including the two accepted deviations in §2.1) and the evidence above — selected through an explicit choice, not inferred from checkpoint 1 | `libs/api/src/abi/syscall.rs` sha256 `7cb9c3ff34f9b354…`; `libs/api/src/abi/service_binding.rs` sha256 `fe6bd6298c2ccc1f…`; `libs/api/src/abi/syscall_tests.rs` sha256 `e4ab0e072d4b5a48…`; `libs/api/src/abi.rs` sha256 `6ba9984caf39263b…`; `libs/ostd/src/syscall.rs` sha256 `a559e70788475e62…`; `kernel/src/task/syscall.rs` sha256 `669726835412ca65…`; `kernel/src/cell/service_registry.rs` sha256 `27ff723bf9d0d049…` |

Both confirmations are recorded, so the surface in §1 and §2.2 is **FROZEN** under Spec 23 §2.1:
removal, rename, layout/discriminant change, or addition requires the ABI process (including two
fresh explicit confirmations) before it lands.

## 4. Preconditions that Law-1 confirmation does not satisfy

Law-1 approval is necessary but not sufficient. This record speaks for neither of the two
independent gates:

1. **Kernel-repair file-owner handoff — GRANTED 2026-10-08 for the full slice** (owner selected the
   option that also covers the two `next_task_id` guard sites in `kernel/src/task/scheduler.rs`).
   Exactly what that covers: `libs/api`, `kernel/src/cell/service_registry.rs`,
   `kernel/src/task/syscall.rs` (dispatch), `kernel/src/cell/hotswap.rs` (cutover identity), and
   `kernel/src/task/scheduler.rs` (fail-closed counter). No other kernel path is in scope.
2. **No remote enablement, no Spec-20 ratification, and no `ServerEpoch` soundness claim** follow
   from this confirmation. ADR-0023 §5 still stands. In particular, freezing this ABI says nothing
   about the remote half of the incarnation axis, which still needs a protected non-rollback
   source (ADR-0023 §2.6).

## 5. Keeping the record honest

The whole-file digests above are provenance for *which revision* was confirmed, not the gate: an
unrelated edit elsewhere in one of those files must not require a fresh Law-1 confirmation, while a
change to a confirmed item must.

`scripts/check-lookupservicebound-law1-digests.sh` asserts every item that is expressible as code
text — the opcode number and its decode arm, the shared bit 37, the record length and all three
fields, `is_live`'s three-nonzero rule, `from_bytes`'s rejection of a non-live record, the absence
of a reserved field (the deliberate item-12 decision), the module registration, the client wrapper,
and the kernel variant/allowlist/decode/dispatch arms — and fails with a per-item message naming
the file. Verified both ways: it passes on the frozen tree and fails on a one-value mutation
(`SERVICE_BINDING_LEN: usize = 24` → `25`, restored afterwards).

After a *confirmed* ABI change, update §1, §2.2, the digests above and this check in the same
commit as the change.
