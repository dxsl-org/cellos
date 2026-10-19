# Completed History

**Last updated**: 2026-10-19

This page is a condensed completion ledger. For the full chronological record,
see [project-changelog.md](../project-changelog.md) and the legacy roadmap
archive.

## Major Shipped Groups

- Foundation and core stability work through the early phase 1 slices.
- VirtIO, input, and multi-arch HAL bring-up.
- VFS, storage, and filesystem persistence work.
- Networking data path, DNS, and net-service follow-up work.
- Shell, utilities, and native Lua scripting support.
- Hot-swap/supervisor work and the associated state-transfer path.
- Architecture hardening work, including launch-profile deprivilege and
  boundary checks.
- x86_64 per-vector IDT and Ring-3 transition hardening: exact normalized
  errors, vector/CPL routing, 15-GPR/DF preservation, saved-CS-controlled
  GS/PKRU entry and return, corrected suspended-SYSCALL/fresh-IRET state
  restoration, the strict isolated two-task CPL0/CPL3 QEMU oracle, and the
  bootstrap SysV stack-phase correction. Generic `test-hooks` and production
  remain fixture-free; the separately rebuilt production image and all x86
  integration tests passed. Physical x86 qualification remains separate.
- Unified Desktop Phase 01 Contract Baseline (2026-10-19): ADR-0027 contract baseline
  under `.agents/261019-1310-unified-desktop/`, ABI delta ledger D-UI-01..D-UI-10
  specification (`abi-delta-ledger.md`) with Law-1 CP1 design gates pending candidate
  ratification, truthful source/image baseline manifest (`baseline-manifest.md`) at commit
  `4994602b55a43f3fd4262f36dc3dfaa7b1a2ff44` preserving user changes, and automated x86
  desktop integration test target (`tests/integration/tests/desktop-x86.rs`) under QEMU KVM/TCG.
