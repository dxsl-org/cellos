# Phase 02 — Runtime profile selection and `board-x86-pc` removal  (checkpoint)

**Status:** done 2026-11-01. Approved by the user ("phase 02"). QEMU/TCG ceiling, plus one KVM boot. Uncommitted working tree. No Law-1 or ABI delta: no `libs/api`/`libs/types` edit and no new syscall. The added Limine request is a bootloader record read only by the kernel.

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
- [x] cmdline request + parser
- [x] bind rules + tests
- [x] early/late caller split
- [x] IOMMU re-key
- [x] feature removal across build/CI/docs

## Implementation
- **Command line (`kernel/src/boot/limine.rs`).**
  - New `EXECUTABLE_FILE_REQUEST`; GUID and layout come from the vendored Limine 8.7.0 `limine.h`. Only the `struct limine_file` prefix up to `cmdline` is declared.
  - `get_executable_cmdline(limit)` checks the response and the file record with `mp_record_valid`.
  - `bootloader_bytes` then reads the string only inside one bootloader-reclaimable memory-map entry, capped at `limit`.
  - A missing response yields an empty command line, so the strict default applies.
- **Binding (`kernel/src/board.rs`).**
  - `profile_option` accepts a printable-ASCII line of at most 1024 bytes and ignores other tokens.
  - `cellos.profile` must carry a slug (`[a-z0-9_-]`, at most 64 bytes). An empty, malformed or duplicate value is an error.
  - `choose_x86_profile` applies the rules in order. A named slug must be in `find_by_slug(x86_catalog(), …)`, otherwise `UnknownProfile`. With no slug, `DEFAULT_X86_PROFILE = "x86_64-pc"` applies. A profile whose `dma_isolation` is not `Required` needs `PlatformIdentity.hypervisor`, otherwise `NeedsHypervisor`.
  - **Deviation:** the hypervisor rule keys on `dma_isolation != Required`, not on the q35 slug, so any future weak profile inherits it. q35 is the only such profile today.
  - `bind_x86_profile` validates the descriptor and pairs it with its SoC profile. It checks `Uart16550PortIo`, then stores the profile once in `ProfileCell(AtomicPtr)`; a second bind panics. It prints `[x86-gate] board=… soc-profile=… source=cmdline|default dma_isolation=…` through the early console.
  - `selected()` panics before binding. `selected_x86_64_soc()` now returns the bound profile.
  - Every `BindError` reaches `panic!("[board] {error}")` in `kmain` (fail-closed).
- **Early/late split (`kernel/src/main.rs`).**
  - The COM setup and the ACPI/SMBIOS `map_physical` windows read `hal_soc_x86::X86_EARLY_FACTS`.
  - Binding runs right after `acpi::parse`, SMBIOS discovery and `[identity]`, before the IOAPIC/HPET/MCFG `has_driver` gates.
  - The pre-ACPI `[x86-gate] board=` line moved to the binding point.
- **IOMMU.** `iommu_x86::init_hw`'s q35 fallback and `iommu::map_range_for_cell`'s `dma_isolation` already call `board::selected()`, which is now the bound profile. Only the fallback's contract comment changed. `syscall.rs` needed no edit for the same reason.
- **Feature removal.**
  - `board-x86-pc` is gone from `kernel/Cargo.toml`, `board.rs`, `scripts/check-board-configs.sh` (single `x86-64` entry), `.github/workflows/ci.yml` and `scripts/mutable-globals.toml`.
  - CI's PC step reuses the job's kernel with `X86_LIMINE_CONF=scripts/x86/limine-pc.conf`.
  - Docs updated: the PC README, the HCL, `ocel-viewer.md`, `hardware-tracks.md` and `current-focus.md`.
- **Limine configs.**
  - `scripts/x86/limine.conf` now carries `cmdline: cellos.profile=qemu-q35-x86_64`. Every existing lane builds its ISO through `make-iso-ci.sh`, as does `run-x86.ps1`, which copies the same file, so all of them keep q35.
  - The new `scripts/x86/limine-pc.conf` has no option.
  - "One ISO" means one kernel ELF, which hashed identically in both ISO roots, with a per-lane `limine.conf`. QEMU cannot inject a Limine command line into an ISO boot.
- **Handoff file-set exceptions, approved by Main/user 2026-11-01:**
  - `scripts/x86/make-iso-ci.sh`: `X86_LIMINE_CONF` override; the default is unchanged.
  - New `scripts/x86/limine-pc.conf`.
  - `scripts/mutable-globals.toml` and the regenerated `docs/generated/mutable-globals.md`: the profile feature list is updated, and `SMBIOS_REQUEST` (a Phase 01 regression), `EXECUTABLE_FILE_REQUEST` and `BOUND_X86_PROFILE` are classified.
  - `tests/integration/tests/{ahci,xhci}-x86.rs`: SKIP-message text only.
