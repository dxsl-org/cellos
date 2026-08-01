# ADR 0001: Readiness notifications remain Draft until implemented

- **Date:** 2026-08-01
- **Status:** Accepted
- **Decision:** D8
- **Scope:** Spec 17 §10 readiness notification ABI

## Context

[Spec 17 §10](../specs/17-ipc-wire-contract.md) assigned `NET_READY = 0x11`,
`REACTOR_WAKE = 0x12`, and `NetRequest` variants 17/18 while marking the section
Ratified. None of those mechanisms exists; the current request enum ends at variant 16.

[Spec 21](../specs/21-documentation-architecture.md) forbids planned or unbuilt
behavior in Ratified sections and assigns implementation state to generated status.
Law-1 confirmation #1 from 2026-07-23 remains historical evidence, but confirmation
#2 is still required immediately before a future ABI edit.

## Decision Drivers

- Keep Ratified specifications aligned with implemented behavior.
- Prevent byte-0 ABI collisions without claiming runtime support.
- Preserve the reviewed design and its confirmation history.
- Avoid expanding D8 into readiness-engine implementation work.

## Considered Options

### Mark §10 Draft and retain the reservations

This complies with Spec 21, represents the implementation state accurately, and
protects `0x11` and `0x12`. The cost is an unavailable but reserved ABI surface that
must be reviewed again before implementation.

### Keep §10 Ratified while it remains unimplemented

This avoids document edits, but was rejected because it violates Spec 21 and conflates
design approval with an available implementation contract.

### Release the slots and delete §10

This would free two wire values, but was rejected because it creates collision risk,
discards reviewed design history, and makes later restoration harder to audit.

### Implement readiness notifications now

This would justify Ratified status, but was rejected as an unrelated scope expansion
that would mutate the ABI without the required immediate Law-1 confirmation #2.

## Decision

Spec 17 §10 is **Draft / reserved-but-unbuilt**. Byte-0 values `0x11` and `0x12`
remain reserved for `NET_READY` and `REACTOR_WAKE`. No enum, protocol, or runtime
implementation changes are authorized by this decision.

Before implementing the design:

1. Obtain Law-1 confirmation #2 immediately before the ABI edit.
2. Implement and verify the complete readiness contract.
3. Add implementation/test anchors and generated status evidence.
4. Ratify §10 only after that evidence exists.

## Consequences

- Ratified documentation no longer claims unavailable readiness behavior.
- Implementations must not emit or accept the proposed variants 17/18 yet.
- New protocol allocations must not reuse `0x11` or `0x12`.
- The first confirmation remains history, not standing authorization for a later edit.

## Links

- [Spec 17: Cell IPC Wire Contract](../specs/17-ipc-wire-contract.md)
- [Spec 21: Documentation Architecture](../specs/21-documentation-architecture.md)
- [Project Roadmap](../project-roadmap.md)
- [Decision Docket 260730](../../.agents/reports/decision-docket-260730.md)
