# Phase 01 — Terminal Emulator Cell (evolve fb-console → vi-terminal)

## Context Links
- Plan: [plan.md](plan.md)
- Reuse template: `cells/apps/robot-dashboard/src/main.rs:86-105` (ViSurface + FramebufferCanvas)
- Evolve from: `cells/apps/fb-console/src/main.rs` (log-ring read loop + scroll)
- Specs: `docs/specs/06-graphics.md`, `docs/specs/15-kernel-boundary.md` (Tier-1 native, no kernel touch)

## Overview
- **Priority:** P1 (the deliverable)
- **Status:** pending
- **Description:** Turn the one-way `fb-console` log mirror into a proper VT100-subset terminal:
  parse the ANSI escapes the shell actually emits, render via ViUI's `FONT8X8` (drop the
  hand-rolled font), maintain a scrollback text grid, and keep zero-copy Grant rendering.
  Output-only — keyboard keeps flowing to the shell via the existing input-focus path, so the
  result is a full interactive shell on HDMI with no UART cable.

## Key Insights (verified)
- Shell session already lands in the kernel **LOG_RING** via `sys_log` (`task.rs:1498`); `ReadLog=237`
  drains it (`task.rs:1486`, `syscall.rs:2641`). No new syscall needed — **zero Law 1 touch**.
- Minimal ANSI set is tiny: `\x1b[2J` (erase display), `\x1b[H` / `\x1b[1;1H` (cursor home),
  `\x08` (backspace), `\n`, `\r`, `\t`. **No colors/SGR are emitted today** — implement swallow-and-ignore
  for unknown CSI (forward-compat); real color support is a Phase 03 stretch.
- `FramebufferCanvas::draw_text(pos, &str, TextStyle)` (`viui/canvas.rs:274`) uses `FONT8X8`
  (monospace 8×8, 95 chars 0x20–0x7E) — reuse it; **do not hand-roll a rasterizer** (deletes
  `fb-console/src/font.rs`).
- LOG_RING is **single-consumer destructive-drain** — vi-terminal must **replace** fb-console in the
  disk + init spawn list, not coexist.

## Requirements
**Functional**
1. Create a full-screen `ViSurface` (BGRA8888), clear to background.
2. Poll `sys_read_log` every loop; feed bytes to a VT100 state machine.
3. Parse: `LF`(new line + scroll), `CR`(col→0), `BS`(cursor left; the `\x08 \x08` idiom then
   overwrites with space + BS again — handled naturally by cursor-move + write), `TAB`(→ next 8-col stop),
   `CSI 2J`(clear grid), `CSI H`/`CSI r;cH`(cursor home/position). Unknown CSI → consume the full
   sequence and ignore.
4. Maintain a text grid `cols×rows` (cols=W/8, rows=H/8) plus a scrollback ring (cap ~1000 lines);
   visible window = tail of scrollback.
5. Render dirty rows via `FramebufferCanvas::draw_text` + `ViSurface::damage`.
6. Supersede fb-console: init spawns `vi-terminal` instead; fb-console no longer spawned.

**Non-functional**
- Cell: `#![forbid(unsafe_code)]`, no `mod.rs`, files < 200 lines (split into modules).
- No `libs/api`/`libs/types` change (no Law 1).
- Render latency: drain full ring + repaint within one compositor tick under normal shell output.

## Architecture
Module split (Law 5, <200 lines each), evolving `cells/apps/fb-console/src/`:
- `main.rs` — surface setup, `sys_read_log` loop, wiring parser→grid→render.
- `vt_parser.rs` — `enum State { Ground, Esc, Csi }`; `fn feed(&mut self, byte, &mut Grid)`; emits grid ops.
- `grid.rs` — `struct Grid { cells: scrollback ring, cursor:(row,col), cols, rows }`; ops:
  `put_char`, `newline`, `carriage_return`, `backspace`, `tab`, `clear`, `move_cursor`, `scroll`.
- `render.rs` — `fn paint(grid, &mut ViSurface)`: build `FramebufferCanvas` over `pixels_mut()`,
  `fill_rect` bg, `draw_text` per dirty row, then caller `damage`s.
- **Delete** `font.rs` (replaced by `ostd::font::FONT8X8` via ViUI canvas).

**Law 2:** the loop is synchronous (poll ring → mutate grid → paint); no borrowed buffer crosses an
async boundary. `ViSurface` is `!Send`, stays on the cell task.