- **Left on purpose:** `docs/project-roadmap.md:286` (roadmap prose), the ADR-0029 text, the changelogs and `.agents/**` evidence.

## Evidence (2026-11-01)
- **Host tests.** `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: 368 passed, 0 failed. This includes 5 new `board::tests`: option parsing, binding rules, error text, catalog↔early-facts pairing, write-once cell. `cargo test -p cellos-boards -p hal-soc-x86`: 15 + 7 passed.
- **`-D warnings` kernel builds.** RUSTFLAGS from `build-x86_64-prod-refusal-ci.sh` plus `-D warnings`, in `CARGO_TARGET_DIR=target/p01-dwarn`, unset afterwards. Default and `--features policy-required` both finished with no warnings.
- **`scripts/check-board-configs.sh` (did not complete).**
  - The README checks passed, including the PC README's plain build command.
  - The script then exits at `check-hal-boundaries.sh` on a pre-existing violation in `hal/arch/x86/src/x86_64/idt/dispatch.rs:10`. That file is committed and unmodified (last commit `775686575`).
  - The matrix entries run by hand: riscv64, aarch64, vf2, pioneer, rpi3 and x86-64 all pass `cargo check` with 0 diagnostics.
  - `board-rpi4` fails in `hal/arch/arm/src/aarch64/monitor.rs:117,318` (`pi_monitor_mmu_init` configured out). That is outside this plan and unmodified.
- **`mutable-globals --check`:** 601 declarations, 0 problems.
- **QEMU 8.2.2, q35/TCG/SeaBIOS.** One kernel (`sha256 bb9f919d…` in both ISO roots); logs in `build/p02-profile/`.
  - (a) `build/vicell-x86.iso`: `source=cmdline dma_isolation=Optional`, `[iommu] board qemu-q35-x86_64 declares no DMA remapper; DMA is untranslated`, reaches `Cellos >`.
  - (b) `build/x86-pc-lane/vicell-x86-pc.iso`, no IOMMU: `board=x86_64-pc … source=default dma_isolation=Required`, `[iommu] DMA isolation REQUIRED by board x86_64-pc but no remapper was discovered — refusing untranslated DMA`, reaches `Cellos >`.
  - (c) The same ISO with `-device intel-iommu`: `Intel VT-d: DMA isolation ACTIVE`, reaches `Cellos >`.
  - (d) q35 ISO with `-cpu …,hypervisor=off`: `hv=0`, then `panicked … [board] cellos.profile=qemu-q35-x86_64 does not require DMA isolation and is admitted only under a hypervisor, but CPUID reports none`.
  - (e) An ISO with `cellos.profile=bogus-board`: `panicked … [board] unknown cellos.profile=bogus-board; this image binds only: x86_64-pc qemu-q35-x86_64`.
  - KVM (`-accel kvm`, q35 ISO): `hv=1`, `source=cmdline`, reaches `Cellos >`. This verifies A2 for TCG and KVM.
- **Integration tests.**
  - `iommu-dmar-x86`: 3/3 passed against the two ISOs.
  - `nvme-x86`: `11 passed`. Four tests ran; the seven `nvme_09c_*` tests skip themselves because `VICELL_AHCI_FAULT_ISO` is unset.
- **Domain lane.**
  - `bash scripts/build-x86_64-domain-test-ci.sh`: exit 0.
  - `X86_CPUS=2 bash scripts/x86/qemu-domain-test.sh`: `PASS: x86_64 Tier-2 domain-entry lane (x86-domain-test-cpu2)`. Its log shows `source=cmdline` for q35.
- **Fingerprint-mismatch safe mode:** Phase 03, not exercised here.
- **`board-x86-pc` grep:** remaining matches are ADR-0029, `docs/project-roadmap.md:286`, the changelogs and `.agents/**` historical evidence.

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
Phase 03 pins the bound profile and the detected facts. It needs:
- its own lock handoff, with an ABI assessment and a CP1 for the kernel→init fingerprint hand-off;
- the fingerprint inputs available at binding: `PlatformIdentity` (ACPI OEM, SMBIOS type 1, hv), the bound slug from `board::selected()`, and `ProfileSource`. The PCI ID set comes later, from `pcie_ecam`;
- a writable cellosfs path for the record (A3).

The bound profile only changes through `limine.conf`, so a measured or secure boot must cover that file. That is part of Phase 03's trust story.
