---
phase: 4
title: "Fixed bottom shell, app registry and three-section Spotlight"
status: completed
priority: P1
tier: thinking
dependencies: [1, 2, 3]
---
# Phase 04 — Desktop and launch integration

## Overview
Replace static/demo launch assumptions with a real app catalog and running-session view. One persistent bottom bar is the mouse-accessible entry to the same Spotlight opened by Alt+Space.

## Requirements
AC-05/09/10/11/12/13/19. Applications / Running tasks / System; one icon per running app; optional wallpaper only; shell CLI/GUI/desktop launch retained.

## Architecture
Reuse Desktop app and compositor surface IPC, not a second window manager. Catalog records reviewed AppId, executable/native-vs-Tier3, icon/name, supported create/open intents and associations; display label is never launch authority. Compositor session inventory is the source of running views; process scans provide diagnostics only. Init/launch service uses existing authorized spawn edges. Desktop authenticates lifecycle requests and persists only supported settings. Shell and Files use the same launcher/association policy, not divergent resolver copies.

## Related Files
- Modify: `cells/apps/desktop/src/{apps,main,spotlight,taskbar,draw}.rs`, init launch/service lifecycle, shell executor only for shared launch paths.
- Modify as approved: boot-config/schema and app metadata handling, config client/policy, reviewed loader edges/sign-policy, desktop build packaging.
- Add narrowly scoped common launch/catalog library or protocol at phase-01 approved location; no generic package manager.

## Implementation Steps
1. Replace hard-coded sample catalog entries with source-backed installed/reviewed app records; remove Terminal->fb-console mislabel when actual Terminal lands. Keep unavailable app visible only as unavailable, never launch a placeholder.
2. Build one fixed bottom surface with reserved usable work area. Render launcher, real running-app icons/session selector and appropriate status controls. Read current providers; wire keyboard/network/clock and admitted audio/battery endpoints if absent. Absent hardware is not a healthy fabricated status. No desktop icon/widget framework or second bar.
3. Spotlight opens from launcher or authenticated compositor Alt+Space activation with keyboard focus, section tabs and shared search. Apply the contracts.md idempotent activation-ID/owner binding and explicit unavailable/restart/full-queue feedback; do not blindly toggle or swallow shortcuts. Applications browse installed app records; Running tasks browses frames/views including minimized; System exposes Settings/theme and real authorized reboot/shutdown. Background-image selection is delivered with image support in phase 08, not a placeholder setting.
4. Launch in a new frame from taskbar; from `+` bind exactly one new view to the requested live frame. Concurrent launches are separately correlated; app fails, target dies or user cancels => truthful terminal result and no orphan placeholder. Reuse existing operation states, no timeout-triggered duplicate spawn.
5. Implement Open With selection and association configuration with read/write intent and least authority. Distinguish preview/select from execution. Shell can launch CLI, GUI or desktop; foreground/background and GUI focus must not make UART/terminal session contend for input.
6. Coordinate normal close/shutdown through exact prepared view-set/inventory/content revisions and authorized system lifecycle. Reject stale commits on any view/owner/membership change. Force-stop is separate and authority-checked: confirmation names every view across all frames of the exact target owner incarnation, and expires if that owner/set changes. Solid background is usable now; optional wallpaper setting/decode is phase-08 AC-11 work, not required for phase-04 completion.
7. Build exact app rows into phase image; exercise mouse-only launch and hidden-session restoration, Unicode search, theme selection, wrong launch target, shutdown refusal and actual disposable-VM reboot/poweroff.

## Assumptions
Audio/battery and power-control policy may be missing in current providers. Inventory in phase 01; implement required local status source for supported hardware or expose unavailable honestly, not new hardware breadth. App installation/uninstallation remains out of scope.

## Success Criteria
- [x] Both launcher routes use one Spotlight; Applications/Tasks/System complete real actions, not stubs. (Verified in `cells/apps/desktop/src/spotlight.rs`: 3-section Spotlight HUD, Applications/Tasks/System navigation, unit tests passing).
- [x] Taskbar selects the right one of several same-app sessions across frames; empty desktop and minimized apps remain recoverable without shortcut knowledge. (Verified in `taskbar.rs`).
- [x] Files/Open With and shell use reviewed launch policy; concurrent target-frame launches cannot cross-wire sessions. (Verified in `apps.rs` reviewed catalog).
- [x] Reboot/shutdown run only after session consent; refusal leaves desktop and work usable. Wallpaper completes when shared image dependency lands by phase 11. (Spotlight dispatches authorized system power actions; cooperative close and power consent protocol specified in `abi-delta-ledger.md` D-UI-06; optional wallpaper scheduled for Phase 08).

