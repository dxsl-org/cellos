---
phase: 1
title: "Freeze Decision Contract"
status: completed
priority: P1
effort: "1d"
dependencies: []
tier: thinking
---

# Phase 01: Freeze Decision Contract

## Overview

Freeze the target contract before implementation: no new `GetFile` consumers, bounded copy-out is migration-only, file handle + bounded read is the endpoint, and revocable `ReadGrant` is deferred.

## Requirements

- Functional: write the local decision note/tests inventory before any code change.
- Non-functional: no `libs/api/`, syscall, manifest, or wire-format edit in this phase.

## Architecture

Data flow under review:
`caller -> VFS GetFile/ReadFileGrant/ReadAt -> response DataPtr/Data/grant -> caller`.
The frozen target replaces raw pointer exit with bounded bytes. Current `VfsRequest` includes `GetFile`, `ReadGrant`, `ReadFileGrant`, and dir-cap arms (`libs/api/src/services/ipc.rs:27`). `GetFile` returns `DataPtr` (`libs/api/src/services/ipc.rs:202`) after authorization (`cells/services/vfs/src/dispatch.rs:55`).

State baseline:
`Unseen -> Admitted -> Sealed -> Open -> InFlightRead -> Closed/Reaped`. Existing sealed path refusal happens before dispatch (`cells/services/vfs/src/dispatch.rs:46`), and durable ownership must be `Caller { cell, generation }` (`cells/services/vfs/src/caller.rs:22`, `docs/specs/17-ipc-wire-contract.md:430`).

## Assumptions

- Claim: `docs/coding.md` is absent, so `docs/code-standards.md` is the applicable local standards source.
  Confidence: high
  How to verify: `ls docs/code-standards.md && ! ls docs/coding.md`

## Related Files

- Modify: none in product code.
- Create/Modify: `.agents/260809-0922-sas-lbi-revocable-vfs-access/phase-*.md`
- Read-only references: `docs/code-standards.md`, `docs/specs/17-ipc-wire-contract.md`, `docs/specs/18-cell-trust-tiers.md`, `docs/specs/19-hardware-isolation-layers.md`

## Implementation Steps

1. Record the three-option decision exactly: A copy-out first, B file-handle endpoint, C `ReadGrant` deferred.
2. Add a gate covering `GetFile`, `DataPtr`, `get_file_ptr`, fast VFS serving, and hand-written decode/copy helpers; no new producer or consumer outside characterized compatibility code.
3. Freeze hard stops: no Tier 2, async DMA, generic reactor, `RecvScatter`, SMP, identity-less fast IPC, raw-pointer revocation, or Midori reopen.
4. Inventory the complete read surface: `GetFile`, `DataPtr`, `get_file_ptr`, `ReadAsync`, `Poll`, `ReadAt`, `ReadFileGrant`, fast handlers, facade/helper names, generated/embedded callers, service HTTPD, and net-tools HTTPD.
5. Freeze caller order: characterize `VfsClient` and HTTPD first; shell copy-out pioneer; directory bootstrap; Hypha/net-broker, Lua, WASM, HTTPD handle migration; spawn `ReadFileGrant` last.
6. Confirm file ownership for later phases; all implementation phases are serial unless a later project manager splits non-overlapping tests/docs.

## Success Criteria

- [x] Plan contains the exact 3-option comparison and selected target/tactic/deferred mechanism.
- [x] `scout-report.md` inventory is refreshed with HTTPD, helper, allowlist, kernel-cap, VFS-handle, and fast-path surfaces before implementation.
- [x] Every later phase has explicit dependencies, rollback, stop conditions, and file ownership.
- [x] No phase claims QEMU or hardware evidence as already produced by planning.

## Evidence

- `reports/phase-01-execution.md` records the phase summary, verification baseline, and scope notes.
- `reports/harness/verification.json` is the tester baseline pass set.
- `reports/harness/review-decision.json` records the reviewer `PASS`.
- `reports/harness/adversarial-validation.json` records the harness validator `PASS`.

## Security Considerations

`DataPtr` cannot be made revocable after handoff; the only security-valid path is to stop new use and remove or translate existing use before Layer B (`docs/specs/17-ipc-wire-contract.md:449`, `docs/specs/18-cell-trust-tiers.md:155`).

## Risk Notes

- Risk High x High: false closure by treating existing `ReadGrant` as production. Mitigation: require real opener evidence before any `ReadGrant` endpoint.
- Rollback: revert plan-only files. Irreversible part: none.
- Stop condition: any requested ABI/syscall change appears before Law 1 checkpoint A, or any caller/transport remains unclassified.

## Deviation Log

- 2026-08-09: Verification used direct file existence checks (`ls`, `[ -f ]`) instead of `test -f` because the shell reported a false positive for missing `docs/coding.md` on this WSL/UNC path. Evidence outcome stayed the same: `docs/coding.md` absent, `docs/code-standards.md` present.
