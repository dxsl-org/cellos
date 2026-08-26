# Phase 03 — Root-Cause Console char-8 Stall (Conditional)

**Context:** [plan.md](plan.md) · Blocked by P01.

## Overview

- **Priority:** P2
- **Status:** done (2026-07-13) — **H0 confirmed, no kernel change.** New regression
  test `console_long_line_with_backspace_no_stall` (`tests/integration/tests/boot.rs`)
  types a >8-char command with a mid-line backspace correction (`HELLX` →
  backspace → `HELLO`) and asserts both the `WROTE0` barrier (no stall) and the
  corrected output (`HELLO`, not `HELLXO` — proves the backspace itself was not
  swallowed). 2/2 green. Confirms the symptom was already resolved by the
  2026-07-07 IPC wildcard-recv poisoning fix; no `console_drv.rs`/`task.rs` edit
  needed.
- **Goal:** Determine whether the "C′ stall char-8" symptom still reproduces on
  fresh images; if so, root-cause and fix the console relay; if not, add a guard
  test so it cannot silently return.
- **CONDITIONAL:** "C′ stall char-8" is a historical runtime label from the
  2026-07-07 IPC wildcard-recv poisoning fix (`.agents/260707…` notes), **not** a
  test-fn name — no such marker exists in the suite source. It may already be
  resolved by that fix (suite went 30→40→46/53). **First action is reproduce.**

## What the symptom means

- ASCII **8 = backspace** (`\x08`). "char-8" plausibly = the 8th character typed,
  OR a literal backspace byte. Both readings are testable.
- "Console relay backpressure": the kernel console driver relays UART RX bytes to
  the input service via a **non-blocking post with a bounded pending queue**; a full
  queue drops/stalls bytes after N.

## Path under suspicion (VERIFIED file:line — re-verify before editing)

```
UART RHR poll                     kernel/src/task/drivers/uart.rs:169 poll_rhr()
  → console driver poll           kernel/src/task/drivers/console_drv.rs:25 poll()
  → relay byte to input service   console_drv.rs:165 relay_ascii_to_input()
      ipc_post_nonblock(...9)      console_drv.rs:193  (false → queue to PENDING_ASCII :70)
      ipc_post_nonblock            kernel/src/task.rs:1016 (not-in-Recv → pending_msgs :1050)
  → input service → shell         (input service EV_ASCII → shell)
  → shell readline echo           cells/tools/shell/src/async_utils.rs:44 sys_recv_timeout(0,..,100)
      printable echo               async_utils.rs:96 ostd::io::print()
      BACKSPACE echo "\x08 \x08"   async_utils.rs:59
      Return echo "\n"             async_utils.rs:54
```

## Documented hypotheses

- **H0 — Does not reproduce (most likely).** On fresh P01 images, type a
  >8-character command and a command containing a backspace; if both echo and
  execute fully, the stall is resolved. Downgrade to "add regression test only."
- **H1 — Console relay backpressure.** `PENDING_ASCII` (console_drv.rs:70) or the
  target's `pending_msgs` fills after a burst; bytes past the queue depth stall.
  Check: does a fast multi-byte paste stall at a fixed count regardless of content?
  If the stall count == queue depth, this is it.
- **H2 — Masked-recv drain gap.** The 2026-07-07 fix required `recv(mask)` to drain
  by mask so a wildcard `recv(0)` in a request/reply path does not eat a queued
  input event. A residual path may still wildcard-drain and swallow the 8th byte.
  Check: is the stall correlated with a concurrent request/reply (e.g. shell doing
  a VFS call mid-line)?
- **H3 — Backspace echo handling.** `async_utils.rs:59` emits `\x08 \x08`. If the
  8th typed char is a backspace and the echo/emit mishandles the 3-byte sequence
  (or the terminal cursor math), the line appears to "stall." Check: does the stall
  only occur when a backspace is the 8th key?
- **H4 — Harness NO-OP barrier (false stall).** The observation may be a
  whole-buffer `contains` matching a partial prior line — not a real guest stall.
  Rule out with the WROTE0 barrier.

## Implementation steps

1. **Reproduce.** On fresh riscv64 image, over serial: (a) type a 12-char command
   ending in a newline and verify full echo + execution via
   `"<12-char-cmd> && echo WROTE$?"` → `wait_for("WROTE0")`; (b) type a command with
   an embedded backspace correction. Record whether either stalls, and at which byte.
2. **If H0 (no repro):** write a regression `#[test]` (a >8-char typed command that
   asserts full echo + WROTE0) and STOP. Document "resolved by 2026-07-07 fix."
3. **If reproduces:** classify H1/H2/H3 by the correlation checks above; instrument
   the relay (`console_drv.rs`, test-hooks only) to log queue depth + drop events.
4. Fix at the identified layer — **confine kernel edits to `console_drv.rs`**
   (task.rs is owned by P02; if the fix genuinely needs `task.rs`, serialize after P02).
5. Verify the >8-char + backspace cases echo and execute fully, 2/2.

## Data flow

`serial byte → RHR → console poll → relay (bounded queue) → input svc → shell recv
→ echo`. The stall is a **drop or blocked-drain** somewhere in the bounded-queue
segment; the oracle is end-to-end echo of a line longer than any single queue depth.

## Related code files

- Modify (if repro): `kernel/src/task/drivers/console_drv.rs`,
  `cells/tools/shell/src/async_utils.rs` (backspace echo).
- Create: regression `#[test]` in `tests/integration/tests/boot.rs`.
- Read: `kernel/src/task/drivers/uart.rs`, `kernel/src/task.rs` (ipc_post_nonblock,
  pending_msgs — read-only; owned by P02).

## Todo

- [x] Reproduce with >8-char typed command (WROTE0 barrier) — did not reproduce
- [x] Reproduce with embedded-backspace command — did not reproduce (combined into same test)
- [x] Classify: H0 no-repro / H1 backpressure / H2 drain-gap / H3 backspace / H4 harness — **H0**
- [x] If repro: instrument + fix at console_drv layer — N/A, no repro
- [x] Regression test added (long line echoes + executes) — `console_long_line_with_backspace_no_stall`, 2/2 green
- [x] 3-arch regression check — N/A, test-only addition, no source (non-test) code changed

## Success criteria

- A typed command longer than any single queue depth echoes and executes fully,
  verified via WROTE0 — 2/2 runs.
- Backspace-in-line editing echoes correctly.
- Either a fix landed with file:line root cause, or "non-reproducing" proven with a
  guard test that would catch a regression.

## Risk assessment

| Issue | Likelihood | Impact | Mitigation |
|-------|-----------|--------|-----------|
| Phantom work on an already-fixed symptom | Med | Low | H0 reproduce-first gate |
| Console-relay change stalls boot console | Low | High | Re-run full boot allowlist; boot-to-shell must still reach `ViCell >` |
| Fix needs task.rs, colliding with P02 | Low | Med | Serialize P03 after P02 if so; else confine to console_drv.rs |
| Backspace fix breaks printable echo | Low | Med | Assert both printable + backspace paths in the regression test |

## Security considerations

Preserve the bounded pending-queue — an unbounded relay queue is a DoS vector
(a fast serial peer could exhaust heap). Fix by correct draining, not by removing
the bound.

## Next steps

Feeds P04 (regression test wired into CI allowlist).
