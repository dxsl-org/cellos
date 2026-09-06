---
phase: 1
title: "Report Real MemInfo"
status: completed
priority: P1
effort: "4h"
dependencies: []
tier: medium
---

# Phase 1: Report Real MemInfo

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs. On a contract-breaking edge case, choose the smallest reversible option, log it, and stop before changing ABI or authority policy.

## Overview

Replace fabricated `/bin/free` and shell `free` values with real KiB totals from the existing opt-in `MemInfo` syscall while retaining explicit denial for cells that omit bit 56.

## Requirements

- Functional: both surfaces print integer total, used, and free KiB derived from one `ViMemInfoV1` snapshot; neither prints estimates or “unwired” text.
- Functional: use `frames.checked_mul(page_size) / 1024`; overflow or syscall failure prints `free: MemInfo denied or unavailable` and returns non-zero rather than fabricating data.
- Non-functional: preserve all `u64` values across RV32/RV64/AArch64/x86_64; use stack-backed decimal formatting rather than truncating to `usize` or heap-formatting each field.
- Security: `/bin/free` and shell are explicitly authorized; `capacity-probe` remains intentionally unauthorized and must still receive the bit-56 denial.

## Architecture

`ViMemInfoV1` already defines four fixed-width fields (`libs/api/src/abi/syscall.rs:1021-1033`), `ostd::syscall::sys_mem_info` already returns the record (`libs/ostd/src/syscall.rs:446-467`), and the kernel snapshots the frame allocator (`kernel/src/task/syscall.rs:4646-4670`). Each surface consumes that wrapper directly and emits the same two-line contract:

```text
              total        used        free
Mem (KiB):    <total>      <used>      <free>
```

No shared helper or ABI layer is added: the two small consumers keep local stack formatting because they have different output sinks (`ostd::io` versus shell capture).

## Assumptions

None — the ABI, wrapper, kernel handler, allowlist bit, output sinks, and denial fixture were read directly.

## Related Files

- Modify: `cells/tools/sys-tools/src/bin/free.rs`
- Modify: `cells/tools/shell/src/cmd_sys.rs`
- Modify: `cells/tools/shell/src/main.rs`
- Modify: `tests/integration/tests/capacity-observability.rs`
- Deviation-approved packaging/routing changes: `gen_disk.ps1`, `kernel/src/loader/launch_profile/targets.rs`, `kernel/src/loader/launch_profile/tests.rs`
- Intentionally unchanged: `cells/tests/bench/src/capacity-probe.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `kernel/src/task/syscall.rs`

## Implementation Steps

1. In standalone `free`, embed the narrow `api::declare_syscalls![Log, MemInfo]` allowlist; `Exit` remains always permitted. Do not retain the current implicit permit-all posture.
2. Call `sys_mem_info` once, convert each frame count to KiB with checked `u64` arithmetic, and print the stable header/row through `ostd::io`; exit 1 on syscall or arithmetic failure and 0 only after a real row.
3. In shell `main.rs`, add only `MemInfo` to the existing explicit syscall list; do not alter coarse manifest capabilities. The exact launch-edge additions discovered during runtime verification are limited to the deviations logged below.
4. Replace `cmd_free` constants with the same one-snapshot arithmetic and row format through `executor::shell_print`; after the truthful error line return `Err(ViError::Unknown)` so shell status is 1.
5. Extend `capacity-observability.rs` before its destructive OOM probe: run shell `free`, then `/bin/free`; parse the latest `Mem (KiB):` row for each, require three integers, nonzero total, and `total == used + free`, and reject `approx`/`not yet wired`.
6. Leave the existing `capacity-probe` call and assertions intact so omission of `MemInfo` still yields `MEMINFO_DENIED` plus the kernel’s bit-56 denial log.
7. Clean-cutover check: `git grep -nE '131072|127000|MemInfo syscall not yet wired|no MemInfo yet' -- cells/tools/sys-tools/src/bin/free.rs cells/tools/shell/src/cmd_sys.rs` must return no matches.

## Commit Contract

1. Source commit: only the two command implementations and their explicit allowlists.
2. Verification commit: only the focused integration fixture changes. Any changelog projection, if release policy triggers it, is a later separate docs commit.

## Regression Commands

```bash
cargo test -p api --target x86_64-unknown-linux-gnu
cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu mem_info_maps_args_and_requires_bit_56
cargo check -p app-shell -p app-sys-tools --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
cargo build --release -p cellos-kernel -p app-shell -p app-sys-tools -p app-bench --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
CELLOS_INCLUDE_CAPACITY_PROBE=1 pwsh ./gen_disk.ps1
cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test capacity-observability -- --nocapture
```

## Completion Evidence

- `cargo fmt --all -- --check` — exit 0.
- `cargo test -p api --target x86_64-unknown-linux-gnu` — exit 0; 91 passed, 0 failed, 4 ignored.
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu mem_info_maps_args_and_requires_bit_56` — exit 0; 1 passed with the explicit bit-56 gate.
- `cargo check --workspace --exclude app-mlibc-smoke --exclude doom --exclude tetris-c --exclude lua --exclude tetris-lua --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` — exit 0.
- `cargo clippy --workspace --exclude app-mlibc-smoke --exclude doom --exclude tetris-c --exclude lua --exclude tetris-lua --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` — exit 0.
- `cargo build --release -p cellos-kernel -p app-shell -p app-sys-tools -p app-bench -p supervisor -p hotswap-demo-v1 -p hotswap-demo-v2 -p service-hypervisor -p service-vfs --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` — exit 0.
- `CELLOS_INCLUDE_CAPACITY_PROBE=1 pwsh ./gen_disk.ps1` — exit 0; signed 47 cells and generated a 16-file VIFS1 and 51-entry disk table containing both required binaries. Validation then ran `git restore -- kernel/src/embedded/init` — exit 0 — so no tracked generated binary remained modified.
- `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test capacity-observability -- --nocapture` — exit 0; 1 passed in 8.88s. Observed shell row: `218044 = 130528 + 87516` KiB. Observed standalone row: `218044 = 131100 + 86944` KiB. The same pass observed `syscall MemInfo (bit 56) denied`, `MEMINFO_DENIED`, `OOM_TYPED`, `spawn OOM: op=SpawnPinned`, an allocation-source marker, no `KERNEL PANIC`, and `A2A3_SHELL_OK_AFTER_OOM`.
- `git grep -nE '131072|127000|MemInfo syscall not yet wired|no MemInfo yet' -- cells/tools/sys-tools/src/bin/free.rs cells/tools/shell/src/cmd_sys.rs` — exit 1, expected no matches.
- Review verdict: **CORRECT / safe to ship**, confidence 0.98, zero findings; source inspection confirmed one checked snapshot per surface, truthful non-zero failure, bit-56 policy, and the exact launch ceilings.

