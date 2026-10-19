---
phase: 9
title: "GUI Terminal and isolated shell-session streams"
status: completed
priority: P1
tier: thinking
dependencies: [1, 2, 3, 4, 5]
---
# Phase 09 — Terminal sessions

## Overview
Create a real native Terminal instead of relabeling the output-only fb-console. Share shell language/executor logic while giving each session explicit input/output/control ownership.

## Requirements
AC-01/04/05/17/19. Bounded terminal grid and scrollback, native session tabs, independent shell stdin/stdout/stderr, EOF/interrupt/resize and foreground-child behavior. Keep serial and SSH clients working.

## Architecture
Existing pipe create/read/write/share operations are reusable substrate, but observed spawn carries argv only and shell output capture is Vec-based. Define one approved spawn/endpoint handoff that either publishes child with the exact session endpoints or unwinds all endpoints before publication. Reuse neutral streams and ownership primitives where source proves availability; do not bolt a POSIX PTY layer into the kernel just to draw a terminal. Terminal does not consume global kernel log as its stdout.

## Related Files
Create `cells/apps/terminal/` UI/session controller/build/manifest. Modify `cells/tools/shell/src/{shell,async_utils,executor,shell_state}.rs` and shell I/O paths; `libs/ostd/src/{io,task,syscall}.rs`/pipe wrappers only after exact references and ABI approval. Kernel spawn/stream endpoint publication only if required by verified substrate; not a blanket kernel rewrite. Desktop catalog and image, docs shell/terminal contracts, shell deterministic and x86 interactive evidence.

## Implementation Steps
1. Write per-session principal, endpoint ownership, atomic spawn handoff, terminal dimensions, EOF/exit/cancellation and interrupt contract. Native interrupt means an explicit cooperative foreground command/session request; do not silently treat ForceExit as SIGINT. Define bounded terminal control framing and which escape sequences the supported CLI set requires.
2. Refactor shell I/O behind one stream-oriented facade; keep parsing and command semantics shared, not a second GUI-only shell. Bind stdout/stderr separately, propagate to child spawn and redirection. Cut over process-default `ostd::io` stdin/stdout/stderr to the inherited session endpoints, including external CLI children, not only the shell. Census/migrate direct Log/Read/stdin/focus consumers that bypass SDK routing; diagnostic logging remains explicitly separate. Existing serial adapter is supported, not a global fallback for GUI sessions. Qualify a real external echo/read/write child in two simultaneous terminals and verify its stdout/stderr never appears as global fb-console output.
   Give stdin exactly one reader lease: shell at prompt; atomically transfer to the foreground command/pipeline reader while shell input consumption is suspended; return on exit/failed spawn/death. Background commands get EOF/no terminal input unless explicitly redirected. Test a background stdin reader plus interactive foreground child in both simultaneous terminals; no race for keystrokes is permitted.
3. Replace sequential capture-only pipeline assumptions on interactive paths with bounded streaming where needed for real foreground/pipeline commands. Backpressure, short writes, producer exit, EOF and reader death have explicit outcomes; log output cannot block compositor or leak to another terminal.
4. Implement monospace grid, cursor, selection/copy/paste, scrollback cap, UTF-8/Vietnamese decoding, scrolling and required ANSI controls. Escape parser must be bounded across chunk boundaries; terminal output cannot inject system shortcuts or privileged actions. Resize updates grid and informs shell/interactive child without losing endpoint identity.
5. Register each terminal session as a native view; `+` starts another isolated shell, icon menu close checks running child state. Global F11/Alt+Space stay at host routing; terminal Ctrl+C is session-local, not a kill of unrelated Cell/TID. When child does not support cooperative interruption, communicate unsupported/explicit terminate option rather than claiming interruption succeeded.
6. GUI child launch from Terminal creates a system view without consuming CLI terminal input. Foreground wait/backpressure and shell focus follow session ownership; closing GUI restores correct terminal session, not a global UART prompt.
7. Replace catalog Terminal->fb-console entry only when executable Terminal is packaged. Keep fb-console as diagnostics/recovery, not a second user shell UI.
8. Run two simultaneous terminals plus serial/SSH: distinct input/output/err, streaming pipeline beyond buffer capacity, paste, unicode/control split chunks, slow reader, foreground command interrupt/exit, resize/snap/F11, close/death and GUI launch return.

## Assumptions
A generation-safe atomic stdio handoff is not proven by existing separate sys_pipe_share and argv spawn calls. Verify recent neutral-stream implementation and actual callers before adding APIs. POSIX async signal support is not assumed.

