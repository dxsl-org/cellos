# ADR: Native filesystem for `/srv`

**Date**: 2026-06-11 | **Status**: Accepted, amended 2026-08-01 | **Authors**: Cellos core team

> Activation policy was amended by [ADR 0002](../decisions/0002-phased-srv-redoxfs-activation.md):
> G1 functional availability is active; G2 production qualification remains gated.

---

## Decision

Use **RedoxFS** (MIT licence, approximately 10 K LOC Rust) as the `/srv` backend. Do not
implement a custom CoW B-tree.

RedoxFS is available for G1/QEMU proof-of-function through the generic block Driver Cell.
That availability does not by itself qualify the filesystem for G2 production use.

---

## Context

G1 robot/embedded systems persist data to:

- `/data` — littlefs on NAND/eMMC P4, power-loss safe;
- `/mnt/sd` — FAT32 on SD card P1, PC-interoperable, without journaling;
- `/srv` — RedoxFS on P5 when a formatted block device is available.

G2 server/PC workloads additionally need:

- copy-on-write behavior for snapshots;
- checksums for silent-data-corruption detection;
- crash recovery without an external fsck dependency;
- large-file and large-directory support;
- measured NVMe and real-hardware qualification.

---

## Options Evaluated

| Option | LOC | Licence | CoW | Checksum | `no_std` | Verdict |
|--------|-----|---------|-----|----------|----------|---------|
| **RedoxFS port** | ~10 K | MIT | Yes | Yes | Adapted | Chosen |
| Custom CoW B-tree | ~30-40 K | N/A | Yes | Yes | Yes | Rejected: filesystem correctness scope is unjustified |
| TFS | ~5 K | MIT | Yes | Yes | Partial | Rejected: upstream inactive since approximately 2018 |
| ext4 FFI | ~300 K | GPL-2 | No | Yes | No | Rejected: licence and FFI footprint |
| BtrFS FFI | ~200 K | GPL-2 | Yes | Yes | No | Rejected: licence and FFI footprint |

RedoxFS provides the required CoW/checksum model in pure Rust with a smaller adaptation
surface than a new filesystem or a large C FFI stack.

---

## Architecture

```text
/srv
  -> RedoxFsBackend
  -> VicellDisk
  -> blk_router
  -> registered BLOCK_DRIVER (VirtIO-BLK or NVMe)
```

`VicellDisk` maps each 4 KiB RedoxFS block to eight 512-byte sectors on P5. `blk_router`
owns transport selection so filesystem code does not depend on a specific device class.
The mount may be attempted during VFS construction; a missing or unformatted P5 degrades
to unavailable operations without crashing the service.

Relevant implementation anchors:

- `cells/services/vfs/src/manager.rs` — `/srv` mount registration;
- `cells/services/vfs/src/backend_redoxfs.rs` — RedoxFS backend and degrade behavior;
- `cells/services/vfs/src/disk_redoxfs.rs` — P5 block translation;
- `cells/services/vfs/src/blk_router.rs` — block Driver Cell routing.

---

## Activation and Qualification

### G1 functional availability — active

RedoxFS remains mounted on P5. RV64 QEMU with VirtIO-BLK verifies basic filesystem
operations, graceful degradation without a disk, and persistence across two boots in
`tests/integration/tests/redoxfs-srv.rs`.

These tests prove behavior, not G2 latency or hardware readiness.

### G2 production qualification — pending

All four gates are required:

1. An automated RedoxFS-on-NVMe write/read/persistence integration test passes.
2. The `<100 us` filesystem-read target is defined with workload, cache state, and sample
   statistic, then measured by a repeatable harness.
3. The same path is validated on an approved, purchasable real-hardware target.
4. P5 authorization is explicitly decided: introduce a dedicated capability in a
   versioned manifest ABI, or accept VFS `AccessTable` path authorization as permanent.

No document may claim G2 `/srv` production readiness until all four gates have evidence.

---

## Partition Authorization

Manifest v1 stores flags in `u8`, so the previously proposed
`MANIFEST_FLAG_PART_SRV` bit 8 cannot be added without a versioned ABI change. P5 is
currently reachable by the VFS and `/srv` access is enforced through its `AccessTable`.

This is accepted for G1 functional use only. ADR 0002 leaves the permanent G2 boundary as
an explicit qualification decision rather than silently accepting the current co-grant.

---

## Consequences

- Working and tested `/srv` behavior remains available before G2 hardware qualification.
- VirtIO-BLK evidence cannot be presented as NVMe or real-hardware evidence.
- The block transport stays centralized and reusable across VirtIO-BLK and NVMe.
- `StubBackend` is no longer the active `/srv` backend, though it may remain as a utility
  for intentionally unsupported mounts.
- RedoxFS upstream divergence or a failed qualification gate may trigger a new ADR, but
  does not retroactively invalidate the G1 proof-of-function role.

---

## Related Decisions

- [ADR 0002: Activate `/srv` RedoxFS in two qualification stages](../decisions/0002-phased-srv-redoxfs-activation.md)
- [Spec 09: VFS and filesystems](09-vfs.md)
