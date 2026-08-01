# ADR 0002: Activate `/srv` RedoxFS in two qualification stages

- **Date:** 2026-08-01
- **Status:** Accepted
- **Decision:** D10
- **Scope:** `/srv` filesystem activation and G2 storage qualification

## Context

[ADR 09b](../specs/09b-vfs-native-fs-adr.md) originally required three conditions
before implementing RedoxFS: an NVMe Driver Cell with block read/write tests, an
available C930/P870 board, and a measurable `<100 us` read-latency target. Until then,
`/srv` was to remain a `StubBackend`.

Two days later, RedoxFS was implemented on MBR P5 and tested through VirtIO-BLK. The
storage stack subsequently converged on a transport-neutral path:
`RedoxFsBackend -> VicellDisk -> blk_router -> BLOCK_DRIVER`. The registered block
driver may be VirtIO-BLK or NVMe. The implementation has correctness, graceful-degrade,
and two-boot persistence coverage, but no RedoxFS-on-NVMe performance qualification.

The named C930/P870 boards are not purchasable hardware, and no storage-read benchmark
definition exists. Applying the original gate literally would therefore remove working
G1/QEMU functionality without bringing G2 qualification closer.

## Decision Drivers

- Keep architecture documents aligned with tested runtime behavior.
- Preserve useful proof-of-function coverage before production hardware exists.
- Keep G2 claims gated by end-to-end NVMe, hardware, and latency evidence.
- Make the transport-neutral block Driver Cell path the architecture of record.
- Avoid an ABI expansion until the P5 capability boundary is explicitly designed.

## Considered Options

### Phased activation

Retain RedoxFS for G1/QEMU functional use and separately gate G2 production
qualification. This preserves tested behavior while preventing QEMU correctness tests
from being presented as hardware-readiness evidence.

### Restore `StubBackend` until every original trigger passes

This would restore literal compliance with the 2026-06-11 text, but was rejected because
it removes a working, gracefully degrading backend and ties all progress to unavailable
C930/P870 hardware.

### Treat the existing implementation as fully G2-qualified

This avoids further work, but was rejected because VirtIO-BLK correctness tests do not
measure NVMe persistence, real-hardware behavior, or the `<100 us` latency target.

### Replace the generic router with an NVMe-specific adapter

This would match the original diagram, but was rejected because it duplicates transport
selection already centralized in `blk_router` and would couple the filesystem to one
device class unnecessarily.

## Decision

`/srv` RedoxFS has two distinct states:

1. **G1 functional availability — active.** RedoxFS remains mounted on P5 through the
   generic block Driver Cell path. VirtIO-BLK tests establish filesystem behavior,
   graceful degradation, and persistence; they do not establish G2 readiness.
2. **G2 production qualification — pending.** A target may claim this only when all of
   the following evidence exists:
   - an automated RedoxFS-on-NVMe write/read/persistence integration test;
   - a specified filesystem-read benchmark, including workload, cache state, sample
     statistic, and the `<100 us` threshold;
   - results from an approved, purchasable real-hardware target;
   - an explicit decision for P5 partition authorization, either a dedicated manifest
     capability in a versioned ABI or permanent reliance on VFS path authorization.

The architecture of record is:

```text
/srv
  -> RedoxFsBackend
  -> VicellDisk
  -> blk_router
  -> registered BLOCK_DRIVER (VirtIO-BLK or NVMe)
```

The VFS may attempt the mount during construction and degrade to unavailable operations
when P5 is missing or unformatted. It does not need to wait for an NVMe-specific
`ServiceReady` notification.

## Consequences

- The current RedoxFS mount is authorized as G1 proof-of-function behavior.
- Documentation must not describe `/srv` as a mounted stub.
- G2 storage-readiness claims remain blocked despite the existing backend.
- C930/P870 are no longer hard-coded qualification boards; hardware must instead be
  purchasable, approved, and representative of the target block path.
- P5 remains controlled through the VFS `AccessTable` until its capability boundary is
  decided; co-granting it with LFS is not silently treated as the final design.

## Links

- [ADR 09b: Native filesystem for `/srv`](../specs/09b-vfs-native-fs-adr.md)
- [Spec 09: VFS and filesystems](../specs/09-vfs.md)
- [Project Roadmap](../project-roadmap.md)
- [Decision Docket 260730](../../.agents/reports/decision-docket-260730.md)
- [D10 analysis](../../.agents/reports/d10-srv-backend-analysis-260801.md)
