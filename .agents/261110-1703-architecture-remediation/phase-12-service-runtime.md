---
phase: 12
title: "Shared service runtime and wire modules"
status: in-progress
priority: P1
effort: "9–13d"
dependencies: [11]
tier: medium
checkpoint: false
---

# Phase 12: Shared service runtime and wire modules

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-13, Medium userspace](../reports/architecture-review-261110.md) · `libs/ostd/src/{dispatch,runtime,service}.rs` · `cells/services/vfs/src/{main,dispatch,blk_router,backend,trust}.rs` · `cells/services/net/src/service-runtime.rs` · `cells/services/{config,kms,compositor,ai,supervisor}/src/main.rs` · `libs/api/src/services/ipc.rs` · [Spec 09](../../docs/specs/09-vfs.md) · [Spec 12](../../docs/specs/12-reliability.md)

## Overview

One service loop in the SDK that owns attested receive, owner-death, reply routing and liveness; one wire module per service imported by server, clients and kernel; and VFS no longer able to hang forever on its block driver.

## Key Insights

- Reviewer-cited: `ostd::dispatch::run_service` (`dispatch.rs:73-77`) and `service_entry!` (`runtime.rs:413`) have zero users; seven hand-rolled loops; owner-death reclaim implemented three times; only net and input heartbeat.
- VFS: masked `sys_recv` with no timeout on the block provider (`blk_router.rs:80`, `blk_router/router.rs:171`), one 512-byte sector per exchange, whole-file reads under `GLOBAL_VFS` (`dispatch.rs:108,133`), never heartbeats.
- Opcodes are re-declared in clients, tools and the kernel (`acpi/sci.rs:40` hard-codes a supervisor byte); errors are bare `u8` with collisions (net 0xFF = failure and EOF).

## Requirements

- Functional: `ostd::service::serve(handler)` with hooks `on_request`, `on_owner_death`, `on_idle`, built-in heartbeat and attested identity parsing; config, kms, ai migrated first, then supervisor, compositor, vfs, net; one `no_std` proto module per service with opcodes, lengths and a `#[repr(u8)]` error enum; literal copies deleted; VFS block receive bounded and mapped to the existing `BlockOutcome::Indeterminate`; multi-sector block requests; `FsBackend` returns `Result<_, FsError>`; SSH trust store leaves VFS (to kms or its own cell); net loop state becomes a `NetState` struct with oracles behind one no-op probe type.
- Non-functional: wire values unchanged (byte tests); Law-1 CP1/CP2 because the proto modules live under `libs/api`.

## Architecture

```
ostd::service::serve ─ recv(attested) ─► parse CallerIdentity ─► handler.on_request
        │                                    └ owner table ─► handler.on_owner_death
        ├ reply via IpcReply (typed outcome from Phase 11)
        └ idle tick ─► heartbeat ─► handler.on_idle
libs/api/src/services/proto/<service>.rs  ◄─ server, ostd client, tools, kernel
```

## Assumptions

- **Claim:** net's non-blocking loop (bounded driver waits, deferred replies) fits the hook model without regressing packet latency.
  **Confidence:** low
  **How to verify:** migrate net last; compare the existing x86 bench lane before/after.
- **Claim:** multi-sector block requests need a driver-side protocol change in every block driver cell.
  **Confidence:** medium
  **How to verify:** read `blk_router/requests.rs` and one driver's dispatch; if so, version the request and keep single-sector as fallback.
- **Claim:** VFS's upward call into input (`trust/first.rs:33-40`) is the only bootstrap-order inversion.
  **Confidence:** medium
  **How to verify:** grep `lookup` calls in `cells/services/vfs/src`.

## Related Code Files

- Create: `libs/ostd/src/service/serve.rs`, `libs/api/src/services/proto/{vfs,net,supervisor,nic,tls,compositor,config,kms}.rs`, `cp1-service-wire.md`, `cp2-service-wire.md`
- Modify: the seven service main loops; `cells/services/vfs/src/{blk_router.rs,blk_router/router.rs,blk_router/requests.rs,dispatch.rs,backend.rs,paths.rs}`; `libs/ostd/src/{tls.rs,clients/**}`; `cells/tools/sys-tools/src/bin/{hotswap,shutdown}.rs`; NIC drivers' `dispatch.rs`; `kernel/src/acpi/sci.rs`; `cells/services/net/src/{service-runtime,handlers,interface,tls_wire}.rs`
- Delete: `libs/ostd/src/dispatch.rs`, `service_entry!`, per-service owner-watch copies, `cells/services/vfs/src/trust.rs` + `trust/` (moved)

## Implementation Steps

1. CP1: proto-module layout, error enums (values pinned to today's bytes), block request versioning, trust-store move. Owner confirmation 1.
2. Extract proto modules with byte tests pinning current values; switch servers, clients, tools and the kernel constant; delete literals.
3. Implement `serve`; migrate config → kms → ai; delete the dead SDK loop.
4. VFS: bounded block receive + heartbeat first (smallest fix), then multi-sector, then `FsError`.
5. Migrate supervisor, compositor, vfs to `serve`.
6. Move the trust store; VFS no longer calls a later service.
7. Net: `NetState` refactor, probe type, migrate to `serve`.
8. CP2.

## Todo List

- [x] CP1
- [ ] Proto modules, literals gone
- [ ] `serve` + first three services
- [ ] VFS liveness and block path
- [ ] Remaining services migrated
- [ ] Trust store moved
- [ ] Net refactor
- [ ] CP2

## Success Criteria

- [ ] Byte tests: every opcode and error value equals its pre-phase literal.
- [ ] x86 fixture: with the block driver frozen (not killed), VFS returns `Indeterminate` within the bound and keeps serving in-memory requests; init's watchdog sees VFS heartbeats — fails before this phase.
- [ ] Grep finds no service opcode literal outside `libs/api/src/services/proto/`.
- [ ] Cell builds for x86_64 and riscv64; service host tests pass; x86 boot lane and `x86-c2c` pass.

## Risk Assessment

- A timed-out write is indeterminate by nature; the bound must not turn into silent data loss — the outcome is surfaced, never retried automatically.
- Service migrations are per service and individually revertable. Proto-module values are pinned, so no wire residue.

## Security Considerations

Attestation, owner-death cleanup and authorisation-before-effect are implemented once; this removes the per-service omission class (e.g. `ai` still unattested).

## Next Steps

Phase 13 moves hot-swap envelopes onto `serve` or removes them.

## Deviation Log

None.
