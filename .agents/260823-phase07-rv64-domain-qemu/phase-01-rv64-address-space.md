---
phase: 1
title: "RV64 AddressSpace substrate"
status: implementation-present-verification-pending
priority: P1
effort: 3d
dependencies: []
tier: thinking
---

# Phase 01: RV64 AddressSpace substrate

## Overview

Create the sole kernel-owned private-root representation for RV64 Sv39. It replaces no
SAS mapping API and is compiled only when `native-domains` is selected.

## Requirements

- Add `native-domains = []` to `kernel/Cargo.toml`; default features MUST NOT include it.
  Add boot-policy enum `Disabled | Enabled | Draining` named `native-domain-admission`,
  defaulting to `Disabled`; the build feature alone MUST NOT admit a domain.
- Define `AddressSpace { identity: DomainId, generation, root: OwnedFrame, asid:
  AsidLease, state: Live|Dying, ledger, copy_readers, current_harts }`. `DomainId` is
  monotonically allocated; `generation` is checked on every cross-hart acknowledgement.
- `AddressSpaceBuilder` owns all uncommitted frames. It may publish `Arc<AddressSpace>`
  only after zeroing root/intermediates, mapping the narrow supervisor allowlist, private
  image/stack/heap, and approved ABI pages, then applying relocation and W^X.
- The ledger records `Private`, `ImmutableImage`, `SharedAbi`, and `Grant` entries with
  page range, physical frame ownership, U/R/W/X permissions, and availability. Unknown
  or global-root-derived mapping requests fail closed. `KERNEL_ROOT`, `map_page`, and
  `unmap_page` are never accepted as a domain-root fallback.
- Add RV64 HAL API `activate_address_space(root_ppn, asid)`, `flush_asid(asid)`, and
  `flush_asid_remote(mask, asid)`; SATP write and required `sfence.vma` ordering reside
  there, not in the scheduler. ASID allocation carries epoch/wrap invalidation before
  reuse. RV32 reports `NotSupported` and never exposes a domain.

## Architecture

`Builder → complete ledger/W^X → Arc<AddressSpace>(Live) → TCB reference`; failed build
runs builder drop and returns every owned frame. Teardown is deferred and root last. The
safe root is the existing kernel root, with no domain USER PTEs.

## Assumptions

- **Claim:** supported RV64 QEMU `virt` boots with Sv39 and SBI RFENCE.
  **Confidence:** medium
  **How to verify:** Phase 07 records QEMU/OpenSBI version and two-hart RFENCE result.

## Related Files

- Modify: `kernel/Cargo.toml`, `kernel/src/memory.rs`, `kernel/src/memory/paging.rs`,
  `kernel/src/policy.rs`, `hal/arch/riscv/src/rv64/mod.rs`.
- Create: `kernel/src/memory/address_space.rs`, `hal/arch/riscv/src/rv64/domain.rs`.
- Tests: `kernel/src/memory/address_space_tests.rs` and `tests/guests/native-domain-probe/`.

## Implementation Steps

1. Introduce non-copyable frame/ASID leases and a ledger whose mutators require a live
   `AddressSpace`; keep global mapping functions type-incompatible with it.
2. Build Sv39 roots from an explicit supervisor mapping list rather than cloning the
   identity-mapped root; audit trap, kernel stack, and HHDM needs before admitting user.
3. Implement RV64 SATP/ASID activation, local and SBI remote fence backend; perform a
   global old-epoch invalidation before an ASID can be reissued.
4. Add feature-disabled stubs returning `NotSupported` without allocating roots or tags.
5. Emit test-only scalar markers: `S22-RV64-ASPACE: PASS` and
   `S22-RV64-ASID-REUSE: PASS`; production logging records only domain id/generation,
   never virtual addresses or page contents.

## Test Matrix

| Runner | Cases | Gate |
|---|---|---|
| `cargo test -p cellos-kernel address_space --features native-domains,test-hooks` | zero/init rollback, unknown mapping denial, W^X, ledger range/permission checks | non-QEMU |
| `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features native-domains` | RV64 backend compiles | non-QEMU |
| `bash scripts/qemu-native-domain-test.sh --harts 1 --case aspace,asid-reuse` | no stale ASID translation after recycle | RV64 QEMU |

## Success Criteria

- [ ] A failed build leaves no root/intermediate/leaf/ASID publication.
- [ ] A private root lacks peer USER pages, broad usable RAM, and user MMIO mappings.
- [ ] Feature/policy off produces only SAS behavior and no domain allocation.

## Security Considerations

Do not copy global identity mappings. Root/intermediate frames are freed after leaves and
only after generation-matched quiescence supplied by Phase 06.

## Risk Notes

Supervisor mapping requirements must be derived from actual trap/switch execution; a
missing mapping fails closed, never by adding broad RAM. SBI RFENCE availability is a
QEMU/firmware prerequisite for the two-hart gate.

## Deviation Log

None.
