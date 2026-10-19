# Unified desktop — approved outcome contract

Source: [ADR-0027](../../docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md) and the maintainer's design approvals in this session. These acceptance IDs are planning traceability, not a new ratified wire ABI. Implementation remains pending. User asked for planning, not implementation.

## Scope

Intel x86-64 desktop development image. Preserve existing CLI/serial boot and shell launch of CLI/GUI applications; support shell -> desktop and desktop -> independent GUI Terminal sessions. File Manager is separate from Ocel. Native session tabs, theme, launcher, input and file-opening contracts are shared. Native app migrations must remove obsolete parallel tab/palette implementations at cutover, not leave permanent compatibility shims.

## Acceptance ledger

| ID | Consumer-visible acceptance | Owner phase |
|---|---|---|
| AC-01 | Light default + Dark/Paper and Comfortable share geometry, components and semantic tokens. Theme changes affect live native UI without resetting sessions; no app-owned full palettes. | 02, 06, 07, 09 |
| AC-02 | Ocel Reader follows system appearance by default; explicit Light/Dark/Paper reading selection uses shared tokens without changing desktop chrome. Images/PDF/video/web content is not blindly recolored. | 02, 06, 08 |
| AC-03 | Multiple native views per process and mixed-owner views per frame work with authenticated, non-reused identities; frame membership conveys no new authority. | 01, 03 |
| AC-04 | One native session-tab strip; icon/app/session label, keyboard navigation, icon menus, correct active/focus/selection/dirty state. | 03, 06, 07, 09 |
| AC-05 | `+` creates active-app session; adjacent choice opens Spotlight into the identified frame. Cancel/failure/dead destination leaves no stranded tab. | 03, 04, 06, 07, 09 |
| AC-06 | Explicit menu group/detach preserves session state. Close/refusal/modal/death affects only intended views; dead owner and reused TID cannot retain access. | 03 |
| AC-07 | Six cells in usable area, 16-DIP/250-ms edge previews, halves and two-column corner targets; release commits, Escape cancels, plain move remains possible. Group moves as a unit; minimum-size conflicts visibly reject. | 05 |
| AC-08 | F11 shows selected content only, hiding host tabs/decorations/taskbar; F11 or mouse exit restores prior layout. Hidden tabs survive, key is not double-delivered; display changes remain reachable. | 05, 10 |
| AC-09 | Exactly one fixed bottom combined launcher/running-app/status bar; app icon restores correct chosen session across frames/minimization. No persistent topbar/Dock. | 04, 05 |
| AC-10 | Spotlight has Applications / Running tasks / System, one search and mouse-browsable content; launcher and Alt+Space use same entry; new-frame vs target-frame launch is explicit. | 04 |
| AC-11 | Clock and relevant real input/network/audio/battery statuses; background image optional, no desktop icons/widgets. Unknown/unsupported is not shown as healthy fake status. | 04, 08, 11 |
| AC-12 | Reboot/shutdown and whole-group close coordinate dirty-session approval and report refusal. Force-stop is separate from normal close and requires authority. | 01, 03, 04 |
| AC-13 | Shared app registry and file associations open selected files in intended native/Tier3 application with least-authority read/write intents; executable/script files are not run just because selected for preview. | 01, 04, 07, 10 |
| AC-14 | Files: List default, Grid and Miller Columns; shared Places/breadcrumb/path/search/selection; mode retained on resize, deep/long names and arrow traversal remain usable. | 07 |
| AC-15 | Files Enter/F2/Ctrl+X/C/V and create/delete/rename/copy/move operate on real backends with conflicts, progress, cancellation and per-item errors; no early source deletion across devices. Trash/Undo only for supported reversible effects. | 07 |
| AC-16 | Space Quick Look in current Files session for bounded image/Markdown/code/PDF, same backend as column preview; Escape/Space restores focus, Open With launches app. No script/network execution, stale result replacement or UI blocking. | 08 |
| AC-17 | GUI Terminal owns independent shell stdin/out/err, scrollback, resize, EOF/interrupt and foreground interaction. Concurrent terminals never mix input/output or leak ownership; existing serial/SSH paths remain functional. | 09 |
| AC-18 | Tier3 is a real admitted single-app guest, always guest-fullscreen, one host tab, no underlying Linux desktop exposed. Dialogs work; resize changes guest display, F11 only toggles host chrome, internal app tabs remain allowed. | 10 |
| AC-19 | Signed reproducible desktop image includes actual consumers, CLI/desktop boot choice and launch paths. QEMU TCG/KVM and VMware development matrix observes changed paths, not print-only readiness. | 01, 11 |
| AC-20 | Live UI keyboard/mouse-only, Vietnamese/Unicode labels, DPI/large text, focus, contrast (normal text >=4.5:1), reduced motion and all palettes verified. Measure idle/repaint/memory/startup and preview response separately without inventing targets. | 02, 11 |

## Explicit deferrals approved in design

Drag-to-group/detach (menu functions remain required); Files split pane and full tree; desktop icons/widgets; extraction of unintegrated Tier3 internal tabs; additional hardware ports and production/remote admission. No app store, installer/uninstaller or new general distributed scheduler is implied by Applications manager.

## Design defaults to validate, not missing user decisions

Snap band 16 DIP, dwell 250 ms; corner target precedence; image/content colors preserved; primary `+` versus adjacent app-choice menu; stable view mode on resize. Typography/padding, taskbar height, precise palette values, placement-selector position and fullscreen pointer-exit dwell are phase-02/05 prototype defaults recorded in that phase, not reasons to reopen agreed architecture.

## Evidence contract

Each phase records actual build/profile/source/image pins, commands/exits and applicable captures. Unit tests cover behavior and plausible failure boundaries, never source-text/default-copy assertions. Each significant surface must be exercised as a running application; a passing protocol model or log line alone is insufficient. Keep retained failure reports as prior evidence rather than rerunning merely to reconfirm.
