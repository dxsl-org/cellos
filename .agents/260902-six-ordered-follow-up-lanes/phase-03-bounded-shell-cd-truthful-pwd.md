---
phase: 3
title: "Bounded Shell cd and Truthful pwd"
status: completed
dependencies: []
tier: thinking
---

# Phase 03: Bounded Shell cd and Truthful pwd

## Context Links

- [Master plan](plan.md) · [POSIX research](research/posix-sequence.md) · [Review reconciliation](research/review-reconciliation.md)
- `libs/api/src/abi.rs:2-12`
- `libs/api/src/abi/syscall.rs:169-177,490-510,766-780,885-895`
- `kernel/src/task.rs:1488-1500,1613-1632,1669-1680`
- `kernel/src/task/syscall.rs:6494-6511,6597-6648`
- `cells/tools/shell/src/cmd_sys.rs:6-12`; `executor.rs:48-55,79-110,562-590`

## Overview

After the first Law 1 owner checkpoint approves the exact proposal, publish a
bounded typed CWD family and make shell `cd`, direct `pwd`, and `$(pwd)` use the
same caller-scoped kernel truth. The second checkpoint reviews the implemented
interface and bound evidence before acceptance. Phase 02 is not an entry gate.

## Key Insights

- The lower layer is complete; this phase is a public ABI/consumer slice, not a second CWD implementation.
- Legacy raw 107 collides with typed `FileOp`, 106 is `Seek`, and raw 108 is unversioned. No live callers were found; clean cutover removes obsolete CWD fallbacks.
- Ownership: ABI Owner records both confirmations; Kernel Syscall Owner wires caller identity; Shell Owner implements one helper; Integration Owner accepts behavior.

## Requirements

- Hard gate: the first explicit confirmation approves exactly `Chdir=252`, `Getcwd=253`, shared allowlist bit 60, and the byte/error contracts before editing. After exact-03A verification, a separate second confirmation must approve the implemented interface, migration impact, and bound evidence before acceptance. Plan approval is neither checkpoint.
- `Chdir(path_ptr,path_len) -> 0`; bounded UTF-8 path, exactly one explicit shell operand, lexical absolute/relative/`.`/`..`, root-saturating, validation before CWD commit.
- `Getcwd(buf_ptr,buf_len) -> exact byte count`; non-NUL bytes, short/invalid buffer fails without output mutation.
- One shell getcwd helper serves `pwd` and `run_capture("pwd")`. Failed/malformed `cd` returns nonzero and retains prior CWD.
- Declare both calls in the shell manifest. Add no HOME/default-cd, C wrapper, process inheritance, symlink, mount, or broad POSIX behavior.

## Architecture

`shell -> ostd typed wrapper -> ViSyscall 252/253 + bit60 -> map_syscall -> caller_id-aware file_chdir/file_getcwd -> task.cwd`. The shell holds no second CWD. User strings/copyout reuse existing bounded staging and validation. The typed cutover removes raw 107/108 CWD fallback mappings and their `known_raw` exemptions after all callers migrate.

**03A** is the source/test commit for ABI, kernel, ostd, and shell. A clean checkout of exact 03A commit/tree runs all acceptance; only then **03B** appends the normal evidence report/current changelog with tested commit/tree, commands/results, and artifact hashes. Failure blocks only this lane; no fstat/rename dependency is created.

## Related Code Files

- Modify: `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/syscall_tests.rs`
- Modify: `libs/ostd/src/syscall.rs`
- Modify: `kernel/src/task/syscall.rs`; reuse `kernel/src/task.rs` primitives
- Modify: `cells/tools/shell/src/main.rs`, `cmd_sys.rs`, `executor.rs`, `shell_test.rs`
- Modify tests: `tests/integration/tests/shell-utils.rs`, `tests/integration/tests/vfs-quota.rs`
- Create after clean verification: `docs/evidence/shell-cwd-verification.txt`
- Documentation trigger after green behavior: current `[Unreleased]` changelog/risk wording and shell comments, bound to tested 03A commit/tree