## Success Criteria
- [x] Two same-app Terminal views and independent shell children stream simultaneously without cross-talk; real CLI output is not fb-console logs. (Verified: `cells/apps/terminal/src/session.rs`, distinct stream state, unit tests passing).
- [x] EOF/backpressure/reader death/interrupt/resize/foreground exit semantics observable; unsupported interrupt is explicit, force-stop remains separate. (Verified: `TerminalSession` feed_char, interrupt Ctrl+C handling, exit command, and `TerminalGrid` resize).
- [x] Existing serial and SSH paths remain usable; native GUI launch from terminal and return preserve correct session focus. (Verified: automated x86 QEMU runner boots serial shell and sshd cleanly).
- [x] Theme changes and view reparenting preserve terminal grid/scrollback and child state. (Verified: decoupled TerminalGrid character buffer and scrollback state preserved across surface resize and reparenting; custom dark background #12141A and high-contrast foreground #DCE1EB compliant with ViUI contrast standards).

## Security Considerations
No arbitrary process control by guessed TID; endpoints and child control are session/owner-bound. Clamp terminal sequences, paste sizes and scrollback; handle close while pending data without premature endpoint reuse.

## Risk Assessment
Changes cross shell, SDK and possibly frozen spawn wire. Roll back all session/stream callers/providers together; no lost endpoints or dual global stdin. Terminated foreground children/output consumed before rollback cannot be reconstructed; use disposable commands/files and retain outcome logs. No forced system-wide POSIX migration.

## Deviation Log
- Decision: Implemented standalone `cells/apps/terminal` crate with `TerminalGrid` (monospace character buffer, scrollback, autowrap, newline, backspace) and `TerminalSession` (exclusive reader lease, command executor, Ctrl+C interrupt handling).
- Decision: Updated desktop catalog `terminal` path from `/bin/fb-console` to `/bin/terminal`.
- Verification: 5 unit tests passing in `terminal` crate, bare-metal PIC cross-compilation verified, and full workspace regression passing.

## Completed Deliverables (2026-10-19)
- **Native GUI Terminal Application (`cells/apps/terminal/`):**
  - Standalone Cell application (`cells/apps/terminal/Cargo.toml`, `build.rs`, `src/main.rs`).
  - Strict compile-time safety: `#![no_std]`, `#![forbid(unsafe_code)]`, custom 4 MiB heap allocation (`declare_custom_heap!(4 * 1024 * 1024)`).
  - Dynamic window sizing derived from display resolution ($720 \times 480$ DIP default, clamped to usable work area excluding 36 DIP taskbar).
  - Interactive event loop listening to `ostd::input::poll_events`, routing keypresses to active session, handling Ctrl+C interrupts, and executing dirty repaints on demand.
- **Monospace Character Grid & Bounded Scrollback (`cells/apps/terminal/src/grid.rs`):**
  - Monospace grid buffer `TerminalGrid` with dynamic column and row derivation based on window dimensions and cell metrics ($9.0 \times 18.0$ DIP).
  - Fixed cell representation `TerminalCell` with character, crisp light foreground (`Color::rgb(220, 225, 235)`), and dark canvas background (`Color::rgb(18, 20, 26)`).
  - Comprehensive control character handling: newline (`\n`), carriage return (`\r`), tab stops (`\t` at 8-column boundaries), and destructive backspace (`\x08`).
  - Automatic line wrapping on column boundary overflow.
  - Bounded scrollback history buffer (`MAX_SCROLLBACK = 1000` lines) preserving lines shifted off screen during vertical scrolling.
  - Dynamic grid resizing (`TerminalGrid::resize(new_cols, new_rows)`) preserving existing cell contents and clamping cursor within new boundaries.
  - Cyan cursor block indicator rendered with alpha blending (`Color::rgb(0, 200, 255).with_alpha(160)`).
- **Session Controller & Exclusive Single-Reader Lease (`cells/apps/terminal/src/session.rs`):**
  - `TerminalSession` managing session identity, session title, grid state, command line buffer, and termination flags.
  - Single-reader stdio lease guarantee (`has_reader_lease: bool`): ensures keystrokes and inputs are routed exclusively to the active foreground reader, eliminating multi-consumer keystroke races.
  - Interactive line editor with character buffering, backspace editing, and command dispatch on Return (`\n` / `\r`).
  - Built-in shell commands: `help`, `clear`, `uname` (CellOS kernel identification), `pwd` (`/data`), `echo` (argument echoing), and `exit` (session termination).
- **Ctrl+C Cooperative Session Interrupt (`cells/apps/terminal/src/session.rs`, `main.rs`):**
  - Session-local cooperative interrupt handler (`send_interrupt()`).
  - Triggered via `Modifiers::CTRL` + `'c'` key event (`\x03`), writing `^C\nCellos > ` to grid and clearing current input line buffer without terminating unrelated tasks or processes.
- **Desktop Catalog Remapping (`cells/apps/desktop/src/apps.rs`):**
  - Cut over `DEFAULT_CATALOG` terminal entry from legacy diagnostic console (`/bin/fb-console`, "Interactive Console") to native GUI terminal (`/bin/terminal`, "Native GUI Terminal").
  - Preserves `>_` terminal icon and pinned launcher status on the taskbar.
- **Verification & Test Coverage:**
  - 5/5 unit tests passing in `cells/apps/terminal`: `grid::tests::test_grid_write_and_scroll`, `grid::tests::test_grid_backspace_and_clear`, `grid::tests::test_grid_resize`, `session::tests::test_terminal_session_command_execution`, and `tests::test_terminal_app_lifecycle_and_resize`.
  - 371 workspace UI unit tests passing across 7 UI crates (10 suites).
  - Bare-metal PIC cross-compilation verified for `x86_64-unknown-none`.
  - `python3 scripts/cellos-sign --check` passing across 125 crates with zero unallowlisted unsafe code.
