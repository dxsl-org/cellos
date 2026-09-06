# POSIX Sequence Summary

## Phase 02

The implementation root is `libs/api/src/services/posix.rs:1-43`; four live references still name deleted `libs/api/src/posix.rs`. Repair only `docs/FAQ.md`, `docs/guides/tier1b-c-zig.md`, `docs/specs/05-application.md`, and the documentation comment in `cells/tests/posix-shim-test/src/main.rs`. Preserve historical records and non-POSIX-complete limits.

## Phase 03

Caller-scoped canonical CWD, validated chdir, and exact non-NUL getcwd already exist (`kernel/src/task.rs:1488-1500,1613-1680`). Public IDs 106–108 collide with `Seek`/`FileOp` routing, so use additive `Chdir=252` and `Getcwd=253` only after two explicit ABI confirmations. Both deliberately share fresh authority bit 60. One shell helper must serve direct `pwd` and `$(pwd)`; `cd` accepts exactly one explicit operand. No HOME, C wrapper, symlink, mount, or inheritance behavior.

## Phase 04

Kernel fstat is a stub and C `_fstat` fabricates success (`kernel/src/task.rs:1608-1611`; `libs/api/src/services/posix/sysio.rs:227-239`). Use `Fstat=254`, bit 61, and a fixed 32-byte `#[repr(C, align(8))]` V1 record: `kind:u32`, `access:u32`, `size:u64`, `reserved:[u64;2]`. Wire success is exactly 32; the C shim maps it to `_fstat` return 0 only after successful zeroed-local translation/copy and returns -1 unchanged otherwise. The smoke manifest adds `Open/Fstat/Close`, proves successful open/fstat with separate markers, and closes every successful open on every later path.

## Phase 05

There is no rename method and VIFS1 cannot write. Before publication/mount activation, prove backend one-call atomicity and explicit authority over every mutator; legacy permit-all grants no write. Ordinary `OpenCap` must become existing-only `CapPerms::FILE_READ` so shell/BootFs/hypervisor readers survive without ambient create/write; `WriteCap`/`TruncateCap` also require `CapPerms::WRITE`. A canonical-path ledger gives read opens/transient reads shared leases and rename/remove/mkdir/rmdir exclusive reservations; any separately confirmed create-open uses exclusive→shared atomic downgrade. `ParkedCapFile` carries its lease across revoke. Equal existing regular source succeeds without `rename_once`; missing fails.

## Sequencing Decision

Keep the strict order. Every ABI lane has separate confirmations; bits 55–57 remain untouched and 63 reserved. Each candidate commit is verified from a clean checkout of its exact commit/tree, then its normal verification report/current changelog binds commands/results and evidence hashes. Any failed phase halts all successors until corrected and accepted.