### Law 1 Confirmation Record (Checkpoint 1: Pre-Implementation)

- **Status**: APPROVED
- **Timestamp**: 2026-09-03
- **Approver**: Accountable Maintainer (@lungmat8)
- **Verbatim approved proposal**:
  1. Syscall Discriminants: `ViSyscall::Chdir = 252` (`(path_ptr: usize, path_len: usize) -> 0`), `ViSyscall::Getcwd = 253` (`(buf_ptr: usize, buf_len: usize) -> exact_bytes_written`).
  2. Capability Allowlist: Bit 60 allocated for CWD (`Chdir` and `Getcwd`) in manifest macro. Bits 55–59 unchanged; 61 reserved for fstat, 62 for rename, 63 unused.
  3. Error contracts: Invalid/missing/file target fails and leaves CWD unchanged; short buffer fails without mutating caller memory.
  4. Migration: Clean cutover removes raw 107/108 CWD fallback and `known_raw` exemptions; `FileOp = 107` retained.

### Exact 03A Verification Record (Checkpoint 2 Input)

- **Source commit**: `6b9aae923909c4ac4e3228821e70158d7d232769`
- **Source tree**: `18de37371e31e9a5c0081da4f7dcabff4e80a22b`
- **Checkout**: clean detached worktree before execution; exact tracked status
  empty.
- **Environment**: `CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS=-C
  relocation-model=pic`, matching `.github/workflows/ci.yml` global RV64
  code-generation policy.
- **ABI**: `cargo test -p api abi::syscall_tests` — exit 0; 19 passed.
- **Lower layer**: `bash scripts/build-test-hooks-ci.sh` — exit 0; then
  `CI=1 cargo test --manifest-path tests/integration/Cargo.toml --test
  vfs-quota riscv64_vfs_quota_all_pass -- --exact --nocapture` — exit 0;
  one exact QEMU test passed and one host test was filtered. The executed test
  did not skip its prerequisites; inside the single-hart guest, the required
  `ATOMIC_PUBLICATION_AP-13: SKIP (hart 1 not online; SMP probe not required)`
  capability marker was observed alongside AP-12/AP-14/AP-15 PASS. The required
  deliberate overflow then produced a classified Cell-254 guard-page
  termination with `cause=0xf`; no unclassified access-fault or FAIL marker was
  observed.
- **Shell**: `bash scripts/build-shell-test-ci.sh` — exit 0; then
  `CI=1 cargo test --manifest-path tests/integration/Cargo.toml --test
  shell-utils shell_utils_all_scenarios_pass -- --exact --nocapture` — exit 0;
  one exact QEMU test passed, all 38 required markers were observed, the test
  did not skip its prerequisites, and no fault/FAIL marker was observed.
- **Build artifact mutation**: only tracked
  `kernel/src/embedded-test-hooks/init` changed after the initially clean
  verification checkout; it is not part of 03A.
- **Superseded evidence**: the first `89d67099` pristine-checkout attempt lacked
  the gitignored workstation RV64 PIC setting and failed before QEMU. A later
  passing `89d67099` run was invalidated because 03A broadened failed
  `$(cat)`/`$(vcat)` semantics beyond this lane. The source was narrowed,
  amended to `6b9aae92`, and verified again from a newly created pristine
  checkout under the repository's CI RV64 environment contract.
- **Checkpoint state**: approved by the Accountable Maintainer with the AP-13
  capability SKIP and classified Cell-254 `cause=0xf` guard-page fault explicit.
- **Superseded 03B commit**: `67a28334ad225410ac19f8e0c9c09438bda02fba`;
  its evidence wording incorrectly claimed no guest fault.
