---
phase: 2
title: "Copy-Out Compatibility Adapter"
status: completed
priority: P1
effort: "2d"
dependencies: [1, 3]
tier: medium
---

# Phase 02: Copy-Out Compatibility Adapter

## Overview

Characterize every caller and migrate only the shell pioneer. Phase 03 has completed the approved scoped frame lifetime for VFS copy-out; all other callers remain on their current paths until their later phase and authority gates.

## Requirements

- Functional: characterize every facade/caller; migrate the shell pioneer only after Phase 03 approves scoped frame lifetime without changing `VfsRequest`.
- Non-functional: no new round trip for known-size whole-file grant reads; reads have explicit byte bounds.
- Stop-first: Phase 03 approval is recorded; any required `VfsRequest`, syscall, allowlist, wire, or manifest change still stops this phase.

## Architecture

Approved future copy-out data flow:
`caller path -> Stat or known buffer limit -> scoped frame lifetime -> ReadFileGrant/ReadAsync+Poll -> bounded bytes -> lifetime close/ack -> Vec/buffer`.
`ReadFileGrant` copies `min(file_len, max, grant_len)` and replies only after copy (`libs/api/src/services/ipc.rs:79`, `cells/services/vfs/src/dispatch.rs:292`). `Poll` caps reply data to 480 bytes because full-frame payloads broke postcard replies (`cells/services/vfs/src/dispatch.rs:194`).

Phase 03's bridge, hardened after the 2026-08-26 signing review, atomically installs an exact request lease under the matching grant-table lock for registered VFS `GrantSlice`; owner death quarantines leased frames, and matching `Send` or holder death releases them. Phase 02 may rely on that bridge but must not broaden it.

Transport matrix to prove before edits:
- Shell: its syscall allowlist contains grant operations, but `cmd_fs.rs` has no grant adapter yet; Phase 02 implements and proves it as the copy-out pioneer.
- Spawn: preserve the existing synchronous `ReadFileGrant` path unchanged.
- Lua, WASM, Hypha, net-broker, and HTTPD: no assumed GrantCap; use future bounded handle chunks or stop for a separately approved allowlist change.
- HTTPD/net-tools HTTPD: characterize current `ReadAsync+Poll` 480-byte truncation, wildcard receive, and empty-on-error behavior before migration.
- `VfsClient`: characterize the current mismatch—sends `GetFile`, expects `Data`, while VFS returns `DataPtr`—before changing downstreams.

## Assumptions

- Claim: current production code can tolerate copy into `Vec<u8>` for these whole-file reads.
  Confidence: medium
  How to verify: run shell/Lua/WASM/hypha/net-broker smoke tests under QEMU after migration.

## Related Files

- Modify: `libs/ostd/src/clients/vfs.rs`
- Modify: `cells/tools/shell/src/cmd_fs.rs`
- Characterize/read-only until Phase 04/05: `cells/runtimes/lua/src/bindings_vfs.rs`, `cells/tools/wasm/src/main.rs`
- Modify only if facade signature forces it: `cells/apps/hypha/tools/fs/src/main.rs`, `cells/services/net-broker/src/identity.rs`, `cells/services/net-broker/src/transport.rs`
- Modify tests: `cells/tests/vfs-test/src/grant_io.rs`, `tests/integration/tests/vfs-quota.rs`
- Characterize/modify later: `cells/services/httpd/src/handlers.rs`, `cells/services/httpd/src/net_ipc.rs`, `cells/tools/net-tools/src/bin/httpd.rs`
- Read-only allowlist proof: `cells/runtimes/lua/src/main.rs`, `cells/tools/wasm/src/main.rs`, `cells/apps/hypha/tools/fs/src/main.rs`, `cells/services/net-broker/src/main.rs`, `cells/services/httpd/src/main.rs`

## Implementation Steps

1. Add characterization tests for the broken `VfsClient` contract and each caller's maximum file size, syscall allowlist, sender mask, truncation, and error handling.
2. Define size discovery and read as the same attested/authorized VFS path; denial, missing data, malformed reply, timeout, and truncation are typed errors.
3. After Phase 03 approval only, migrate shell to bounded `ReadFileGrant`; preserve spawn's existing synchronous grant path until the approved lifetime covers caller death during copy.
4. Keep Lua/WASM/Hypha/net-broker/HTTPD on their current path until Phase 04 supplies bounded chunks; do not add GrantCap in this plan without a separate checkpoint.
5. Add negative tests proving a migrated caller never retries `GetFile/DataPtr` after any error.

## Success Criteria

- [x] Phase 03 approval is recorded before any new `ReadFileGrant` caller lands.
- [x] Shell has no `DataPtr` consumer and never falls back; every other caller has a recorded transport/limit/authority row and explicit later phase.
- [x] Existing `ReadFileGrant` clamp/nonzero/deny-after-seal markers still pass in QEMU RV64.
- [x] Facade and HTTPD characterization exposes typed mismatch/truncation errors rather than empty success.
- [x] No edit under `libs/api/` or `libs/types/`.

## Security Considerations

This phase reduces raw SAS authority but remains path-addressed. Sealed cells must still receive `Err(3)` for path-addressed requests (`libs/api/src/services/ipc.rs:162`, `cells/services/vfs/src/dispatch.rs:50`).

## Risk Notes

- Risk Medium x High: hidden size assumptions cause truncation. Mitigation: explicit max, stat-before-grant where needed, fail loud on truncation.
- Risk Medium x Medium: facade migration masks errors as empty data. Mitigation: typed `Err` propagation per Spec 17 fail-loud rules (`docs/specs/17-ipc-wire-contract.md:173`).
- Rollback: characterization-only work is deleted from tests/reports; post-approval caller edits restore callers to old `GetFile` facade. Irreversible part: none if no ABI/docs ratification lands.
- Stop condition: any required `VfsRequest`/syscall/allowlist change, any new `ReadFileGrant` caller before Phase 03 approval, or inability to prove grant-copy lifetime cannot outlive the caller; move to the relevant checkpoint first.

## Deviation Log

- 2026-08-09: Phase 02 is blocked behind Phase 03. The previous synchronous-copy safety assumption failed because `ReadFileGrant` does not pin/lease the target frame across caller death/preemption.
- 2026-08-09: Phase 03 completed after explicit semantic approval with RV64 QEMU lifetime proof and standard/security review PASS. Phase 02 started on the ABI-stable shell pioneer only.
- 2026-08-09: Phase 02 completed after shell caller regression fixes, sequential RV64 QEMU revalidation, and production/security review PASS. Phase 04 remains separately gated.
- 2026-08-26: Repository signing review found a High CWE-416 surprise in the
  Phase 03 bridge: `GrantSlice` copied grant fields, dropped the PAGE/REG table
  lock, and only then published the VFS lease, allowing concurrent
  `GrantFree`/`GrantUnregister` to remove and recycle the frames in between.
  The blocker was corrected without ABI or wire changes by holding the matching
  grant-table lock through validation and exact VFS lease publication; VFS
  grant writes now copy through the safe bounded OSTD adapter instead of a raw
  pointer.
