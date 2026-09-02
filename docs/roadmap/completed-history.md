# Completed History

**Last updated**: 2026-09-02

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

## Historical Runtime Snapshot

- MicroPython appears in historical milestones and changelog entries only.
- It is not a current workspace deliverable.

## How to Read This Ledger

- If you need exact milestone prose, read `project-changelog.md`.
- If you need the current meaning of a milestone, read the topic file that now
  owns it instead of reusing archived prose.