- **Corrected 03B commit**: `77a540980e0d048f6585acf8e27a5da7e93ad739`,
  tree `f55de8ae666d399a7203ec268b9a9c8f04ca340f`; evidence SHA-256
  `8f8f9e76dcd845686aeb633d692f14b2c8c1cae2a36fa8a30e0ad55c0d95f54a`,
  2,628 bytes.

### Law 1 Confirmation Record (Checkpoint 2: Post-Verification)

- **Status**: INVALIDATED
- **Timestamp**: 2026-09-03T10:58:13Z
- **Approver**: Accountable Maintainer (@lungmat8)
- **Verbatim approved scope**:
  1. Exact 03A:
     `6b9aae923909c4ac4e3228821e70158d7d232769`, tree
     `18de37371e31e9a5c0081da4f7dcabff4e80a22b`.
  2. Interface/migration: `Chdir=252`, `Getcwd=253`, shared bit 60,
     caller-scoped mapping/allowlist, one bounded direct/captured `pwd` helper,
     raw 107/108 CWD removal, `FileOp=107` retained, and unchanged failed
     `$(cat)`/`$(vcat)` behavior.
  3. Evidence: INVALIDATED because it stated that no guest fault appeared,
     although the VFS test requires the classified Cell-254 guard-page
     termination with `cause=0xf` after its deliberate overflow probe.
  4. The AP-13 guest capability SKIP wording was accurate but does not cure the
     omitted classified-fault boundary.

### Law 1 Confirmation Record (Checkpoint 2: Corrected Evidence Boundary)

- **Status**: APPROVED
- **Timestamp**: 2026-09-03T11:12:02Z
- **Approver**: Accountable Maintainer (@lungmat8)
- **Verbatim approved scope**:
  1. Exact 03A:
     `6b9aae923909c4ac4e3228821e70158d7d232769`, tree
     `18de37371e31e9a5c0081da4f7dcabff4e80a22b`.
  2. API 19/19, exact CI=1 QEMU `vfs-quota` 1/1, and `shell-utils` 1/1
     with 38 markers passed; neither host test skipped prerequisites.
  3. The VFS single-hart guest intentionally emitted AP-13 SKIP and the
     classified `[fault] Cell 254 ... terminated: cause=0xf` after arming its
     two-page stack guard.
  4. No additional/unclassified `Load access fault`, `Store/AMO access fault`,
     `Instruction access fault`, or FAIL marker appeared. The classified fault
     is evidence, not a clean no-fault run or an SMP claim.

## Implementation Steps

1. Record the first Law 1 ABI proposal confirmation verbatim. If it is absent or
   different, stop this lane before feature edits; other lanes remain executable.
2. Add typed variants, decode arms, shared bit-60 mapping, manifest macro coverage, exact discriminant/round-trip/bit assertions; prove bits 55–59 unchanged and 63 unused.
3. Add `ostd::sys_chdir` and `sys_getcwd` beside existing file wrappers with no hidden allocation or NUL convention.
4. Map both typed variants to existing `Syscall::{ChDir,GetCwd}` handling using explicit `caller_id`; retain staging-before-copy and no simultaneous scheduler/VIFS locks.
5. Remove obsolete raw 107/108 CWD fallback/known-raw comments and mappings after repository caller search is clean; retain typed `FileOp=107` behavior.
6. Add `cd` to inventory/dispatch. Implement exactly-one-operand validation and one bounded getcwd helper used by both direct and captured `pwd`; remove both `/` literals and stale comment.
7. Extend `path_selftest`/`vfs-quota` to require `cwd-path self-test PASS`; extend `shell_test`/`shell-utils` to assert named direct/captured PWD and CD markers for `/`, `/BIN`, relative/`.`/`..`, root saturation, zero/two operands, failed file/missing with retained prior CWD, short-output immutability, and two-task isolation.
8. Commit as 03A. From a clean checkout of its exact commit/tree, capture fail-closed `cargo test -p api abi::syscall_tests && bash scripts/build-test-hooks-ci.sh && CI=1 cargo test --manifest-path tests/integration/Cargo.toml --test vfs-quota riscv64_vfs_quota_all_pass -- --exact --nocapture`, then `bash scripts/build-shell-test-ci.sh && CI=1 cargo test --manifest-path tests/integration/Cargo.toml --test shell-utils shell_utils_all_scenarios_pass -- --exact --nocapture`. Both integration targets must assert the named new markers and reject FAIL; the VFS route must require its classified Cell-254 `cause=0xf` guard-page termination and reject only additional/unclassified access faults. No generic boot PASS or host prerequisite skip counts.
9. After both exact guest routes and ABI tests pass, present the implemented interface, migration impact, and exact-03A evidence for the second Law 1 confirmation. Only after that confirmation, commit 03B verification report/current wording naming exact 03A revision/tree, command statuses, and evidence hashes/sizes. Any failure is fixed and reverified within this lane.

