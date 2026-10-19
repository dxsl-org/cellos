---
title: "CellOS unified desktop, native sessions and shared Linear theme"
status: in-progress
priority: P1
tier: thinking
created: 2026-10-19
blockedBy: []
blocks: []
---
# Unified desktop implementation plan

## Scope and authority
Implement all approved [ADR-0027](../../docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md) outcomes on Intel x86-64; see [acceptance ledger](acceptance.md), [proposed contracts](contracts.md) and [source recon](scout-report.md). Phase 01 baseline and contract gates are complete; implementation phases (02–11) proceed under Law-1 ABI gates. Existing user changes remain untouched. Phase 01 resolves exact file-owner/ABI admission; this plan does not unlock G2-J, remote/protected/physical or broad legacy programs.

## Phases
| Phase | Deliverable | Depends on | State |
|---|---|---|---|
| 01 | [Baseline and exact contract gates](phase-01-contract-baseline.md) | — | completed |
| 02 | [Shared Linear theme and components](phase-02-system-theme.md) | 01 | completed |
| 03 | [Native views, frames, tabs and lifecycle](phase-03-native-sessions.md) | 01,02 | completed |
| 04 | [Bottom taskbar, Spotlight and launch registry](phase-04-desktop-launcher.md) | 01,02,03 | completed |
| 05 | [Six-zone snap and F11 fullscreen](phase-05-placement-fullscreen.md) | 03,04 | completed |
| 06 | [Ocel native session and reader cutover](phase-06-ocel-cutover.md) | 02,03,04,05 | completed |
| 07 | [Standalone Files and complete VFS operations](phase-07-files-vfs.md) | 01,02,03,04,05,06 | completed |
| 08 | [Shared preview and Quick Look](phase-08-quick-look.md) | 02,06,07 | pending |
| 09 | [GUI Terminal and shell-session streams](phase-09-terminal-streams.md) | 01,02,03,04,05 | pending |
| 10 | [Real Tier3 single-app guest integration](phase-10-tier3-app-session.md) | 01,03,04,05,07 | pending |
| 11 | [Complete image and user-path qualification](phase-11-qualification.md) | 02–10 | pending |

## Overall progress
- **Phase 01 — Baseline and exact contract gates**: **Completed** (2026-10-19).
  - Materialized `abi-delta-ledger.md` (D-UI-01 through D-UI-10 specification, candidate digest, wire layouts, Law-1 CP1 pending ratification).
  - Materialized `baseline-manifest.md` (source baseline, dirty tracked and untracked files inventory with full SHA-256 hashes, toolchain state, verification matrix).
  - Materialized and verified `tests/integration/tests/desktop-x86.rs` and `boot_x86_bios_with_accel` in `tests/integration/src/lib.rs` (tested TCG and KVM, QMP pointer routing + screendump diff, QMP keyboard injection).
  - Reconciled `docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md` scope; updated `contracts.md` and `phase-01-contract-baseline.md`.
