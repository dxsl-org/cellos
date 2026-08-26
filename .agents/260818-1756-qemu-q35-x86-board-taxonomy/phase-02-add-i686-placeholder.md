---
phase: 2
title: "Add i686 Placeholder"
status: completed
priority: P2
effort: "45m"
dependencies: [1]
tier: fast
---

# Phase 2: Add 32-bit QEMU Placeholders

## Overview

Create documentation-only placeholders for future QEMU q35 i686, QEMU virt
RISC-V 32-bit, and QEMU virt Arm 32-bit support without making any of them
supported boards.

## Requirements

- Functional: create `boards/qemu/q35-i686/README.md`,
  `boards/qemu/virt-riscv32/README.md`, and `boards/qemu/virt-aarch32/README.md`
  stating planned, not implemented, no build contract, no runtime evidence, and
  no support status.
- Non-functional: do not add `board.rs`; do not register any placeholder in
  `boards/src/lib.rs`; do not add `BoardDescriptor`, `Architecture`, `SocId`,
  Cargo feature, CI/build matrix entry, or board-config count.

## Architecture

The placeholders have no data flow into Rust or CI. They are roadmap markers
only. Active QEMU flows remain `boards/qemu/q35-x86_64/board.rs`,
`boards/qemu/virt-riscv64/board.rs`, and `boards/qemu/virt-aarch64/board.rs`.

## Assumptions

None - no unverified claims.

## Related Files

- Create: `boards/qemu/q35-i686/README.md`
- Create: `boards/qemu/virt-riscv32/README.md`
- Create: `boards/qemu/virt-aarch32/README.md`
- Do not modify: `boards/src/lib.rs`
- Do not modify: `boards/src/catalog_tests.rs`
- Do not modify: `.github/workflows/ci.yml`
- Do not modify: `scripts/check-board-configs.sh` except to ensure it does not count this placeholder.

## Implementation Steps

1. Add the three README placeholders only.
2. State that 32-bit x86 q35, RISC-V virt, and Arm virt support are planned, not implemented.
3. State there is no `BoardDescriptor`, no `board.rs`, no Cargo target/build contract, no QEMU boot evidence, and no CI lane.
4. Confirm `check-board-configs.sh` does not require placeholder `board.rs` files and fails if any appear.
5. Confirm catalog tests do not include the placeholders.

## Success Criteria

- [x] `test -f boards/qemu/q35-i686/README.md`
- [x] `test -f boards/qemu/virt-riscv32/README.md`
- [x] `test -f boards/qemu/virt-aarch32/README.md`
- [x] `test ! -f boards/qemu/q35-i686/board.rs`
- [x] `test ! -f boards/qemu/virt-riscv32/board.rs`
- [x] `test ! -f boards/qemu/virt-aarch32/board.rs`
- [x] The registration gate finds no placeholder names in root/workspace Cargo manifests, `boards/src`, `kernel`, `hal`, or `.github`; names occur only in placeholder READMEs, living docs, and the negative guard itself.
- [x] Supported board count remains unchanged from the q35 x86_64 set.

## Security Considerations

N/A. Placeholder has no executable path.

## Risk Notes

Likelihood medium, impact low: a later script may treat every directory under
`boards/qemu` as supported. Mitigation: explicit README wording plus grep gate
in verification. Rollback: delete the placeholder READMEs. Cannot undo: none,
until committed.

## Deviation Log

- Decision: placeholder non-registration is enforced in `scripts/check-board-configs.sh`.
  Why: the contract is absence from catalog, CI, and builds, so a shell gate
  proves that directly without adding fake Rust registration just to test it.
  Impact: all three 32-bit QEMU placeholders stay README-only and future
  accidental registration fails fast.
  Revert: remove the shell guard only when the corresponding 32-bit targets become real.