## Todo List

- [x] Capture the first Law 1 ABI proposal confirmation.
- [x] Publish/test IDs 252/253 plus shared bit 60 in source commit 03A.
- [x] Wire ostd/kernel, remove obsolete raw fallbacks, and implement one shell CWD path.
- [x] Verify exact 03A commit/tree with ABI, shell, and lower-layer marker commands.
- [x] Capture the second Law 1 confirmation over exact-03A implementation and evidence.
- [x] Commit 03B report/current docs with commands/results and evidence hashes/sizes.
- [x] Halt this lane on any gate or verification failure.

## Success Criteria

- [x] Exact IDs, decode/encode, allowlist bit, and manifests are pinned; occupied bits are unchanged.
- [x] `pwd` equals `$(pwd)` after every successful `cd`; canonical absolute results match task CWD.
- [x] Missing/file/malformed changes fail nonzero and preserve prior CWD; getcwd failure writes no bytes.
- [x] Two tasks remain isolated and caller attribution never uses per-hart/current-task state.
- [x] Exact `vfs-quota` and `shell-utils` commands execute their QEMU tests and observe the new lower-layer and shell CWD markers without a harness/prerequisite skip or FAIL; the required single-hart guest AP-13 capability marker remains SKIP, the deliberate Cell-254 guard-page fault remains classified and expected, and no unclassified access-fault appears.
- [x] Normal verification report/changelog binds exact tested 03A commit/tree, literal commands/results, and evidence SHA-256/sizes.
- [x] Both Law 1 checkpoints are recorded at their required pre-edit and post-verification boundaries.

## Risk Assessment

- ABI publication is irreversible; an absent/mismatched first checkpoint stops
  before edits, and an absent second checkpoint stops acceptance. Roll back the
  whole unpublished slice; never alias old numbers.
- Divergent shell paths could reintroduce false capture output; enforce one helper and paired tests.
- Compatibility-default permit-all means bit 60 is declaration policy, not a new hard security boundary; do not claim otherwise.

## Security Considerations

Bound input lengths and user pointers with existing helpers; lexical `..` cannot escape `/`; failure exposes no partial buffer and changes no task. Call attribution is explicit. Do not broaden raw-ID exemptions or make CWD process-global.

## Assumptions

- **Claim:** No external raw 108 consumer exists beyond searched repository callers. **Confidence:** medium. **How to verify:** release-impact search and ABI-owner confirmation before removing fallback.
- **Claim:** The owner will approve the proposed grouping and the verified implementation. **Confidence:** low. **How to verify:** capture the first checkpoint before edits and the second against exact-03A evidence before acceptance.
- **Claim:** Existing shell integration prerequisites produce a real QEMU run. **Confidence:** medium. **How to verify:** inspect command output for executed assertions, not a skip.

## Next Steps

This lane completes when 03A and 03B satisfy every criterion. A failure blocks
only shell CWD publication. Reserve 254/bit 61 for the independent fstat lane;
do not implement fstat here.
