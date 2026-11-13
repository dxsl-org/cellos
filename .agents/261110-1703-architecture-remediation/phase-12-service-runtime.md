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
- [x] Proto modules, literals gone
- [x] `serve` + first three services
- [x] VFS liveness and block path
- [x] Remaining services migrated
- [x] Trust store moved
- [x] Net refactor
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

- **Decision (owner, S6 approval):** multi-sector block requests are done by plan `261112-1035` (`OP_READ_RUN`) and dropped here; input, net-broker and silo stay hand-rolled (CP1 §8).
- **Surprise (step 2):** `lua` did not compile since Phase 11 (`CallOutcome` from `ServiceRef::call`); it is excluded from the workspace clippy runs, so only its CI host test would have caught it. Fixed in its own commit (`1c1ca38ae`).
- **Decision (step 2):** the literal gate (`scripts/check-service-wire-literals.py`) forbids the proto-owned *names* plus three literal shapes. `OP_SNAPSHOT`/`OP_STATUS` are not in the name list: other wires (silo, net-broker bench oracle) own the same names with different meanings. Test-local synthetic bytes and non-IPC formats stay.
- **Decision (step 2):** `DESTROY_SURFACE` replies are a status (0 destroyed, 1 refused), not the boolean `REPLY_TRUE/FALSE`; `proto::compositor` names them `DESTROY_OK/DESTROY_REFUSED`.
- **Decision (step 3, W2):** a timed `Recv` reports expiry as the generic error sentinel, never `Ok(0)`, so a timeout cannot be read as a message from the kernel (sender 0). Allowed under the `Recv` bit; no new bit.
- **Decision (step 3):** `serve` landed with what config, kms and ai need (attested receive, heartbeat, idle ticks, unattributed messages). Owner-watch policy and deferred replies arrive with the services that use them (steps 5, 7).
- **Decision (step 3):** ai sessions are keyed by the attested `(cell_id, generation)`, not the sender tid, so a client's threads share their sessions.
- **Re-pin (step 3):** `check-multi-source-wait-law1-digests.sh` counted 4 allowlist exports in `runtime.rs`; `service_entry!` was deleted, so 3.
- **Surprise (lanes after step 3):** `hypha-local-ai` fails with `DENY launch edge: caller=hypha-core … target=/bin/llm-gateway`. Loader and catalog files are unchanged on this branch; logged as pre-existing, not fixed here.
- **Decision (step 4):** the VFS heartbeat is armed only once the request loop starts (`liveness::arm`); mount and discovery run unarmed, each block request bounded. Block requests re-arm it before each exchange.
- **Decision (step 4):** a stalled device stays refused until its route names another provider; there is no timed retry. A driver that overruns its own 30 s budget is broken; the restart path (new provider) clears the stall.
- **Surprise (step 4, `FsError`):** an unreadable `/srv` trust store now answers `ssh_trust::UNAVAILABLE` where it answered `UNKNOWN` (the stat error is now distinguishable). The trust store moves to kms in step 6.
- **Left (step 4):** CellosFS folds an indeterminate write into its own `IoError`, so `/srv` reports `Io`, not `Indeterminate`; separating them needs a `libs/cellos-fs` change, out of this phase.
- **Evidence (step 4):** the frozen-driver witness `a_frozen_block_driver_does_not_hang_vfs_x86` fails on `e676d0ca9` with only the fault hook and test added (VFS stops answering: `vcat /tmp/alive.txt` → `cannot read`) and passes on the branch. `nvme_09c_nic_owner_survives_replacement_x86` failed once inside the 12-test sequence (fault marker not seen in 20 s) and passed alone; logged as a flake.
- **Decision (step 5):** owner watches moved into `ostd::service::owners` and `serve` (`WatchPolicy::{Off, OnRequest, Every}`); services keep only what they reclaim. VFS keeps a set of watched `(cell, generation)` keys for reconcile; the compositor keeps its per-owner creator tids.
- **Surprise (step 5):** under `AppContext` the supervisor's owner-exit relay never fired: it required `data.len() == 9`, but the payload is always the whole zero-padded window. On `serve` the frame is the 9-byte prefix followed by zero padding, so init's notice now reaches the compositor.
- **Surprise (step 5):** `supervisor --features hostile-backend-recovery` does not compile on `main` (`sender_has_exact_name` no longer exists). Pre-existing and feature-gated; not fixed here.
- **Decision (step 5):** the supervisor's power settle keeps dropping other messages instead of deferring them (CP1 §3 said "defers"): every sender except the storage stack is frozen and the machine powers off when the window ends, so a deferred message would never be served.
- **Decision (owner, step 6):** the canonical x86 images did not ship `/bin/kms` (headless: never; boot config: only with `ui`), so moving the trust store would have left `cellbox ssh` without a trust provider. The owner chose to ship kms in every non-board image: `service-kms` joins the headless profile (desktop inherits it), `generate-boot-config.py` selects `/bin/kms` whenever not a board, the phase 09 and Tier 3 ssh lanes add it in every mode, and the assembler's signing pin (`X86_CANDIDATE_RECIPE_SHA256`) and signed-binary set follow.
- **Decision (step 6):** kms trust queries are staged like the SSH key import (Begin/Chunk/Commit, 96-byte chunks); admission needs an attested caller with the outbound-connect right. Staged queries: 4 open, 1 per cell, 10 s expiry. `save_owned`/`stage_and_publish` were deleted from VFS (only the trust store used them).
- **Left (step 6):** the VFS allowlist is unchanged (`prepared_cwd`, `blk_router` and `grant_read` still use its entries). kms has no `SetTimer`, so the input session re-query retries without a delay, as VFS did. Not checked: whether VFS's per-cell quota lets kms grow the store to its 1 MiB limit.
- **Decision (step 7):** net watches owners inside its handler (`WatchPolicy::Off` + `ostd::service::owners`) so admission still precedes the watch; serve gained `on_empty_poll` for net's NET_RX park. `NetState<'s>` borrows the socket storage from `run()`'s frame, which never returns. Deferred replies keep `ipc::current()` (serve does no receive before `on_request`).
- **Evidence (step 7):** RV64 guest `wget` of a 96 KiB body, 7 timed runs after a warm-up, two boots each: median 1853 ms before (`2e9779814`) and 1853 ms after (`144e794e6`); RV network subset 7/7 after.
- **Surprise (step 7):** `service-net --features qemu-dev-https` alone does not build at HEAD (two TLS flavours); checked with `--no-default-features --features qemu-dev-https,verified-tls`.
- **Review (CP2):** independent review PASS_WITH_RISK; 3 medium and 4 low findings fixed (`fix(services): review findings…`), see [cp2-service-wire.md §5](cp2-service-wire.md). The stall latch gained a 60 s probe window (`STALL_RETRY_TICKS`), so a slow driver recovers.
- **Surprise (CP2 lanes):** the phase 09 ssh lane's ceiling gate (`cellbox-ceiling-diff.py`) failed on Phase 11's approved deltas (`LookupServiceBound`, `SendGather` added; `LookupService`, `Reply` retired) since Phase 11; the gate now lists them.

