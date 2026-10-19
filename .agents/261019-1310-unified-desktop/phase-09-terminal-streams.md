---
phase: 9
title: "GUI Terminal and isolated shell-session streams"
status: pending
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
- [ ] Two same-app Terminal views and independent shell children stream simultaneously without cross-talk; real CLI output is not fb-console logs.
- [ ] EOF/backpressure/reader death/interrupt/resize/foreground exit semantics observable; unsupported interrupt is explicit, force-stop remains separate.
- [ ] Existing serial and SSH paths remain usable; native GUI launch from terminal and return preserve correct session focus.
- [ ] Theme changes and view reparenting preserve terminal grid/scrollback and child state.

## Security Considerations
No arbitrary process control by guessed TID; endpoints and child control are session/owner-bound. Clamp terminal sequences, paste sizes and scrollback; handle close while pending data without premature endpoint reuse.

## Risk Assessment
Changes cross shell, SDK and possibly frozen spawn wire. Roll back all session/stream callers/providers together; no lost endpoints or dual global stdin. Terminated foreground children/output consumed before rollback cannot be reconstructed; use disposable commands/files and retain outcome logs. No forced system-wide POSIX migration.

## Deviation Log
Append during execution.
