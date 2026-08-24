---
phase: 3
title: "Recoverable domain-aware user copy"
status: completed
priority: P1
effort: 3d
dependencies: [1, 2]
tier: thinking
---

# Phase 03: Recoverable domain-aware user copy

## Overview

Replace every syscall-facing raw user dereference with one checked copy boundary that is
safe for SAS and domains. It is a complete ABI cutover, not a Tier-2 syscall allowlist.

## Requirements

- Define `copy_from_user(TaskDomainRef, UserReadSlice, &mut [u8])` and
  `copy_to_user(TaskDomainRef, UserWriteSlice, &[u8]) -> Result<(), SyscallError>`.
  The domain ref contains immutable identity/generation and rejects `Dying` before and
  after acquiring its ledger read guard.
- Validate null, length-zero where disallowed, overflow, canonical/user range, every page,
  ledger availability, and R/W permission before touching bytes. Return the existing
  recoverable invalid-address ABI error and leave output/destination state unchanged.
- Install a narrow, per-hart recoverable fault guard only around byte copy. RV64 trap code
  recognizes guard-owned U-page faults, unwinds to the helper error path, and continues;
  all other kernel faults retain their existing fatal path. Do not globally enable SUM or
  treat an arbitrary kernel fault as recoverable.
- Unmap/revoke first prevents new guards/readers, then waits for ledger readers before PTE
  removal or frame reuse. SAS uses the same APIs with explicit `Sas` view; it retains its
  current mapping semantics but not raw-pointer bypasses.
- Audit every syscall input/output pointer, iovec/string/ELF-memory path, IPC payload,
  `ReadLog`, block buffers, PCI discovery output, and hypervisor exit output. Migrate every
  caller and delete direct `from_raw_parts`, volatile write, and pointer-cast syscall use.

## Architecture

`syscall ABI pointer → typed range → ledger validation + reader → guarded kernel buffer copy → clear guard → ABI result`; revoke closes ledger availability before waiting for readers.

## Assumptions

- **Claim:** all syscall user-pointer paths can return the current recoverable ABI error.
  **Confidence:** medium
  **How to verify:** complete the Phase 03 pointer-arm inventory before implementation.

## Related Files

- Modify: `kernel/src/task/syscall.rs`, `hal/arch/riscv/src/rv64/trap.rs`,
  `kernel/src/task/hart_local.rs`, `kernel/src/memory/address_space.rs`.
- Create: `kernel/src/task/user_copy.rs`, `kernel/src/task/user_copy_tests.rs`.

## Implementation Steps

1. Define typed user ranges and no-allocation copy helpers; a helper call cannot hold the
   scheduler lock or invoke arbitrary callbacks during a recoverable-fault window.
2. Add trap-frame guard metadata in hart-local storage and prove guard set/clear ordering
   across nested traps and context switches.
3. Replace all syscall ABI pointer paths in one cutover; compiler-visible private helper
   access prevents reintroducing raw user access outside `user_copy`.
4. Add fault injection that unmaps a validated second page while a copy holds its reader.
5. Emit `S22-RV64-COPY: PASS` and `S22-RV64-COPY-RACE: PASS` with `harts=1|2`; no marker
   includes copied bytes or virtual addresses.

## Test Matrix

| Runner | Cases | Gate |
|---|---|---|
| `cargo test -p cellos-kernel user_copy --features native-domains,test-hooks` | null, overflow, kernel, peer, unmapped, cross-page, permission, output atomicity | non-QEMU |
| `bash scripts/qemu-native-domain-test.sh --harts 1 --case user-copy` | trap guard returns ABI error, no panic | RV64 QEMU, 1 hart |
| `bash scripts/qemu-native-domain-test.sh --harts 2 --case user-copy-race` | concurrent unmap waits for reader; no reuse before drain | RV64 QEMU, 2 harts |

## Success Criteria

- [x] A hostile pointer never reaches direct kernel dereference.
- [x] Guard-owned fault returns recoverable ABI error; generic kernel faults still panic.
- [x] No pointer-bearing syscall arm remains outside the helper audit allowlist.

## Security Considerations

Range validation alone is insufficient; the reader guard is the revoke/teardown lifetime
edge. Copies use kernel-owned buffers and bounded lengths.

## Risk Notes

This phase is blocked if a syscall ABI cannot state its error/partial-output behavior;
resolve the contract before admission, not through a permissive fallback.

## Deviation Log

None.
