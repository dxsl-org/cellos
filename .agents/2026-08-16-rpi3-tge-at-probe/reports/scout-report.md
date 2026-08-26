# Scout Report: RPi3 TGE AT Probe

## Verified Paths

- `hal/arch/arm/src/aarch64/trap.rs:89` defines `probe_uncategorized_el2_fault` behind `#[cfg(feature = "board-rpi3")]`.
- `hal/arch/arm/src/aarch64/trap.rs:116` samples baseline `AT S1E0R`.
- `hal/arch/arm/src/aarch64/trap.rs:126` samples `AT S1E2R`.
- `hal/arch/arm/src/aarch64/trap.rs:183` calls the probe only when `ec == 0`.
- `hal/arch/arm/src/aarch64/el2.rs:50` documents boot HCR as `RW|TGE`.
- `hal/arch/arm/src/aarch64/paging.rs:138` sets `PTE_PXN` for USER mappings.
- `run-rpi3.ps1:49` builds the board-rpi3 kernel.
- `gen_disk_rpi3.ps1:37` expects the board-rpi3 release kernel before image generation.

## Constraints

- `docs/coding.md` and `docs/engineering-standards.md` were not present in this checkout; nearest repo standards read were `docs/PATTERNS.md` and `docs/system-architecture.md`.
- `rg` in WSL resolves to an inaccessible Windows app binary in this session; verification used native `grep`, `find`, `sed`, and `nl`.
- `.claude/scripts/set-active-plan.cjs` is absent, so active-plan sync could not be executed.
