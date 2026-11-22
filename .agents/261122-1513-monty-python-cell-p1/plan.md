---
title: "Monty P1: usable Tier 2 `python` cell"
description: "Turn the P0 monty-smoke witness into /bin/python and /bin/python-net: VFS-backed OS calls, REPL, module import, enforced memory/time limits, a configurable PAL heap, and a hostile test lane"
status: pending
priority: P2
effort: 10d
branch: main
tags: [feature, backend]
blockedBy: [261122-1036-monty-thin-port-p0]
blocks: []
created: 2026-11-22
---

# Monty P1: usable Tier 2 `python` cell

## Overview

P0 proved Monty runs in an `UNTRUSTED` Tier 2 rust-std cell on x86_64 (commit `943552a03d`).
P1 ships a real runtime: `/bin/python [script.py]` (REPL without an argument) and `/bin/python-net`,
the same signed binary on a network-client edge, following the `lua` / `lua-net` precedent (annex S2-N D-N3).
Scope was fixed by the user on 2026-11-22: cell, memory limits, larger PAL heap, net split and hostile suite. Kernel debt from P0
and upstream PRs are **out** (see below). x86_64 gets runtime witnesses; riscv64/aarch64 must link.

Codebase map: [scout report](./scout-report.md).

## Phases

| Phase | Name | Status | Checkpoint |
|-------|------|--------|------------|
| 1 | [Per-cell PAL heap size + governance re-pin](./phase-01-pal-heap-size.md) | pending | |
| 2 | [Per-path cell quota and large stack for the python paths](./phase-02-python-path-quota-and-stack.md) | pending | ✓ |
| 3 | [Counting allocator and Monty resource limits](./phase-03-memory-limits.md) | pending | |
| 4 | [`python` runtime cell: VFS OS calls, script/REPL, imports](./phase-04-python-runtime-cell.md) | pending | ✓ |
| 5 | [`python-net`: client-network host module](./phase-05-python-net.md) | pending | |
| 6 | [Hostile suite, CI lane, guide](./phase-06-hostile-suite-and-docs.md) | pending | ✓ |

Slice A (1–2): platform capacity. Slice B (3–4): the runtime. Slice C (5–6): network edge and hardening.

## Dependencies

- 2 needs 1 (the quota must cover the heap the cell declares). 3 needs 1 (the counting allocator wraps the PAL allocator).
- 4 needs 2 and 3. 5 needs 4. 6 needs 4 and 5.
- **Required authority:**
  - Phase 1 is a PAL change. It invalidates the compiler-strategy decision's "allocator" item, so both COMPILER-INTEGRATION
    roles re-sign, using `scripts/rust_std_promotion/regen_pal_inventory.py` and the phase 3 (P0) procedure.
  - Phases 2 and 4: new unsafe-allowlist entries and catalog rows need owner approval.
  - Phase 5: the network edge is a capability grant, so it needs the same owner decision as `lua-net` (2026-11-10).

## Out of scope (recorded, not dropped)

- Charging root stacks to the cell quota; speeding up the `allocate_contiguous` scan; verifying signatures in place for ELFs
  larger than ~8 MB (the python ELF is expected to stay around 8–9 MB; phase 4 gates on this).
- Upstream PRs (pydantic/monty stacker gate, jiter SSE2 gate): external side effects that need an explicit go-ahead.
- Runtime on riscv64/aarch64, threads in Python (Monty is single-threaded) and C extensions.

## Rollback

Each slice reverts on its own. Phase 1 rollback restores `HEAP_SIZE` and re-pins the PAL records again.
Phases 4–6 remove `cells/runtimes/python`, its catalog, lanes and allowlist rows, and the CI steps.
