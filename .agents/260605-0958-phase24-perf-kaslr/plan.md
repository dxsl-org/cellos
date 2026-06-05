# Phase 24 — Performance Baseline + KASLR

**Status**: 📋 PLANNED  
**Priority**: P0  
**Target**: 2026-07-07  
**Effort**: ~2 weeks  
**Created**: 2026-06-05

---

## Goal

1. Establish a committed performance baseline and a CI gate that fails when p99 regresses > 10%.
2. Implement KASLR via Limine boot randomization so the kernel loads at a different physical address each boot.

Without a baseline, all performance claims are fiction. Without KASLR, the kernel is trivially exploitable via fixed-address attacks.

---

## Phases

| # | File | Status | Effort |
|---|------|--------|--------|
| 1 | [phase-01-bench-ci-baseline.md](phase-01-bench-ci-baseline.md) | ✅ COMPLETE | 3 days |
| 2 | [phase-02-kaslr.md](phase-02-kaslr.md) | ✅ COMPLETE | 7 days |

---

## Current State (2026-06-05 — UPDATED 2026-06-05 POST-PHASE-02)

### Phase 01 (✅ COMPLETE 2026-06-05)
- `cells/apps/bench/` — bench cell with 4 scenarios, JSON output, PDR targets ✅
- `.github/workflows/perf.yml` — weekly CI job, calls `compare-bench-results.sh` ✅
- `scripts/gen-bench-disk.sh` — Linux FAT16 disk builder for CI ✅
- `scripts/compare-bench-results.sh` — p99 regression detection (deferred first baseline to 2nd run) ✅

### Phase 02 (✅ COMPLETE 2026-06-05)
**All KASLR tasks complete:**
- `limine.conf` — created (KASLR=yes, protocol=limine) ✅
- `scripts/download-limine.sh` — created (v8.9.2 RISC-V binary) ✅
- `.gitignore` — added `tools/limine-riscv64` ✅
- `kernel/build.rs` — PIE link args (-pie, --no-dynamic-linker) via cargo:rustc-link-arg ✅
- `kernel/src/main.rs` — KASLR log (kernel_phys_base from boot_info.kernel_base()) ✅
- `scripts/gen-bench-disk.sh` — rewritten: FAT16 with limine.conf + kernel ELF + cells ✅
- `.github/workflows/perf.yml` — Limine download + RUSTFLAGS "-C relocation-model=pic" ✅
- `.github/workflows/ci.yml` — Limine download + QEMU via -kernel tools/limine-riscv64 ✅

**Design changes from plan:**
- `kernel/.cargo/config.toml` → approach replaced: PIE flags moved to `kernel/build.rs` via `cargo:rustc-link-arg` (scoped to kernel target only, avoids workspace issues) ✅
- `kernel/linker.ld` parameterization → not needed: mmap already handles KASLR correctly with existing script ✅
- `kernel/src/memory/paging.rs` parameterization → `init_kernel_paging(kernel_phys_base)` verified working with boot_info base ✅

---

## Key Constraints

- Law 3: Use `VAddr`/`PAddr` from `libs/types` — no hardcoded addresses in kernel logic
- Law 4: `unsafe` only in kernel/HAL with `// SAFETY:` comment
- Law 5: No `mod.rs`
- KASLR must not break MMIO identity-map (device addresses 0x1000_0000–0x1001_0000 etc. are hardware-fixed, not KASLR-affected)
- CI must remain green on all three arch matrix jobs (rv64, aarch64, x86_64)

---

## Dependencies

- Phase 24-1 must complete before Phase 24-2 testing (need working bench in CI to verify perf not regressed by KASLR)
- Phase 24-2 requires Limine as actual bootloader — affects `run.ps1`, `ci.yml`, `perf.yml` QEMU invocations

---

## Success Criteria

- [ ] `cargo test --all --release` passes on rv64 (all 65 integration tests green)
- [ ] `perf.yml` runs `bench` cell in CI, emits JSON, fails build on > 10% p99 regression
- [ ] `perf-baseline.json` committed to repo root
- [ ] Two consecutive QEMU boots (with Limine KASLR) log different `physical_base` values
- [ ] Kernel boots and all 65 integration tests pass with KASLR enabled