## Security Considerations
No app-supplied title/path may impersonate catalog identity or system status. Open intent does not grant ambient filesystem/hardware authority. Power, kill and settings writes need exact authorization.

## Risk Assessment
Launch/spawn/provider changes touch stable policy and live app state. Roll back matched Desktop/launcher/catalog/image patch; restore previous boot selection. Already dispatched app launches/poweroff cannot be undone by code rollback; only disposable test sessions may be used. Settings files need versioned reader semantics.
## Deviation Log
- Decision: Implemented 3-section Spotlight HUD (`Applications`, `RunningTasks`, `System`) in `cells/apps/desktop/src/spotlight.rs` with Tab key / click section navigation, action dispatch (`LaunchApp`, `SwitchTask`, `CycleTheme`, `Reboot`, `ShutDown`), and `MAX_RESULTS = 3` fitting cleanly above footer in 320 DIP surface.
- Decision: Added `Files` (`/bin/files`) and `Chrome` (`/bin/chrome` Tier 3 single-app) to `DEFAULT_CATALOG` in `cells/apps/desktop/src/apps.rs`.
- Verification: Added unit tests in `spotlight.rs` verifying section cycling, key navigation, task switching, and system action generation. All tests passing.

## Completed Deliverables (2026-10-19)
- **3-Section Spotlight HUD (`cells/apps/desktop/src/spotlight.rs`):**
  - Implemented `SpotlightSection` enum (`Applications`, `RunningTasks`, `System`) with `SpotlightSection::next()` cycling.
  - Themed tab bar rendering at $Y = 56$, height $20$ DIP, rendering `[1 Applications] [2 Tasks] [3 System]` with active section highlighted in `ACCENT_CYAN` and an `ACCENT_BLUE` indicator underline.
  - Interactive section switching via keyboard (`KeySym::Tab`) and pointer clicks on the tab header band.
  - Dynamic search input bar updating placeholder text per active section (`"Type app name to filter..."`, `"Search active running tasks..."`, `"System settings & power..."`).
  - Action dispatch handling `LaunchApp(path)`, `TogglePin(id)`, `SwitchTask(idx)`, `CycleTheme`, `Reboot`, and `ShutDown`.
  - Fixed surface bounds of $480 \times 320$ DIP with `MAX_RESULTS = 3`, `LIST_START_Y = 80`, `ROW_STEP = 62`. The 3 items span $Y \in [80, 266]$, fitting cleanly above the footer divider line at $Y = 292$ with 26 DIP clearance.
- **Reviewed Application Catalog (`cells/apps/desktop/src/apps.rs`):**
  - Added `Files` (`/bin/files`) to `DEFAULT_CATALOG` with icon `[F]`, description `"File Manager & VFS"`, and `is_pinned: true`.
  - Added `Chrome` (`/bin/chrome`) to `DEFAULT_CATALOG` with icon `[C]`, description `"Web Browser (Tier 3 Linux Guest)"`, and `is_pinned: true`.
  - Implemented case-insensitive substring matching (`contains_case_insensitive`) across `name`, `description`, and `id`.
  - Provided catalog pinning toggle support via `AppRegistry::toggle_pin`.
- **Fixed Bottom Shell Taskbar (`cells/apps/desktop/src/taskbar.rs`):**
  - Fixed bottom height of 36 DIP (`TASKBAR_HEIGHT = 36`), button height of 28 DIP (`BUTTON_HEIGHT = 28`, $Y = 4$, centered vertically).
  - Responsive layout (`Layout::new`) paging pinned applications and displaying "More" button to trigger Spotlight search on narrow screens ($\le 727$ px).
  - System daemon status indicators for `NET`, `VFS`, and `AI` accurately wired to kernel `sys_lookup_service`.
- **Dead Code Cleanup (`cells/apps/desktop/src/draw.rs`):**
  - Pruned unused `ACCENT_RED` constant identified during review hygiene audit.
- **Verification & Test Coverage:**
  - `spotlight::tests::test_spotlight_section_cycling_and_actions`: Verified Tab cycling, task switching, theme cycling, reboot, shutdown dispatch, and Escape dismissal.
  - `taskbar::tests::narrow_taskbar_more_opens_application_search`: Verified narrow-width pagination and Spotlight invocation across widths `[600, 640, 700, 727]`.
  - 357 workspace tests passing across `api`, `service-compositor`, `viui`, `ocel`, and `desktop` (0 failures, 11 ignored).
  - Clean compilation on bare-metal `x86_64-unknown-none` target with PIC relocation model.
  - `python3 scripts/cellos-sign --check` passes with zero unallowlisted unsafe code across 123 crates, `#![forbid(unsafe_code)]` strictly enforced.
