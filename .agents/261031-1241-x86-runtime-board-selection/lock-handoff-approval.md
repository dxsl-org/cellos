# Lock handoff approval — x86-64 runtime board selection

**Date:** 2026-10-31
**Approver:** accountable owner (the user), in the planning conversation: "bàn giao file trong lock inventory cho kế hoạch x86, sau đó chờ ginux chạy xong để tránh đụng độ" ("hand over the files in the lock inventory to the x86 plan, then wait for Ginux to finish to avoid a collision").
**Plan:** [plan.md](plan.md) (ADR-0029, ADR-0022 amendment 2026-10-31)

## Activation condition (sequencing)

The handoff is **approved but dormant**. It becomes effective only when the
[Ginux lessons slice](../261030-1945-ginux-lessons-hardening/plan.md) (Phase 01 + 04)
is recorded as **closed** in `.agents/plan-portfolio.md` (no further admitted
Ginux edits to `kernel/src/main.rs` or `hal/arch/x86/**`). Until then, no file
below may be edited for this plan. Whoever starts `/skill:hc-cook` must check the
condition first and stop if it is unmet.

## File-bounded exception

Phase 01 and 02 (no Law-1 delta):
- `kernel/src/acpi.rs`, `kernel/src/boot/limine.rs`, new `kernel/src/smbios.rs`
- `kernel/src/board.rs`; `kernel/src/main.rs` (x86 profile-binding and early-facts call sites only)
- `kernel/src/task/syscall.rs` (the `selected_x86_64_soc()` serial lookup only)
- `kernel/src/task/drivers/iommu.rs`, `kernel/src/task/drivers/iommu_x86.rs` (profile re-key only)
- `kernel/Cargo.toml` (removing `board-x86-pc` only)
- `boards/src/{lib.rs,catalog_tests.rs}`, `hal/soc/x86/src/{lib.rs,tests.rs}`
- `scripts/check-board-configs.sh`, `scripts/qemu-x86_64-test.sh`, `run-x86.ps1`, `build/boot-x86-uefi.ps1`, x86 ISO `limine.conf`
- `.github/workflows/ci.yml` (the `board-x86-pc` ISO lane only)
- Docs: `boards/pc/x86_64-pc/README.md`, `docs/hardware-compatibility-list.md`, `docs/guides/ocel-viewer.md`, `docs/roadmap/hardware-tracks.md`, `docs/roadmap/current-focus.md`, `CHANGELOG.md`

Phase 03 is **not** handed off here. The kernel→init fingerprint hand-off, the init gate and the cellbox command need their own ABI assessment, plus a CP1 if there is a delta, and a separate handoff.

## Constraints

- `dma_isolation` semantics unchanged. The default profile is `Required`. Nothing in ADR-0029 §2 is weakened.
- No `libs/api`/`libs/types` edit. No new syscall.
- All 38 `CELL-CONTRACT-0024` relations keep their whole-plan scope.
- Evidence ceiling: QEMU/TCG.
