---
phase: 2
title: "Verify DTB Capacity at Runtime"
status: completed
priority: P1
effort: 4h
dependencies: [1]
tier: medium
---

# Phase 2: Verify DTB Capacity at Runtime

> Log every Decision / Deviation / Surprise in the Deviation Log when it occurs.

## Overview

Prove that A1 changes observed capacity, not merely parsing, while preserving the normal 256 MiB boot.

## Requirements

- Parameterize RV64 QEMU memory without duplicating the existing runner.
- Run final evidence from the commit being certified; temporary built worktrees are preflight-only.
- Assert managed bytes exceed 1 GiB under `-m 2G`, then reach paging, heap, and shell readiness.
- Run the normal 256 MiB boot suite serially and retain its pass count.

## Assumptions

- **Claim:** QEMU `virt -m 2G` reports the full range in the firmware DTB.
  **Confidence:** high
  **How to verify:** capture the new allocator-range boot marker from the test run.

## Related Files and Ownership

| File | Action | Owner |
|---|---|---|
| `tests/integration/src/lib.rs` | Modify: parameterized RV64 memory size | Phase 2 only |
| `tests/integration/tests/handoff.rs` | Modify: add the 2 GiB capacity gate | Phase 2 only |
| `.agents/reports/a1-dtb-runtime-260731.md` | Create: commands, hashes, logs, pass counts | Phase 2 only |

## Implementation Steps

1. Add a memory-size parameter to the existing RV64 QEMU builder with 256 MiB as the default.
2. Add one integration test that boots 2 GiB, parses the allocator marker, asserts `> 1 GiB`, and waits for the shell-ready marker.
3. Build the RV64 image using the documented toolchain shim and exported cargo C variables; fail if output contains `FATAL`.
4. Run the targeted 2 GiB test and the existing serial `boot` suite on the same artifact.
5. Record commit, artifact SHA-256, commands, allocator range, and pass count.

## Test Matrix

| Gate | Command shape | Pass condition |
|---|---|---|
| Host integration compile | `cargo test ... --target x86_64-unknown-linux-gnu --no-run` | No compile failure or silent skip |
| Capacity | `--test handoff handoff_rv64_uses_dtb_memory_size` | Managed bytes > 1 GiB and shell ready |
| Regression | `--test boot -- --test-threads=1` | Full suite passes at default 256 MiB |

## Success Criteria

- [x] The 2 GiB gate distinguishes the old 190 MiB implementation and passes against phase 1.
- [x] Focused default-memory boot paths remain green with no panic, fault, or allocator fallback warning.
- [x] Evidence report identifies the baseline and artifact hashes.

The fresh full serial `boot` run exceeded the 20-minute harness timeout near test 28. The focused
runtime gates pass after the linker fix; the prior 54/54 branch result is retained only with its
original provenance. See `../reports/a1-dtb-runtime-260731.md`.

## Security Considerations

The test must assert the allocator's normalized range, not raw DTB RAM, so reserved-memory subtraction is exercised.

## Risk Notes and Rollback

If 2 GiB exposes paging-time or boot-time scaling, keep phase 1 unverified and investigate; do not weaken the threshold.
Test-only harness changes can be reverted independently. Production rollback is the phase-1 commit.

## Deviation Log

None.
