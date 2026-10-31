# Phase 02 — Runtime profile selection and `board-x86-pc` removal  (checkpoint)

## Context Links
[plan](plan.md) · [phase 01](phase-01-identity-facts-and-catalog.md) · ADR-0029 §2, consequences

## Overview
Replace the compile-time `SELECTED_X86_64_BOARD` with a once-bound runtime profile, and remove the cargo feature and every build path that uses it.

## Key Insights
- Every pre-ACPI consumer needs only the early facts, which are identical across profiles. So the binding can happen after ACPI without reordering boot.
- The only policy difference between profiles is `dma_isolation`, plus the Q35 VT-d base fallback and the driver list.

## Requirements
- Add a Limine executable command-line request and parse `cellos.profile=<slug>` (bounded, ASCII, unknown keys ignored).
- Binding rules, applied once and stored in a write-once cell:
  1. Command line names a slug: look it up in the catalog. An unknown slug panics with a clear message (fail-closed).
  2. The `qemu-q35-x86_64` slug additionally requires `hypervisor=true`. Otherwise reject.
  3. No option: select `x86_64-pc`.
  - Detected facts never pick an `Optional` profile.
- `board::selected()` and `selected_x86_64_soc()` on x86 return the bound profile. Calling them before binding panics. Pre-ACPI callers switch to `X86_EARLY_FACTS`.
- `iommu_x86.rs` keys the fallback on the bound profile, not on a compile-time constant.
- Remove `board-x86-pc` from `kernel/Cargo.toml`, `kernel/src/board.rs`, `scripts/check-board-configs.sh`, `.github/workflows/ci.yml` (the PC ISO lane becomes the same ISO with a different `limine.conf` command line), `boards/pc/x86_64-pc/README.md`, `docs/hardware-compatibility-list.md`, `docs/guides/ocel-viewer.md`, `docs/roadmap/hardware-tracks.md` and `docs/roadmap/current-focus.md`. Historical changelog/roadmap entries stay.
- QEMU scripts (`scripts/qemu-x86_64-test.sh`, `run-x86.ps1`, `build/boot-x86-uefi.ps1`, the ISO `limine.conf`) pass `cellos.profile=qemu-q35-x86_64` where they previously relied on the default Q35 build.

## Architecture
```text
entry ─ X86_EARLY_FACTS ─ COM/firmware windows ─ acpi::parse
      └ bind_profile(cmdline, identity) ─ has_driver gates ─ PCI ─ IOMMU(dma_isolation)
```

## Related Code Files
`kernel/src/board.rs`, `kernel/src/main.rs`, `kernel/src/boot/limine.rs`, `kernel/src/task/syscall.rs`, `kernel/src/task/drivers/iommu.rs`, `kernel/src/task/drivers/iommu_x86.rs`, `kernel/Cargo.toml`, `scripts/check-board-configs.sh`, `scripts/qemu-x86_64-test.sh`, `.github/workflows/ci.yml`, ISO/limine config, the docs listed above.

## Implementation Steps
1. Command-line request and parser, with host tests for the slug parsing edge cases.
2. Write-once profile binding plus the rule table, with unit tests for the precedence and rejection rules.
3. Switch the pre-ACPI callers to the early facts and bind after `acpi::parse`.
4. Re-key the IOMMU fallback.
5. Remove the feature and update the scripts, CI and docs.

## Todo List
- [ ] cmdline request + parser
- [ ] bind rules + tests
- [ ] early/late caller split
- [ ] IOMMU re-key
- [ ] feature removal across build/CI/docs

## Success Criteria (checkpoint)
- `cargo test` for the boards/kernel host tests passes, and `scripts/check-board-configs.sh` passes without an `x86-pc` matrix entry.
- One ISO, three QEMU runs:
  - (a) `cellos.profile=qemu-q35-x86_64` without IOMMU boots to the shell, with the identity DMA log.
  - (b) No option and no IOMMU: the PC profile refuses DMA drivers, with the refusal log.
  - (c) No option and `-device intel-iommu`: the PC profile boots with VT-d active.
- The `iommu-dmar-x86` integration test passes against the single ISO.
- No `board-x86-pc` match remains outside historical changelog/roadmap entries.

## Risk Assessment
- Lanes that silently relied on the Q35 default would now fail closed (b). That is intended; fix the scripts, not the rule.
- Early-facts drift if a future profile differs: guarded by the const assertion.
- Rollback: revert the commit set and the feature returns.

## Security Considerations
- Default is the strict profile. The weak profile needs both an explicit operator option and hypervisor presence.
- A hostile firmware can fake `hv=1`, but it cannot inject the command line on a measured or secure-boot setup. On unmeasured boot, the operator who controls `limine.conf` already controls the kernel.

## Next Steps
Phase 03 pins the bound profile and the detected facts.
