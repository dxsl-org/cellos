---
phase: 5
title: "Migrate Callers And Retire DataPtr"
status: in-progress
priority: P1
effort: "2d"
dependencies: [4]
tier: medium
---

# Phase 05: Migrate Callers And Retire DataPtr

## Overview

Move remaining callers to handle reads and retire `GetFile/DataPtr` under a second Law 1 removal checkpoint. This phase closes the raw-pointer escape.

## Requirements

- Functional: remove production dependency on `GetFile/DataPtr`; migrate path users in a controlled order.
- Non-functional: backward compatibility remains until final removal approval; no old fast `GetFile` restoration.

## Architecture

Migration data flow:
`facade/direct caller -> open dir/file handle -> bounded read -> owned bytes -> close`.
The old flow remains only in not-yet-migrated callers during staged rollout. A migrated caller never retries it; rollback requires an explicit code/operator rollback to the prior phase.

Retirement state:
`Deprecated -> NoProductionCallers -> Law1RemovalApproved -> Disabled/Deleted -> VerifiedAbsent`.
Fast IPC may stay only if exact auth parity holds and it no longer serves `GetFile`; otherwise disable/delete the fast `GetFile` handler (`kernel/src/fast_ipc.rs:141`, `cells/services/vfs/src/main.rs:125`).

## Assumptions

- Claim: the Phase 01 multi-symbol inventory plus source, generated, and embedded scans covers every production read path.
  Confidence: medium
  How to verify: repeat the full inventory and reconcile every hit against `scout-report.md` before disablement.

## Related Files

- Modify: `libs/ostd/src/clients/vfs.rs`
- Modify: `libs/ostd/src/fs.rs`
- Modify: `cells/apps/hypha/tools/fs/src/main.rs`
- Modify: `cells/services/net-broker/src/identity.rs`
- Modify: `cells/services/net-broker/src/transport.rs`
- Modify: `cells/tools/shell/src/cmd_fs.rs`
- Modify: `cells/runtimes/lua/src/bindings_vfs.rs`
- Modify: `cells/tools/wasm/src/main.rs`
- Modify: `cells/services/httpd/src/handlers.rs`, `cells/services/httpd/src/net_ipc.rs`, `cells/tools/net-tools/src/bin/httpd.rs`
- Modify after Law 1 checkpoint B: `docs/specs/17-ipc-wire-contract.md`; verify `libs/api/src/services/ipc.rs` remains discriminant-stable and reserved.
- Modify/disable if needed: `kernel/src/fast_ipc.rs`, `libs/ostd/src/fast_ipc.rs`, `cells/services/vfs/src/main.rs`

## Implementation Steps

1. Re-inventory `GetFile`, `DataPtr`, `get_file_ptr`, `ReadAsync`, `Poll`, `ReadAt`, `ReadFileGrant`, fast handlers, helpers, generated/embedded callers, tables, and facade/downstream surfaces.
2. Migrate `VfsClient` downstreams first so Hypha/net-broker inherit the safer path without bespoke changes.
3. Migrate shell, then Lua, WASM, service HTTPD/net-tools HTTPD, each using its approved dir bootstrap and bounded chunks; denial/truncation/decode/timeout/service-death never falls back.
4. Preserve spawn `ReadFileGrant` until handle read proves `/bin` overlay and boot parity.
5. Add CI/test grep gate for no production `DataPtr` decode outside VFS/tests/bench.
6. Verify disablement still matches the recorded Law 1 #1/#2 approval: service behavior off, ABI slots reserved, no deletion/renumbering; otherwise stop for a new pair.
7. Disable serving `GetFile` and its fast arm only after all callers pass; retain public variants/discriminants as reserved until a separately approved major ABI window.

## Success Criteria

- [ ] No production code outside VFS/tests/bench matches `VfsRequest::GetFile` or `VfsResponse::DataPtr`.
- [ ] Shell `cat`/Lua file read/WASM loader/Hypha fs/net-broker config reads pass QEMU RV64 smoke.
- [ ] `GetFile` after seal and fast-path unknown caller remain denied/disabled.
- [ ] `ReadFileGrant` spawn path still boots until a separate approved replacement is proven.
- [ ] Law 1 checkpoint B is recorded before behavior is disabled; public discriminants remain stable.

## Security Considerations

Identity-less fast IPC is invalid. If exact kernel-attested identity and auth parity cannot be preserved, future implementation disables/deletes the fast path instead of reviving old direct `GetFile`.

## Risk Notes

- Risk High x High: one leftover raw-pointer caller keeps Tier-2 blocker alive. Mitigation: grep gate plus QEMU smoke per caller class.
- Risk Medium x High: removing `GetFile` too early breaks boot or loader paths. Mitigation: spawn `ReadFileGrant` last; Law 1 removal checkpoint B.
- Risk Low x Medium: fast path fallback masks message path failures. Mitigation: require same auth parity or disable it.
- Rollback: explicitly restore the prior source/config revision; never runtime-fallback on an individual read. Irreversible part: externally published ABI behavior cannot be silently redesigned.
- Stop condition: any caller still requires `DataPtr` for correctness or performance.

## Deviation Log

- 2026-08-10: Caller migration and static inventory are complete. Fresh RV64
  QEMU shell and net-tools HTTPD static/dynamic lanes pass after the queued-IPC
  nested-lock fix. Retirement remains stopped before checkpoint B because Lua,
  WASM, Hypha, service HTTPD, and the remaining net-broker runtime evidence are
  not yet complete. See
  `reports/phase-05-caller-migration-execution.md`.
