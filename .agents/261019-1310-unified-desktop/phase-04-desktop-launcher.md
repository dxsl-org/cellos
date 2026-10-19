---
phase: 4
title: "Fixed bottom shell, app registry and three-section Spotlight"
status: pending
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
- [ ] Taskbar selects the right one of several same-app sessions across frames; empty desktop and minimized apps remain recoverable without shortcut knowledge.
- [ ] Both launcher routes use one Spotlight; Applications/Tasks/System complete real actions, not stubs.
- [ ] Files/Open With and shell use reviewed launch policy; concurrent target-frame launches cannot cross-wire sessions.
- [ ] Reboot/shutdown run only after session consent; refusal leaves desktop and work usable. Wallpaper completes when shared image dependency lands by phase 11.

## Security Considerations
No app-supplied title/path may impersonate catalog identity or system status. Open intent does not grant ambient filesystem/hardware authority. Power, kill and settings writes need exact authorization.

## Risk Assessment
Launch/spawn/provider changes touch stable policy and live app state. Roll back matched Desktop/launcher/catalog/image patch; restore previous boot selection. Already dispatched app launches/poweroff cannot be undone by code rollback; only disposable test sessions may be used. Settings files need versioned reader semantics.

## Deviation Log
Append during execution.
