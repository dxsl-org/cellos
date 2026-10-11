---
phase: 5
title: "Service resources, namespaces and buffer/device revocation"
status: pending
priority: P1
tier: thinking
dependencies: [2, 3, 4]
---

# P05 — Resource ownership across Cells

## Deliverable and requirements

AC-02/04/05/13. Capability-directed VFS/NET/device access, explicit namespace binding and a safe optional local data plane. Shared semantics do not mean a shared authoritative ACL table across kernel and services.

## Related files / ownership

- `cells/services/vfs/src/manager/`, VFS directory/file/namespace resource tables, `kernel/src/task/dir_inherit.rs`.
- `libs/api/src/abi/dir_handles.rs`, `libs/api/src/services/`, `libs/ostd/src/clients/`, `libs/ostd/src/service.rs`.
- `kernel/src/task/{domain_grant.rs,drivers.rs}`, `kernel/src/memory/pin.rs`, device/IOMMU resource owners located through their existing registry.
- `libs/ostd/src/ring_channel.rs`, `libs/api/src/services/ring_channel.rs` and known benchmark consumers.
- Native VFS/NET driver/service clients enumerated in P01; related Spec 09c/17/22 amendments.

## Implementation steps

1. Keep VFS/NET authoritative resource records generation-bound to issuer and holder. Kernel attests caller and transfer provenance; never validate a VFS handle by duplicating its ACL into kernel CSpace. Method authority is explicit even when the endpoint is reachable.
2. Implement immutable per-Cell namespace ceiling and explicitly granted binds. Names resolve only within granted roots/services; no ambient global path lookup as authority. Preserve namespace sealing and revoked-parent propagation.
3. Amend spawn-only directory-grant rules only through exact approved protocol. Support bounded explicit post-start receive within fixed ceiling and receive policy; validate issuer/source/grantee atomically. Reacquisition after restart obtains new grants, not widened old rights.
4. Migrate named VFS/NET/lifecycle callers to granted endpoints/resources, including privileged administrative methods and driver registration. Remove redundant access paths only after census closure; do not make a generic service gateway that exercises its own rights for arbitrary caller input.
5. Adopt unfinished kernel repair grant/DMA/VFS lifetime work under this phase with owner handoff. Linearize admission revoke first, retain pinned/mapped objects, complete CPU shootdown/safe-root and device quiescence, then recycle. Missing acknowledgement quarantines; domain DMA remains refused until its own proof passes.
6. Grant descriptors bind owner, offset, length, rights and lease/pin lifetime; snapshot and validate before use. Deny foreign/stale/out-of-range buffers and permission escalation. Charge all retained pages/descriptors to bounded ledgers through dead owners.
7. Either put existing shared-ring consumer behind an authorized local channel with generation/revoke/correlation/death semantics or remove that benchmark-only path from supported SDK. Do not ship unsafe raw-address connect as a capability. Maintain SPSC-only constraints or select an explicitly proven MPMC implementation for multithread use; no hidden allocation on fast path.
8. Keep stream objects native and capability-owned. Retain kernel ring/lifetime mechanism where Spec 15 requires it; Unix fd/errno/popen behavior moves in P07. No blanket pipe deletion based on its name.

## Verification / success criteria

- [ ] Two Cell domains with same resource names see only explicitly granted roots/endpoints; caller cannot trick a privileged service into opening an ungranted path or executing an admin method.
- [ ] Spawn and post-start grant matrices cover source revoke/death, target retirement, ceiling widening, namespace seal, object-slot reuse and parent-revoke descendants.
- [ ] Actual x86 domain pair reads/writes allowed slices; RO writes, out-of-bounds, foreign grant, stale provider and revoke are refused/fault-contained; old frames are not reused before acknowledgements.
- [ ] Device/IOMMU model plus exact hardware proof for any enabled device mapping. Without hardware evidence leave that profile denied, not a fake successful driver reset.
- [ ] Optional ring shows correlated concurrent use within its declared producer/consumer constraints and bounded peer-death behavior. If removed, all consumers migrated and no native fastpath claim remains.
- [ ] Existing VFS functionality and network client runtime smoke pass through the new grants, not legacy fallback.

## Security and rollback

Revocation cannot reclaim mappings while a CPU/device still uses them; timeouts do not prove quiescence. C/C++ stays hardware isolated. Roll back with remote/device ingress closed and cold boot known-safe image; external writes and revoked identities cannot be undone. Returning to raw-pointer channels or broad legacy launch authority is forbidden.

## Assumptions and deviation log

Service tables differ and require resource-specific migrate/reopen rules. Domain DMA safety is not implied by CPU mapping tests. Hardware absence gates that profile but does not remove implementation obligations. Deviation log: none.
