# Phase 05 slice S5-4 — secrets, hostile escapes, glyphs/theme and consumer removal (CP1 proposal)

Status: **APPROVED; implemented.** The user's “go” is recorded retroactively on 2026-10-23 with the Phase 05 CP2 ([dossier](phase-05-cp2-dossier.md) §10, §16), which also ratifies the `${NAME}`/`${#NAME}` expansion and the builtin `cat` fix as changes outside the named closure. Decisions (2026-10-23): D54-2 theme accepted as `vitui::theme`; D54-3 done as `vitui::ratatui::{core, widgets}`; D54-6 done (renamed `tui-cellos` → `vitui` with modules `backend`/`session`/`event`; `mio-cellos` → `vimio`, `polling-cellos` → `vipolling`; naming rule recorded in `docs/code-standards.md` Law 6). D54-1 analysed (font coverage sufficient, §6), D54-4 accepted (remove `tui-proof` after S5-4 evidence), D54-5 accepted (8-bit C1 stays U+FFFD). The status before implementation was “All S5-4 decisions are made; awaiting "go" for implementation”, with no source change. Targets Phase 05 success criterion 3 ("secret input does not enter terminal history/logs/scrollback; unsupported escape effects are bounded and cannot access clipboard/devices without policy") and closes the phase. Evidence: read-only census 2026-10-23 (citations inline).

## 1. Facts
- **Secret input exists and is mostly sound:**
  - `read -s` takes a SessionV1 echo-off lease (mode bit 2) before reading, refuses if echo control is unavailable, has no redraw or history, and zeroizes its line (`cells/tools/cellbox/src/executor.rs:491-549,2039-2085`).
  - Nothing else echoes: the GUI Terminal and input service never draw typed characters, only drained output (`cells/apps/terminal/src/session.rs:131-207`).
  - History (128 inline + 1,000 global, no persistence) skips `read -s` (`shell.rs:58-79`, `history.rs:12-88`). Scrollback stores output only (`grid.rs:155-172`).
  - Input zeroizes event/raw/paste buffers and seals completed secret-bearing requests to SHA-256 (`input/src/session.rs:47-132`, `main.rs:168-184`, `dispatcher.rs:212-220`).
  - **Residual:** a request's `Outcome` keeps plaintext while that request is executing, before sealing.
  - **Serial:** input refuses echo-off on the physical UART (`input/src/session.rs:471-476,532-534`), so `read -s` fails closed there.
- **Escapes (S5-2):**
  - OSC/DCS/SOS/PM/APC are discarded up to 4 KiB.
  - There is no reply path, so no OSC 52 clipboard, OSC 8 links, title, URL, file or device effect.
  - CSI is limited to 256 bytes and 16 params.
  - 8-bit C1 is not recognized: 0x80–0xbf in ground becomes U+FFFD (`cells/apps/terminal/src/sink.rs:3-320`).
  - Paste: the SDK raw decoder rejects a whole batch containing ESC/C0 inside `ESC[200~…ESC[201~` (`libs/terminal/src/raw_input.rs:171-188`).
  - None of this has a **runtime** proof yet; only host tests exist.
- **Glyphs:**
  - The GUI Terminal rasterizes bundled JetBrains Mono Regular/Bold at 14 px via `GlyphAtlas` (`cells/apps/terminal/src/main.rs:104-173`, `libs/ostd/src/font_atlas.rs:110-151`).
  - **Coverage of box-drawing U+2500–257F, blocks U+2580–259F and rounded corners U+256D–2570 is unproven.** Missing scalars share one missing-glyph entry.
  - Ratatui's `Block::bordered()` uses these characters, so TUI frames may currently show tofu.
- **No theme layer** exists in `vitui`; the Terminal palette is local (`main.rs:168-223`).
- **`tui-proof` removal footprint:**
  - crate and workspace/lock entries;
  - private launch row and its test (`launch_profile/{mod,profiles,tests}.rs`) — three digest pins change;
  - driver `tui.rs`;
  - one cellbox admission test (`typed.rs:1684-1688`);
  - runner and GUI-driver sections;
  - CHANGELOG and S5-3 docs. Archived evidence stays.

