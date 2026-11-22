---
phase: 2
title: "Per-path cell quota and large stack for the python paths"
status: pending
priority: P1
effort: "1d"
dependencies: [1]
tier: thinking
checkpoint: true
---

# Phase 2: Per-path cell quota and large stack for the python paths

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

`DEFAULT_QUOTA_BYTES` (16 MiB) is both the default and the ceiling, and nothing is set per path. A ~10 MB image with a 32 MiB heap
exceeds it. This phase adds a catalog-driven quota for named paths and extends the P0 large-stack rule from
`/bin/monty-smoke` to `/bin/python` and `/bin/python-net`.

## Requirements

- Functional: a catalog row may declare `cell_quota` bytes. `governed_spawn` uses it **only for kernel-resolved launches** of that
  exact path, never by basename, and falls back to `default_quota_bytes()` otherwise.
- Functional: `/bin/python`, `/bin/python-net` and `/bin/monty-smoke` get a quota that covers image + PAL heap + 2 MiB stack + slack;
  the exact number is derived from phase 1's heap size. All three also get the 2 MiB user stack.
- Non-functional: the quota ceiling for these rows is a compile-time constant with an assert. No row may exceed it, and the default
  quota for every other path is unchanged.

## Architecture

- `catalog::Cell` gains `cell_quota: usize` (0 = default), set through a constructor such as `Cell { cell_quota: N, ..target(path) }`. The catalog
  export (`catalog_export.rs` → `scripts/catalog.generated.json`) carries the new field.
- `governed_spawn.rs:226` switches from `default_quota_bytes()` to `catalog::quota_for(path, kernel_resolved)`.
- `task::user_stack_pages_for` matches the three paths. Large-stack arena slots (64) are unchanged.

## Assumptions

- **Claim:** `scripts/sign-policy.py` and `check-catalog.py` read the generated JSON tolerantly, or need a one-line field addition.
  **Confidence:** medium. **How to verify:** run both after regenerating.
- **Claim:** a quota above 16 MiB does not trip other asserts (`set_default_quota_bytes` bounds apply only to the default).
  **Confidence:** medium. **How to verify:** `rg DEFAULT_QUOTA_BYTES kernel/src`.

## Related Files

- Modify: `kernel/src/catalog.rs`, `kernel/src/catalog_export.rs`, `scripts/catalog.generated.json`, `kernel/src/loader/governed_spawn.rs`, `kernel/src/task.rs`
- Modify: `scripts/check-catalog.py` / `scripts/sign-policy.py` (only if the new field needs handling)

## Implementation Steps

1. Add the field, the constructor and `quota_for`, with a host test: a kernel-resolved catalog path gets its quota; the same basename elsewhere,
   or a non-resolved launch, gets the default.
2. Extend `user_stack_pages_for` and `stack_sizing_policy_self_test` to cover the python paths.
3. Regenerate the catalog export. Run `check-catalog.py`, `cellos-sign --check --strict` and the kernel host tests.
4. Checkpoint: rebuild the monty-smoke image with phase 1's heap and the new quota, and boot it. Run `qemu-x86_64-boot`, `x86-c2c`,
   `rv64-tier2-disk` and `boot-suite` (the quota path is shared).

## Success Criteria

- [ ] Host tests for `quota_for` and the stack policy pass; the catalog export test passes.
- [ ] Checkpoint: Monty witness PASS with the >4 MiB case; the four ci-local jobs above pass.

## Security Considerations

Quota and stack are scarce kernel resources. Keying them on the resolved path means a spawner cannot obtain them by naming a
binary `python` elsewhere (P0 review finding 2). The ceiling assert bounds what a mistaken catalog edit can grant.

## Risk Notes

- If the quota alone is not enough because frames are pre-charged elsewhere (for example the image copy during signing), it shows up as `PrepareChild … OutOfMemory`
  in step 4. Log it and size from the measured high-water.

## Deviation Log

None.
