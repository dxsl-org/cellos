---
phase: 3
title: "Enforce and verify x86 boundaries"
status: completed
priority: P1
effort: "1h"
dependencies: [2]
tier: medium
---

# Phase 3: Enforce and Verify x86 Boundaries

## Overview

Extend static gates, CI, and architecture documentation, then execute the full relevant verification matrix.

## Requirements

- Functional: board matrix includes the seventh x86 descriptor and x86 target build.
- Functional: boundary checks reject regression of x86 static facts into board or arch integration files.
- Non-functional: QEMU and physical-hardware evidence remain clearly separated.

## Architecture

Static scripts enforce ownership cheaply; target builds and BIOS/UEFI QEMU lanes prove integration without replacing physical validation.

## Assumptions

- **Claim:** Local WSL has QEMU/xorriso and usable Limine assets for the BIOS witness.
  **Confidence:** medium
  **How to verify:** check commands/assets before running the runtime gate; report unavailable UEFI firmware separately.

## Related Files

- Modify: `scripts/check-board-configs.sh`
- Modify: `scripts/check-hal-boundaries.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/code-standards.md`
- Modify: `docs/system-architecture.md`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/project-changelog.md`

## Implementation Steps

1. Add the x86 board assets/build lane and ownership guards.
2. Update CI target installation and project documentation.
3. Run formatting, host tests, all board configs, x86 checks/build, BIOS QEMU, and UEFI marker when firmware is available.

## Success Criteria

- [x] Formatting, host tests, boundary script, and seven-board matrix pass.
- [x] x86 release kernel and curated cells build pass.
- [x] BIOS QEMU reaches the shell; UEFI result is evidenced or explicitly host-gated.
- [x] Physical x86 remains hardware-gated.

## Security Considerations

CI must continue to reject unvalidated firmware fallbacks and secret files.

## Risk Notes

QEMU/coverage may destabilize WSL; use bounded invocations and preserve honest evidence if the service fails.

## Deviation Log

None.
