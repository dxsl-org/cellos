---
title: "Scout Report"
description: "Verified codebase facts for the QEMU q35 x86 taxonomy plan."
status: completed
created: 2026-08-18
---

# Scout Report

## Observed Facts

- The q35 x86 board file already exists as untracked work at `boards/qemu/q35-x86_64/board.rs`; it defines `QEMU_Q35_X86_64`, slug `qemu-q35-x86_64`, vendor `qemu`, model `q35-x86_64`, and `SocId::QemuX86Q35` (`boards/qemu/q35-x86_64/board.rs:17`, `boards/qemu/q35-x86_64/board.rs:18`, `boards/qemu/q35-x86_64/board.rs:19`, `boards/qemu/q35-x86_64/board.rs:20`, `boards/qemu/q35-x86_64/board.rs:22`).
- The descriptor includes q35-compatible strings and shared-driver IDs for UART, IOAPIC, HPET, PCIe, NVMe, and e1000 (`boards/qemu/q35-x86_64/board.rs:6`, `boards/qemu/q35-x86_64/board.rs:7`).
- The board export path now points at `../qemu/q35-x86_64/board.rs` and exports `qemu_q35_x86_64` (`boards/src/lib.rs:27`, `boards/src/lib.rs:28`).
- `SocId::QemuX86Q35` is present in the board descriptor enum (`boards/src/descriptor.rs:9`, `boards/src/descriptor.rs:14`).
- Kernel x86 selection validates `QEMU_Q35_X86_64` and maps `SocId::QemuX86Q35` to `hal_soc_x86::QEMU_Q35` (`kernel/src/board.rs:68`, `kernel/src/board.rs:73`, `kernel/src/board.rs:85`).
- The x86 SoC profile owns COM1 base `0x03F8`, IRQ `4`, and bounded legacy firmware windows (`hal/soc/x86/src/lib.rs:85`, `hal/soc/x86/src/lib.rs:88`, `hal/soc/x86/src/lib.rs:89`, `hal/soc/x86/src/lib.rs:91`, `hal/soc/x86/src/lib.rs:95`).
- `hal/soc/x86` explicitly excludes ACPI-discovered LAPIC, IOAPIC, HPET, and ECAM fallback addresses (`hal/soc/x86/src/lib.rs:3`, `hal/soc/x86/src/lib.rs:5`).
- Board config script already counts `boards/qemu/q35-x86_64` as a supported board dir and lane (`scripts/check-board-configs.sh:65`, `scripts/check-board-configs.sh:72`, `scripts/check-board-configs.sh:125`).
- Boundary script rejects COM1 and firmware-window facts in `hal/arch/x86` or kernel integration and requires `QEMU_Q35` in `hal/soc/x86` (`scripts/check-hal-boundaries.sh:45`, `scripts/check-hal-boundaries.sh:50`, `scripts/check-hal-boundaries.sh:54`).
- QEMU BIOS runtime script uses `qemu-system-x86_64 -machine q35` and passes only when `Cellos >` appears (`scripts/qemu-x86_64-test.sh:39`, `scripts/qemu-x86_64-test.sh:40`, `scripts/qemu-x86_64-test.sh:65`).
- The current q35 README says QEMU is integration witness only and physical PC support remains hardware-gated (`boards/qemu/q35-x86_64/README.md:15`).

## Dirty Worktree Input

- `git diff --name-status HEAD -- boards hal kernel scripts .github docs Cargo.toml` showed deletion of `boards/generic/x86_64-pc/*`, modifications in board/kernel/script/doc files, and untracked `boards/qemu/q35-x86_64/`.
- This report treats those edits as unapproved implementation state to be reviewed by Build, not as shipped truth.

## Precedent Commits

- `309d401b` introduced the x86 board and SoC separation across boards, kernel, docs, scripts, CI, and x86 UART/firmware gates.
- `c6244f26` closed the docs/CI portion of board and SoC separation.
- `14141053` enforced SoC-owned hardware facts for earlier board families and added board/boundary scripts.

## Failure Modes to Watch

- Naming drift: slug uses underscore (`qemu-q35_x86_64` or `q35-x86_64`) while scripts/tests expect another form.
- Evidence overclaim: docs imply real PC support even though current evidence is QEMU q35 only.
- Placeholder leak: `q35-i686` accidentally becomes a catalog entry, feature, or CI lane.
- Boundary regression: COM1/IRQ4 or legacy firmware windows move back into `hal/arch/x86` or kernel constants.
- Generated artifact churn: BIOS/UEFI build updates tracked generated files and pollutes the commit.
