---
phase: 4
title: "Truthful fstat"
status: completed
dependencies: []
tier: thinking
---

# Phase 04: Truthful fstat

## Context Links

- [Master plan](plan.md) · [POSIX research](research/posix-sequence.md) · [Record decision](research/review-reconciliation.md#fstat-record-decision)
- `libs/api/src/abi.rs:2-12`
- `libs/api/src/services/posix/sysio.rs:83-97,227-239`
- `libs/api/src/services/fs.rs:23-35,71-103`
- `kernel/src/task.rs:1608-1611`
- `tests/integration/tests/boot.rs:2157-2180`

## Overview

After the first Law 1 owner checkpoint approves the exact proposal, replace
fabricated `_fstat` success with caller-scoped descriptor facts conveyed through
a fixed-width, zero-initialized V1 wire record. The second checkpoint reviews
the implemented interface and bound evidence before acceptance. Phase 03 is not
an entry gate.

## Key Insights

- Target C `stat` uses target-dependent `c_int/c_long`; it cannot be the frozen kernel wire.
- Backend truth is limited to descriptor kind, access direction, and size. Ownership, inode, link count, permissions, device IDs, and timestamps are unavailable.
- Ownership: ABI Owner freezes record/ID/bit; Kernel FS Owner supplies caller-scoped facts; POSIX Shim Owner translates; Integration Owner requires a real guest marker.

## Requirements

- Hard gate: the first explicit confirmation approves `Fstat=254`, bit 61, `(fd,out_ptr,out_len)->bytes_written`, and the exact record below before editing. After exact-04A verification, a separate second confirmation must approve the implemented interface, migration impact, and bound evidence before acceptance. Phase 03 confirmations do not carry over.
- Define 32-byte `#[repr(C, align(8))] ViFstatV1 { kind:u32, access:u32, size:u64, reserved:[u64;2] }`; offsets 0/4/8/16, alignment 8.
- Freeze kinds `1=character`, `2=regular`, `3=directory`; access bits `READ=1`, `WRITE=2`, no other v1 bits. Reserved words are always zero.
- Require `out_len >= 32`; return exactly 32. Invalid pointer/length/FD/backend error writes nothing.
- fd 0 is character/read; 1 and 2 character/write. Current VIFS opens are read-only (`file_open` always uses `OpenMode::Read`), so those files/directories report READ plus true kind/size; directory size is zero. Fstat never changes cursor.
- `_fstat` treats wire return `32` as transport success, builds a zeroed local C `stat`, sets type/size only, copies it, and returns C success `0`; every transport/translation failure returns `-1` with caller bytes unchanged.
- Extend the restrictive POSIX smoke manifest with exactly `Open`, `Fstat`, and `Close`. Every successful `_open` is closed on success and every later failure path before a terminal marker.

## Architecture

`C _fstat -> local ViFstatV1 -> ViSyscall::Fstat(254,bit61) -> validate output range -> file_fstat(caller_id,fd) -> local zeroed wire -> one final copyout -> local zeroed C stat -> caller copy`. The kernel never sees target C layout. Legacy raw 106 FStat mapping/stub is removed after caller search; typed `Seek=106` remains.

**04A** commits record/ABI/kernel/shim/manifest/tests. A clean checkout of exact 04A commit/tree runs all acceptance; only then **04B** appends the normal evidence report/current changelog with tested commit/tree, commands/results, and hashes. Failure blocks only this lane; no rename dependency is created.

### Law 1 Confirmation Record (Checkpoint 1: ABI and Wire Proposal)

- **Status**: APPROVED
- **Recorded**: 2026-09-03T11:13:59Z
- **Approver**: Accountable Maintainer (@lungmat8)
- **Verbatim approved scope**:
  1. `ViSyscall::Fstat=254`, `(fd,out_ptr,out_len)->bytes_written`, allowlist bit 61.
  2. `#[repr(C, align(8))] ViFstatV1 { kind:u32, access:u32, size:u64,
     reserved:[u64;2] }`, size 32, alignment 8, offsets 0/4/8/16.
  3. Kinds character=1, regular=2, directory=3; READ=1, WRITE=2; reserved zero.
  4. `out_len >= 32`; success returns 32. Invalid pointer/length/FD or backend
     error writes nothing.
  5. fd 0 character/read; fd 1/2 character/write; VIFS handles READ with
     truthful kind/size, directory size zero, and unchanged cursor.
  6. `_fstat` returns C 0 only after an exact 32-byte wire success and
     zero-initialized type/size-only translation; failure remains -1 unchanged.
  7. Obsolete raw 106 FStat is removed while typed `Seek=106` remains;
     bit 62 stays reserved for rename and bit 63 remains unused.

### Exact 04A Verification Record (Checkpoint 2 Input)

- **Source commit**: `4856f4de50320b33f283f2db6862dd9fca1b300f`
- **Source tree**: `ad20ae093fe30e0b4f15fc176712cf559c7c2520`
- **Clean checkout**: detached exact source with no source changes;
  `tests/integration/tests/vfs-quota.rs` is 194 lines.
- **API**: host-target `cargo test -p api abi::syscall_tests` passed 21/21
  (74 filtered across the two binaries).
- **Lower layer**: PIC `scripts/build-test-hooks-ci.sh` passed F1/F5 and signed
  8 cells. Exact CI `vfs-quota::riscv64_vfs_quota_all_pass` passed 1/1
  (1 filtered): fstat PASS once, required AP-13 guest SKIP once, exactly one
  classified Cell-254 `cause=0xf`, and no additional/unclassified fault or FAIL.
- **Test-hooks artifact**:
  `target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks`,
  SHA-256 `afaa81616d6e4f1b09e1c3d5704ab1696a0d4142b6c50cfbb9c185a48f828f43`,
  6,250,512 bytes.
- **POSIX route**: PIC `gen_disk.ps1` passed F1/F5, signed 46 cells, and packaged
  the required POSIX smoke. Optional `tetris-lua` failed and was omitted without
  a stale artifact. Exact CI `boot::posix_shim_fstat` passed 1/1 (61 filtered):
  OPEN OK once, FSTAT OK once, and no fstat-specific skip/fault/FAIL. Its one
  unrelated thread-user-entry runtime-gate SKIP is outside this lane.
- **Production artifact**:
  `target/riscv64gc-unknown-none-elf/release/cellos-kernel`,
  SHA-256 `9d88eed8d68c0ef384ff14e8dff8c2f5d97c1d9d7779b476b31f7438afa01bd3`,
  43,636,944 bytes.
- **Single-thread supplement**: a fresh clean checkout reran PIC `gen_disk.ps1`
  and the plan-literal exact POSIX command with `--test-threads=1`; both passed,
  the exact test was 1/1 (61 filtered), both OK markers appeared once, and no
  fstat-specific skip/fault/FAIL appeared. The production kernel was
  43,636,944 bytes with SHA-256
  `fdd239f2465352b56e7e608b0f17a0962cbd7ee81b3aa6937849820e6ec98aa6`.
- **Superseded candidates**: `77ff6a78` failed signing because the split FFI
  file lacked its allowlist entry; `fe44be95` passed the lower-layer route but
  was invalidated before POSIX verification because `vfs-quota.rs` exceeded
  the 200-line project bound. Both were amended and fully replaced by 04A above.
- **Checkpoint state**: APPROVED by the Accountable Maintainer over the complete
  exact-04A boundary, including the required single-thread supplement.
  The superseded earlier approval remains recorded separately below.

### Law 1 Confirmation Record (Checkpoint 2: Complete Evidence Boundary)

- **Status**: APPROVED
- **Timestamp**: 2026-09-03T12:20:54Z
- **Approver**: Accountable Maintainer (@lungmat8)
- **Verbatim approved scope**:
  1. Exact 04A `4856f4de50320b33f283f2db6862dd9fca1b300f`, tree
     `ad20ae093fe30e0b4f15fc176712cf559c7c2520`.
  2. Frozen Fstat 254/bit 61 interface and clean raw-106-to-typed-254 migration
     with typed Seek 106 unchanged.
  3. Primary clean API 21/21, signed lower-layer build, exact VFS 1/1 with
     required AP-13 SKIP and classified Cell-254 `cause=0xf`, signed production
     disk, and exact POSIX 1/1 with both named OK markers once.
  4. Supplemental clean exact POSIX command with `--test-threads=1` passed 1/1
     with 61 filtered and no fstat-specific skip/fault/FAIL; production artifact
     SHA-256 `fdd239f2465352b56e7e608b0f17a0962cbd7ee81b3aa6937849820e6ec98aa6`,
     43,636,944 bytes.
  5. The 2026-09-03T12:14:19Z approval is superseded because it preceded item 4.

- **04B commit**: `5aacccd1d66d7a96b334138081385fc23726e8a1`,
  tree `7ae0776174cde30946782c42afc1798b46ca9c76`; evidence SHA-256
  `14a3f53d725f748afa4976158cfcd0b2c73c3adcf2fee8bebfbce4ed7e4e3171`,
  3,960 bytes.

## Related Code Files

- Modify: `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/syscall_tests.rs`
- Modify: `libs/ostd/src/syscall.rs`
- Modify: `kernel/src/task.rs`, `kernel/src/task/syscall.rs`, `kernel/src/main.rs`
- Create: `kernel/src/task/fstat_selftest.rs` and focused modules under
  `kernel/src/task/fstat_selftest/`
- Modify: `libs/api/src/services/posix/sysio.rs`
- Modify: `cells/tests/posix-shim-test/src/main.rs`; create its focused
  `src/fstat.rs`; extend its syscall declaration and unsafe FFI allowlist.
- Modify: `tests/integration/tests/vfs-quota.rs`, `tests/integration/tests/boot.rs`; package through `gen_disk.ps1`
- Create after clean verification: `docs/evidence/posix-fstat-verification.txt`
- Documentation trigger after full acceptance: current risk/changelog “fstat future”
  wording, bound to exact tested 04A commit/tree

## Implementation Steps

1. Record the first Law 1 ABI/wire proposal confirmation. On absence or
   mismatch, stop this lane before feature edits; other lanes remain executable.
2. Add the record and constants at the frozen syscall boundary. Add compile-time/runtime assertions for size, alignment, offsets, zeroed reserved bytes, kind/access values, ID 254, decode round trip, and bit 61.
3. Add an ostd wrapper taking fd and `&mut ViFstatV1`, always passing 32; do not expose target C `stat` to the syscall.
4. Replace the kernel stub with `file_fstat(caller_id,fd)->ViFstatV1`. Classify 0/1/2 explicitly; otherwise look up that caller's mutable handle, assign READ under the current read-only open contract, call `is_dir`/cursor-preserving `size`, and build from an all-zero value.
5. In dispatch validate `out_len` and writable user range first, gather metadata second, and copy exactly once last. Remove obsolete raw 106 FStat fallback/known-raw handling after all callers use 254.
6. Rewrite `_fstat`: reject null; require the wire wrapper to return exactly 32; map that transport success to C return `0` only after zeroing/translating/copying the local C `stat`. Any other return, invalid kind/access, or size conversion failure returns `-1` unchanged.
7. Add `fstat_selftest` cases for stdio, negative/nonexistent FD, actual file/directory size/kind/access, cursor preservation, short buffer, backend error, caller isolation, reserved zero, and sentinel output unchanged; emit one `fstat self-test PASS`.
8. Extend the POSIX smoke declaration with `Open`, `Fstat`, and `Close`. Exercise `_open`, emit `POSIX-FSTAT-OPEN: OK` only after success, assert `_fstat(...) == 0` plus truthful fields, and route every post-open exit through exactly one `_close`; emit `POSIX-FSTAT: OK` only after assertions and close. Assert invalid FD returns `-1` with output unchanged.
9. Extend `vfs-quota` to assert the exact lower-layer marker. Add focused integration test `posix_shim_fstat` to `boot.rs`: boot a fresh disk, run `posix-shim-test`, require exactly one each of `POSIX-FSTAT-OPEN: OK` and `POSIX-FSTAT: OK`, and reject corresponding FAIL, kernel panic, or cell fault.
10. Commit as 04A. From a clean checkout of its exact commit/tree capture fail-closed `cargo test -p api abi::syscall_tests && bash scripts/build-test-hooks-ci.sh && CI=1 cargo test --manifest-path tests/integration/Cargo.toml --test vfs-quota riscv64_vfs_quota_all_pass -- --exact --nocapture`, then `pwsh ./gen_disk.ps1 && CI=1 cargo test --manifest-path tests/integration/Cargo.toml --test boot posix_shim_fstat -- --exact --nocapture --test-threads=1`. Neither host test may skip its prerequisites. The VFS guest must retain its single-hart AP-13 capability SKIP and expected classified Cell-254 `cause=0xf` guard-page termination, with no additional/unclassified access fault or FAIL. The POSIX route permits no fstat-specific skip/fault/FAIL; unrelated, explicitly classified boot capability skips are outside this lane. No generic boot PASS qualifies.
11. After every exact command passes, present the implemented interface, migration impact, and exact-04A evidence for the second Law 1 confirmation. Only after that confirmation, commit 04B verification report/current wording naming exact 04A revision/tree, statuses, and evidence hashes/sizes. Any failure is fixed and reverified within this lane.

## Todo List

- [x] Capture the first Law 1 fstat ABI/wire proposal confirmation.
- [x] Freeze/test the 32-byte record, ID 254, and bit 61 in 04A.
- [x] Implement caller-scoped metadata, failure-before-copy, and C translation.
- [x] Declare `Open/Fstat/Close`, close every successful open, and verify both markers from exact 04A.
- [x] Capture the second Law 1 confirmation over exact-04A implementation and evidence.
- [x] Commit 04B report/current docs with commands/results and evidence hashes/sizes.
- [x] Halt this lane on any gate or verification failure.

## Success Criteria

- [x] Layout/discriminant/allowlist tests pin every byte and value; bits 55–60 and 62–63 are unchanged.
- [x] stdio, file, and directory results report only true kind/access/size; file cursor is unchanged.
- [x] Invalid FD, null/short buffer, overflow, and backend error return failure with output sentinels unchanged.
- [x] C output is zero except documented type/size fields; no invented metadata appears.
- [x] The exact `vfs-quota` command observes its named marker, required single-hart AP-13 capability SKIP, and expected classified Cell-254 `cause=0xf` guard-page termination with no host prerequisite skip, additional/unclassified access fault, or FAIL. The exact `boot::posix_shim_fstat` command executes its prerequisites and observes both named markers with no fstat-specific skip/fault/FAIL; unrelated classified boot capability skips are outside this lane, and generic boot PASS is insufficient.
- [x] Wire success is exactly 32 bytes while C `_fstat` success is exactly 0; invalid/translation failures return -1, and smoke asserts both mappings.
- [x] Normal verification report/changelog binds exact tested 04A commit/tree, literal commands/results, and evidence SHA-256/sizes.
- [x] Both Law 1 checkpoints are recorded at their required pre-edit and post-verification boundaries.

## Risk Assessment

- Layout ambiguity is permanent ABI debt; a mismatched first checkpoint stops
  before publication, and a missing second checkpoint stops acceptance. Roll
  back the complete unpublished slice.
- A default `ViFile::size` restore can fail; propagate the error and do not copy partial truth. Tests must observe cursor preservation.
- Integer conversion into C `st_size` may overflow; fail unchanged rather than truncate.

## Security Considerations

Validate caller ownership and output bounds before access; do not leak another task's FD metadata. Zero reserved bytes to prevent stack disclosure. Bit 61 is opt-in manifest policy but compatibility permit-all remains; make no stronger capability claim.

## Assumptions

- **Claim:** This phase does not introduce writable file opens, so every VIFS handle remains read-only. **Confidence:** high. **How to verify:** confirm `kernel/src/task.rs::file_open` still passes `OpenMode::Read` and reject unrelated open-mode changes.
- **Claim:** The existing POSIX boot prerequisite can execute rather than skip. **Confidence:** medium. **How to verify:** build named kernel/disk prerequisites and inspect test output/marker.
- **Claim:** The owner will approve the 32-byte record and its verified implementation. **Confidence:** low. **How to verify:** capture the first checkpoint before edits and the second against exact-04A evidence before acceptance.

## Next Steps

This lane completes when 04A and 04B satisfy every criterion. A failure blocks
only fstat publication. Keep `Rename=255` and bit 62 reserved for the
independent rename lane.