- **Phase 02 — Shared Linear theme and components**: **Completed** (2026-10-19).
  - Implemented `ViTheme` semantic tokens in `libs/viui/src/theme.rs` (tabs, taskbar, spotlight, status badges, focus ring, and reader mode).
  - Added `PaperTheme` (#F6F3EB warm sepia palette), `SystemPalette` dynamic enum with boxed dispatch, and `ThemePreferenceState` monotonic revision update validation.
  - Sourced base colors in `LightTheme` and `DarkTheme` aligned with `contracts.md`.
  - Added semantic `reader_link` token ensuring $\ge 5.0:1$ contrast (Light 6.92:1, Dark 5.93:1, Paper 6.04:1); implemented mathematical WCAG 2.1 contrast ratio test suite in `viui` verifying $\ge 4.5:1$ text contrast across Light, Dark, and Paper themes.
  - Reconciled shared geometry across widgets: `WindowChrome` close hit target 20 DIP with centered 16 DIP icon; `desktop` taskbar height 36 DIP (28 DIP buttons); spotlight dimensions 480x320 DIP with `MAX_RESULTS = 3`.
  - Cut over Ocel reader theme (`cells/apps/ocel/src/{draw,doc,main}.rs`) to `ReaderThemeMode::{FollowSystem, Light, Dark, Paper}` backed by shared ViUI palette colors; wired `active_system_palette` atomic dynamic resolution and preserved syntax code colors.
  - Verified 64 tests passing in `viui`, 170 tests in `ocel`, 1 test in `desktop`, and real x86 QEMU runner (`desktop_x86_boots_to_shell_and_compositor`) passing in 18.73s.
- **Phase 03 — Native views, frames, tabs and lifecycle**: **Completed** (2026-10-19).
  - Materialized D-UI-01 wire protocol specifications and typed structures in `libs/api/src/services/display.rs` (`CreateViewRequest`, `ViewOpResponse`, `ViewRef`, `FrameRef`, `ViewOpResult`, and opcodes `0x20..=0x26`). Enforced compile-time size invariants (`size_of::<CreateViewRequest>() == 64`, `ViewOpResponse == 20`, `ViewRef == 8`, `FrameRef == 8`) and natural 8-byte alignment (`file_grant_id` at offset 32).
  - Implemented `FrameRegistry`, `Frame`, and `View` in `cells/services/compositor/src/surface_table/frame.rs` supporting multiple views per process, mixed-owner frames, opaque monotonically allocated identifiers, and hard resource admission limits (`MAX_VIEWS_PER_FRAME = 32`, `MAX_FRAMES = 64`).
  - Implemented request dispatch and handling in `cells/services/compositor/src/message_handler/view.rs` for `CREATE_VIEW` (`0x20`), `DESTROY_VIEW` (`0x21`), `SELECT_VIEW` (`0x22`), `GROUP_VIEW` (`0x23`), and `DETACH_VIEW` (`0x24`).
  - Generational validation and invalidation: strictly validated `(frame_id, frame_generation)` and `(view_id, view_incarnation)` on every view/frame operation with `ViewOpResult::StaleGeneration` rejection; enforced monotonic generation increments on attach, destroy, group, detach, and owner death.
  - Surface geometry synchronization and decoration dirtying: updated backing surface positions on create, group, and detach via `move_to`, dirtying decoration regions for immediate compositor repaint.
  - Input focus routing and cleanup: `DESTROY_VIEW` and `SELECT_VIEW` query surface authenticated ownership from table to route focus to verified owner TID rather than caller; `input.remove_surface` prevents stale capability routing on destroy.
  - Owner crash reclamation: integrated `frames.cleanup_owner(dead_tid)` in compositor supervisor exit sweeper (`message_handler.rs:116`), safely destroying dead owner's views while leaving surviving views from other processes in mixed-owner frames intact and transferring focus to surviving active view.
  - Verified 10/10 tests in `service-compositor` and 356/356 workspace tests passing across `api`, `service-compositor`, `viui`, `ocel`, and `desktop`.
- **Phase 04 — Bottom taskbar, Spotlight and launch registry**: **Completed** (2026-10-19).
  - Implemented 3-section Spotlight HUD in `cells/apps/desktop/src/spotlight.rs` with `Applications`, `RunningTasks`, and `System` sections, dynamic placeholder search bar, `[1 Applications] [2 Tasks] [3 System]` tab band, Tab key and pointer click cycling, and action dispatch (`LaunchApp`, `TogglePin`, `SwitchTask`, `CycleTheme`, `Reboot`, `ShutDown`).
  - Enforced Spotlight geometry: 480x320 DIP modal surface, tab header at Y=56 H=20, list items starting at Y=80 with row height 56 and step 62, `MAX_RESULTS = 3` fitting cleanly above footer at Y=292 with 26 DIP clearance.
  - Expanded reviewed application catalog in `cells/apps/desktop/src/apps.rs` (`DEFAULT_CATALOG`), adding `Files` (`/bin/files`, `[F]`, File Manager & VFS) and `Chrome` (`/bin/chrome`, `[C]`, Web Browser Tier 3 Linux Guest); implemented case-insensitive substring searching across name, id, and description.
  - Implemented fixed bottom shell taskbar in `cells/apps/desktop/src/taskbar.rs`: fixed 36 DIP height, 28 DIP button height centered vertically at Y=4, responsive narrow-screen pagination ($\le 727$ px) with "More" button triggering Spotlight search, and real service daemon indicators (`NET`, `VFS`, `AI`) wired to kernel `sys_lookup_service`.
  - Added unit test suite in `cells/apps/desktop` (`spotlight::tests::test_spotlight_section_cycling_and_actions`, `taskbar::tests::narrow_taskbar_more_opens_application_search`).
  - Verified 357 workspace tests passing across `api`, `service-compositor`, `viui`, `ocel`, and `desktop` (0 failures, 11 ignored); clean bare-metal x86 PIC build; `scripts/cellos-sign --check` passes with zero unallowlisted unsafe code.
- **Phase 05 — Six-zone snap and F11 fullscreen**: **Completed** (2026-10-19).
  - Materialized D-UI-02 wire protocol opcodes and types in `libs/api/src/services/display.rs`:
    - `SNAP_FRAME` (`0x27`) with payload `[frame_id: u32, frame_generation: u32, zone: u8, _pad: [u8; 3]]` (12 bytes).
    - `TOGGLE_FULLSCREEN` (`0x28`) with payload `[frame_id: u32, frame_generation: u32, enable: u8, _pad: [u8; 3]]` (12 bytes).
    - `SnapZone` enum (`LeftHalf = 1`, `RightHalf = 2`, `TopLeftQuarter = 3`, `BottomLeftQuarter = 4`, `TopRightQuarter = 5`, `BottomRightQuarter = 6`) with `from_u8`.
    - `FramePlacement` enum (`Floating`, `Snapped(SnapZone)`, `Maximized`, `Fullscreen`).
  - Implemented placement algorithms and state restoration in `cells/services/compositor/src/surface_table/frame.rs`:
    - `calculate_snap_rect`: Integer grid math using shared endpoints (`half_w = work_area.w / 2`, `other_half_w = work_area.w - half_w`), preventing pixel gaps or overlap at odd screen resolutions.
    - `snap_frame`: Validates frame generation, preflights member views' `min_w` and `min_h` against target snap rect (returning `ViewOpResult::Refused` on conflict without altering state), saves prior floating rect (`saved_floating_rect`), transitions placement to `Snapped(zone)`, and updates backing surface geometry.
    - `toggle_fullscreen`: Saves current placement (`saved_placement`) and floating rect (`saved_floating_rect`) on fullscreen enter; restores previous placement (floating or snapped) and geometry on fullscreen exit with 36 DIP taskbar exclusion.
  - Implemented message dispatch and dynamic screen dimension handling in `cells/services/compositor/src/message_handler/view.rs`:
    - Parametrized view handler with dynamic screen dimensions `(screen_w, screen_h)`.
    - Dynamically computes usable desktop work area excluding the 36 DIP bottom taskbar: `Rect::new(0, 0, screen_w, screen_h.saturating_sub(36))`.
    - Dispatches `SNAP_FRAME` and `TOGGLE_FULLSCREEN`, returning typed `ViewOpResponse` and unioning old and new frame bounds into `pending_dirty` for full repainting.
  - Added unit test suite in `cells/services/compositor` (`surface_table::frame::tests::test_six_zone_snap_geometry_and_min_size_refusal`, `surface_table::frame::tests::test_f11_fullscreen_toggle_and_state_restoration`).
  - Verified: 12/12 unit tests pass in `service-compositor`, 359 workspace tests pass across `api`, `service-compositor`, `viui`, `ocel`, and `desktop` (0 failures, 11 ignored); clean bare-metal PIC cross-build.
- **Phase 06 — Ocel native session and reader cutover**: **Completed** (2026-10-19).
  - Cut over Ocel document viewer to compositor-managed native session tabs and shared ViUI reader themes:
    - Removed redundant local tab bar by setting `TABBAR_HEIGHT = 0`, eliminating in-app tab rendering and hit testing while expanding active document viewport by +28 DIP vertical space (`height - 68` DIP).
    - Synchronized document title to the compositor window surface via `self.surface.set_title(&self.current_tab().title)` across `new_tab`, `switch_tab`, `close_tab`, and `finish_load`.
    - Integrated shared ViUI reader theme modes: `ReaderThemeMode::{FollowSystem, Light, Dark, Paper}` dynamically resolving system palette via atomic `active_system_palette()` without requiring app restart.
    - Verified WCAG 2.1 contrast invariants: normal reader text $\ge 4.5:1$ (Light: 17.5:1, Dark: 13.8:1, Paper: 12.1:1), reader hyperlinks $\ge 5.0:1$ (`mode.link_color()`; Light: 6.92:1, Dark: 5.93:1, Paper: 6.04:1), and table headers with dynamic background/text achieving $\ge 11:1$ (Light: 15.64:1, Dark: 13.71:1, Paper: 11.16:1).
    - Preserved source color fidelity: direct BGRA blits for raster/PDF images without pixel color inversion; preserved syntax highlighting colors on code tokens via `remap_span_color`.
    - Supported F11 fullscreen geometry derivation and safe workload cancellation on tab close (`cancel_loading` and `loader::poll_cleanup`).
  - Verified 170/170 unit tests passing in `cells/apps/ocel`, 359/359 workspace tests passing across `api`, `service-compositor`, `viui`, `ocel`, and `desktop` (0 failures, 11 ignored); clean bare-metal PIC cross-build.
- **Phase 07 — Standalone Files and complete VFS operations**: **Completed** (2026-10-19).
  - Delivered standalone Files application shell in `cells/apps/files/` (`#![no_std]`, `#![forbid(unsafe_code)]`, 4 MiB custom heap).
  - Implemented 3 view modes:
    - List View (`views/list.rs`): Default layout with header row (Name, Size, Type), human-readable sizes, distinct type icons, and alternating row background.
    - Grid View (`views/grid.rs`): 100x110 DIP card layout with preview container, file name, and size.
    - Miller Columns (`views/columns.rs`): Left navigation column and right inspection preview pane showing detailed metadata.
  - Implemented Places sidebar (`views/places.rs`): 160 DIP width navigation for bookmarks (`/`, `/apps`, `/data`, `/storage`, `/services`) with active path highlight.
  - Implemented mouse click hit-testing & pointer tracking (`main.rs`): continuous cursor tracking (`MouseMove`) and Left click dispatch across toolbar mode buttons (`[List]`, `[Grid]`, `[Cols]`), Places sidebar entries, and active view content items.
  - Implemented safe mutation controller and conflict resolution (`model.rs`): two-phase non-conflicting name generation (`resolve_conflict_name` -> `(1)`), and explicit irreversible deletion disclosure.
  - Keyboard navigation and shortcut operations: arrow traversal, Return to enter directory/file, Backspace for parent directory, '1'/'2'/'3' view mode switching, clipboard copy/cut/paste, and Delete with disclosure notice.
  - Verified: 4/4 unit tests passing in `cells/apps/files`, 363 workspace tests passing across UI crates, clean bare-metal PIC cross-build (`x86_64-unknown-none`), and `scripts/cellos-sign --check` passing with zero unallowlisted unsafe code.
- **Phase 08 — Shared preview and Quick Look**: **Pending (Unblocked)** — Phase 02 theme, Phase 06 shared reader appearances/primitives, and Phase 07 Files app shell/navigation dependencies satisfied; unblocked for Spacebar preview overlay and column preview pipeline integration.
- **Phase 09 — GUI Terminal and shell-session streams**: **Pending (Unblocked)** — Phase 01 baseline, Phase 02 theme, Phase 03 native views/tabs, Phase 04 taskbar, and Phase 05 placement dependencies satisfied.
- **Phase 10 — Real Tier3 single-app guest integration**: **Pending (Unblocked)** — Phase 01 baseline, Phase 03 native views/tabs, Phase 04 taskbar, Phase 05 placement, and Phase 07 Files app dependencies satisfied.
- **Phase 11**: Pending.

## Execution policy
One integrating owner for API/SDK/kernel/shared lifecycle changes. Independent research and bounded app-specific slices may parallelize after interfaces freeze; do not concurrently edit shared source. Package and smoke a real consumer each phase; phase 11 is cross-feature qualification, not first integration. Every AC remains required for whole-plan completion, even when a prerequisite blocks one phase. Local captures do not qualify physical or production behavior.

## Key findings
- Existing surface resize/close and ViTheme are reusable; generation-bound session frames/Paper/live theme are missing.
- Files needs complete directory paging and safe operation results, not only a UI over a truncated listing.
- `fb-console` is not a GUI Terminal; atomic session I/O and explicit interrupt/resize need work.
- `vm-runner`/Chrome currently log and exit; real app VM/payload/display/resize must be implemented and evidenced.
- Conflicting plan-complete/freeze projections require exact provenance review, not blanket substrate assumptions.

## Assumptions and dependencies
[Contracts](contracts.md) name proposed owner/state semantics; exact wire/capacity constants are phase-01 design/approval work. Reuse current lifecycle/VFS/stream/VMX implementation only when source and exact approval demonstrate it. Guest/runtime artifacts, status providers and KVM/VMware availability remain explicit checks; unavailable hardware blocks only that evidence, never silently narrows delivery. No paid service, procurement or new platform assumed.

## Deferred by approved scope
Drag-to-group/detach, Files split-pane/full tree, desktop icons/widgets, generic app installer/store and third-party inner-tab extraction. Menu group/detach, all three Files views, four preview families, native sessions and actual Tier3 app remain in scope.

## Verification and rollback
Use actual-module behavior tests plus live UI/state captures on exact signed images; [verification commands and scenarios](verification.md) distinguish current commands from the planned desktop-x86 runner. Preserve prior reported failures without redundant confirmation. Measure performance rather than inheriting unsupported RAM/startup/preview targets. Each phase states reversible code/image scope and irreversible session/file/guest effects. [Acceptance ledger](acceptance.md) owns AC coverage and evidence requirements.

## Red Team Review
Four internal review lenses completed; [20 unique findings and corrections](review.md) recorded (17 Major, 3 Minor, no Critical). Fixed dependencies, Files new sessions, shortcut dispatch, snap selector, prepared-close/kill/open authority, bounded decoder termination, actual VMM dispatch/device topology, child stdio/single-reader stdin, atomic group configure and coherent file replacement. No external review egress or independent human approval claimed.

## Validation Log
Product choices are approved in conversation. Three read-only recon slices plus direct source checks ground prerequisites; artifact validation checks links, AC IDs and phase/index DAG consistency. Phase 01 execution is completed: baseline manifest frozen, D-UI ABI deltas specified, and real x86 integration test runner materialized and verified in `tests/integration/tests/desktop-x86.rs`. Exact ABI approval, uncertain storage/VMX provenance and unavailable platform evidence remain phase-specific gates, not reasons to reopen settled UI choices.

## Handoff
Phases 01 through 07 are completed and fully verified.
Phase 08 (Shared preview and Quick Look) is now fully unblocked for implementation, with all prerequisite theme, Ocel reader components, and Files view navigation in place.
Phase 09 (GUI Terminal and shell-session streams) remains unblocked for independent terminal/stream work.
Phase 10 (Real Tier3 single-app guest integration) is now fully unblocked, with prerequisite baseline, native views/tabs, taskbar, six-zone snap, and Files app dependencies satisfied.
