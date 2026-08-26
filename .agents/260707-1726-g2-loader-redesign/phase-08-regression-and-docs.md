---
phase: 08
title: 3-arch QEMU regression + docs/spec reconcile
tier: medium
status: pending
depends_on: [06]
---

# Phase 08 — 3-arch regression + docs/spec reconcile

## Context links
- Plan: [plan.md](plan.md) · Scout: [scout-report.md](scout-report.md)

## Overview
Prove the full closure on the hardened harness across all three arches, then reconcile the specs/docs/memory that described `virtio_blk` as a kernel bootstrap resident (it is now a Driver Cell). Runs after Phase 06 regardless of the Phase-07 (SUM) decision.

## Key insights
- The harness is TCB of the process (RC-2 / BS#4) — hardened in Bước 1 (fail-loud boot.rs, CI-gated suite). Use it; do not weaken assertions to go green.
- Docs currently (post-#1, 2026-07-07) call virtio_blk a **bootstrap root-of-trust exception** (spec 15 §2C). After Phase 06 it is a full Driver Cell → the exception must be **removed**, not kept.

## Requirements
- **Functional:** riscv64 + aarch64 + x86_64 boot to shell; a disk-resident cell spawns via `/bin/…` (VFS→Block Cell); VFS FAT32 + littlefs I/O; net DHCP; shell burst typing — all green in the CI-gated suite.
- **Docs:** spec 15, CLAUDE.md tech-debt table, kernel-boundary memory, security-model.md, roadmap/changelog reflect virtio_blk-as-Cell + SUM status.

## Architecture / doc changes
- `docs/specs/15-kernel-boundary.md`: **remove** the §2C "Boot block device (virtio_blk + virtio_pci)" carve-out and §3.1 note; move virtio_blk to the "Already migrated → `cells/drivers/virtio-blk/`" list. Update decision-test examples citing virtio_blk.
- `CLAUDE.md`: move virtio_blk from "Bootstrap residents — NOT violations" to "Already migrated"; drop from tech-debt.
- Memory `project-kernel-boundary-law.md`: Phase 05 row "RECLASSIFIED — NOT a violation" → "DONE — migrated via G2 loader redesign (ramdisk boot + `sys_spawn_from_elf` + `/bin` FS overlay)".
- `docs/security-model.md`: SUM status — removed→scoped if Phase 07 shipped, else note it remains + link the follow-up plan.
- `docs/project-changelog.md`/roadmap: record the closure + warm-boot snapshot status (Phase 05 ADR) + the P2→disk-FS cell-store migration.

## Related code files
- Modify: `docs/specs/15-kernel-boundary.md`, `CLAUDE.md`, `docs/security-model.md`, `docs/project-changelog.md`, `docs/project-roadmap.md`; memory `project-kernel-boundary-law.md` (+ MEMORY.md pointer if wording changes).
- CI: add Block-Cell + `spawn_from_elf` + `/bin`-overlay tests to the CI allowlist.

## Implementation steps
1. Run full integration suite 3-arch on the hardened harness; capture boot logs as evidence.
2. Triage red against the documented low-leverage tail (gpu/input-QMP/mqtt/bench, per `project-ipc-wildcard-recv-poisoning.md`) — any *new* red is a real regression; don't paper over.
3. Reconcile all docs/specs/memory above.
4. Add new tests to CI allowlist.
5. `/hl-log` the plan outcome.

## Todo
- [ ] 3-arch suite green (minus documented tail) with evidence logs
- [ ] spec 15 §2C/§3.1 virtio_blk carve-out removed
- [ ] CLAUDE.md + kernel-boundary memory updated
- [ ] security-model SUM status reconciled
- [ ] changelog/roadmap + warm-boot + cell-store migration recorded
- [ ] CI allowlist includes new tests

## Success criteria
- **Runtime evidence:** CI-gated 3-arch boot + disk-cell-spawn + block-I/O tests green; `grep` shows no live kernel virtio_blk; docs no longer describe it as a kernel resident. RC-4 formally closed in memory.

## Risk assessment
- *Tail-test noise masking a real regression* — compare red set against the known tail list; any new red is real.

## Security considerations
- Final posture: block driver exiled to a trusted-first-party cell (BS#1); if SUM scoped, hardware S/U boundary restored. Update the "LBI isolates cells" caveat wording.

## Next steps
- Unblocked follow-ups: **Supervisory Cell** (owns snapshot/hotswap + snapshot's block access, per Phase 05 ADR); **scoped-SUM** plan (if Phase 07 split out).
- Original roadmap resumes: Bước 4 (comprehensive QEMU regression — largely covered here); Bước 5 (real board RPi3→VF2).
