---
title: "Six Independent Follow-up Lanes"
status: in_progress
created: 2026-09-02
mode: planning-only
---

# Six Independent Follow-up Lanes

## Goal

Execute six bounded lanes independently under
[ADR-0013](../../docs/decisions/0013-solo-first-development-independent-promotion.md).
A failure blocks only the affected lane and its named claim. The sole maintainer
may hold all development roles; AI and CI remain automated assurance. A distinct
repository member is required only for an independently promoted claim and must
answer `DECISION: YES` or `DECISION: NO` through the bound GitHub issue or pull
request.

## Lanes

| # | Phase | Status | Owner | Hard gate / dependency |
|---|---|---|---|---|
| 01 | [AArch64 evidence and governance](phase-01-aarch64-semihosting-evidence-governance.md) | completed | Accountable Maintainer | Schema v4 correction/resolution ratified by repository collaborator `datgausaigon`; production Phase 3 remains `PLANNED` |
| 02 | [Live POSIX documentation repair](phase-02-live-posix-documentation-path-repair.md) | completed | Documentation Owner | 02A `07aae8b6`; clean verification bound in `posix-live-path-repair-verification.txt` |
| 03 | [Bounded shell cd and truthful pwd](phase-03-bounded-shell-cd-truthful-pwd.md) | completed | Shell/ABI Integrator | 03A `6b9aae92`; corrected 03B `77a54098`; Checkpoint 2 names AP-13 SKIP and classified Cell-254 guard fault |
| 04 | [Truthful fstat](phase-04-truthful-fstat.md) | completed | POSIX/ABI Integrator | 04A `4856f4de`; 04B `5aacccd1`; both owner checkpoints and clean exact wire/QEMU evidence complete |
| 05 | [Atomic rename backend gate](phase-05-atomic-rename-backend-gate.md) | completed | Filesystem Owner | 05G `c770e928`; 05A `ad07ede4`; 05B `82f9be2f`; Checkpoints 1 & 2 verified; evidence bound |
| 06 | [Pinned QEMU-TCG x86 parity](phase-06-pinned-qemu-tcg-x86-compatibility.md) | completed | x86 Virtualization Owner | 06A `0117192b`; 06B `docs/evidence/qemu-x86-10.2.0-verification.txt` |

## Current Execution State (2026-09-03)

- Phase 01 completed schema-v4 migration, AArch64 correction, and blocker resolution against fresh QEMU evidence. Issue #47 records the bound `DECISION: YES`; the authenticated collaborators endpoint returned HTTP 204 for `datgausaigon` on 2026-09-03. Acceptance-ledger production Phase 3 remains `PLANNED`.
- Phase 02 completed its four live POSIX corrections at clean commit `07aae8b6a067b4fe6b49bc05b054d5aaa53eb4e8` (tree `7f794da69bc18fa53c6a6f5ae186ba269cb5e070`), bound by `docs/evidence/posix-live-path-repair-verification.txt`.
- Phases 03 and 04 completed their caller-scoped CWD and truthful fstat lanes.
- Phase 05 completed the selected VFS-service `/srv` RedoxFS architecture: 05G `c770e928`, 05A `ad07ede4`, and 05B `82f9be2f` bind Checkpoints 1–2 and `docs/evidence/atomic-rename-verification.txt`. The rejected writable kernel-VIFS/VIFS1 design remains unpublished, immutable kernel BootFS stays outside the writable namespace, and Phase 06 remains independently executable.
- Phase 06 completed its literal QEMU 10.2.0 version parity preflights (06A `0117192b`), clean-prefix official source build (SHA-256 `849afef0f261903c6ab3aba4a5b1b6042388acdabe34554cc9e1baf71d8e1077`), prelaunch rejection proof against QEMU 8.2.2 across all three runners, and clean execution of strict 1 GiB boot smoke, two-boot VirtIO persistence E2E, and 27-scenario hostile corpus with recovery write flush (exit 0). Evidence is bound in `docs/evidence/qemu-x86-10.2.0-installer.txt` and `docs/evidence/qemu-x86-10.2.0-verification.txt`.

## Shared Contracts

- Provisional frozen-ABI allocation is `Chdir=252`, `Getcwd=253`, `Fstat=254`, `Rename=255`; nothing is published before its phase gate.
- Authority bits 55–59 are occupied. CWD uses bit 60 and fstat bit 61; rename bit 62 remains provisional until Phase 05 Checkpoint 1. Bit 63 remains unavailable as a callable syscall bit and is reserved in Phase 05 only as explicit `VfsMutate` declaration metadata.
- Phase 01 corrected and resolved the stale blocker under schema v4 without advancing acceptance-ledger production Phase 3.
- Phase 04 uses a fixed-width, zero-initialized V1 wire record; the kernel never writes target C `stat`.
- Phase 05 may prove its private Gate A harness, authority trailer derivation, and unconnected service ledger before Checkpoint 1; it publishes no opcode 255/bit 62 mapping, backend rename method, IPC variant, wrapper, or production claim until Gates A–C pass and the owner approves the exact contract.
- Every VFS-service mutator requires the dedicated attested mutation flag. The kernel derives it only from an explicit non-`ALL` syscall declaration carrying the marker; legacy/no-manifest `u64::MAX` never grants mutation. Every current mutator caller is migrated explicitly or its mutation remains unavailable.
- Ordinary kernel `OpenCap` is existing-file/`CapPerms::FILE_READ`, operations enforce stored `CapPerms`, and no writable/create cap opener is added.
- Phase 06 pins literal first-line version `QEMU emulator version 10.2.0`; no legacy backport, guest workaround, distro/generalization, or oracle weakening.
- Phase 05's canonical ledger lives in the VFS service and covers direct/transient I/O plus both service file-handle tables through cleanup. `/srv` rename uses sorted exclusive old/new reservations; equal existing regular paths succeed without a backend call. Cross-mount, directory, root, and open-conflict cases are denied.

## Commit and Documentation Policy

- Each lane owns only files listed in its phase file. Concurrent lanes that
  touch shared ABI or documentation files must serialize those edits, but they
  do not acquire a semantic predecessor dependency.
- For code lanes, create the source commit first; verify that exact commit/tree from a clean checkout; only then append the normal verification report/current changelog with tested commit/tree, literal commands/results, and evidence path/SHA-256/size. This report commit is not a bypass.
- Phase 02 follows the same two-commit documentation verification binding. Phase 01 additionally obeys its governed ledger event sequence.
- Any failed gate/test leaves only that lane and its claim pending and rolls back
  prohibited partial publication. Other lanes continue independently.
- Documentation changes occur only at each lane's named trigger; historical changelog, legacy roadmap, dated research, and prior `.agents/` records remain historical.

## Evidence Index

- [Scout report](scout-report.md)
- [ARM/ledger research](research/aarch64-ledger.md)
- [POSIX sequence research](research/posix-sequence.md)
- [x86 QEMU research](research/x86-qemu.md)
- [Review reconciliation](research/review-reconciliation.md)

## Out of Scope

Broad POSIX/Linux completeness, production admission, physical qualification, symlinks, process-CWD inheritance, mount namespaces, cross-device rename, crash durability, legacy-QEMU backports, guest VMCB workarounds, and any weaker fatal/liveness/persistence/hostile oracle.
