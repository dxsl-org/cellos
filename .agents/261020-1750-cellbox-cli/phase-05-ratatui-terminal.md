---
phase: 5
title: "Ratatui backend and complete GUI terminal emulation"
status: pending
priority: P1
tier: thinking
dependencies: [2, 4]
---
# Phase 05: Ratatui and terminal emulator

## Overview and requirements

Deliver CBX-13/15/16/29 using selected Ratatui outside viUI and the real shell bridge already cut over in P04. This phase completes terminal capabilities and verifies selective interaction, not another UI/business-logic stack.

## Architecture and related files

Create proposed `libs/tui-cellos` for the pinned Ratatui backend/integration over `libs/terminal`. Modify `cells/apps/terminal/src/{session,grid,main}.rs` and focused parser modules as required. viUI remains the pixel/window renderer; its core must not depend on Ratatui or terminal sessions. Reuse admitted desktop session/tab and input contracts from ADR-0027.

## Implementation steps

1. Integrate pinned Ratatui core/widgets with unsuitable default OS backends disabled. Implement changed-cell/cursor/style output against the P02 terminal contract; keep event polling and session modes explicit.
2. Extend the P04 real `cellbox sh`/foreground bridge for complete supported key/paste/resize behavior without adding a private command matcher or another attachment/ownership model. Keep the already-proven shell connection intact while implementing the emulator.
3. Implement or adapt a bounded ANSI/VT parser/screen model for the frozen profile: fragmented sequences/UTF-8, cursor/erase/wrap/scroll/color/alternate screen and supported controls. Preserve true byte stream behavior and safe handling of unsupported terminal operations.
4. Handle Unicode display width/continuations and resizing without splitting text state or invalidating unrelated views. Propagate size to the running app rather than only resizing local grid storage.
5. Enforce one input consumer and correctly restore raw/echo/cursor/screen state on exit/cancel and app loss. Do not claim an in-process destructor survives every crash; rely on session-owner cleanup where needed.
6. Drive a throwaway standalone TUI app linked only to public SDK/backend to prove library reuse outside cellbox. Remove it after actual target interaction.
7. Capture actual GUI output for real filesystem/process/time changes, cursor movement, selection, resize, alternate-screen entry/exit and two independent sessions. Exercise the same backend via a serial terminal with desktop disabled.
8. Exercise representative one-shot commands on a terminal and through pipes/non-terminal outputs: no unexpected full-screen entry, key reads or screen controls. Request TUI on an unsupported session and observe explicit failure or the documented equivalent top batch mode, never a successful editor no-op.

## Success criteria

- [ ] A real command's effect/output reaches GUI Terminal; no fabricated table/time or duplicated command engine remains.
- [ ] TUI frames/keys/resize work on actual serial and GUI surfaces, and prompt/echo recover after cancellation and app failure.
- [ ] Secret input does not enter terminal history/logs/scrollback; unsupported escape effects are bounded and cannot access clipboard/devices without policy.
- [ ] Independent CLI consumer needs neither cellbox executable nor viUI; plain CLI/data paths do not require desktop startup.
- [ ] Selective-TUI negative cases pass: ordinary commands stay one-shot; machine output is non-interactive; unsupported explicit TUI requests are handled truthfully.

## Verification

Use deterministic parser/input/mode lifetime tests for meaningful fragmented/untrusted data edges; capture real QMP/serial surface behavior. Existing incidental fixed-output tests are deleted/replaced by behavioral coverage, not re-pinned. Follow `verification.md` for safe private images and do not expose inbound SSH.

## Assumptions

Chosen Ratatui dependencies/Unicode behavior fit the target profile; the emulator profile covers the selected editor/top/SSH requirements. Verify with real consumers; do not advertise unsupported TERM functionality.

## Security and risk assessment

Treat all terminal output, especially remote output, as untrusted. Bound dimensions, sequences, scrollback and redraw work. Rollback needs matched SDK/emulator/backend/image and session teardown; do not restore the fake interpreter as a successful fallback. Displayed or typed secrets cannot be recovered, so prevent leakage before proof.

## Deviation log

None; implementation pending. Record each change in contract, dependency or failure behavior with reason/impact/revert.
