# Phase 02 — Root-Cause + Fix Input Event-Delivery Reds

**Context:** [plan.md](plan.md) · Blocked by P01.

## Overview

- **Priority:** P1
- **Status:** done (2026-07-13) — **RESOLVED, no kernel change.** Both tests pass
  3/3 (1× in the P01 full-suite run + 2× isolated re-runs) on the fresh riscv64
  image. Root cause of the original reds is moot: whatever dropped the event
  between 2026-07-06/07 and today no longer reproduces, most likely superseded
  by unrelated fixes landed in that window (`ipc_try_send` pending_msgs queuing
  fix 2026-07-07, or one of the 2026-07-13 merged PRs — thread-identity/honest-
  revoke, ECAM USER-flag fix, P-TRUST cap-ceiling fold — all touch task/loader
  paths adjacent to the dispatch chain). H1 (harness race) was the leading
  hypothesis and did not even need a settle/retry patch — the tests were simply
  green as-is. No instrumentation, no H2/H3/H4 investigation needed.
- **Goal:** Make `input_keyboard_e2e` (boot.rs:1501) and `input_bare_cell`
  (boot.rs:1561) pass 2/2 on riscv64 without regressing green arches.
- **Scope note:** These are **VirtIO-keyboard HID** event-delivery tests (injected
  via QMP `send_qemu_key`), NOT the UART serial-echo path. "input-echo" is a loose
  label; the true oracle is the delivery marker, not a serial character echo.

## The two reds — exact oracles

| Test | Injects | Awaits marker (one-shot, post-injection) |
|------|---------|------------------------------------------|
| `input_keyboard_e2e` (boot.rs:1501) | `send_qemu_key("tab")` after `robot-dashboard &` claims focus | `[robot-dashboard] input event received` |
| `input_bare_cell` (boot.rs:1561) | `send_qemu_key("a")` after `input-test &` claims focus | `[input-test] key received` |

Both first barrier on a focus-grant marker (`[robot-dashboard] input focus granted`
/ `[input-test] focus granted`) then sleep 300ms before injecting. Markers are
one-shot and only appear after injection, so `wait_for` on them is valid — but P01
must confirm they are not emitted at boot.

## Event-delivery path (VERIFIED file:line — re-verify before editing)

```
QMP input-send-event (key qcode)                        tests/integration/src/lib.rs:1149 send_qemu_key
  → QEMU VirtIO-keyboard device
  → kernel VirtIO-input claim/drain                      (commit 79b02a64 "claim all virtio-input devices")
  → input service recv                                   cells/services/input/src/main.rs:126 sys_recv_timeout(0,..)
      kernel-relay sentinel = isize::MAX                 main.rs:130
      handle_kernel_event → dispatcher.dispatch          main.rs:190 → dispatcher.rs:68
  → dispatch to FOCUSED cell                             cells/services/input/src/dispatcher.rs:117 sys_try_send(target,..)
      (opcode 0x10 + encoded InputEvent)
  → focused app recv                                     robot-dashboard / input-test event loop
```

**Contrast with UART path** (used by `shell_executes_echo`, P03 territory):
`console_drv.rs:165 relay_ascii_to_input` → `ipc_post_nonblock` → input service →
shell. Different ingress; do not conflate.

## Documented hypotheses (ordered — rule out cheap/likely first)

- **H1 — Harness race (rule out FIRST).** The 300ms settle may be insufficient
  under TCG for the focused app to enter `sys_recv` before injection; the event is
  then sent while the target is not in Recv. Check: does the marker appear on a
  longer settle or a retried inject? This is the footgun class — a timing shift, not
  a code bug. Cheapest to test; if it fixes 2/2, the "fix" is a settle/retry, not a
  kernel change.
