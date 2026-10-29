---
phase: 5
title: "Ratatui backend and complete GUI terminal emulation"
status: completed-private-evidence
priority: P1
tier: thinking
dependencies: [2, 4]
---
# Phase 05: Ratatui and terminal emulator

## Overview and requirements

Deliver CBX-13/15/16/29 using selected Ratatui outside viUI and the real shell bridge required from P04, which remains unproven. This phase must complete terminal capabilities and verify selective interaction, not another UI/business-logic stack.

## Architecture and related files

Create proposed `libs/tui-cellos` for the pinned Ratatui backend/integration over `libs/terminal`. Modify `cells/apps/terminal/src/{session,grid,main}.rs` and focused parser modules as required. viUI remains the pixel/window renderer; its core must not depend on Ratatui or terminal sessions. Reuse admitted desktop session/tab and input contracts from ADR-0027.

## Implementation steps

1. Integrate pinned Ratatui core/widgets with unsuitable default OS backends disabled. Implement changed-cell/cursor/style output against the P02 terminal contract; keep event polling and session modes explicit.
2. After P04 actually proves a real `cellbox sh`/foreground bridge, extend it for complete supported key/paste/resize behavior without a private command matcher or another attachment/ownership model. Preserve that demonstrated connection while implementing the emulator; its proof is a prerequisite, not current evidence.
3. Implement or adapt a bounded ANSI/VT parser/screen model for the frozen profile: fragmented sequences/UTF-8, cursor/erase/wrap/scroll/color/alternate screen and supported controls. Preserve true byte stream behavior and safe handling of unsupported terminal operations.
4. Handle Unicode display width/continuations and resizing without splitting text state or invalidating unrelated views. Propagate size to the running app rather than only resizing local grid storage.
5. Enforce one input consumer and correctly restore raw/echo/cursor/screen state on exit/cancel and app loss. Do not claim an in-process destructor survives every crash; rely on session-owner cleanup where needed.
6. Drive a throwaway standalone TUI app linked only to public SDK/backend to prove library reuse outside cellbox. Remove it after actual target interaction.
7. Capture actual GUI output for real filesystem/process/time changes, cursor movement, selection, resize, alternate-screen entry/exit and two independent sessions. Exercise the same backend via a serial terminal with desktop disabled.
8. Exercise representative one-shot commands on a terminal and through pipes/non-terminal outputs: no unexpected full-screen entry, key reads or screen controls. Request TUI on an unsupported session and observe explicit failure or the documented equivalent top batch mode, never a successful editor no-op.

## Success criteria

- [x] A real command's effect/output reaches GUI Terminal; no fabricated table/time or duplicated command engine remains. **Met (private QEMU/TCG):** GUI runs the controller `cellbox sh` via option C; GUI 6/6.
- [x] TUI frames/keys/resize work on actual serial and GUI surfaces, and prompt/echo recover after cancellation and app failure. **Met (private QEMU/TCG):** S5-3 tui-proof on the frontend/serial-controller path and GUI (frames, keys, resize, q/Ctrl-C/abandon recovery); Phase02 UART `top`+q. Evidence archived; consumer removed (S5-4e).
- [x] Secret input does not enter terminal history/logs/scrollback; unsupported escape effects are bounded and cannot access clipboard/devices without policy. **Met (private QEMU/TCG):** S5-4 secret/hostile/paste checks; input stores only digests.
- [x] Independent CLI consumer needs neither cellbox executable nor viUI; plain CLI/data paths do not require desktop startup. **Met:** tui-proof linked only terminal-sdk+vitui (archived evidence); C5a headless image has no UI task.
- [x] Selective-TUI negative cases pass: ordinary commands stay one-shot; machine output is non-interactive; unsupported explicit TUI requests are handled truthfully. **Met (private QEMU/TCG):** `oneshot-no-tui-bytes`, `top-pipe-batch` (kept), pipe/redirect refusal (archived).

## Verification

Use deterministic parser/input/mode lifetime tests for meaningful fragmented/untrusted data edges; capture real QMP/serial surface behavior. Existing incidental fixed-output tests are deleted/replaced by behavioral coverage, not re-pinned. Follow `verification.md` for safe private images and do not expose inbound SSH.

## Assumptions

Chosen Ratatui dependencies/Unicode behavior fit the target profile; the emulator profile covers the selected editor/top/SSH requirements. Verify with real consumers; do not advertise unsupported TERM functionality.

## Security and risk assessment

Treat all terminal output, especially remote output, as untrusted. Bound dimensions, sequences, scrollback and redraw work. Rollback needs matched SDK/emulator/backend/image and session teardown; do not restore the fake interpreter as a successful fallback. Displayed or typed secrets cannot be recovered, so prevent leakage before proof.

## Deviation log