## 2. Proposed work
**S5-4a — runtime proof of secrets and hostile output** (fixtures and runners; product code only where a check fails):
1. **GUI, through the frontend:**
   - `read -s SECRET` while a known token is typed. The token must never appear on screen or in scrollback (exact-screen check after scrolling back), in `history` output, on the UART, or in driver logs.
   - `echo ${#SECRET}` proves the value was read (length only).
2. **Serial:** `read -s` on the physical-UART session fails with its refusal and status, and the token is not consumed into history.
3. **Hostile output:** `cat` a host-built `vt-hostile.bin` containing:
   - OSC 52 clipboard set/query, OSC 8 link, OSC 0/2 title;
   - DCS/APC/PM strings over 4 KiB;
   - device queries (DA, DSR 6n, DECRQM), 8-bit C1 CSI, CSI with 17+ params;
   - embedded `ESC[201~`.

   Checks: the screen shows only the expected text runs; the shell receives **no reply bytes** (the next typed command runs exactly as typed); no clipboard or title effect; the prompt is intact.
4. **Paste injection:** a frontend paste containing `ESC[201~`/ESC is rejected as a whole (`WrongEpoch`/`Malformed`, per the SDK), and nothing is executed.
5. **Residual fix:** seal or zeroize the input `Outcome` plaintext at the moment it is consumed, not after completion. Bounded change in `cells/services/input/src/session.rs`, with a host test that the buffer is zero after use.

**S5-4b — glyph coverage:**
1. A host census of the bundled JetBrains Mono for U+2500–259F (plus the common geometric shapes used by Ratatui widgets: `▲▼`, `•`, `…`).
2. If any are missing, the terminal **draws box-drawing and block characters procedurally**: lines, arcs, half and full blocks, computed per cell from the cell metrics, as kitty, WezTerm and Alacritty do. This keeps the grid seamless and independent of font coverage. Other missing glyphs keep the missing-glyph box.
3. A GUI exact-screen check of a bordered Ratatui block and a block-element bar.

**S5-4c — theme layer (optional, D54-2):** `libs/vitui/src/theme.rs (`vitui::theme`)`:
- A `Theme` with a truecolor palette that falls back to 256 and 16 colors from the session's advertised color mode.
- Border sets (plain/rounded/thick).
- Styled building blocks: title bar, status line, selectable list highlight, key-hint footer.

Pure style data on top of Ratatui types; no new rendering engine. `tui-proof` (or its successor) uses it for one GUI screen check.

**S5-4d — import convenience (optional, D54-3):** a `vitui::ratatui` module re-exporting `ratatui_core` and `ratatui_widgets` under one path. A name change only; no dependency change.

**S5-4e — remove `tui-proof`:** after S5-4a–c evidence, delete the crate, row, test, driver module, runner sections and CHANGELOG/S5-3 claims, and re-pin the three launch-profile digests. Regression checks that depended on it move to a real consumer when Phase 06 interactive `top` lands; until then the TUI runtime checks are history-only (archived logs).

## 3. Verification
- Host: input `Outcome` zeroization; glyph census; procedural box-drawing cell tests (each line/arc/block maps to the exact pixel spans for a given cell size); theme fallbacks.
- Private images: terminal runner (393 + new), GUI 6× (252 + new), Phase03 60, Phase02 48, kernel AP lane.

## 4. Decisions requested (recommendations first)
- **D54-1 Glyphs:** *procedural box-drawing/blocks if the census finds gaps* (recommended), or bundle a second font with full coverage (larger image, still font-dependent alignment).
- **D54-2 Theme layer:** *include S5-4c* (recommended, because you asked about "Charm-like" looks), or defer to Phase 06 with the first real consumer.
- **D54-3 Re-export module:** *yes* (small convenience), or no (keep explicit upstream crate paths).
- **D54-4 `tui-proof`:** *remove after S5-4 evidence (plan step 6)* (recommended), or keep it as a maintained example and regression fixture until Phase 06's `top` replaces it.
- **D54-5 8-bit C1:** *keep as U+FFFD (not interpreted)* (recommended; smaller attack surface and spec-consistent), or interpret 0x9b as CSI.

