---
phase: 01
title: Enable PIC Relocation Model
priority: P0
status: planned
depends_on: []
risk: low
---

# Phase 01 — Enable PIC Relocation Model

## Context Links
- Report: [../reports/debugger-260605-2239-release-build-broken-at-head.md](../reports/debugger-260605-2239-release-build-broken-at-head.md) (blocker #2)
- [kernel/build.rs](../../kernel/build.rs) (`-pie` + `--no-dynamic-linker`, riscv64 only @26-29)
- [.cargo/config.toml](../../.cargo/config.toml)

## Overview
- **Priority:** P0
- **Description:** `build.rs` links the kernel as a PIE (`-pie`, for Limine KASLR) but rustc
  compiles with the default `static` relocation model, emitting absolute `R_RISCV_64`
  relocations the PIE linker rejects (`R_RISCV_64 cannot be used against '.L0'`). Setting rustc
  `relocation-model=pic` makes it emit PC-relative/relative relocations that link cleanly.

## Key Insight
Confirmed by diagnostic: `RUSTFLAGS="-C relocation-model=pic" cargo build --release -p vicell-kernel`
eliminates **all** `.L0` errors (only blocker #3 remains after). The `-pie` build already relies
on `R_RISCV_RELATIVE` addends being identity-transformed at slide=0, so PIC is the intended model.

## Implementation Steps
1. Add to `.cargo/config.toml` under `[target.riscv64gc-unknown-none-elf]`:
   ```toml
   rustflags = ["-C", "relocation-model=pic"]
   ```
   Keep the existing linker-script comment. (Scope to the riscv64 target so aarch64/x86 are
   unaffected unless they later need it.)
2. `cargo build --release -p vicell-kernel` → confirm all `.L0`/`R_RISCV_64` errors are gone
   (blocker #3 undefined-symbol errors will remain — handled in Phase 02).

## Todo List
- [ ] Add `relocation-model=pic` rustflag for the riscv64 target
- [ ] Confirm `.L0`/R_RISCV_64 relocation errors are gone

## Success Criteria
- No `R_RISCV_64 ... '.L0'` / "recompile with -fPIC" errors in the release build.

## Risk Assessment
- **PIC codegen perf/size delta (Low).** Minor; kernel already intended PIE. Verify boot still
  works at slide=0 (direct `-kernel` boot) in Phase 03.
- **Affecting other targets (Low).** Mitigated by scoping the rustflag to the riscv64 target table.

## Next Steps
- Phase 02 resolves the remaining undefined fast-IPC symbols so the kernel fully links.
