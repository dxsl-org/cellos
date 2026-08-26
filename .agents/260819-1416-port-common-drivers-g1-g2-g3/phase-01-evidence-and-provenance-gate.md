---
phase: 1
title: "Evidence and Provenance Gate"
status: completed
priority: P1
effort: "3d"
dependencies: []
tier: thinking
---

# Phase 01: Evidence and Provenance Gate

## Context Links

- `docs/project-roadmap.md:169-203`; `docs/specs/04-hardware.md:48-146`; `docs/specs/13-peripherals.md:4-5`; `docs/specs/13-peripherals.md:160-177`; `docs/TODO.md:8`; `docs/TODO.md:28`.
- Research: `./research/haily-researcher-01-current-cellos-driver-gaps.md`, `./research/haily-researcher-02-reference-os-driver-map.md`.

## Overview

Freeze the driver backlog and provenance rules before porting. This prevents mixing compile/QEMU evidence with physical-board claims or importing incompatible external code.

## Requirements

- Functional: produce a checked driver inventory, stage priority map, license matrix, and evidence ledger format.
- Non-functional: every external source is classified before code is read deeply or copied; no product code changes.

## Architecture

Data flow: roadmap/spec/code inventory/reference tree -> provenance classifier -> driver capability matrix -> phase gates. Outputs are plan/docs artifacts and implementation tickets only.

## Related Code Files

- Modify later: `docs/project-roadmap.md`, `docs/specs/04-hardware.md`, `.agents/plan-portfolio.md` [only if user approves doc sync].
- Read-only: `D:\Cellos\.references\*`, `boards/src/descriptor.rs`, `cells/drivers/*`, `kernel/src/task/drivers/*`.

## Implementation Steps

1. Re-run inventory with `git grep`/`Select-String`; avoid WSL `rg` if it still resolves to WindowsApps.
2. Classify drivers as present/prototype/missing/promoted for G1/G2/G3.
3. Build a license BOM: permissive-adaptable, concept-only, unknown-blocked.
4. Record evidence labels: compile, QEMU, synthetic/fallback, real controller, physical board.
5. Mark out of scope: USB xHCI, WiFi/Bluetooth, audio, Mellanox mlx5, detailed G3 kernel scheduler.

## Todo List

- [x] Inventory every `cells/drivers/*` crate and kernel fallback driver.
- [x] Record observed reference path correction: `D:\Cellos\.references`.
- [x] Create driver-source BOM with SPDX/notice obligations.
- [x] Add evidence ledger template for future implementation.

## Success Criteria

- [x] Capability matrix has every driver from `boards/src/descriptor.rs:20-45`.
- [x] No external source is used without license classification.
- [x] RPi3/QEMU/physical evidence are reported separately.

## Test Matrix

- Unit: N/A.
- Integration: dry-run inventory scripts only.
- E2E: reviewer can trace each planned driver to a source path or `[ASSUMED]`.

## Risk Assessment

| Risk | LxI | Mitigation |
|---|---|---|
| Wrong reference path | LxM | Use observed `D:\Cellos\.references`; re-check only if user supplies another tree. |
| License contamination | MxH | Block copy until SPDX/BOM pass; GPL/unknown concept-only. |
| Stale roadmap prose | HxM | Code inventory wins over status prose. |

## Security Considerations

No code import before license and trust review. Vendor binary SDKs stay out of tree.

## Backward Compatibility

No runtime behavior changes.

## File Ownership

Planning/docs only; no overlap with implementation phases.

## Rollback

Undo by deleting this plan artifact. Irreversible part: none, unless later docs are updated and published.

## Assumptions

- None for reference location: `D:\Cellos\.references` was directly observed and supersedes the typed `D:\Cellos.references`.

## Deviation Log

- 2026-08-19: `.claude/rules/haily-coding.md` is absent in this checkout; used `docs/code-standards.md` and `docs/system-architecture.md` as the local standards sources.
- 2026-08-19: WSL `rg` resolves to an unusable WindowsApps binary here; inventory was re-run with `find`, `grep`, and PowerShell `Select-String` instead, matching the phase guidance.
- 2026-08-19: Corrected stale Phase 01 claims after repo re-check: current docs already prove the RPi3 mini-UART and MMC/SDHCI physical lanes, while the local `D:\Cellos\.references\Redox` checkout is only the Redox cookbook/build system.
- 2026-08-19: Split BCM IRQ evidence more narrowly: AUX legacy IRQ29 for mini-UART RX is physically proven, but broader BCM local/legacy-controller qualification stays blocked.

## Evidence

- Inventory and stage gates: `./reports/driver-inventory-and-stage-gates.md`
- License/provenance BOM: `./reports/driver-source-license-bom.md`
- Future evidence ledger template: `./reports/driver-evidence-ledger-template.md`
- Physical UART lane anchor: `docs/baremetal/load-cellos.md:107-114`
- Physical AUX IRQ29 anchor: `docs/baremetal/load-cellos.md:114-119`, `docs/project-changelog.md:266-271`
- Physical MMC lane anchor: `docs/project-changelog.md:230-234`

## Next Steps

Phase 02 consumes the inventory and locks shared driver substrate edits.
