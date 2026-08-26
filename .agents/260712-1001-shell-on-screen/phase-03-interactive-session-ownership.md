# Phase 03 — DEFERRED: Interactive session ownership + clean transport + QMP oracle

> **Status: DEFERRED / design-captured, not scheduled.** Phase 01 already delivers "shell on screen,
> no cable" (output-only terminal + existing keyboard-focus path). This phase is only worth building
> if the MVP's mixed-log UX or shared-focus model proves inadequate. **Requires Law 1 → 2x user confirm.**

## Context Links
- Plan: [plan.md](plan.md) · builds on Phase 01

## Problem it solves
MVP limitations: (a) terminal renders the **whole** LOG_RING (kernel `log::info!` interleaved with
shell output); (b) terminal does not own the session — keyboard goes to the shell directly, so the
terminal is a renderer, not a true tty. A "real terminal" owns focus, forwards keystrokes to the
shell, and receives **only** the shell's stdout/stderr.

## Design options (transport) — each is a Law 1 touch
1. **Service-ID + IPC sink.** Add `service::TERMINAL` to `libs/api/src/services/` (**Law 1: libs/api,
   2x confirm**). Terminal registers it; shell gains a `OutputSink::Terminal` variant (extends the
   existing `SinkGuard`/`OutputSink` at `executor.rs:73-91`) that `sys_send`s stdout to the terminal.
   Terminal forwards received keystrokes to the shell via `sys_send`.
   - Pro: clean fd separation; no kernel change. Con: shell modification; **relay backpressure** — a
     single-threaded shell blocking on `sys_send` to a busy terminal stalls the shell (the
     wildcard-recv poisoning / try_send-drop hazard). Needs bounded queue + drop-oldest policy.
2. **Dedicated pipe syscall.** New `sys_pipe`-style channel between shell and terminal (**Law 1: new
   syscall, 2x confirm**). Heavier; only justified if IPC-sink backpressure is unsolvable.

**Recommendation:** option 1 if pursued; keep raw-byte wire (no new `libs/types` struct) to minimize
the Law 1 surface. Resolve only against a concrete UX complaint.

## Focus handoff
Terminal calls `request_focus`; shell must stop reading the input service and instead read forwarded
bytes from the terminal (shell input-source switch at `async_utils.rs:42`). This is the riskiest part
— two cells contend for one focus; mis-sequencing drops keystrokes (known input focus-routing pitfall,
buf[0] vs postcard discriminant). Needs a deterministic handoff protocol.

## Optional: QMP screendump oracle (net-new test infra)
No screendump test exists today (verification is serial-text only; `compositor-cursor.rs` uses QMP
`input-send-event`, not `screendump`). This phase could add a `QemuRunner` helper that issues QMP
`screendump out.ppm`, then asserts non-blank / samples known glyph pixels — a true pixel oracle for
the whole display stack. Effort ~2d; independent of the transport work.

## ANSI color (SGR) stretch
Only if a utility starts emitting `\x1b[3Nm`/`\x1b[4Nm`. `Color(u32)` + per-cell fg/bg already
support it (`canvas.rs:10`); extend the grid cell with color attrs and the parser with SGR.

## Success Criteria (if built)
- Terminal owns focus; typed keys reach the shell; only shell stdout renders (no kernel log lines).
- Backpressure: sustained output never deadlocks the shell (bounded queue, measured).
- QMP screendump test asserts a non-blank framebuffer after a driven command.

## Risk Assessment
| Risk | L×I | Mitigation |
|---|---|---|
| Law 1 change destabilizes ABI | Med×High | 2x confirm; raw-byte wire; feature-gate |
| Focus handoff drops keystrokes | High×High | Deterministic handoff protocol; integration test |
| Shell stalls on terminal backpressure | Med×High | Bounded queue + drop-oldest; try_send |

## Decision gate
Do **not** start until: (1) MVP shipped, and (2) a concrete UX need is recorded, and (3) user grants
the Law 1 confirmation for the chosen transport.
