# Phase 05 entry dossier — Ratatui backend and GUI terminal emulation

Status: **APPROVED by the user on 2026-10-22 (“chốt”): D5-1 keep `=0.1.0`/`=0.3.0`; D5-2 order S5-1 → S5-2 → S5-3 → S5-4; D5-3 S5-1 CP1 as written in §3.** No source change at approval time. Entry allowed by D-CL3 with Phase 04 `in_progress` (criterion 4 only).

## 1. Starting state (facts)
- `libs/tui-cellos` (`backend.rs` 160 lines, `lib.rs` 60) is a home-made cell buffer with **no Ratatui dependency** (`libs/tui-cellos/Cargo.toml`: terminal-sdk, types, api only). The 2026-10-20 review finding still holds: "no selected Ratatui import".
- GUI Terminal (`cells/apps/terminal/src/grid.rs`, 233 lines) is a plain character grid: write/newline/scroll/backspace/clear/resize. It has **no ANSI/VT parser**; escape sequences from the shell are not interpreted. The real shell bridge exists (S3: `TerminalHost` → `cellbox sh`, merged tty stream).
- `libs/terminal` (terminal-sdk) supplies sessions, modes (Raw/Echo/CursorVisible/AlternateScreen/BracketedPaste), resize/interrupt/EOF control frames, codec and raw input (Phase 02).
- **Dependency probe** (throwaway crate in `/tmp/ratatui-probe`, not in the workspace), using the reviewed pins `ratatui-core =0.1.0` and `ratatui-widgets =0.3.0`, defaults off:
  - Resolves to 40 crates (list in §5).
  - Licenses are all inside `deny.toml`'s allow list: MIT, Apache-2.0, Zlib (`foldhash`) and Unicode-3.0 (`unicode-ident`, build-time only). `ryu` is `Apache-2.0 OR BSL-1.0`, and Apache is chosen.
  - A `#![no_std]` lib rendering `Paragraph`+`Block`+`Table` into a `Buffer` **builds for `x86_64-unknown-none` with `-Z build-std=core,alloc`, PIC and soft-float, no SSE** (`cargo +nightly-2026-05-01 build --release`; finished in 32 s).
  - Not shown: linking into a signed Cell, runtime behavior, heap use.
  - Newer releases exist (`ratatui-core 0.1.2`, `ratatui-widgets 0.3.2`) but were not reviewed.

## 2. Proposed slices (each gets its own CP1 when reached)
| Slice | Delivers | Main files | Gate |
|---|---|---|---|
| **S5-1 Ratatui backend** | Workspace admits the pinned `ratatui-core`/`ratatui-widgets` (defaults off). `tui-cellos` implements `ratatui_core::backend::Backend` (draw changed cells, cursor, clear, size, flush) as ANSI bytes over terminal-sdk's stdout writer, plus event polling over SDK typed input/control frames. The home-made cell buffer is deleted (clean cutover). | `Cargo.toml`/`Cargo.lock`, `libs/tui-cellos/**`, `deny.toml` only if a license entry is missing (none is expected) | Host tests: Backend output bytes for diff/cursor/style/resize; bare-metal check |
| **S5-2 VT screen in GUI Terminal** | A bounded ANSI/VT parser and screen model for the frozen profile: fragmented sequences and UTF-8, CSI cursor/erase/scroll region/SGR color, alternate screen, wrap. Unicode width via `unicode-width 0.2.2` (`width`, ambiguous=1). Unsupported sequences are dropped safely with bounded parameter and length limits. Resize propagates to the session (SDK Resize). | `cells/apps/terminal/src/{grid,session,sink,main}.rs` + new parser module | Host parser tests on fragmented and hostile input; GUI runner screens |
| **S5-3 Proof consumer and selective TUI** | A throwaway standalone TUI app linked only to `terminal-sdk` + `tui-cellos` (not cellbox, not viUI) runs on the serial (desktop off) and GUI surfaces: frames, keys, resize, alternate-screen enter/exit, restoration after exit/Ctrl-C/kill. Negative cases: one-shot commands and pipes stay non-interactive; a TUI request on a non-terminal fails truthfully. The consumer is removed afterwards. | new fixture crate, private runner scenarios | Private-image evidence |
| **S5-4 Secret/escape hardening** | No secret input in scrollback, history or logs (no-echo lease honored by the emulator); clipboard/OSC/device-query escapes ignored by policy; dimension, scrollback and sequence bounds. | terminal emulator + SDK if needed | Host + image checks |

Phase 05 success criteria map to them: 1 (real command output; already true from S3, re-proven after S5-2), 2 (S5-3), 3 (S5-4), 4 (S5-3 independent consumer + headless C5a), 5 (S5-3 negatives).

