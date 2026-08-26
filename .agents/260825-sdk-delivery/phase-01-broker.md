# Phase 01 — Broker K1 and LAN Beacon

## Context Links
`docs/project-roadmap.md:148-150`; `docs/specs/14-distributed.md`; `docs/network-api.md`; `cells/services/net-broker/src/{main,transport,beacon}.rs`.

## Overview
Make existing K1 and beacon primitives operational through the broker startup/runtime path.

## Key Insights
K1 loading and beacon crypto already exist. UDP receive prepends six source bytes, channel setup ignores bind/join errors, and reboot replay lacks re-baselining.

## Requirements
Fail closed on unavailable/invalid K1; preserve IPC ABI; authenticate before peer-table mutation; preserve bounded buffers and monotonic timing rules.

## Architecture
Broker-owned state holds K1-derived gossip key, identity, static keypair, NetRef-backed beacon channel, and peer table. No split mutable statics.

## Related Code Files
`main.rs`, `transport.rs`, `beacon.rs`, `transport/tests.rs`, `beacon` tests.

## Implementation Steps
1. Correct channel response and UDP-envelope handling.
2. Correct reboot replay behavior and test it.
3. Load K1 before all network paths and initialize beacon only after successful derivation.
4. Add focused startup/codec tests.

## Todo List
- [ ] Inspect runtime ownership and test seams.
- [ ] Implement K1/beacon correctness changes.
- [ ] Run focused broker tests.

## Success Criteria
A valid K1 alone enables authenticated beacon setup; short/missing K1 and failed socket setup leave no usable channel; valid prefixed datagrams decrypt; reboot epochs re-baseline replay state.

## Risk Assessment
Network service response details are protocol-sensitive. Preserve source-prefix parsing and fail closed on all unexpected responses.

## Security Considerations
Never log or transmit K1. Never trust beacon plaintext before AEAD verification. Do not infer peer static keys from the current beacon frame.

## Next Steps
Phase 02 requires a user-selected relay registration/enrollment protocol.