2026-10-20 — **Completion withdrawn; incomplete/blocked.** `tui-cellos` source was added, GUI Terminal source changed and an updated terminal ELF was signed. Historical observations record 3 host unit tests and bare-metal PIC compilation; they do not establish Ratatui integration, real terminal interaction, restoration or reusable standalone target consumer behavior. Review found no selected Ratatui import and the GUI command matcher remains; replacing some canned values with local runtime queries is not a real migrated shell bridge. No serial/GUI keyboard/screen/resize proof closes CBX-13/15/16/29. Design consent is preserved; owner/allocation admission is incomplete and CP2 pending/not eligible. All criteria remain unchecked, and rollout is blocked under [reports/rollout-containment.md](reports/rollout-containment.md).

2026-10-22 — Phase 05 entered (D-CL3). Entry dossier: [phase-05-entry-dossier.md](phase-05-entry-dossier.md); S5-1 CP1 awaiting decision. Throwaway Ratatui probe built for x86_64-unknown-none (core+alloc, soft-float) and was removed.

2026-10-22 — S5-1 implemented: tui-cellos on pinned Ratatui, host 15/15, bare-metal build OK. See [entry dossier §6](phase-05-entry-dossier.md).

2026-10-22 — S5-2 CP1 drafted: [phase-05-s2-vt-screen-proposal.md](phase-05-s2-vt-screen-proposal.md).

2026-10-22 — S5-2 implemented (CP1 + option A deviation): GUI 6/6 at 197/197, terminal 368/368, P03 60/60, P02 48. Open for S5-3: GUI TUI child needs a real foreground path; LF/CR spec conflict recorded.

2026-10-23 — spec §4 LF amended (newline semantics). S5-3 CP1 drafted: [phase-05-s3-proposal.md](phase-05-s3-proposal.md).

2026-10-23 — Option C chosen for the GUI foreground path; annex drafted: [phase-05-s3c-frontend-annex.md](phase-05-s3c-frontend-annex.md).

2026-10-23 — S5-3 option C implemented: terminal 380/380, GUI 6/6 at 227/227, P03 60/60, P02 48, kernel AP 38/38. Deviations 1–10 recorded in the [annex §11](phase-05-s3c-frontend-annex.md); deviation 5 (kernel reads op 18/9) and 7 (record retention) need ratification. Next: S5-3 proper (tui-proof, negatives).

2026-10-23 — Option C deviations 5 and 7 ratified. Starting S5-3 proper (tui-proof, serial/GUI, negatives); D53-3 taken as recommended (throwaway consumer removed after evidence).

2026-10-23 — S5-3 done: terminal 393/393, GUI 6/6 at 252/252, P03 60/60, P02 48. tui-proof removal deferred to after S5-4.

2026-10-23 — S5-4 CP1 drafted: [phase-05-s4-proposal.md](phase-05-s4-proposal.md).

2026-10-23 — Renamed tui-cellos → vitui (modules backend/session/event; ratatui re-export), mio-cellos → vimio, polling-cellos → vipolling; naming rule added to docs/code-standards.md Law 6.

2026-10-23 — S5-4a–c done: terminal 408/408, GUI 6/6 at 306/306, P02 48, P03 60/60. Found: GUI last column never drawn (grid ignores padding) — decision pending. Next: S5-4e remove tui-proof.

2026-10-23 — GUI grid fix: 720x480 → 79x26, GUI 6/6 at 323/323, terminal 408/408. Next: S5-4e remove tui-proof.

2026-10-23 — S5-4e done (tui-proof removed). Final: terminal 397/397 (115), GUI 6/6 at 259/259, P03 60/60, P02 48, kernel AP 38/38 (logs `reports/phase-05-final-evidence/`). All five Phase 05 criteria checked within private-fixture limits; CP2 for Phase 05 deltas (S5-1, S5-2, option C, S5-3, S5-4, renames) pending; rollout blocked.

2026-10-23 — CP2 dossier for the Phase 05 slices (S5-1, S5-2, option C, S5-3, S5-4, renames), one approval line each: [phase-05-cp2-dossier.md](phase-05-cp2-dossier.md). No CP2 approved yet; rollout blocked.

2026-10-23 — **CP2 APPROVED by the user for all six Phase 05 slices** (S5-1, S5-2, option C, S5-3, S5-4, renames) at the [dossier](phase-05-cp2-dossier.md) §3 hashes (§16 record). The approval includes the retroactive S5-3 CP1 and S5-4 “go” and ratifies the S5-1 mode guard, S5-2 deviations 3–5 and option C deviations 3, 4, 6, 8, 9 and 10. It accepts the lone-redirect byte pipeline, `${NAME}`/`${#NAME}` and the `cat` UTF-8 fix. The spec.md digest drift is acknowledged and diff-verified as the §4 LF amendment only. Status → `completed-private-evidence`: all criteria are met on private QEMU/TCG fixtures only. Quarantine, canonical packaging, signing and rollout stay **BLOCKED** under [containment](reports/rollout-containment.md), and CBX-13/15/16/29 stay pending for Phase 10.