## Success Criteria

- [x] Both `free` surfaces emit only real integer KiB values satisfying `total == used + free` for their snapshot.
- [x] Both surfaces fail non-zero with no fake row when `sys_mem_info` or checked arithmetic fails.
- [x] `/bin/free` declares only `Log` and `MemInfo`; shell adds only `MemInfo` to its existing allowlist.
- [x] The positive runtime checks pass before `capacity-probe`, and the existing unauthorized denial/OOM/shell-recovery assertions still pass.
- [x] No ABI, kernel handler, allocator, capacity evidence, or generic/default authority changed; only the exact packaging and launch-profile deviations below were required.

## Security Considerations

Global allocator totals expose cross-cell activity. Never add `MemInfo` to `app_syscall_set`, a coarse manifest capability, the denial probe, or an image-wide default. The positive consumers are named opt-ins; the negative fixture proves omission still fails closed.

## Risk Notes

The wrapper intentionally collapses kernel errors to `SyscallError::Unknown`, so the user message must say “denied or unavailable,” not claim a specific cause. External `free` consumes frames while spawning, so its values need not equal the built-in’s earlier snapshot; validate each row independently.

## Documentation Trigger

No architecture/spec edit: ABI and policy do not change. Update the project changelog only if the repository’s ship process records user-visible command fixes, and keep that projection separate from source and runtime-test commits; never publish the local QEMU counts as production evidence.

## Deviation Log

- **Packaging deviation:** `app-sys-tools` already built `free`, but `gen_disk.ps1` neither signed nor packaged it. Added it to required sys-tools signing and the P6 disk table. Because shell `exec /bin/free` opens through Kernel-FS/VIFS1 before `SpawnFromElf`, also added it to VIFS1.
- **Exact launch-edge deviation:** The reviewed user-target ceiling did not include `/bin/free` or `/bin/capacity-probe`. Added `/bin/free` with `CapSet::EMPTY` for both exact Path and Elf shell routes. Added `/bin/capacity-probe` only to the existing spawn-capable group: exact Path receives `{ spawn: true, ..CapSet::EMPTY }`, while the existing capability-bearing-Elf guard denies its Elf route.
- **Probe authority correction:** A trial `CapSet::EMPTY` probe ceiling retained MemInfo denial but removed the probe's declared `SpawnPinned` authority, blocking its typed-OOM leg. The final exact Path ceiling restores only SpawnCap. The probe source still omits MemInfo; no device, service, generic policy algorithm, or unrelated loader behavior changed.
- **Pre-commit test cleanup:** Moved the capacity-probe Path spawn-only and Elf-denied assertions into a focused policy test. Table-compacted the repeated capability-free shell route assertions while retaining exact Path and Elf coverage for `/bin/free`.