## Related Code Files
**Modify / evolve**
- `cells/apps/fb-console/src/main.rs` — rewrite loop to parser+grid+render.
- `cells/apps/fb-console/Cargo.toml` — (if renaming) crate name; add `viui` dep.
- `cells/tools/init/src/main.rs:199-201` — spawn `/bin/vi-terminal` (or keep `/bin/fb-console`); **remove** the second display cell so only one drains the ring.
- `gen_disk.ps1:100,199,266,446` — build/sign/table-map the (renamed) binary.
- `Cargo.toml:92` — workspace member path (only if renamed).

**Create**
- `cells/apps/fb-console/src/vt_parser.rs`, `grid.rs`, `render.rs`.

**Delete**
- `cells/apps/fb-console/src/font.rs`.

**Rename decision (severable):** recommended `fb-console` → `vi-terminal` for identity, but the
functional work is name-independent. If rename friction is high, keep the crate/bin name and ship
the terminal under `/bin/fb-console`. Do NOT create a parallel cell (violates "update existing").

## Implementation Steps
1. Add `viui` dependency; import `FramebufferCanvas`, `TextStyle`, `Color`, `ostd::font`.
2. Write `grid.rs` (scrollback ring + cursor ops) with host unit tests for wrap/scroll/backspace/tab.
3. Write `vt_parser.rs` state machine; host unit tests feeding the exact byte sequences from
   `commands.rs:20` (`\x1b[2J\x1b[1;1H`) and `async_utils.rs:59` (`\x08 \x08`).
4. Write `render.rs`; rewrite `main.rs` loop: create surface, drain `sys_read_log`, feed parser,
   paint dirty rows, `damage`.
5. Add a startup probe (emit-once) `"[vi-term] ready cols=.. rows=.."` for the boot oracle.
6. Swap init spawn + gen_disk to the terminal; ensure fb-console is not also spawned.
7. Build the riscv64 disk; boot `run-gui.ps1`; visually confirm prompt + typed command render.

## Todo List
- [ ] `grid.rs` + host tests
- [ ] `vt_parser.rs` + host tests (real byte sequences)
- [ ] `render.rs` using `FramebufferCanvas::draw_text`
- [ ] rewrite `main.rs` loop; delete `font.rs`
- [ ] startup probe line
- [ ] init + gen_disk swap (single ring consumer)
- [ ] boot on `run-gui.ps1` (riscv64), confirm render

## Success Criteria (boot-verifiable oracle)
- **Serial oracle (automatable, headless):** boot with `-serial tcp`; assert `wait_for("[vi-term] ready")`.
  Under `#[cfg(feature="test-hooks")]`, terminal logs post-parse grid state
  (e.g. `"[vi-term] grid[0]=ViCell >"`) after processing a driven `clear` + `echo HELLO`; assert the
  grid row equals expected — verifies parse→grid end-to-end without pixels (mirrors
  `compositor-cursor.rs` "[compositor] cursor at" pattern).
- **Visual oracle (manual, run-gui):** on riscv64 `-display gtk`, the HDMI window shows the shell
  prompt; a command typed on the (virtio) keyboard executes and its output renders on screen;
  `clear` blanks the grid.
- fb-console no longer spawned; only one ring consumer.

## Risk Assessment
| Risk | L×I | Mitigation |
|---|---|---|
| Two ring consumers (fb-console + terminal) steal bytes | Med×High | Single spawn; remove fb-console from init/gen_disk |
| 8KB ring drops oldest during output bursts | Med×Med | Drain fully each loop without yielding while `n>0`; consider bump to 16/32KB (kernel const) |
| Test probe re-enters ring (feedback echo) | Low×Low | Emit-once at startup; test-hooks probes tolerated |
| `\x08 \x08` erase renders wrong | Low×Med | BS = cursor-left only; space overwrites; unit-tested |
| FONT8X8 gap for non-0x20–0x7E bytes | Low×Low | Substitute blank/`?` glyph |
| Ring mixes kernel log + shell output (noisy) | Med×Low | Accept for MVP (kernel quiet post-boot); fd separation = Phase 03 |

## Security Considerations
- Manifest keeps `ReadLog` (allowlist bit 54) + compositor Grant caps only; no block_io/network/spawn.
- Read-only Grant share to compositor (existing `ViSurface` contract) — compositor cannot write cell pixels.
- No new syscall / no capability surface growth.

## Next Steps
- Phase 02 documents the as-built terminal + tier re-scope.
- Phase 03 (deferred) adds interactive session ownership + clean transport (Law 1).