## 5. Rename record (D54-6, done 2026-10-23)
User decision: viTUI = one crate `vitui` that imports Ratatui core+widgets, with modules `vitui::backend`, `vitui::session`, `vitui::event` and (S5-4c) `vitui::theme`. The `*-cellos` suffix is retired and names must not repeat `cellos`. Implemented as a mechanical rename (no behavior change): `libs/vitui`, `libs/vimio`, `libs/vipolling` and all consumers. Root re-exports were removed in favor of module paths; `vitui::ratatui::{core, widgets}` was added. Checks: vitui 15/15, terminal 63/63, tui-proof 9/9; bare-metal checks for all eleven affected packages; runner syntax OK; no digest pin affected. S5-4 will move `tui-proof`/Terminal imports onto `vitui::ratatui` where that avoids a direct Ratatui dependency.

## 6. D54-1 analysis: font coverage (2026-10-23)
User direction: if glyphs are missing, consider switching the font rather than drawing procedurally. A `cmap` census of the bundled fonts (`libs/ostd/assets/fonts/jetbrains-mono/JetBrainsMono-{Regular,Bold}.ttf`, 1,363 code points each, read directly from the font files) gives:

| Range | Covered |
|---|---|
| Box drawing U+2500–257F (incl. rounded corners U+256D–2570) | **128/128** |
| Block elements U+2580–259F (Sparkline/Gauge bars, shades) | **32/32** |
| Geometric shapes U+25A0–25FF | 43/96 (includes ▲ ▼ ■ □ ●) |
| Arrows U+2190–21FF | 35/112 |
| Braille U+2800–28FF (Ratatui Canvas/Chart braille marker) | **0/256** |
| Spot checks | `•` `…` and powerline U+E0B0 present; `✔` U+2714 missing |

**Conclusion:**
- No font switch is needed for borders, blocks or bars, and **procedural drawing is dropped** (S5-4b no longer implements it).
- The only material gap is **Braille**, which Ratatui uses only when an app chooses `Marker::Braille` for Canvas/Chart.
- S5-4 therefore:
  1. keeps JetBrains Mono;
  2. makes `vitui::theme` default chart markers to block/dot markers that the font covers, and documents that Braille renders as the missing-glyph box;
  3. adds a GUI exact-screen check of a bordered block plus a block-element bar. If borders show gaps between rows, the fix is the terminal's cell height/baseline metrics, not the font.
- If a later consumer (Phase 06 `top` graphs) needs Braille, a font change will be evaluated then against concrete candidates. Licence (OFL/permissive), coverage of box+blocks+Braille, and image size impact would be weighed together, as its own decision.

## 7. Implementation record S5-4a–c (2026-10-23)
- **S5-4a:** input seals secret-bearing requests to SHA-256 when consumed (`session.rs` `retained`; host test: pending outcome holds only the digest). Cellbox gains `${NAME}`/`${#NAME}`; the builtin `cat` decode bug found by the hostile fixture (bytes after an invalid UTF-8 byte near EOF were dropped) is fixed with host tests. Fixture `scripts/phase05_vt_hostile.py` (shared by both runners). Phase04 terminal runner **408/408 checks, 124 markers** (`secret-read-*`, `hostile-*`, `paste-*`); Phase02 serial runner 48 passes plus the physical-UART `read -s` refusal check.
- **S5-4b:** no blank pixel rows/columns between stacked `│ ─ █ ▀▄` cells (host render through the guest rasterizer); cell metrics unchanged. Open finding: the GUI grid's 80th column lies outside the 720 px window (`grid_dims` ignores the 8 px padding). GUI exact-screen + row-gap scenario added (`tui-proof --theme`), not yet run.
- **S5-4c:** `vitui::theme` (explicit `ColorMode`: capabilities advertise no colour mode), tui-proof and the Terminal round-trip test on `vitui::ratatui` without direct Ratatui deps.
- Deviations: serial `read -s` types no token (the refusal precedes any key read; type-ahead after a refusal is ordinary input). The terminal runner judges hostile output by exact relayed bytes and absence of injected input (no emulator in that loop); screen effects are the GUI runner's claim.