## 3. S5-1 CP1 request (concrete)
- **Dependencies:** `ratatui-core = { version = "=0.1.0", default-features = false }` and `ratatui-widgets = { version = "=0.3.0", default-features = false }` in `libs/tui-cellos` only. Lockfile updated by Cargo; no facade crate (dependency-review §2 "manifest trap"). No `std`, crossterm, termion, serde, macros, layout-cache or calendar.
- **Backend contract:**
  - `CellosBackend<W: ByteSink>` implements `Backend`. It emits only the profile's sequences: CUP, ED/EL, SGR 16/256/RGB, cursor show/hide, alternate screen on/off, DECSET 2004 bracketed paste. Output is chunked within SDK write bounds.
  - Size comes from the session's latest Resize frame. The backend never probes the terminal with query sequences.
  - Mode entry and exit go through the SDK's mode lease (Raw/AlternateScreen/CursorVisible), with restoration on drop and on session-owner cleanup. The backend issues no ad-hoc escape sequences for modes.
- **Deleted:** the current `tui-cellos` cell buffer types and their tests that pin its own wording.
- **Tests:** host tests on real `ratatui_core::Terminal` draws producing exact byte streams (diff-only redraw, cursor, style reset, resize clear), plus that write-error propagation stops a draw. Plus a `cargo check --release --target x86_64-unknown-none -p tui-cellos` with `-Z build-std` and an audit (`cargo deny check licenses advisories` if `cargo-deny` is installed; otherwise a recorded license list as in §5).
- **Not in S5-1:** GUI emulator, consumers, image runs.

## 4. Decisions requested
- **D5-1 Pins:** *keep the reviewed `=0.1.0`/`=0.3.0`* (recommended; reviewed and probed), or move to `0.1.2`/`0.3.2` (needs a fresh source/feature review first).
- **D5-2 Order:** *S5-1 → S5-2 → S5-3 → S5-4* (recommended; S5-3 needs both the backend and the emulator), or S5-2 first (GUI emulator is visible sooner, but the backend contract is then unproven).
- **D5-3 Approve S5-1 CP1 as written in §3.**

## 5. Probe dependency list (ratatui-core 0.1.0 + ratatui-widgets 0.3.0, defaults off)
allocator-api2 0.2.21, bitflags 2.13.2, castaway 0.2.4, cfg-if 1.0.5, compact_str 0.9.1, darling 0.24.1 (+core, macro; proc-macro), either 1.19.0, equivalent 1.0.3, foldhash 0.2.0 (Zlib), hashbrown 0.16.1 and 0.17.1, heck 0.5.0, ident_case 1.0.1, indoc 2.0.8, instability 0.3.14, itertools 0.14.0, itoa 1.0.18, kasuari 0.4.13, line-clipping 0.3.8, lru 0.16.4, proc-macro2, quote, rustversion, ryu 1.0.23, static_assertions 1.1.0, strsim 0.11.1, strum 0.27.2 (+macros), syn 2.0.119 and 3.0.9, thiserror 2.0.21 (+impl), unicode-ident 1.0.27, unicode-segmentation 1.13.3, unicode-truncate 2.0.1, unicode-width 0.2.2.

## 6. S5-1 results (2026-10-22)
- **`libs/tui-cellos`:**
  - Pinned `ratatui-core =0.1.0` and `ratatui-widgets =0.3.0`, defaults off (28 lock additions; resolved features have no std/crossterm/termion/serde/macros/layout-cache/calendar). The unused `types`/`api` deps were removed.
  - The home-made buffer was deleted; it had no external callers.
  - New `backend.rs`: `ByteSink` (512-byte chunks over the stdout `Endpoint`) and `CellosBackend` implementing `Backend` with CUP/ED/EL/SGR only, diff-tracked cursor/style, no queries; `append_lines` returns Unsupported.
  - New `event.rs`: `Event`/`EventAdapter`; paste is coalesced and redacted in Debug.
  - New `session.rs`: `TuiSession`.
- **Deviation (mode guard):** the input service admits only Raw/Echo/BracketedPaste leases (`cells/services/input/src/session.rs:13`, `MODE_MASK = 1|2|16`). Alternate screen and cursor hiding are therefore emitted as bytes through the SDK `ModeGuard`/`TerminalMode::transition_to` helpers and restored on exit/drop. **Open gap:** if the TUI process is killed, the input owner restores raw/paste, but nothing restores the alternate screen or cursor. S5-2's emulator must reset screen mode on foreground loss/exit (proposed below).
- **Verified:**
  - `cargo test -p tui-cellos --target x86_64-unknown-linux-gnu` **15/15** (re-run by the parent): exact bytes for first draw, diff redraw, style, cursor, resize, erase modes, wide char, control-char sanitizing, chunking, write-error stop, mode restore, event mapping.
  - Bare-metal PIC check and build for `x86_64-unknown-none` pass.
  - `cargo deny` licenses/advisories pass for the crate. The workspace-wide license check fails only on a pre-existing `cells/services/ocel-pdf` `AGPL-3.0-or-later`, which is not this slice.
- **Not exercised:** `TuiSession` at runtime (needs kernel; S5-3).
