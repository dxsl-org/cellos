---
phase: 1
title: "AArch64 Test-Hooks Semihosting"
status: completed
priority: P2
effort: "4h"
dependencies: []
tier: medium
---

# Phase 01: AArch64 Test-Hooks Semihosting

## Overview

Unblock the AArch64 `test-hooks` build lane and wire a deterministic QEMU exit
so AArch64 integration tests terminate cleanly instead of relying on serial-log
timeout. This closes the stale `B-AARCH64-SEMHOSTING` ledger blocker.

## Requirements

- AArch64 `--features test-hooks` must compile cleanly with `-D warnings`.
- A QEMU AArch64 test-hooks runner must boot, emit serial markers, and exit
  via semihosting with an explicit success/failure code.
- The `B-AARCH64-SEMHOSTING` blocker in the acceptance ledger and its fixture
  must be resolved with evidence.

## Architecture

The existing `qemu_exit::AArch64` (crate `qemu-exit` 4.0.0) uses ARM
semihosting `SYS_EXIT` via `HLT #0xF000`. QEMU requires `-semihosting` on the
command line. The RV64 reference uses SiFive test MMIO at `0x100000` and needs
no special flag.

Flow: test-hooks boot → run self-tests → aggregate result → `qemu_exit(pass)` →
QEMU exits with code 0 (success) or 1 (failure).

## Assumptions

- **Claim:** `qemu_exit::AArch64` is the correct API and the historical
  `AArch64Semihosting` naming error is already fixed in the working tree.
  **Confidence:** high
  **How to verify:** `cargo check --target aarch64-unknown-none-softfloat -p cellos-kernel --features test-hooks`

- **Claim:** QEMU `virt` machine supports `-semihosting` for `HLT #0xF000`.
  **Confidence:** high
  **How to verify:** `qemu-system-aarch64 -machine virt -semihosting -nographic -kernel test.elf`

## Related Files

- Modify: `kernel/src/main.rs` (wire `qemu_exit` call at test-hooks terminal)
- Modify: `scripts/qemu-aarch64-test.sh` (add `-semihosting`, accept exit code)
- Create: `scripts/build-aarch64-test-hooks-ci.sh` (AArch64 test-hooks builder)
- Modify: `docs/app-tier-acceptance-ledger.json` (resolve `B-AARCH64-SEMHOSTING`)
- Modify: `tests/app-tier-acceptance/fixture-data/app-tier-acceptance-seed.json`
- Modify: `scripts/app_tier_acceptance/ledger.py` (if schema update needed)

## Implementation Steps

1. Run `cargo check --target aarch64-unknown-none-softfloat -p cellos-kernel
   --features test-hooks` to confirm the `qemu_exit::AArch64` API compiles.
   If not, fix the import and type usage to match `qemu-exit` 4.0.0.

2. Create `scripts/build-aarch64-test-hooks-ci.sh` mirroring
   `scripts/build-test-hooks-ci.sh` but targeting
   `aarch64-unknown-none-softfloat` with `--features test-hooks`.

3. In `kernel/src/main.rs`, identify the AArch64 test-hooks completion point
   (equivalent to RV64's terminal markers). Wire exactly one call to
   `crate::qemu_exit(true)` on aggregate success and `crate::qemu_exit(false)`
   on any failure, after all UART evidence markers have been emitted.

4. Update `scripts/qemu-aarch64-test.sh` (or create a dedicated
   `scripts/qemu-aarch64-test-hooks.sh`) to:
   - Build the test-hooks image via the new builder script.
   - Pass `-semihosting` to `qemu-system-aarch64`.
   - Accept QEMU's exit code as PASS/FAIL instead of relying solely on timeout.
   - Still validate serial markers for evidence integrity.

5. Run the updated runner and verify:
   - QEMU exits with code 0 on success.
   - Serial markers are present in captured output.
   - Timeout still acts as a safety net for hangs.

6. Update `docs/app-tier-acceptance-ledger.json`: change `B-AARCH64-SEMHOSTING`
   status from `BLOCKED` to `RESOLVED`, add evidence path and resolution text.
   Fix the incorrect `subject: "qemu-rv64"` to the correct subject.

7. Synchronize `tests/app-tier-acceptance/fixture-data/app-tier-acceptance-seed.json`
   with the ledger change. Run acceptance tests to confirm consistency.

## Success Criteria

- [x] `cargo check --features test-hooks` passes for AArch64 target.
- [x] AArch64 QEMU test-hooks runner exits deterministically (not by timeout).
- [x] Serial evidence markers present in runner output.
- [x] `B-AARCH64-SEMHOSTING` blocker resolved in code and test runner.
- [x] Acceptance tests (`test_acceptance_ledger.py`) pass.

## Security Considerations

Semihosting is test-only; `qemu_exit` is gated behind `#[cfg(feature = "test-hooks")]`
and never compiles into production images. The QEMU runner must not accept
semihosting in non-test configurations.

## Risk Notes

- If AArch64 test-hooks has other compile errors beyond the semihosting import,
  those must be fixed first. The scout found no other reported errors but did
  not compile.
- QEMU exit code mapping: `qemu-exit` 4.0.0 AArch64 may map success to a
  nonzero code depending on the semihosting parameter block; verify the actual
  exit code before wiring the runner's PASS/FAIL logic.

## Deviation Log

None.
