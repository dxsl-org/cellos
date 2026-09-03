---
phase: 5
title: "Atomic Rename Backend Gate"
status: completed
priority: P1
effort: "3d"
dependencies: []
tier: thinking
---

# Phase 05: Atomic Rename Backend Gate

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs. Choose the smallest reversible response; escalate any contract-breaking change.

## Overview

Implement no-replace rename on the VFS service's RedoxFS `/srv` mount only. Work begins with three unpublished gates; no gate is claimed passed, and no `Rename=255`, bit 62 mapping, backend method, or IPC variant may appear before Checkpoint 1.

## Requirements

- Gate A proves `Transaction::rename_node_no_replace` on an in-memory/failing `Disk`: regular source, absent destination, one transaction, and remount-visible failure atomicity.
- Gate B uses `__ViCell_syscalls` bit 63 only as the explicit `VfsMutate` declaration marker (never a callable syscall bit). The kernel sets `CALLER_FLAG_VFS_MUTATE` in the existing trailer word at bytes 4–7 only when `allowlist != u64::MAX` and the marker is set; missing trailers, explicit declarations without the marker, and legacy/no-manifest `u64::MAX` never grant mutation.
- Gate C is a VFS-service-local canonical `/srv` ledger. Direct path I/O holds a transient shared lease; both service file-handle tables retain shared leases through entry/in-flight/tombstone lifetime; namespace mutation holds sorted exclusive reservations. Kernel BootFS/VIFS1 is immutable and outside this ledger.
- Ordinary kernel `OpenCap` becomes existing-only `OpenMode::Read` with `CapPerms::FILE_READ`; Read/Seek/Stat require read permission, Write/Truncate require write permission, and no writable/create capability opener is added.
- Unequal rename accepts only same `/srv` RedoxFS mount, regular source, absent destination, and no shared/exclusive lease on either key. Root, directory, cross-mount, and open conflicts are denied. Equal canonical paths require an existing regular file, succeed even when open, and never call the backend.
- Every VFS mutator (`Write`, `Append`, `WriteGrant`, `WriteAt`, `WriteHandleGrant`, `Mkdir`, `Rmdir`, `Unlink`, `UnlinkAt`, `RmdirRecursive`, `SyncHandle`, and `Rename`) checks the attested mutation flag before policy, quota, handles, grants, or backend work.
- No replacement, directory rename, cross-device rename, stable-unlink claim, crash-durability/fsync claim, C/shell wrapper, or production-admission claim.

## Architecture

`CallerIdentity.flags` decodes the reserved u32 trailer word. `declare_syscalls![VfsMutate, ...]` emits marker bit 63, while syscall dispatch continues to allocate rename bit 62 only after Checkpoint 1. VFS converts every accepted absolute path once to a canonical key; mount identity is part of the key and only `/srv` is renameable.

`NamespaceLedger: key -> { transient, service_handle, exclusive }` performs checked atomic acquisition. Two-key exclusive acquisition sorts/deduplicates keys and succeeds only when both entries are clear. RAII releases exactly once; remove empty entries. No ledger borrow/lock spans backend I/O, grant access, reply encoding, or handle-table mutation. Close, failure, owner death, directory revocation, and hot-swap cleanup first extract/drain entries, then release leases.

## Gate Proofs and Checkpoints

