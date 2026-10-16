# Native development handoff r5 — N05 resource ownership preparation

## Decision requested and current status

**DRAFT FOR REVIEW; NOT AUTHORIZED.** This packet prepares the exact source, caller migration, and verification boundaries for tranche **N05**. It does not authorize editing frozen syscall numbers, wire protocol renumbering, releasing locks, or promoting CP1/CP2. Existing scoped consents (Clusters 1–5), N02-A, N02-B, N03, and N04 implementation records remain unchanged; D05 and D06 wire/carrier changes remain pending Law-1 CP1.

Approval of N05 requires an explicit user instruction naming both this Markdown candidate's SHA-256 and its companion JSON SHA-256, appended to `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl`.

## Tranche scope: N05 resource ownership: receiver ceilings, issuer-owned resource descriptors, grant/pin/device lifetime, and clean VFS/NET cutover

Integrating owner: controlling P01 owner. WIP: one tranche.
N05 establishes capability-directed resource ownership across Cells, implementing issuer-owned VFS/NET resource descriptors, immutable receiver namespace ceilings, atomic receive policy reconciliation, and grant/pin/device revocation quarantines under D05/D06.

### Exact allowed paths for N05 implementation (upon approval)

- `cells/services/vfs/src/dirs.rs`: VFS directory handle tables, issuer/holder binding, and generation validation.
- `cells/services/vfs/src/dirs/bind.rs`: directory bind operations, subtree isolation, and namespace sealing.
- `cells/services/vfs/src/namespace.rs`: per-Cell namespace resolution, root/service bindings, and ungranted path rejection.
- `kernel/src/task/dir_inherit.rs`: kernel directory capability inheritance and atomic rollback.
- `kernel/src/task/domain_grant.rs`: domain grant descriptors, subrange attenuation, and rights enforcement.
- `kernel/src/memory/pin.rs`: memory frame pinning, concurrent user tracking, and quarantine before reuse.
- `tests/unit_cspace_smoke.rs`: host harness tests for N05 resource ceilings, namespace isolation, and grant lifetime.
- `CHANGELOG.md`: bounded tranche record.
- `.agents/261011-1200-cell-contract-authority/validation.md`: append observed evidence.

Read-only consumers: `kernel/src/cell/cspace.rs`, `kernel/src/cell/cap_registry.rs`, `kernel/src/task/launch.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/tcb.rs`, `kernel/src/task/async_ipc.rs`, `kernel/src/task/syscall.rs`, `kernel/src/memory/cell_quota.rs`, `hal/arch/x86/src/x86_64/context.rs`, `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/dir_handles.rs`, `kernel/src/loader.rs`, `kernel/src/policy.rs`.
Forbidden: new public syscall numbers, wire protocol renumbering, kernel-duplicate VFS ACL tables, raw-pointer compatibility fallbacks, and unapproved manifest v3 formats.

### Invariants and cutover requirements

1. **Issuer-Owned VFS/NET Descriptors:** Resource authority records remain generation-bound to issuer and holder. The kernel attests caller and provenance; no kernel duplicate VFS ACL table or raw-pointer bypass is permitted.
2. **Immutable Per-Cell Namespace Ceilings:** Paths and service names resolve strictly within granted roots and services; no ambient global path lookup is admitted as authority. Namespace sealing and revoked-parent propagation are enforced.
3. **Atomic Receive Policy & Ceiling Validation:** Post-start capability receive must be bounded by the recipient's predeclared ceiling and receive policy. Grants exceeding the ceiling or from unauthorized sources fail closed atomically.
4. **Grant/DMA/Memory Revoke & Quarantine:** Revocation linearizes admission cutoff first, retains pinned/mapped objects, and waits for CPU TLB shootdown and device quiescence before frame reuse. Missing DMA acknowledgement quarantines resources.
5. **No Early Frame or Reference Reuse:** Dead owner memory and grant descriptors remain charged until confirmed release; no ticket or slot index wrapping.

### Required verification and gates

- Host harness: verify issuer/holder generation binding, namespace path isolation, receive ceiling enforcement, and grant revocation quarantine.
- Kernel compilation: clean compile of `cellos-kernel` on `x86_64-unknown-none` (both default and `test-hooks`).
- Boot smoke: verify clean boot of `vicell-x86.iso` in VMware Workstation with storage mounts and VFS intact.
- Document consistency: `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0.

### External prerequisites remaining blocked

- Public D05/D06 wire carrier formats and new syscall entrypoints remain pending Law-1 CP1.
- Physical IOMMU/VT-d hardware qualification and bare-metal deployment remain uncertified.
- Production issuer credentials and cluster placement remain unassigned.
