# Phase 05 slice S5-2 — `cellos-vt1` screen in the GUI Terminal (CP1 proposal)

Status: **CP1 APPROVED by the user on 2026-10-22 (“chốt quyết định”): D52-1 full spec §4 output profile; D52-2 cellbox soft reset after a raw-lease child; D52-3 GUI proof by `cat` of a host-built `vt-sample.bin`.** Parent: [Phase 05 entry dossier](phase-05-entry-dossier.md). Contract: [spec.md §4](spec.md).

## 1. Starting state
- `cells/apps/terminal/src/sink.rs` is an incremental, bounded decoder. It handles UTF-8 and `\r \n \b \t`, but it **consumes every ESC/CSI/OSC sequence without effect**: CSI is clamped at 64 bytes and OSC at 256. Its header already names "Full `cellos-vt1` emulation (cursor addressing, SGR, erase)" as Phase 05.
- `grid.rs` is a character-only grid (no attributes, no alternate buffer, no scroll margins, no wide-cell handling). `session.rs` resizes the grid and the SDK host.
- The S5-1 backend emits exactly CUP, ED/EL, SGR and DECSET ?25/?1049/?2004, so a TUI drawn by `tui-cellos` currently shows as raw text fragments in the GUI Terminal.

## 2. Change (closure: `cells/apps/terminal/src/{sink,grid,session,main}.rs`, new `cells/apps/terminal/src/vt.rs` if the parser is split out; `cells/apps/terminal/Cargo.toml` for `unicode-width =0.2.2`)
1. **Parser** (bounds from spec §4):
   - ESC `7/8`.
   - CSI A/B/C/D/E/F/G/H/f, J/K 0/1/2, `m` (reset, bold, normal, inverse, 8/16 colors, 38/48 indexed and RGB), s/u, r, S/T.
   - Private ?7, ?25, ?1049, ?2004.
   - Any other sequence is consumed and ignored. At most 16 parameters and 256 bytes per control; OSC/DCS are discarded until the terminator or 4 KiB (spec value; today's sink stops at 256), with no side effect. State survives chunk edges.
   - No reply bytes are ever written back (no device query answers), and no OSC clipboard, title, URL or file effect.
2. **Screen model:**
   - Cells carry a char plus attributes (fg/bg/bold/inverse).
   - Autowrap, tab stops every 8, scroll margins, primary and alternate buffers (?1049 saves and restores the cursor and primary content), cursor visibility.
   - Width via `unicode-width 0.2.2` `width()` (ambiguous = 1). Combining marks join the previous cell, wide characters take 2 cells, and the cursor never rests on a trailing half.
   - Dimension bounds: rows 1..256, cols 1..512, ≤131,072 cells. Scrollback stays as today, capped by spec (1,024 lines and 1 MiB).
   - The renderer draws attributes and colors with the existing viUI text calls. viUI gains no dependency.
3. **Foreground-loss reset (closes the S5-1 open gap):**
   - When the terminal sees the shell's prompt return after a foreground child, it cannot observe that event directly. The **shell**, as session controller, already restores the foreground in `cellbox` after a child ends.
   - Proposal: in that same restore path, `cellbox` writes the fixed `cellos-vt1` soft reset `ESC[?1049l ESC[?25h ESC[?2004l ESC[0m` to its own stdout, only when the finished foreground child had held a raw lease (the TUI case). The emulator then leaves the alternate screen and shows the cursor even if the TUI process was killed.
   - Closure addition: `cells/tools/cellbox/src/executor.rs` (foreground restore only). No ABI or input-service change.
4. **Resize propagation:** this already exists (`session.rs:141` → SDK host Resize). It is re-verified with the alternate screen active; the window grid re-layout preserves the primary buffer.

## 3. Verification
- **Host (terminal crate):**
  - Parser and screen tests drive real byte streams: fragmented CSI/UTF-8 at every split point; cursor addressing at edges and clamping; erase modes; SGR colors; scroll margins with S/T and LF at the bottom margin; ?1049 enter/exit restoring the primary screen; wide and combining characters, including a wide char at the last column.
  - Hostile input: 17+ params, 300-byte CSI, 5 KiB OSC, NUL/CAN/SUB mid-sequence, invalid UTF-8. The test checks state, the bounds and ground recovery, and that no reply is produced.
  - **Round trip:** draw with `tui-cellos` (S5-1 backend) into a byte buffer, feed it to the emulator, and compare the screen cells with Ratatui's own `Buffer`. This is the contract test between S5-1 and S5-2.
- **Private GUI image (existing runner, 6x like S3/C5):**
  - In the Terminal, `printf`-style output is not available from cellbox `echo`, so a test needs a byte source. Proposal: the S5-3 consumer provides it. In S5-2, a host-built fixture file `/p04-output/vt-sample.bin` of `cellos-vt1` sequences is `cat`'d (the exact-bytes `cat` path exists).
  - The screen is checked with exact-screen glyph references: colored text, cursor addressing, erase, and the alternate screen enter/exit restoring the prompt screen.
  - Existing 173 GUI checks unchanged.
- **Headless/serial:** unaffected (the emulator is GUI-side only). The cellbox soft-reset bytes are checked in the terminal runner: after a raw-lease child ends, the stdout stream contains the exact reset, and none is written after ordinary commands.
- **Regression:** Phase04 terminal (354), Phase03 (60), Phase02.

## 4. Risks
- Glyph references for colored or inverse cells need new rendering fixtures (`scripts/phase04-gui-glyphs`).
- The soft reset after a TUI child is visible output on serial too (harmless control bytes on a serial terminal, but bytes nonetheless). It is gated on the raw-lease case only.
- Larger per-cell state raises Terminal heap use: 131,072 cells × a few bytes, bounded by spec. The heap is checked with the existing `free`/OOM signals; no new limit is invented.

## 5. Decisions requested (recommendations first)
- **D52-1 Scope:** *full spec §4 output profile as listed* (recommended), or a minimal subset (CUP/ED/EL/SGR/?25/?1049) covering only what the S5-1 backend emits (smaller, but `less`-style or remote output stays wrong until later).
- **D52-2 Foreground-loss reset:** *cellbox soft reset after a raw-lease child* (recommended), or the emulator resets on every prompt-shaped output (heuristic; rejected as guessing), or leave the gap to S5-4.
- **D52-3 GUI byte fixture:** *`cat` a host-built `vt-sample.bin` in S5-2* (recommended), or wait for the S5-3 consumer before any GUI proof.

## Results (2026-10-22)
- **Implemented:**
  - `cells/apps/terminal`: `sink.rs` interprets the full spec §4 output profile within its bounds; `grid.rs` is the attributed dual-buffer screen model; `session.rs` refuses an out-of-bound resize while preserving state, and resets modes on session start/exit/fail; `main.rs` renders bg/glyph/bold/inverse/palette and the hidden cursor. New `roundtrip.rs` checks tui-cellos output → emulator against Ratatui's `Buffer`. Uses `unicode-width =0.2.2`.
  - cellbox: `screen_reset.rs` plus `executor.rs`/`main.rs` hooks.
  - Fixtures and runners: `phase03-filter tui-abandon`, 5 driver markers, GUI VT-1..VT-4.
- **Deviations:**
  1. **D52-2 premise false; user approved option A:** cellbox cannot observe a child's raw lease without an ABI change (RestoreForeground reply carries only the epoch and reader). Instead, a fixed-state tracker over relayed child output follows `?1049/?25/?2004`, and the 26-byte soft reset is written only when a mode was left on.
     - Covered: serial foreground children and context-mode/pipeline children.
     - Not covered: legacy `spawn_external`, `hv`, redirected/captured output, and unread bytes after the final read rounds.
  2. **LF also returns to column 0** (spec says CR separate from LF). CellOS has no output line translation, and strict LF would break every existing screen. This needs a spec amendment or ONLCR-style translation later.
  3. One combining mark per cell.
  4. Over-long CSI is abandoned and its excess shown as text; OSC/DCS is discarded for 4 KiB, then the rest is text.
  5. Cursor moves onto a wide character's trailing half snap to its lead cell.
- **S5-3 input (gap):** in the GUI Terminal, cellbox runs children as pipeline stages with EOF stdin, so a GUI TUI child cannot hold a raw lease. A real GUI foreground path is needed.
- **Verified (logs `reports/s5-2-evidence/`):**
  - Host: terminal 62/62, app-cellbox 97/97; bare-metal checks; cellbox ceiling unchanged.
  - Private terminal runner **368/368** (93 markers).
  - **GUI 6/6 runs at 197/197** (5 click + 1 autoload), incl. VT-1..VT-4 exact screens: colors, cursor addressing, erase, alt-screen enter/exit restoring the prompt.
  - Phase03 60/60, Phase02 48.
