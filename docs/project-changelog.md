# ViOS Project Changelog

**Format**: [YYYY-MM-DD] Brief summary of changes, versioned by phase.

---

## [2026-05-28] Phase 01 — Workspace Cleanup (0.2.0 → 0.2.1-dev)

**Changes**:
- Removed all sub-crate `[profile.*]` blocks from `cells/drivers/*/Cargo.toml`, `cells/services/*/Cargo.toml`, and `cells/apps/*/Cargo.toml`
- Consolidated profile configuration at workspace root (`Cargo.toml`)
- Added `posix = []` feature flag to `libs/api/Cargo.toml` for optional POSIX C Library shim
- Workspace now builds with 0 cargo warnings across all targets
- Established zero-warning baseline for subsequent CI enforcement (`-D warnings`)

**Files Modified**:
- `Cargo.toml` (workspace root) — centralized profiles
- `libs/api/Cargo.toml` — added posix feature
- 11 sub-crate `Cargo.toml` files — removed profile blocks

**Impact**: Clean build foundation for Phase 02 CI/CD integration.

---

## [2026-05-28] Phase 02 — CI/CD Pipeline (0.2.1-dev)

**Changes**:
- Created `rust-toolchain.toml` pinning `nightly-2026-05-01` with targets: `riscv64gc-unknown-none-elf`, `aarch64-unknown-none`, `x86_64-unknown-none`
- Implemented `.github/workflows/ci.yml`: 4-job pipeline (lint, build-matrix, qemu-boot, security)
- Implemented `.github/workflows/security.yml`: weekly cargo-audit, cargo-deny, cargo-geiger
- Created `deny.toml` for license scanning and security ban lists
- Added shell scripts: `scripts/qemu-boot-test.sh`, `scripts/qemu-virtio-trace.sh`
- Created GitHub issue templates (bug, feature, refactor) and PR checklist template

**Files Created**:
- `rust-toolchain.toml`
- `.github/workflows/ci.yml`
- `.github/workflows/security.yml`
- `deny.toml`
- `scripts/qemu-boot-test.sh`
- `scripts/qemu-virtio-trace.sh`
- `.github/ISSUE_TEMPLATE/bug_report.md`
- `.github/ISSUE_TEMPLATE/feature_request.md`
- `.github/PULL_REQUEST_TEMPLATE.md`

**Impact**: Automated CI gates all PRs; security scanning weekly; prevents regression across multi-arch targets.

---

## [2026-05-28] Phase 04 — VirtIO Block Device (PARTIAL)

**Changes**:
- **Root Cause Identified**: Limine bootloader does not report MMIO ranges to kernel, causing VirtIO device registers to be unmapped after `activate_paging()`
- **Solution Implemented**:
  - Added explicit identity-mapping of QEMU MMIO regions in `kernel/src/memory/paging.rs`:
    - CLINT: `0x0200_0000`–`0x0200_FFFF`
    - PLIC: `0x0C00_0000`–`0x1000_0000`
    - UART + VirtIO: `0x1000_0000`–`0x1001_0000`
  - Removed duplicate MMIO entries from `kernel/src/boot.rs` FALLBACK_MEMORY_MAP (now contains only RAM regions; MMIO handled by paging.rs)
  - Memset safety verified in `kernel/src/intrinsics.rs`

**Files Modified**:
- `kernel/src/memory/paging.rs` — added explicit MMIO identity-mapping block to `init_kernel_paging()`
- `kernel/src/boot.rs` — removed duplicate MMIO entries from FALLBACK_MEMORY_MAP

**Status**: Root cause fixed. Full I/O testing deferred to Phase 06 (External ELF Loading) integration.

**Impact**: Unblocks VirtIO device discovery and interrupt delivery; kernel no longer panics on MMIO access.

---

## [2026-05-28] Phase 05 — Keyboard Input Fix (Complete)

**Changes**:
- **Root Cause Identified**: VirtIO input IRQ was never acknowledged, leaving `InterruptStatus` set; PLIC re-fired interrupt forever (interrupt storm) → kernel hung
- **Solution Implemented**:
  - Added `pub static INPUT_DEVICE_IRQ` constant and `pub fn ack_irq(irq: u32) -> bool` to `kernel/src/task/drivers/virtio_input.rs`
  - Expanded `vi_handle_virtio_irq()` in `kernel/src/task/drivers/virtio_blk.rs` to dispatch to both block and input devices
  - Established IRQ numbering pattern: QEMU VirtIO MMIO slot `i` → IRQ `i+1` (applies to all VirtIO device types)
  - Input device properly re-arms virtqueue and publishes buffers back to available ring after consuming events

**Files Modified**:
- `kernel/src/task/drivers/virtio_input.rs` — added IRQ constant and acknowledgment function
- `kernel/src/task/drivers/virtio_blk.rs` — expanded interrupt dispatch to include input devices

**Status**: Complete. Verified and ready for Phase 2 shell interaction testing.

**Impact**: Shell now reliably reads multiple consecutive keystrokes; no deadlock on subsequent input. Foundational fix enabling interactive REPL.

---

## See Also

- **project-roadmap.md** — Live phase tracking and milestone definitions
- **system-architecture.md** — Updated with VirtIO IRQ dispatch pattern and MMIO mapping strategy
- **code-standards.md** — Development rules and project structure
- **codebase-summary.md** — Current file structure and LOC counts

---

## Version History

| Version | Date | Phase(s) | Status |
|---------|------|----------|--------|
| 0.2.0 | 2026-05-01 | Phase 0 (Alpha) | Stable baseline |
| 0.2.1-dev | 2026-05-28 | Phases 01, 02, 04, 05 | In progress |
| 0.2.1 | TBD | Phase 1 complete | Pending |
| 0.3.0 | 2026-09-30 | Phases 2–3 | Planned |
| 1.0.0 | 2027-03-31 | Phases 4+ | Planned |

