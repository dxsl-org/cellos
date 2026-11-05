# Slice TL — Tier-2 thread-exit memory leak (investigation + fix)

Status: **closed 2026-11-04 by the owner** with the fix verified. The VFS 64-launch limit (Deviation Log item 2) moves to its own slice, [slice VL](slice-vfs-launch-log-retirement.md), admitted the same day. That slice takes over the Intel WIP lane. Method: `hc-debug`. Report: `.agents/debug/debug-261104-1230-tier2-thread-exit-leak.md`.

## Root cause (CONFIRMED)

`SwitchPlan` (`kernel/src/task/domain_switch.rs`) holds an `Arc<AddressSpace>` of the incoming task's domain and lived in the outgoing task's `yield_cpu` frame across the raw switch. An exiting task never resumes, so the plan was never dropped: every exiting thread whose successor was a domain task left one permanent reference. The retired Cell's domain (root, 13 table frames, ledger) was never dropped — 14 frames and ~623 KB of kernel heap per 8-thread run until the heap could not hold a 792 KB VIFS1 read. Not mlibc-specific: the Rust `futex-test` Cell leaked too.

## Fix and regression

- `kernel/src/task/domain_switch.rs`: `SwitchPlan::root_switch` takes `self`, so the plan's reference drops at the raw-switch boundary. The hart's execution pin keeps the programmed root alive.
- `scripts/x86/qemu-mlibc-witness.sh`: requires one `DOMAIN-FRAME-RELEASE: PASS` per posix run. Negative control (fix reverted, 3 runs): `FAIL: 0 of 3 posix domains released`.

## Symptom (from slice RE, item 9)

- One TCG boot of `scripts/x86/qemu-mlibc-witness.sh` with `MLIBC_POSIX_RUNS=100`: 44 consecutive `mlibc-posix-x86` runs pass, then the 45th spawn fails: `[fs] OOM: VIFS1 read of 792712 bytes` (`kernel/src/fs.rs:62-65`, a kernel-heap `try_reserve_exact`), `PrepareChild refused … OutOfMemory`.
- `free` (MemInfo frames): +564 KiB per 10 runs of the 8-thread witness; +0 per 10 `mlibc-smoke` (no threads) and per 10 `ps`.

## Scope

- Kernel task/thread lifecycle files the cause points at (named in the Deviation Log when known), plus a regression test or witness for the fix.
- Temporary instrumentation is allowed during the investigation and removed before closing.
- Not touched: `libs/api/**`, `libs/types/**` (a Law-1 change would need its own CP).

## Acceptance

1. Root cause documented with a confidence level (`hc-debug` vocabulary) and the evidence behind it.
2. After the fix: `MLIBC_POSIX_RUNS=100` passes in one boot on TCG; `free` shows no growth across repeated threaded runs.
3. Regression: kernel host tests, x86 domain lane, RV64 default boot.

## Results

| Check | Result |
|---|---|
| Witness lane TCG, `MLIBC_POSIX_RUNS=60` | PASS, 60/60 `7/7 pass`, 60 of 60 domains released |
| Witness lane KVM (`qemu64,+pdpe1gb`), 20 runs | PASS, 20 of 20 domains released |
| `free` used, 60 TCG runs | 65216 → 65236 KiB (before the fix: +56 KiB per run) |
| Kernel host tests | 421/421 |
| x86 domain lane (`qemu-domain-test.sh`) | PASS |
| RV64 default boot (`qemu-boot-test.sh`) | PASS |

## Deviation Log

1. **File set.** Fix: `kernel/src/task/domain_switch.rs`. Regression: `scripts/x86/qemu-mlibc-witness.sh`. Temporary `TL-PROBE` instrumentation in `kernel/src/task.rs`, `kernel/src/task/syscall.rs`, `kernel/src/task/cell_session.rs`, `kernel/src/task/launch.rs`, `libs/ostd/src/session.rs` and `cells/services/vfs/src/prepared_cwd.rs` was used during the investigation and has been removed (`grep TL-PROBE` is empty).
2. **100 runs in one boot not reached: a second, independent limit.** After the fix the shell stops at its 64th program launch with `foreground preparation failed: QuotaExceeded`. Single-thread `mlibc-smoke` hits the same limit, so it is unrelated to threads. The cause is confirmed by a probe: the VFS service keeps one request log per `(creator, child operation)` (`cells/services/vfs/src/prepared_cwd.rs`, `MAX_CREATOR_LOGS = 64`) and removes them only when the creator dies. A long-lived shell can therefore launch at most 64 programs with a cwd. Out of this slice's scope (VFS service / session protocol), so it is not fixed here. Verification therefore used 60 posix runs plus the smoke witness, under the limit.
3. **Residual `free` growth.** +20 KiB over 60 runs and +4 KiB over 68 single-thread `mlibc-smoke` runs. It is the same with and without threads, matching userspace bookkeeping that grows per launch, such as the VFS logs in item 2 [INFERENCE: not attributed by probe]. The kernel heap drifts ~32 B per run.