## 8. Results S5-4a–c (2026-10-23; logs `reports/s5-4-evidence/`)
- **Implemented:**
  - Secret/hostile/paste driver scenarios.
  - `scripts/phase05_vt_hostile.py`.
  - Input service: text/raw/paste are stored only as a SHA-256 digest at receipt, with no plaintext in `Outcome` even while running.
  - Cellbox `${NAME}`/`${#NAME}` expansion with the copy wiped.
  - Builtin `cat` invalid-UTF-8 fix: it dropped bytes after an invalid byte near EOF, a real defect found by the hostile check.
  - `vitui::theme`: truecolor with 256/16 fallback, borders, helpers, block markers by default.
  - `tui-proof --theme`; tui-proof and Terminal moved onto `vitui::ratatui`, with no direct Ratatui dependency.
- **Verified:**
  - Host: app-cellbox 97, service-input 34, vitui 22, tui-proof 12, terminal 63.
  - **Phase04 terminal runner 408/408 (124 markers):** the secret token is never relayed, in `history` or on the UART; hostile bytes produce no reply and the next command runs as typed; injected paste is refused whole with a clean-paste positive control.
  - **Phase02 serial 48:** `read -s` on the physical UART is refused, status 1, `${#SECRET}`=0, `history` shows only typed lines.
  - **GUI 6/6 at 306/306:** hostile-output screen, theme screen and row-gap checks.
  - Phase03 60/60.
- **Row gaps:** none (host measurement through the same font path; vertical glyph box 18.48 px in 18 px cells). There is a faint horizontal seam (8.4 px advance in a 9 px cell); fonts and metrics unchanged.
- **Open defect found (needs decision):** the GUI Terminal grid ignores the 8 px padding. 8 + 80×9 = 728 px > 720 px window, so **column 80 is never drawn** while apps are told the width is 80. The theme screen uses 44 columns to avoid it. The fix (grid from window minus padding → 79 columns at 720 px) changes every GUI expected screen.
- **Remaining:** S5-4e (remove `tui-proof`).

## 9. GUI grid fix (user-approved 2026-10-23; logs `reports/s5-4-gridfix-evidence/`)
- `grid_dims` now subtracts the renderer's left/top padding, and start, resize and re-attach all use the one function. 720×480 → **79×26** (8+79×9 = 719 ≤ 720); the session is told the drawn size. Windows below the 40×12 session minimum still lose trailing cells (session limit, unchanged).
- New `tui-proof --frame`: a rounded border over the full reported size. GUI checks `FRAME-*` include "last column inked on every row", with border-one-column-left, one-char and before-frame controls. Existing expectations moved to 79 columns; none were weakened.
- Verified: terminal host 64, tui-proof 15; **GUI 6/6 at 323/323**; Phase04 terminal 408/408. The §8 "open defect" is closed.

## 10. Results S5-4e (2026-10-23; logs `reports/phase-05-final-evidence/`)
- **Removed (D54-4):**
  - the `cells/tests/tui-proof` crate and its workspace/lock entries;
  - the private `cellbox -> /bin/tui-proof` row under `phase04-driver-profile` and its test (the launch-profile files are back at their S2-T/W CP2 bytes);
  - the driver's TUI module and the runner's two proof-UART checks;
  - the GUI `TUI-*`, `THEME-*` and `FRAME-*` scenarios. The GUI kernel is no longer built with `phase04-driver-profile`.
- **Kept:**
  - the selective-TUI negatives `oneshot-no-tui-bytes` and `top-pipe-batch`, moved into the driver's main module;
  - the soft-reset, secret, hostile-output and paste checks;
  - the 79-column expected screens;
  - `vitui` (incl. `theme`) and its host tests;
  - the stdout-redirect admission test, now on `/bin/echo`.
- **Lost:** the GUI last-column runtime check (`FRAME-2`). The §9 grid fix now has host tests plus archived evidence only, until Phase 06 interactive `top` is a real consumer.
- **Verified:**
  - Phase04 terminal 397/397 (115 markers);
  - GUI 6/6 at 259/259 (canonical cellbox/terminal/`kernel_fs.img` hashes unchanged);
  - Phase03 60/60, Phase02 48, kernel AP 38/38;
  - host (implementer report): app-cellbox 97, terminal 64, vitui 22, launch_profile 28/28 with and without the feature.
- CP2 approved 2026-10-23 ([dossier](phase-05-cp2-dossier.md) §16); rollout stays blocked.
