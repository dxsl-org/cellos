---
phase: 2
title: "Reconcile Fast-IPC Consequents"
status: complete
priority: P1
effort: "2h"
dependencies: [1]
tier: thinking
---

# Phase 2: Reconcile Fast-IPC Consequents

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Apply D1's consequences without re-litigating D1: Spec 17 is the model of record, direct fast-IPC is Tier-1-only, and any old 2-3-cycle claim must be marked aspirational or tied to a runnable implementation.

## Requirements

- Functional: Specs must stop comparing an unrunnable direct path against measured prior-art numbers.
- Non-functional: Do not change Law-1 ABI or runtime dispatch without a separate implementation plan.

## Architecture

Observed current state:
- `docs/specs/00-context.md:185` still says Cellos vtable dispatch is `~2-3 cycles`.
- `docs/specs/16-rustc-tcb.md:235` keeps prior-art comparison prose without the measured D1 caveat.
- `kernel/src/fast_ipc.rs:116` through `kernel/src/fast_ipc.rs:121` says non-PIE cells still fall back until loader JUMP_SLOT patching exists.
- `kernel/src/fast_ipc.rs:147` through `kernel/src/fast_ipc.rs:160` derives caller identity from scheduler state before disabling interrupts.
- `kernel/src/fast_ipc.rs:170` exposes `resolve_export`, and `kernel/src/loader/reloc.rs:18` defines `R_RISCV_JUMP_SLOT`, but the loader import-resolution path must be verified before code deletion.

Data flow after the future rewrite: caller enters through a loader-resolved shared dispatch entry, kernel derives identity from current task state, handler receives an attested caller, VFS returns data or denial, and fallback remains `sys_send`/`sys_recv`.

## Assumptions

- **Claim:** No loader code calls `resolve_export` today.
  **Confidence:** high
  **How to verify:** grep `resolve_export` in `kernel`, `libs`, and `cells` immediately before implementation.

## Related Files

- Modify: `docs/specs/00-context.md`
- Modify: `docs/specs/16-rustc-tcb.md`
- Modify: `docs/specs/17-ipc-wire-contract.md`
- Maybe modify/delete: `kernel/src/fast_ipc.rs`
- Maybe modify/delete: `kernel/src/loader/reloc.rs`
- Maybe modify: `libs/ostd/src/fast_ipc.rs`

## Implementation Steps

1. Replace absolute 2-3-cycle wording with measured D1 round-trip numbers plus direct-dispatch target caveat.
2. Add Spec 21-style status anchors: current fallback path is implemented; Tier-1 direct path is accepted/partial unless runtime-verified.
3. Audit `resolve_export` and `R_RISCV_JUMP_SLOT`; if unused scaffold is retained, mark it `absent`/future in docs and comments instead of describing it as live.
4. Do not delete scaffold unless all references are verified absent and `cargo check -p vicell-kernel` stays green.

## Success Criteria

- [x] No normative doc claims a shipped 2-3-cycle Cellos IPC path.
- [x] Spec 17 remains the governing fast-IPC contract.
- [x] Any retained scaffold is explicitly marked not live.
- [x] `cargo check -p vicell-kernel` is run if code changes.

## Security Considerations

Caller identity is the load-bearing security boundary for direct calls. Any code change must preserve scheduler-derived identity and denial on unattributed calls.

## Risk Notes

- Likelihood medium, impact high: deleting apparently dead relocation scaffold could break a future PIE branch or hidden test. Mitigation: prefer documentation/status correction unless build and grep prove safe deletion.
- Rollback: revert docs/code edits. Irreversible part: none.

## Deviation Log

None.