1. **Gate A:** `FailingMemDisk` owns shared bytes, records every `write_at(block,len)`, and can fail before ordinal N. Format, create `/source` with sentinel bytes, and snapshot the generation. A successful calibration must record CoW writes followed by exactly one final header-ring write; after dropping and reopening `FileSystem`, `/source` is absent, `/target` is regular with identical bytes, and generation advanced. For every `N=1..=write_count`, restore the same pre-rename image, inject `EIO` before write N, require rename error, drop without inspecting the live object, disable failure, remount, and require the prior generation, `/source` plus exact bytes, and absent `/target`. Destination-present, missing-source, root, and directory validation cases must perform zero writes and remount unchanged. Any counterexample rejects RedoxFS and stops the lane.
2. **Gate B:** round-trip flag encoding; prove kernel derivation for marker/non-marker/`u64::MAX`; prove VFS denies every mutator for absent, malformed, legacy-ALL, and explicit-unmarked identities before side effects. Bit 63 is metadata only and cannot authorize a syscall.
3. **Gate C:** prove canonical alias rejection, shared/exclusive conflicts, atomic sorted two-key acquisition, equal-key deduplication, checked no-underflow release, and extract-then-release cleanup for both handle tables, in-flight/tombstoned handles, transient request errors, owner death, revocation, and hot-swap reset.
4. **Checkpoint 1:** Bound to exact 05G commit `c770e928e358f0ce9270cee0bfc6db1d01cd831a` (tree `41854345e9222839be3eb68a2855b706b2ea58e8`). Commands and results: (a) Gate A: `cargo test --target x86_64-unknown-linux-gnu --manifest-path third_party/redoxfs/Cargo.toml --features std,log --test rename_failure_atomic` -> 1 passed, 0 failed; (b) Gate C: `cargo test --target x86_64-unknown-linux-gnu --manifest-path third_party/redoxfs/Cargo.toml --features std,log --test vfs_namespace_gate` -> 11 passed, 0 failed; (c) Gate B: untouched API tests: `cargo test --target x86_64-unknown-linux-gnu -p api` -> 95 passed, 0 failed. Owner decision recorded below.
5. **Checkpoint 1 contract:** publish raw `Rename=255` with allowlist bit 62; append (never insert) `VfsRequest::Rename { old, new }`; add `FsBackend::rename_no_replace`; restrict it to the semantics in Requirements; preserve `VfsMutate` as distinct bit-63 declaration metadata and trailer flag; add no wrapper or production claim.
6. **Checkpoint 2:** Bound to exact clean 05A commit `ad07ede4e68161c5938ddbdab7cef58a457d0924` (tree `ace1af32651dcb6daba6997b7f8da82f57b32c7e`). Complete caller migration, gate and host suite passes (Gate A sweep 1/1, Gate C 11/11, API 98/98, Net-Broker 114/114, KMS 59/59, cap-file self-test, and QEMU `redoxfs-srv` 3/3 with `test_s6_rename` 8 matrix scenarios), and evidence file `docs/evidence/atomic-rename-verification.txt` (SHA-256 `6099828df7eaa2e06a5df1d726d392a9f226c162e5c90247273cd8330791a75e`, 3,141 bytes) were confirmed by owner (`DECISION: YES`).

## Related Files

| Stage | Files |
|---|---|
| Gate A | Create `third_party/redoxfs/tests/rename_failure_atomic.rs`; read-only dependency basis `third_party/redoxfs/src/{transaction.rs,disk/mod.rs,filesystem.rs}` |
| Gate B | Modify `libs/api/src/abi/{caller_identity.rs,syscall.rs,syscall_tests.rs}`, `kernel/src/task/{syscall.rs,ipc_pending_selftest.rs}`, `cells/services/vfs/src/{caller.rs,dispatch.rs}` |
| Gate C | Create `cells/services/vfs/src/namespace.rs`, `cells/services/vfs/src/namespace/tests.rs`; modify `cells/services/vfs/src/{main.rs,manager.rs,paths.rs,handle_table.rs,pending.rs,file_handles/table.rs,dispatch_paths.rs,dispatch_dirs.rs,dispatch_file_handles.rs,manager/owned_state.rs,manager/state_transfer.rs}` |
| Public rename | Modify `libs/api/src/{abi/syscall.rs,services/ipc.rs,services/dir_name_tests.rs}`, `libs/ostd/src/syscall.rs`, `kernel/src/task/syscall.rs`, `cells/services/vfs/src/{backend.rs,backend_redoxfs.rs,manager.rs,dispatch.rs,dispatch_paths.rs}` |
| Cap safety | Modify `kernel/src/{cell/cap_registry.rs,task/syscall.rs,task.rs}`; create `kernel/src/task/cap_file_selftest.rs` |
| Caller declarations | Modify `cells/apps/hypha/tools/fs/src/main.rs`, `cells/runtimes/lua/src/main.rs`, `cells/services/{hypervisor,kms}/src/main.rs`, `cells/tests/{srv-test,vfs-test}/src/main.rs`, `cells/tools/net-tools/src/bin/wget.rs`, `cells/tools/shell/src/main.rs` |
| Runtime tests | Modify `cells/tests/srv-test/src/main.rs`, `cells/tests/vfs-test/src/{main.rs,dircap.rs}` and the phase-local QEMU runner that already launches `srv-test`; create no C/shell wrapper |

## Implementation Steps and Commits

