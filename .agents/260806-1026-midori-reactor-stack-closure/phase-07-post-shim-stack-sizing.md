---
phase: 7
title: "Post-Shim Stack Sizing"
status: completed
priority: P2
effort: "1d"
dependencies: [6]
tier: thinking
---

# Phase 07: Post-Shim Stack Sizing

## Overview

Use the parked-executor runtime and Phase06 guard/probe closure to produce conservative per-path stack sizing. Six measured paths now sit at 16 usable pages with two guards; unmeasured or risky paths stay at the 64-page default.

## Blockers

- None.

## Requirements

- Functional: re-measure init, shell, vfs, vfs-test, net, and at least one driver path after Phase05.
- Functional: add non-default `stack_pages_for(path)` entries only for measured paths with evidence and safety factor.
- Non-functional: no manifest ABI field; static kernel table only unless a later Law 1 gate authorizes manifest sizing.

## Architecture

Data flow: test-hooks watermark markers enter a sizing report, sizing logic applies safety factor and minimum floors, `stack_pages_for(path)` exits a conservative page count, spawn allocates with Phase06 guard/probe protection. Unknown paths exit as default 64 pages.

## Assumptions

- **Claim:** Six representative paths are enough for an initial production table.
  **Confidence:** medium
  **How to verify:** Compare measured paths against boot image contents and keep all unmeasured paths at default 64.

## Related Files

- Modify: `kernel/src/task.rs`
- Modify: `tests/integration/tests/vfs-quota.rs`
- Modify: `tests/integration/tests/boot.rs`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/project-changelog.md`
- Create: `.agents/260806-1026-midori-reactor-stack-closure/reports/phase-07-test-review.md`
- Create: `.agents/260806-1026-midori-reactor-stack-closure/reports/stack-sizing-evidence.md`

## Implementation Steps

1. Run RV64 test-hooks and capture `[stack-baseline]` after Phase05 and Phase06 are landed.
2. Add net and driver markers if absent; do not infer their sizes from init/shell/vfs.
3. Compute table entries with at least 2x observed usage and a fixed minimum; document formula in `stack-sizing-evidence.md`.
4. Update `stack_pages_for(path)` with measured paths only; default remains 64.
5. Run RV64, AArch64, and x86_64 boot gates plus RV64 network/VFS integration.
6. Update living docs in the same change only after QEMU evidence exists.

## Success Criteria

- [x] `stack-sizing-evidence.md` lists raw bytes, pages, safety factor, selected pages, and QEMU log path for each non-default entry.
- [x] Unknown path test proves fallback is still 64 pages.
- [x] Three-arch boot gates pass after the table is applied.

## Validation Commands

```bash
bash scripts/build-test-hooks-ci.sh
cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test vfs-quota
export CARGO_BUILD_TARGET=riscv64gc-unknown-none-elf
export CC_riscv64gc_unknown_none_elf=riscv64-unknown-elf-gcc
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
export OBJCOPY=riscv64-unknown-elf-objcopy
pwsh ./gen_disk.ps1
BOOT_WINDOW=120 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel disk_v3.img
BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh
BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso
```

## Security Considerations

Wrong sizing is a memory-safety risk in SAS, not a contained process crash. Keep default for unmeasured paths and fail closed on guard/probe errors.

## Risk Notes

- High x High: measurements miss rare error paths. Mitigation: include error-path tests and keep a 2x safety factor.
- Medium x Medium: non-default table saves too little memory. Mitigation: accept no-shrink entries when evidence does not justify risk.
- Rollback: revert `stack_pages_for` entries and docs; default 64 restores old behavior. Irreversible part: evidence files are historical only.

## Deviation Log

None.
