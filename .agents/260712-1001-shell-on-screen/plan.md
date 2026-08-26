---
title: "Shell-on-screen: Terminal Emulator Cell (HDMI, no UART cable)"
description: "Evolve fb-console into a VT100 terminal cell rendering the shell session on HDMI via ViUI + compositor Grant surfaces."
status: pending
priority: P2
effort: 6 (scheduled) + 8 (deferred P03)
branch: main
tags: [display, terminal, viui, compositor, shell, g1]
created: 2026-07-12
---

# Shell-on-screen — Terminal Emulator Cell

## Goal

Make the interactive shell usable on an HDMI display **without a USB-UART cable**. Today the
shell session is reachable only over serial. This plan delivers a VT100-subset terminal
emulator **Tier-1 native cell** that renders the shell session on screen via the compositor
+ ViUI, reusing SAS/LBI strengths (zero-copy Grant surfaces + input routing).

## Verified reality (grounds the whole plan)

| Claim in roadmap §B | Truth (file:line) |
|---|---|
| "fb_console ✅" (kernel) | **STALE.** Kernel `fb_console.rs` added in `0bcd1833`, **deleted** in `6036f2dd` (Boundary Law P08). Kernel is headless — `GpuFlush=300` forwards to GPU Driver Cell (`syscall.rs:2317`). No kernel text rendering. |
| Tier A = kernel fb_console keyboard relay | **DEAD.** Superseded by userspace `cells/apps/fb-console/` (340 LOC) which mirrors the kernel LOG_RING → HDMI via `ReadLog=237` (`fb-console/src/main.rs:55`), spawned by init (`init/src/main.rs:201`). One-way, no keyboard, hand-rolled 8×8 font. |
| — | Shell output flows `ostd::io::print` → `sys_log(Log)` → `print_user_log` → **LOG_RING** (`task.rs:1498`) + UART. Drainable via `ReadLog=237` (`task.rs:1486`). **Terminal reads the shell session from this ring — no new syscall.** |
| — | Shell reads keyboard via input service focus (`shell/main.rs:64`, `async_utils.rs:42`), opcode `0x10`. Keyboard already works from virtio-keyboard with no cable. |
| — | ANSI emitted by shell+utils: **only** `\x1b[2J\x1b[1;1H` (`commands.rs:20,329,370`), `\x08 \x08` (`async_utils.rs:59`), `\n \r \t`. **No colors, no cursor positioning.** |
| ViUI text rendering ✅ | TRUE. `FramebufferCanvas::draw_text(pos,text,style)` + `ostd::font::FONT8X8` (95 chars, 0x20–0x7E) at `viui/src/canvas.rs:274`, `ostd/src/font.rs:17`. `Color(u32)` BGRA (`canvas.rs:10`). Template: `robot-dashboard/src/main.rs:86-105`. |
| Grant surfaces ✅ | TRUE. `ViSurface::create/pixels_mut/damage` (`ostd/src/display.rs:69-142`); compositor = service 5. |

## Tier verdicts (re-scoped)

- **Tier A (cheap boot-text on screen)** — **ALREADY SHIPS** as the `fb-console` cell (log ring → HDMI). The original kernel-fb_console premise is dead. Nothing new to build; Tier A folds into Tier B (the terminal supersedes fb-console).
- **Tier B (Terminal Emulator Cell)** — **THE deliverable.** Evolve `fb-console` into `vi-terminal`: VT100 subset parser + ViUI font + scrollback grid. Phase 01.
- **Tier C (SSH via Tier 3b VM)** — **OUT OF SCOPE.** Config-only (dropbear in Alpine VM), gated on Tier 3b deployment; needs no plan.

## Primary target

**riscv64** — the only arch that boots the full GPU+compositor+ViUI+input stack today
(`run-gui.ps1:40,45` = `-device virtio-gpu-device` + `-display gtk`; `gen_disk.ps1:380` hardcodes
`riscv64gc`). The virtio-gpu driver is coded for aarch64 too (`virtio-gpu/src/display.rs:159-174`)
but aarch64 disk-gen is unwired and x86 is `-display none`. **aarch64 = documented follow-up.**

## Data flow

```
[unchanged] USB/virtio-keyboard → input service → shell (focused, reads 0x10 events)
[unchanged] shell stdout/echo → ostd::io::print → sys_log(Log) → print_user_log → LOG_RING(8KB)
[NEW]       vi-terminal: sys_read_log(237) → VT100 state machine → text grid + scrollback
                       → FramebufferCanvas::draw_text(FONT8X8) → ViSurface Grant pixels
                       → ViSurface::damage → compositor → virtio-gpu → HDMI
```
MVP is **output-only**: the terminal renders the session; keyboard keeps flowing to the shell
via the existing input-focus path. Full loop, no cable.

## Phases

| # | Phase | Priority | Effort | Status | Law 1? |
|---|-------|----------|--------|--------|--------|
| 01 | [Terminal Emulator Cell (evolve fb-console → vi-terminal)](phase-01-terminal-emulator-cell.md) | P1 | 5d | pending | **No** |
| 02 | [Docs — rewrite roadmap §B stale claims](phase-02-docs-roadmap-rescope.md) | P2 | 1d | pending | No |
| 03 | [DEFERRED — Interactive session ownership + Law 1 transport + QMP oracle](phase-03-interactive-session-ownership.md) | P3 | 8d | deferred | **Yes** |

Scheduled = 01 + 02 (6d). Phase 03 is design-captured, not scheduled.

## Dependencies

- Phase 01: blockers = none (all APIs exist). Must land before 02 (docs describe shipped state) and 03.
- Phase 02: depends on 01 (documents the as-built terminal).
- Phase 03: depends on 01; requires user 2x-confirm on Law 1 (libs/api service ID or new syscall).

## Law 1 touchpoints

- **Phase 01 (MVP): NONE.** Reuses `ReadLog=237`, `ViSurface`, compositor ops, `FONT8X8` — no `libs/api`/`libs/types` change.
- **Phase 03 (deferred): YES.** Clean shell↔terminal pipe (fd separation, terminal owns focus)
  needs either a `service::TERMINAL` constant in `libs/api/src/services/` OR a new pipe syscall —
  both Law 1 → require **2x user confirmation**. Deferred precisely to avoid an unforced Law 1 change.

## Top risk

**LOG_RING is a single-consumer, destructive-drain 8KB ring** (`task.rs:1444-1487`). fb-console and
vi-terminal cannot both drain it — they would steal each other's bytes. Mitigation: vi-terminal
**supersedes** fb-console (init spawns one, not both). Secondary: 8KB ring drops oldest on overflow
during output bursts → terminal must drain every loop iteration without yielding while `n>0`.

## Open questions

1. Rename `fb-console` crate → `vi-terminal`, or evolve in place keeping the name? (Rename = clearer
   identity but touches Cargo/gen_disk/init/build.rs; recommended but severable — see Phase 01.)
2. Bump LOG_RING 8KB → 16/32KB to survive output bursts? (kernel const, minor; decide during Phase 01 stress test.)
3. Phase 03 transport: service-ID (libs/api) vs syscall — resolve only if MVP's mixed-log UX proves unacceptable.