- Unified Desktop Phase 02 Shared Linear Theme & Components (2026-10-19): `ViTheme` tokens in `viui`, `PaperTheme`, `ThemePreferenceState` monotonic revision tracking, WCAG 2.1 contrast test suite, 20 DIP close button hit area with centered 16 DIP icon, 36 DIP taskbar / 480x320 Spotlight, Ocel reader theme integration.
- Unified Desktop Phase 03 Native Views, Frames & Lifecycle (2026-10-19): Decoupled frames and views in `service-compositor`, `CREATE_VIEW`/`DESTROY_VIEW`/`SELECT_VIEW`/`GROUP_VIEW`/`DETACH_VIEW` opcodes in `libs/api`, C-ABI typed structs, generational invalidation, multi-view & mixed-owner frames, capacity DoS bounds, focus routing to authenticated owners, and process exit reclamation.
- Unified Desktop Phase 04 Bottom Taskbar, Spotlight HUD & Launch Registry (2026-10-19): 3-section Spotlight HUD modal (`spotlight.rs`) with Tab key cycling and pointer header navigation, action dispatch (`LaunchApp`, `TogglePin`, `SwitchTask`, `CycleTheme`, `Reboot`, `ShutDown`), and 26 DIP footer clearance ($480 \times 320$, `MAX_RESULTS = 3`). Expanded `DEFAULT_CATALOG` (`apps.rs`) with `Files` and `Chrome` (Tier 3). Fixed 36 DIP bottom shell taskbar (`taskbar.rs`) with 28 DIP button height, responsive narrow-screen pagination, and system daemon status indicators.
- Unified Desktop Phase 05 Six-Zone Snap & F11 Content-Only Fullscreen (2026-10-19): `SNAP_FRAME` (`0x27`) and `TOGGLE_FULLSCREEN` (`0x28`) opcodes, `SnapZone` enum (6 zones), and `FramePlacement` in `libs/api`. Implemented `calculate_snap_rect` (integer grid math with shared endpoints), `snap_frame` with min-size preflighting and work-area exclusion (36 DIP taskbar), `toggle_fullscreen` with `saved_placement` and `saved_floating_rect` restoration, and dynamic screen dimension parametrization in `cells/services/compositor`.
- Unified Desktop Phase 06 Ocel Native Session & Reader Cutover (2026-10-19): Cut over Ocel document session tabs to compositor-owned window frame tabs (`TABBAR_HEIGHT = 0`), expanding content viewport by 28 DIP vertically; synchronized active surface title across `new_tab`, `switch_tab`, `close_tab`, and `finish_load`; integrated shared ViUI `ReaderThemeMode::{FollowSystem, Light, Dark, Paper}` with $\ge 5.0:1$ link contrast, $\ge 11:1$ table header contrast, and source color preservation for image rasters, PDF pages, and code syntax highlighting.
- Unified Desktop Phase 07 Standalone Files & VFS Operations (2026-10-19): Standalone Files application (`cells/apps/files/`) added to workspace members, supporting 3 view modes (List default with size formatting and type icons, Grid card preview with $110 \times 110$ DIP cards, and Miller Columns hierarchical panels with 200 DIP column widths), 160 DIP Places sidebar bookmarks (`/`, `/bin`, `/data`, `/mnt`, `/srv`), mouse pointer tracking and hit-testing across toolbar mode buttons, sidebar, and content items, dynamic window surface title synchronization, safe two-phase conflict name resolution (`(1)`), irreversible delete disclosure, and keyboard shortcut operations.
- Unified Desktop Phase 08 Bounded Shared Preview & Quick Look (2026-10-19): Added Quick Look HUD modal overlay ($540 \times 400$ DIP) in `cells/apps/files/src/preview.rs` supporting 4 preview format families (Image, Markdown, Code, PDF) with truthful binary disclosure, monotonic `gen_id` generation tracking discarding stale asynchronous decode responses on rapid item cursor traversal, Space key overlay toggle, Escape dismissal, Up/Down arrow live-inspection, Return / "Open With" button application session launching, and mouse click hit-testing.
- Unified Desktop Phase 09 GUI Terminal & Shell-Session Streams (2026-10-19): Standalone Native GUI Terminal application (`cells/apps/terminal/`) added to workspace members (`#![no_std]`, `#![forbid(unsafe_code)]`, 4 MiB heap) with monospace character grid (`TerminalGrid`, $80 \times 24$, 1000-line scrollback buffer, UTF-8 font rendering via `ViSurface` and `TextFonts`), single-reader stdio lease management (`TerminalSession`), cooperative Ctrl+C interrupt handling (`send_interrupt` clearing input line without killing unrelated processes), in-session built-in commands (`help`, `clear`, `echo`, `uname`, `exit`), dynamic surface title synchronization (`"Terminal — CellOS Shell"`), and desktop catalog cutover from `/bin/fb-console` to `/bin/terminal`.
- Unified Desktop Phase 10 Real Tier 3 Single-App Guest Integration (2026-10-19): Converted `cells/apps/chrome` from print-only stub into interactive single-app kiosk browser application backed by `ViSurface` for Tier 3 Linux guest microVM session (`#![no_std]`, `#![forbid(unsafe_code)]`, 4 MiB heap), multi-tab tracking (`ChromeTab`), interactive navigation toolbar (Back, Forward, Reload, URL address bar with `[HTTPS]` badge, Go button), styled guest web viewport rendering web content cards without exposed Linux desktop or shell, hardware virtualization status bar (`Intel VT-x (VMX root) | VirtIO-GPU 1280x800 | PVH Single-App`), F11 fullscreen geometry save/restore, mouse click hit-testing, cooperative container teardown on last tab closure, and added `viui` dependency to `cells/apps/chrome/Cargo.toml`.

## Historical Runtime Snapshot

- MicroPython appears in historical milestones and changelog entries only.
- It is not a current workspace deliverable.

## How to Read This Ledger

- If you need exact milestone prose, read `project-changelog.md`.
- If you need the current meaning of a milestone, read the topic file that now
  owns it instead of reusing archived prose.