1. Commit **05G** only after adding and running the three gate harnesses. It may add trailer derivation and the unconnected ledger, but must not add opcode 255, bit 62, `VfsRequest::Rename`, `FsBackend` rename, or dispatch wiring. Record exact commit/tree and commands for Checkpoint 1.
2. Add `VfsMutate` to all eight declaration files above. For every other inventory result that can emit a mutating request, either add the marker to its explicit non-ALL declaration or make that call unreachable/denied; an undeclared/no-manifest binary is never grandfathered. Add an exhaustive request match so future mutators default to denial until classified.
3. Wire leases before backend lookup: transient leases cover direct/path/grant I/O until completion; `HandleEntry` and `FileEntry` own persistent leases; every error, close, purge, revoke, tombstone, and state-transfer path extracts then drops exactly once. Namespace mutators reserve before stat/quota/backend work.
4. After Checkpoint 1, append the IPC variant and publish 255/62. Kernel forwarding preserves the original caller as the attested sender; VFS rechecks the trailer mutation flag. Add `rename_no_replace` only to RedoxFS and reject every other mount before backend dispatch.
5. Implement equal-path short-circuit and unequal validation/reservation. The unequal path performs exactly one `fs.tx(|tx| tx.rename_node_no_replace(...))`; on success move quota writer metadata atomically with the namespace result, and on error leave quota and ledger clean.
6. Convert `OpenCap` and enforce `CapPerms`; migrate tests/readers without adding writable capability creation. Add negative matrices for every mutator × absent/malformed/ALL/unmarked caller and read-only cap write/truncate.
7. Commit source/tests as **05A**. From a clean checkout of exact 05A, run the Gate A sweep, ABI/trailer tests, ledger/cleanup/race tests, cap tests, caller inventory check, service tests, and `/srv` QEMU oracle: unequal success, equal-open success/no backend call, missing-equal failure, and cross-mount/directory/root/open-conflict denials.
8. After Checkpoint 2, commit **05B** with the normal evidence/current changelog bound to exact 05A commit/tree, literal commands/results, and evidence SHA-256/sizes. Do not alter historical records.

## Success Criteria

- [X] Gates A–C pass before Checkpoint 1 and no public rename surface exists in 05G.
- [X] Legacy `u64::MAX`, missing/unmarked declarations, malformed trailers, and read-only caps cannot mutate; every mutator caller is explicitly migrated or denied.
- [X] All direct/transient and service-handle lifetimes conflict correctly with rename/remove and clean up exactly once on every exit.
- [X] `/srv` regular-file rename obeys the exact equal/unequal contract and remount evidence; all excluded cases fail without backend rename.
- [X] Both owner checkpoints and clean exact-05A evidence are bound before 05B; no wrapper or production claim exists.

## Rollback and Risk Notes

Any gate/checkpoint/test failure stops this lane: revert 05A/public changes, keep `/srv` rename unreachable, remove opcode/bit/IPC/backend publication, and retain immutable kernel BootFS plus read-only `OpenCap`. Do not emulate rename with copy/unlink. Primary risks are a false failure model, alias-split ledger keys, leaked leases during owner/hot-swap cleanup, or an omitted mutator declaration.

## Security Considerations

The service trusts only the kernel-written identity and mutation flag, never request payload identity or broad path policy alone. Authorization precedes existence, quota, handle, grant, ledger, and backend observations; canonicalization prevents alias-split authority, and all missing/legacy identities fail closed.

## Assumptions

- **Claim:** pre-write `EIO` at every observed RedoxFS write ordinal models the required backend failure boundary. **Confidence:** medium. **How to verify:** Gate A remount sweep plus recorded header-last ordering; reject on any old/new ambiguity outside the stated contract.
- **Claim:** the eight listed declaration sites exhaust current VFS mutation emitters. **Confidence:** medium. **How to verify:** repeat repository-wide `VfsRequest` and `VfsClient` mutator inventory immediately before 05G and fail closed on every new result.

## Deviation Log

- Decision: Rejected the prior writable kernel-VIFS/VIFS1 design; kernel BootFS remains immutable and writable rename belongs only to VFS `/srv` RedoxFS.
  Why: owner selected the service-VFS architecture and RedoxFS already provides transactional no-replace rename.
  Impact: removes kernel namespace-ledger/mount activation work; adds service-local authority and lifetime accounting.
  Revert: restore this plan revision only; no runtime surface has been published.
