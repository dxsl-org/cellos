---
phase: 3
title: "Remove Temporary Probe After Hardware Pass"
status: pending
priority: P2
effort: "1h"
dependencies: [2]
tier: fast
---

# Phase 3: Remove Temporary Probe After Hardware Pass

## Overview

After hardware proof, remove the temporary TGE diagnostic from `trap.rs` while keeping any durable comments that explain why board-rpi3 enters EL1h.

## Requirements

- Functional: remove `par_s1e0r_tge0`, temporary HCR.TGE toggle assembly, and `FS3 par_tge0` logging.
- Functional: retain normal fault logging that is useful beyond this investigation.
- Non-functional: do not remove the final boot fix, and do not alter page-table execute policy.

## Architecture

Input is the hardware pass report from Phase 2. Transform is source cleanup in `trap.rs`. Exit is a clean board-rpi3 boot image without the temporary diagnostic.

## Assumptions

- Claim: The temporary probe is not needed after the EL1-drop fix is proven on real hardware.
  Confidence: high
  How to verify: Phase 2 report contains a passing real RPi3 boot log.

## Related Files

- Modify: `hal/arch/arm/src/aarch64/trap.rs`
- Create/modify: `.agents/debug/<timestamp>-rpi3-el1-drop-report.md`

## Implementation Steps

1. Confirm Phase 2 hardware evidence exists and explicitly passes.
2. Remove only the temporary TGE-toggle diagnostic additions from `probe_uncategorized_el2_fault`.
3. Keep the ordinary uncategorized-fault probe only if still needed for this bring-up lane; otherwise reduce to the pre-diagnostic baseline separately after approval.
4. Rebuild board-rpi3 and rerun at least QEMU raspi3b smoke.
5. If practical, boot hardware once more to confirm cleanup did not mask a regression.

## Success Criteria

- [ ] `grep -n "par_tge0\\|hcr_tge0" hal/arch/arm/src/aarch64/trap.rs` returns no matches.
- [ ] Board-rpi3 release build passes after cleanup.
- [ ] Hardware pass evidence from Phase 2 remains linked in the final report.

## Test Matrix

- Static: grep confirms temporary probe removal.
- Build: board-rpi3 release cargo build.
- Integration: QEMU raspi3b smoke.
- E2E: optional hardware reboot after cleanup.

## Backwards Compatibility

Cleanup removes diagnostic-only output. It should not affect runtime ABI, boot sequence, task scheduling, or page permissions.

## Risk Assessment

- Low likelihood x Medium impact: removing too much trap logging hides future board failure. Mitigation: remove only TGE-specific fields unless separately approved.
- Low likelihood x Medium impact: cleanup accidentally changes fault handling. Mitigation: keep edits localized and grep diff before build.
- Rollback: revert the `trap.rs` cleanup hunk. Irreversible part: none.

## Security Considerations

Cleanup reduces privileged register logging, which is preferable for non-diagnostic builds.

## Deviation Log

None.
