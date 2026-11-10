---
phase: 3b
title: "Single image recipe per architecture (RV64, AArch64)"
status: pending
priority: P3
effort: "4–6d"
dependencies: [3]
tier: medium
checkpoint: false
---

# Phase 03b: Single image recipe per architecture

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Phase 03](phase-03-build-inputs.md) (split from its step 6 by owner decision 2026-11-10) · `gen_disk.ps1` · `scripts/gen-disk-ci.sh` · `scripts/gen-disk-aarch64-ci.sh` · `scripts/assemble-x86-image.py` (the model)

## Overview

Replace the hand-mirrored RV64 PowerShell/bash recipes and the AArch64 bash recipe with one Python assembler per architecture driven by a declarative cell list, keeping the `.ps1`/`.sh` files as thin callers.

## Key Insights

- `gen-disk-ci.sh` says it mirrors `gen_disk.ps1` "one-for-one" and "byte-for-byte in sync"; CI runs both. Drift is recorded in `ci.yml` comments.
- Partition literals are already pinned by `scripts/check-disk-layout-literals.py` (Phase 03), so the remaining risk is cell-set and flavour drift, not layout drift.

## Requirements

- Functional: `scripts/assemble-image.py --arch {riscv64,aarch64}` reading `scripts/images/<arch>.toml`; identical per-partition SHA-256 to the current recipes for the same inputs; flavour assertions (production vs test-hooks) preserved.
- Non-functional: no change to image contents or layout; Windows entry point keeps working through `pwsh` calling Python.

## Assumptions

- **Claim:** `gen_disk.ps1` and `gen-disk-ci.sh` build the same RV64 cell set today.
  **Confidence:** low
  **How to verify:** diff the two cell lists before porting.

## Related Code Files

- Create: `scripts/assemble-image.py`, `scripts/images/{riscv64,aarch64}.toml`
- Modify: `gen_disk.ps1`, `gen_disk_rpi3.ps1`, `scripts/gen-disk-ci.sh`, `scripts/gen-disk-aarch64-ci.sh`, `scripts/check-disk-layout-literals.py` (sites move)

## Implementation Steps

1. Diff cell lists and flavours of the three recipes; record differences as decisions.
2. Port RV64; prove per-partition byte identity against `gen-disk-ci.sh` on the same inputs.
3. Port AArch64 the same way.
4. Reduce old scripts to callers; update CI and the literal check.

## Todo List

- [ ] Cell-list diff
- [ ] RV64 port, byte-identical
- [ ] AArch64 port, byte-identical
- [ ] Callers + CI

## Success Criteria

- [ ] Old and new recipes produce identical per-partition SHA-256 for RV64 and AArch64.
- [ ] RV64 integration lanes using `disk_v3.img` (`launch-profile`, `tier2-fault-isolation`) pass on the new recipe.

## Risk Assessment

Wrong cell flavour on a disk makes lanes fail for image reasons; the byte-identity proof is the control. Rollback: revert; scripts are build-time only.

## Security Considerations

Signing steps must stay identical (dev key only); no new key handling.

## Next Steps

None; independent of later slices.

## Deviation Log

None.
