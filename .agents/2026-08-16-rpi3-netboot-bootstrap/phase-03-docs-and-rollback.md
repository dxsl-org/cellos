---
phase: 3
title: "Document Operator Workflow"
status: pending
priority: P2
effort: "1h"
dependencies: [2]
tier: fast
---

# Phase 3: Document Operator Workflow

## Overview

Update bare-metal docs so future board-rpi3 sessions can choose full SD image boot or bootcode-only SD plus PC-hosted DHCP/TFTP.

## Requirements

- Functional: document when to use netboot: fast kernel iteration after SD/firmware bootstrap is known good.
- Functional: document exact admin boundary for static IP/firewall and exact rollback steps.
- Functional: link generated logs and backup manifests from validation.
- Non-functional: keep current SD-flash instructions intact.

## Architecture

Docs consume the validated scripts and evidence from Phases 1-2, then expose two operator flows: recoverable local SD boot and faster TFTP kernel iteration.

## Assumptions

- Claim: `docs/baremetal/load-cellos.md` is the right user-facing entry point for this workflow.
  Confidence: high
  How to verify: current doc already owns SD flashing and hardware setup instructions.

## Related Files

- Modify: `docs/baremetal/load-cellos.md`
- Modify: `tools/rpi3-netboot/README.md`

## Implementation Steps

1. Add a "Fast RPi3 netboot lane" section after existing SD flashing instructions.
2. Explain bootcode-only SD bootstrap and why OTP is explicitly out of scope.
3. Document the required TFTP root files and backup-before-change rule.
4. Document Windows NIC static IP/firewall changes and rollback commands.
5. Document expected server log sequence and where logs are stored.
6. Add a short troubleshooting table for no DHCP, no TFTP, wrong kernel hash, and UART silence.

## Success Criteria

- [ ] Docs name both workflows and when to choose each.
- [ ] Docs include rollback steps before any destructive SD rewrite instruction.
- [ ] Docs state no OTP programming is part of this lane.
- [ ] Docs reference `tools/rpi3-netboot/` scripts and log locations.

## Test Matrix

- Docs lint/manual read: steps are order-safe and do not ask for irreversible action without rollback.
- Operator dry-run: follow docs through preflight without RPi3 powered.
- Evidence cross-check: docs examples match actual Phase 2 log names/fields.

## Backwards Compatibility

Existing manual SD flashing instructions remain. Netboot is an additional lane, not a replacement.

## Risk Assessment

- Medium likelihood x Medium impact: docs make admin network changes look safer than they are. Mitigation: explicit admin boundary, exact NIC alias/ifIndex, rollback first.
- Low likelihood x Medium impact: docs encourage bootcode-only SD before backup. Mitigation: backup rule appears before conversion steps.
- Rollback: revert doc changes or mark netboot lane experimental. Irreversible part: none.

## Security Considerations

Docs must call out unauthenticated TFTP and isolated-cable-only DHCP.

## Deviation Log

None.
