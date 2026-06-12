---
title: "M2.2 Input End-to-End Closeout"
description: "Add observability + QEMU monitor key injection to prove kernel→input-service→app path, then close Milestone 2.2."
status: complete
priority: P1
effort: 3 phases (~1 day)
branch: main
tags: [input, integration-test, qemu, milestone, observability]
created: 2026-06-12
---

# M2.2 — Input End-to-End Closeout

Close out Milestone 2.2 "Complete Input Service". The kernel→input-service→app
wiring already exists and the input service is verified registered at boot. What
remains is **observability + an end-to-end CI test** that proves a real keypress
travels the full path, plus the roadmap status update.

PS/2 mouse is **out of scope** — officially deferred to G2.

## Problem Statement

The full input path is implemented but never proven end-to-end from CI:
- The input service emits no log when it receives a kernel event or dispatches one → CI cannot observe the path.
- All boot configs use `-monitor none` → no channel to inject a real keypress into QEMU's VirtIO keyboard.
- The Phase 04 e2e test was planned but never written.
- `docs/project-roadmap.md:821` still marks M2.2 as IN PROGRESS.

## Architecture (data flow under test)

```
host test → QEMU monitor (sendkey tab) → VirtIO keyboard device
   → kernel virtio_input::dispatch_pending() (IRQ) → IPC Send (sender=0)
   → input-svc handle_kernel_event()  [LOG: kernel event]
   → Dispatcher::dispatch() → IPC Send to focused TID  [LOG: dispatch to TID]
   → robot-dashboard collect_input_events()
```

The test asserts the two log probes appear in serial output after `sendkey tab`,
which proves both legs (kernel→svc and svc→app) of the path.

## Verified Ground Truth (file:line)

- Input service receive loop: `cells/services/input/src/main.rs:63-74`; kernel-event handler `:100-144`.
- Dispatcher: `cells/services/input/src/dispatcher.rs:48-55` (`dispatch`).
- Boot path with VirtIO keyboard: `tests/integration/src/lib.rs:558` `boot_with_netdev` (`-device virtio-keyboard-device` at :575, `-monitor none` at :577).
- `QemuRunner` struct: `tests/integration/src/lib.rs:167-174` (fields: `child`, `writer`, `output`, `temp_disk`).
- **8** struct constructor sites (`Self { child, ... }`): lib.rs lines 273, 318, 370, 418, 462, 506, 551, 610 — all must be updated when adding a field.
- `Drop for QemuRunner`: lib.rs:677.
- Existing input integration tests: `tests/integration/tests/boot.rs:1458` `input_service_registered_at_boot`, `:1472` `compositor_input_routing_active`.
- Dashboard focus + collection: `cells/apps/robot-dashboard/src/main.rs:103-106, 242`.
- Roadmap M2.2 block: `docs/project-roadmap.md:821-833`.

## Phases

| # | Phase | Status | Effort | Blocks |
|---|-------|--------|--------|--------|
| 01 | [Log probes](phase-01-log-probes.md) | complete | ~2h | none |
| 02 | [Monitor + e2e test](phase-02-monitor-and-test.md) | complete | ~4h | P01 (probe strings are the test's assertions) |
| 03 | [Roadmap close](phase-03-roadmap-close.md) | pending | ~1h | P01, P02 (must pass first) |

## Dependency Graph

```
P01 (probe strings) ──► P02 (test asserts on probe strings) ──► P03 (close after green)
```

P01 and P02 touch **disjoint files** (input service vs. test harness) but P02's
assertions are the exact strings P01 emits — so P01 must define the strings
first. P03 is gated on a green P02 run.

## File Ownership (no overlap between phases)

- **P01:** `cells/services/input/src/main.rs`, `cells/services/input/src/dispatcher.rs`
- **P02:** `tests/integration/src/lib.rs`, `tests/integration/tests/boot.rs`
- **P03:** `docs/project-roadmap.md` (+ read-only verification run)

## Cross-Cutting Risks

| Risk | L×I | Mitigation |
|------|-----|------------|
| Adding `monitor` field breaks 7 other boot fns (compile error) | High×High | P02 explicitly enumerates all 8 `Self { child, ... }` sites; set `monitor: None` in the 7 non-keyboard ones |
| `sendkey tab` not delivered (keyboard not focused by guest yet) | Med×Med | Test waits for `[robot-dashboard] input focus granted` before injecting |
| QEMU TCG timing flakiness on probe arrival | Med×Med | Generous `wait_for` timeout (BOOT_TIMEOUT); test skips gracefully if prerequisites absent |
| Probe logging floods serial in normal boot | Low×Low | `log::info!` per discrete event only; no per-frame logging |

## Rollback

Each phase is an independent, additive change:
- P01: revert the two service files — logging is purely additive, no behavior change.
- P02: revert lib.rs + boot.rs — monitor field and new test are additive; existing tests still set `monitor: None`.
- P03: revert the roadmap edit (doc-only).

## Success Criteria (milestone-level)

- [x] `cargo check --workspace` passes after every phase.
- [x] `input_keyboard_e2e` test passes (or skips cleanly without QEMU/kernel/disk).
- [x] Serial output shows `[input-svc] key event 0` and `[input-svc] dispatch to TID` after `sendkey tab`.
- [ ] `docs/project-roadmap.md` M2.2 marked COMPLETE with PS/2 noted as G2-deferred.

## Open Questions

- Does the input service crate depend on `log`, or only `ostd::io::println`? P01 must confirm and fall back to `println` with the `[input-svc]` prefix if `log` is unavailable in the cell. (`println` is already imported at main.rs:42; recon's `log::info!` may not be wired in a `#![no_std]` cell.)
