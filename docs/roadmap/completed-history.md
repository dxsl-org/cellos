# Completed History

**Last updated**: 2026-10-19

This page is a condensed completion ledger. For the full chronological record,
see [project-changelog.md](../project-changelog.md) and the legacy roadmap
archive.

## Major Shipped Groups

- Foundation and core stability work through the early phase 1 slices.
- VirtIO, input, and multi-arch HAL bring-up.
- VFS, storage, and filesystem persistence work.
- Networking data path, DNS, and net-service follow-up work.
- Shell, utilities, and native Lua scripting support.
- Hot-swap/supervisor work and the associated state-transfer path.
- Architecture hardening work, including launch-profile deprivilege and
  boundary checks.
- x86_64 per-vector IDT and Ring-3 transition hardening: exact normalized
  errors, vector/CPL routing, 15-GPR/DF preservation, saved-CS-controlled
  GS/PKRU entry and return, corrected suspended-SYSCALL/fresh-IRET state
  restoration, the strict isolated two-task CPL0/CPL3 QEMU oracle, and the
  bootstrap SysV stack-phase correction. Generic `test-hooks` and production
  remain fixture-free; the separately rebuilt production image and all x86
  integration tests passed. Physical x86 qualification remains separate.
- Unified Desktop Phase 01 Contract Baseline (2026-10-19): ADR-0027 contract baseline
  under `.agents/261019-1310-unified-desktop/`, ABI delta ledger D-UI-01..D-UI-10
  specification (`abi-delta-ledger.md`) with Law-1 CP1 design gates pending candidate
  ratification, truthful source/image baseline manifest (`baseline-manifest.md`) at commit
  `4994602b55a43f3fd4262f36dc3dfaa7b1a2ff44` preserving user changes, and automated x86
  desktop integration test target (`tests/integration/tests/desktop-x86.rs`) under QEMU KVM/TCG.

## Historical Runtime Snapshot

- MicroPython appears in historical milestones and changelog entries only.
- It is not a current workspace deliverable.

## How to Read This Ledger

- If you need exact milestone prose, read `project-changelog.md`.
- If you need the current meaning of a milestone, read the topic file that now
  owns it instead of reusing archived prose.
