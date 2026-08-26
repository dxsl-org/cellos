---
title: "Phase 08 - Failure Injection and Isolated Two-Node Gates"
status: pending
priority: P1
effort: 3
depends_on: [07]
owner: "validation"
---

# Phase 08 - Failure Injection and Isolated Two-Node Gates

## Context Links

- Test matrix: `reports/test-matrix.md`
- Success gates: `reports/success-gates.md`

## Overview

Priority P1. Prove relay and LAN behavior in isolated two-node oracles before any COMPLETE status can be claimed.

## Key Insights

- D38 requires a two-node remote-call oracle before COMPLETE.
- This phase still does not claim hardware evidence unless a later run actually captures it.
- Failure injection must cover security, transport, queue, deadline, and restart behavior.

## Requirements

- Functional: relay-only oracle, LAN-direct oracle, fallback oracle, duplicate request oracle, wrong-key oracle, restart epoch oracle, path-transition reordering oracle, stale response oracle, broker restart in-flight oracle, half-open TCP oracle, silent-drop relay oracle, and log-redaction oracle.
- Non-functional: isolated topology, deterministic logs, no public relay dependency, no hardware claim unless separately run.

## Architecture

Data flow: oracle driver starts node A and node B -> configures exports and relay/direct paths -> sends typed remote request -> captures broker counters/logs -> injects failure -> verifies status mapping.

## Related Code Files

- Future owner phase: integration test harness and scripts only after approval.
- Future owner phase: no product code outside broker/test modules.

## Implementation Steps

1. Define relay-only topology.
2. Define LAN-direct topology.
3. Define failure injection matrix.
4. Define retained evidence format.
5. Define COMPLETE gate checklist.
6. Define QEMU-only versus hardware evidence labels in retained logs.

## Todo List

- [ ] Define oracle command line.
- [ ] Define expected logs.
- [ ] Define evidence retention path.
- [ ] Define non-hardware vs hardware evidence labels.
- [ ] Define path reorder, stale response, broker restart, half-open TCP, and silent-drop oracle cases.
- [ ] Define log scan that fails on payloads, PSKs, private keys, and full request bodies.

## Success Criteria

- Relay-only two-node request succeeds.
- LAN-direct two-node request succeeds after relay path exists.
- Direct failure falls back to relay or returns `Indeterminate` based on deadline/retry class.
- Wrong node/key cannot complete a request.
- Broker restart, stale response, half-open TCP, path switch, and relay silent-drop attempts produce the expected typed status and retained evidence.
- Normal and oracle logs contain no payloads, PSKs, private keys, or full request bodies.

## Risk Assessment

- Risk: oracle passes in QEMU but not hardware. Likelihood medium, impact medium. Mitigation: label evidence class; do not claim hardware.
- Risk: nondeterministic timing. Likelihood high, impact medium. Mitigation: relative deadlines with generous oracle bounds.

## Security Considerations

Negative tests include wrong key, wrong cluster, replayed request id, stale boot epoch, oversized frame, and forbidden log content.

## Rollback

If oracle fails, keep product status partial and do not proceed to rollout. Disable remote exports by default.

## Next Steps

Proceed to rollout docs handoff and Candidate A contingency decision.
