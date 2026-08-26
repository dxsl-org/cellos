---
title: "Phase X — Remaining ViCell Work Items"
description: "VirtIO DMA fix + 3 shell scripting features + MQTT client cell."
status: pending
priority: P1
effort: 15h
branch: main
tags: [ViCell, kernel, shell, networking, virtio]
created: 2026-06-04
---

# Phase X — Remaining ViCell Work Items

Five independent work items closing the remaining gaps: one P0 kernel
correctness fix (unblocks an `#[ignore]`'d test), three shell scripting
features, and one new network tool. All claims below re-verified against the
codebase on 2026-06-04 (see per-phase Verification notes).

## Phases

| # | Phase | Priority | Effort | Status | Files | Depends |
|---|-------|----------|--------|--------|-------|---------|
| 01 | [VirtIO bounce buffer](phase-01-virtio-bounce-buffer.md) | P0 | 1h | pending | 2 | — |
| 02 | [Function positional args](phase-02-function-positional-args.md) | P1 | 2h | pending | 1 | — |
| 03 | [`$(cmd)` substitution](phase-03-cmd-substitution.md) | P2 | 4h | pending | 2 | 02 |
| 04 | [`read VAR` builtin](phase-04-read-builtin.md) | P2 | 2h | pending | 1 | — |
| 05 | [MQTT client cell](phase-05-mqtt-client.md) | P3 | 6h | pending | 3 | — |

## Dependency Graph

- 01, 02, 04, 05 are independent — parallelizable.
- 03 depends on 02 only for ordering of `expand_token` edits (same function,
  same file) to avoid merge churn — not a logical dependency.

## File Ownership (no parallel-phase collisions)

- 01 → `kernel/src/task/syscall.rs`, `tests/integration/tests/boot.rs`
- 02 → `cells/apps/shell/src/executor.rs`
- 03 → `cells/apps/shell/src/executor.rs`, `cells/apps/shell/src/commands.rs`
- 04 → `cells/apps/shell/src/executor.rs`
- 05 → `cells/apps/net-tools/src/bin/mqtt.rs` (new), `Cargo.toml`, `gen_disk.ps1`

**Conflict zone:** 02, 03, 04 all touch `executor.rs`. Run them sequentially
(02 → 03 → 04) or land 02+03 together. 05 also touches `boot.rs` (test) which
01 edits — coordinate the test-file edit if both run in parallel.

## Cross-Cutting: disk image rebuild

Any phase touching a cell binary (02–05) requires:
`cargo build --release` → `./gen_disk.ps1` → reboot QEMU. Phase 01 changes the
kernel, so it ALSO needs a kernel rebuild before `gen_disk.ps1`.

## Verification Corrections vs Original Brief

- **X-2**: `i32_to_str` (executor.rs:452) returns `&'static str` backed by a
  single static buffer — overwritten on every call. The brief's loop reuses it
  unsafely. Fix detailed in phase 02.
- **X-4**: `read` must use `sys_read(0, &mut c)` (fd 0 = stdin), NOT
  `sys_recv(5)`. Confirmed: `AsyncStdin::read_line` uses `sys_read(0,..)`
  (async_utils.rs:25). Brief's endpoint-5 approach is wrong.
- **X-5**: brief missed `gen_disk.ps1` — the new `mqtt` bin must be added there
  (lines 53/135 pattern) in addition to `Cargo.toml`.