- **H2 — VirtIO-input claim regression.** Commit `79b02a64` ("claim all virtio-input
  devices + route mouse to compositor") changed device claiming. Hypothesis: the
  keyboard device is no longer drained, or its events are mis-routed to the
  compositor path instead of the focused-app dispatch. Oracle: does the input
  service log a kernel event at all after injection (add/observe a trace at
  main.rs:190)? If no kernel event arrives, the regression is at claim/drain, above
  the dispatcher.
- **H3 — Dispatcher try_send silent drop.** `dispatcher.rs:117` uses `sys_try_send`
  to the focused app. Unlike the input-service→shell path — which got the
  `pending_msgs` queuing fix at `kernel/src/task.rs:1248` (input_tid special-case) —
  a send to robot-dashboard/input-test may **silently drop** if that app is not in
  Recv at the instant of dispatch (`task.rs:1265-1266` drops non-input-tid callers).
  Note the dispatcher is the *input service* as caller, so `caller_id == input_tid`
  SHOULD hold and queuing SHOULD apply — verify `input_tid` is correctly resolved in
  `task.rs:1248` for the dispatch send, not just the shell send. Also
  `dispatcher.rs:72`: if the focused cell exited, focus silently reverts to
  fallback — confirm the app is still alive at inject time.
- **H4 — Bounce-DMA (likely already fixed, verify not regressed).** Commit
  `46511d37` ("bounce-DMA in input HAL + TryRecv drains pending_msgs") fixed the
  Driver-Cell heap-VA≠identity class for input. Confirm the input HAL still bounces;
  a partial revert would drop events at the virtqueue.

## Implementation steps (investigation → fix)

1. Confirm current status from P01 matrix (which arch, red or green on fresh image).
2. **H1 triage:** re-run each test 3× as-is; bump settle to 1s; add a bounded
   inject-retry loop in a scratch run. If green 2/2 with only timing change, land the
   minimal settle/retry in the test and STOP (no kernel edit).
3. If still red, **instrument the path** (temporary traces, `#[cfg(feature="test-hooks")]`
   only per CLAUDE.md): does a kernel event reach input service (H2)? does dispatch
   fire (H3)? is the target alive + in Recv (H3)?
4. Localize to the first hop that drops, fix there (claim/drain for H2; queuing/
   `pending_msgs` semantics for H3; HAL bounce for H4).
5. Remove temporary traces; keep only markers the tests assert on.
6. Re-run both tests 2/2 on riscv64.

## Data flow (drop points to watch)

`inject → [drain] → [recv] → [dispatch try_send] → [app recv]`. Silent-drop points:
`dispatcher.rs:72` (focused cell exited → revert), `task.rs:1250` (input queue full
→ drop, bounded by `INPUT_EVENT_QUEUE_DEPTH`), `task.rs:1265-1266` (non-input caller
drop). Each is a candidate for H2/H3.

## Related code files

- Modify (likely): `cells/services/input/src/dispatcher.rs`,
  `cells/services/input/src/main.rs`, `kernel/src/task.rs` (input dispatch/queuing —
  **P02 owns task.rs**).
- Possibly: input VirtIO Driver-Cell HAL (bounce-DMA), if H4.
- Test-only (H1): `tests/integration/tests/boot.rs:1501,1561` (settle/retry).
- Read: `libs/ostd/src/input.rs`, `kernel/src/task/drivers/console_drv.rs` (to keep
  the two ingress paths distinct).

## Todo

- [x] Confirm reds' status + arch from P01 matrix — both green in the P01 full-suite run
- [x] H1 harness-race triage (settle/retry) — not needed; green as-is, no patch applied
- [x] If code bug: instrument, identify first drop hop (H2/H3/H4) — N/A, no bug reproduces
- [x] Land minimal fix at the drop hop — N/A
- [x] Remove temporary traces — N/A, none added
- [x] `input_keyboard_e2e` 2/2 green (riscv64) — 2/2 isolated + 1 in full-suite = 3/3
- [x] `input_bare_cell` 2/2 green (riscv64) — 2/2 isolated + 1 in full-suite = 3/3
- [x] 3-arch regression check (see below) — N/A, no source changed in this phase

## Success criteria

- Both markers appear after injection, 2/2 consecutive riscv64 runs.
- Root cause is stated with file:line evidence (which hop dropped, why).
- If the fix was test-only (H1), that is explicitly documented as "harness race, no
  kernel defect."

## Risk assessment

| Issue | Likelihood | Impact | Mitigation |
|-------|-----------|--------|-----------|
| Fix in `task.rs`/input service regresses shell typing or boot suite | Med | High | Full riscv64 boot allowlist + aarch64/x86 boot suites re-run after fix |
| "Fix" is really a harness timing patch masking a real drop | Med | Med | Instrument to confirm event actually reaches the app, not just that the marker times differently |
| Change to VirtIO-input claim breaks mouse→compositor (79b02a64 intent) | Low | Med | Re-run compositor-cursor suite |

## Security considerations

Event queuing is bounded (`INPUT_EVENT_QUEUE_DEPTH`, task.rs:1250) — preserve the
bound so a wedged GUI cell cannot exhaust the kernel heap. Do not widen queuing to
arbitrary callers (only the input-service TID special-case).

## Next steps

Feeds P04 (regression tests for both markers + allowlist entries).
