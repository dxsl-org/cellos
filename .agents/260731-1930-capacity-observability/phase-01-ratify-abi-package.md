---
phase: 1
title: "Ratify the ABI package"
status: completed
priority: P1
effort: "0.5h"
dependencies: []
tier: thinking
---

# Phase 1: Ratify the ABI Package

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs.

## Overview

Freeze the exact A2/A3 contract before implementation. This phase performs no production edit and remains a hard stop until both Law 1 confirmations are recorded.

## Requirements

- Functional: present the unchanged ABI package twice and obtain two explicit user confirmations.
- Non-functional: no ABI edit, staging, or commit may occur between confirmations; a revised value resets the sequence.

## Architecture

The package to confirm is:

1. Cell-spawn OOM uses return `-2`; legacy/generic failure remains `-1`; SpawnFromMem `10`, SpawnFromPath `12`, SpawnPinned `16`, and SpawnFromElf `238` do not change.
2. `ViSyscall::MemInfo = 243`; allowlist bit `56`, opt-in only.
3. `#[repr(C)] ViMemInfoV1` is 32 bytes: `total_frames`, `used_frames`, `free_frames`, `page_size`, all `u64`.
4. MemInfo ABI: `a0=out_ptr`, `a1=out_len`; return bytes written or the existing error sentinel.
5. The reported benchmark basis is allocator-committed physical frames. It may fail `<10 MB`; implementation must report that result honestly.

Confirmation 1 asks the user to approve all five items. Immediately before editing `libs/api/src/abi/syscall.rs`, repeat them verbatim as confirmation 2. “Continue” or approval of the plan alone is not sufficient.

## Assumptions

None — the gate follows `docs/code-standards.md:12-16` and the frozen-ABI warning in `libs/api/src/abi.rs`.

## Related Files

- Read: `docs/code-standards.md`
- Read: `libs/api/src/abi.rs`
- Modify: none

## Implementation Steps

1. Re-read the live stable ABI files and verify opcode 243 and bit 56 remain unassigned.
2. Present the exact package and obtain explicit confirmation 1.
3. Ensure no ABI-affecting edit has occurred.
4. Repeat the exact package, identify it as Law 1 confirmation 2, and obtain explicit confirmation 2.
5. Record both confirmations in the phase Deviation Log before Phase 2 starts.

## Success Criteria

- [x] Two explicit confirmations approve the same five-item package.
- [x] `git diff -- libs/api libs/types` shows no task edit before confirmation 2.
- [x] No ABI value changed between confirmations, so no restart was required.

## Security Considerations

The user explicitly accepts global memory telemetry as an opt-in cross-cell side channel.

## Risk Notes

An ambiguous “yes” attached only to the general plan is not an ABI confirmation. Undo: no code exists to revert. The confirmation record itself is not reversible, but any later package change invalidates it.

## Deviation Log

- **Confirmation 1:** user replied `xác nhận` after the unchanged five-item ABI package was
  presented.
- **Confirmation 2:** the same package was repeated immediately before implementation; user
  replied `xác nhận` again. No ABI-affecting edit occurred between the confirmations.
