# Plan: Tier 3 H-Extension Kernel Prep — rv64 Hypervisor Detect + HypervisorCap ZST

**Goal:** Lay the non-breaking kernel groundwork for Tier 3 VMM work — detect RISC-V H-extension at boot via DTB, expose it as a `HypervisorCap` ZST that the manifest can request.

**Context:** Tier 3 hypervisor strategy (Project memory) targets a custom ~9K LOC VMM cell in G1-optional / G2. This prep step makes H-ext presence queryable from the loader without committing to the full VMM implementation.

**Key decisions:**
- **Detection**: Parse DTB `riscv,isa` property via `fdt` crate (kernel runs S-mode via OpenSBI; reading `misa` from S-mode is M-mode–only and will trap).
- **HypervisorCap**: Follows established ZST pattern from `BlockIoCap`/`NetworkCap`/`SpawnCap`.
- **Non-breaking**: `hypervisor = false` defaults mean all existing cells and TCBs are unaffected.
- **Phase 02 is Law 1** — `libs/api/src/manifest.rs` changes require 2× user confirmation before implementation.

---

## Phases

| # | File | Status | Summary |
|---|------|--------|---------|
| [01](phase-01-h-ext-detect-cap.md) | `kernel/src/`, `kernel/Cargo.toml` | ✅ Done | `fdt` dep + `cpu_features` module + `HypervisorCap` ZST in `cap.rs` + `tcb.rs` field |
| [02](phase-02-manifest-loader-grant.md) | `libs/api/src/manifest.rs`, `kernel/src/loader.rs` | ✅ Done | **⚠️ Law 1** — `hypervisor` manifest flag + loader grant |

Phase 02 depends on Phase 01.

---

## Key Dependencies

- `fdt = "0.1"` (no_std, no_alloc DTB parser — add to `kernel/Cargo.toml`)
- DTB passed from OpenSBI to `kmain(hartid, dtb)` — currently discarded; Phase 01 uses it
- Cap pattern: `kernel/src/task/cap.rs` (established)
- TCB: `kernel/src/task/tcb.rs:148-157` (established cap field layout)
- Loader: `kernel/src/loader.rs:147-196` (spawn-time cap grant logic)
