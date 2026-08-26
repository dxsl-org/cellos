# Phase 03: QEMU evidence

## Context Links

- [allowlist](phase-01-supervisor-allowlist.md)
- [handoff fixture](phase-02-domain-handoff-fixture.md)
- `scripts/qemu-native-domain-test.sh`

## Overview

Capture only the final execution evidence for the private-root handoff.

## Key Insights

Configured hart count is insufficient: `migration` must require a terminal
emitted only after hart 0 has resumed the worker and verified its domain tuple.

## Requirements

- Keep separate one-hart `switch,sas-fastpath` and two-hart `migration` runs.
- Require exact, distinct terminals and reject panics, unclassified faults,
  wrong hart availability, duplicate requested cases, and wrong harts.
- Preserve raw/normalized logs, QEMU version, command, ELF digest, firmware,
  feature tuple, and `NON_QUALIFYING_QEMU` labels.

## Architecture

`isolated image → exact QEMU tuple → host-owned terminal assertion → scoped log
artifact`; the guest never controls case selection or acceptance semantics.

## Related Code Files

`scripts/build-native-domain-test-ci.sh`, `scripts/qemu-native-domain-test.sh`,
`tests/integration/tests/native-domain-qemu.rs`.

## Implementation Steps

1. Update the migration expectation to its distinct terminal.
2. Run strict feature-off and feature-on RV64 builds.
3. Run one-hart `switch,sas-fastpath` and two-hart `migration`.
4. Preserve failures as blocked evidence; do not touch ledger/qualification state.

## Todo List

- [ ] Verify exact one-hart terminals.
- [ ] Verify the cross-hart terminal.
- [ ] Record non-promotional outcome.

## Success Criteria

The runner accepts no primary-hart switch marker as migration evidence.

## Risk Assessment

QEMU TCG does not prove physical DMA, IOMMU, larger-hart behavior, admission,
or qualification.

## Security Considerations

Host parsing rejects broad failure terminals and exact case/hart mismatches.

## Next Steps

Return raw evidence to the app-tier steward without status promotion.
