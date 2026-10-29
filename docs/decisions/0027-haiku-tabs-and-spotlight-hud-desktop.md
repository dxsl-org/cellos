# 27. Unified Session Tabs and Spotlight for CellOS Desktop

Date: 2026-10-19

## Status

Accepted following user approval of unified session tabs, session labels, edge placement, a fixed bottom combined taskbar, three-section Spotlight, background-image-only desktop and F11 content-only fullscreen. This records design decisions, not implemented or verified behavior. Numerical snap thresholds below are selected design defaults, not measured usability results or numbers supplied by the user.

## Context and drivers

CellOS needs consistent session creation, switching and window organization across native applications without redundant system and application tab strips. A standalone File Manager remains separate from Ocel. Mouse users need a visible route to launch applications, restore hidden work and access system controls without knowing shortcuts.

Haiku demonstrates compact window tabs and intentional Stack & Tile grouping, but retains Deskbar for global application/window discovery. Its grouping gesture requires OPT and a highlighted target; ordinary overlapping windows are not automatically merged. macOS provides application-level window merging and global window discovery through Mission Control. Neither source establishes that a tab-only desktop is universally easier to use.

Sources:
- https://www.haiku-os.org/docs/userguide/en/gui.html#stack-tile
- https://www.haiku-os.org/docs/userguide/en/deskbar.html
- https://support.apple.com/en-euro/guide/mac-help/mchlp2469/mac

## Decision

### Unified native session tabs (Cell Tabs)

- In CellOS, applications are formally designated as **App Cells** (or simply **Cells**).
- A tab perched on the top-left of the window frame (BeOS/Haiku OS style) represents an application view/session and is officially termed a **Cell Tab**. The window container hosting one or more Cell Tabs is a **Cell Frame**.
- One Cell may own multiple Cell Tabs; one Cell Frame may contain Cell Tabs owned by different Cells (Cell Stacking / Stack & Tile).
- Native Cells register sessions with the system tab mechanism rather than drawing a second session-tab strip. Ocel pages, terminal sessions and Files browsing sessions use this contract.
- The system manages presentation, selection, ordering, frame membership and placement. Cells own session content, rendering, titles and lifecycle decisions, including unsaved-data confirmation.
- Membership is a presentation relationship, never a grant of capabilities, shared memory or access to another Cell's data.
- Dialogs, inspectors and tightly coupled panes need not become independent Cell Tabs.

### Labels, menus and new sessions

- Each tab shows its application icon and application name. When multiple same-app sessions need distinction, include a short session suffix, for example `Terminal · build`. Session names are also available in session selectors; tooltip alone is insufficient.
- Clicking the icon opens a menu organized into session actions, grouping/detaching and window placement. Keep closing a group distinct from closing one tab.
- Primary click on `+` creates a session of the selected application. An adjacent menu offers Spotlight to choose another application and open its view as a tab in this frame. Applications without session creation open Spotlight instead.
- Spotlight invoked from `+` explicitly identifies the destination frame. A failed or cancelled launch leaves no dead placeholder tab and does not disturb existing sessions.
- Grouping and detaching initially use explicit menu commands. Drag-to-group and drag-to-detach are deferred.
- Closing a tab requests closure of its view, not unconditional process termination. Group closure respects every application's unsaved-data handling.

### Tier 3 applications

- Each Tier 3 application session exposes one application through one outer system tab. Inside the VM that application always runs fullscreen; Linux is runtime infrastructure, not a user-facing desktop. Do not expose a Linux taskbar, launcher, terminal or desktop navigation as part of normal application interaction.
- Guest application fullscreen is relative to the guest display backing the CellOS tab, not necessarily the physical host display. Resizing or snapping the outer frame updates the guest display/application presentation; leaving CellOS fullscreen does not leave guest application fullscreen.
- Application-owned dialogs, popups and auxiliary windows must remain usable within this single-application presentation without exposing the underlying Linux desktop.
- The application may retain its own internal tabs, such as Chromium pages. Third-party internal tabs are exempt from native session-tab unification. Fullscreen presentation does not itself guarantee that an application removes every internal toolbar or control.

### Six-zone placement and edge targets

- The usable work area, excluding the fixed taskbar, has three equal columns and two equal rows. Define boundaries from work-area coordinates with consistent rounding so adjacent targets have no gaps or overlaps.
- Dragging a tab moves/snaps its entire containing frame. Moving only one session requires explicitly detaching it first. Snap never changes group membership.
- Provide a six-cell placement selector during the drag so all six targets, including center cells, can be selected deliberately. Preview the resulting frame geometry; release commits and Escape cancels. Ordinary floating movement remains possible outside active placement targets.
- Combined edge targets: left/right edge centers select the respective half-screen side; top/bottom edge centers select the respective half-screen row. Corners select the adjacent two-column-by-one-row region (two-thirds width and half height). Corner targets take precedence over edge-center targets.
- Numerical design defaults: edge/corner activation band is 16 device-independent UI units, scaled using the display scale. Remaining within a candidate for 250 ms activates its preview; leaving or changing the candidate resets the dwell. Release without an active preview remains a normal move. These defaults require usability validation, not a claim of optimality.
- Edge distances are measured against the usable work area, including its bottom boundary above the taskbar. Maximize fills that work area and remains available through the icon menu.
- Respect every tab's minimum-size requirements when placing a group. Reject an incompatible target with visible feedback rather than silently clipping essential UI. Preserve previous floating geometry for restoration.

### Fixed combined taskbar and desktop background

- One fixed bottom bar combines launcher, running applications and status. No second topbar or Dock, and no auto-hide in normal windowed/maximized operation.
- The launcher button opens Spotlight. Running applications have one icon per application; multiple sessions/frames are exposed through a session selector that restores the correct frame and activates the chosen tab.
- Status includes clock and relevant keyboard/input-method, network, audio and battery indicators. Detailed diagnostics belong in expanded UI, not permanent taskbar clutter.
- The desktop supports a background image if desired. Desktop icons, widgets and a widget framework are out of current scope.

### Spotlight: three sections

- **Applications:** browse/search available applications and launch them. Do not imply install/uninstall support until implemented.
- **Running tasks:** browse frames/groups and their session tabs, including minimized work, and switch to or close the selected item. Process-level diagnostics and force-stop are secondary actions, not the primary task view.
- **System:** settings and supported system actions, including reboot and shutdown. Destructive/session-ending actions handle unsaved work rather than bypassing application lifecycle rules.
- Use one search field with mouse-browsable content; typing is not required. The taskbar launcher and `Alt+Space` open this same interface. Launches from the taskbar use a new frame by default; launches from `+` target the identified existing frame.

### F11 content-only fullscreen

- F11 toggles fullscreen for the selected tab. Display its content over the full display, hiding system tab headers, window decorations and the bottom taskbar. Do not stretch the entire group's tab strip into fullscreen.
- Other tabs remain alive and keep their frame membership; they are not closed or detached. Do not show other windows over fullscreen as part of ordinary placement.
- F11 again exits fullscreen and restores the previous frame geometry/placement, active tab, system tabs and fixed taskbar. Geometry must be made reachable if display configuration changed.
- CellOS consumes F11 once, including for Tier 3, and does not forward the same event to toggle the guest application's fullscreen state. Guest application fullscreen remains enabled independently of the outer CellOS frame state.
- A deliberate pointer dwell at the top screen edge exposes a temporary exit-fullscreen control for mouse-only use. This is an escape affordance, not a permanent topbar; do not reveal tabs/taskbar on every incidental mouse movement.
- Application-owned content controls remain the application's responsibility; content-only means removal of system chrome, not arbitrary removal of pixels inside a guest/app surface.

### Utility applications

Files is a standalone application, not a feature of Ocel. Terminal is a separate interactive application connected to shell sessions. File associations and desktop image packaging, originally backlog in initial draft, are formally brought into execution scope by the maintainer's approval of the Unified Desktop Plan (`.agents/261019-1310-unified-desktop`) and Acceptance Ledger (AC-13, AC-19).

### Files: one interaction model, three views

- Accepted File Manager design: standalone Files with List as the default, Grid for visual browsing and Miller Columns for hierarchical browsing. These are views of one application/session, not separate Windows/Mac compatibility modes. This supersedes the earlier Columns-first proposal.
- All views share Places, Back/Forward, breadcrumb with editable path, search, selection, file actions and preview. Places contains familiar locations, mounted storage and user-pinned locations; a full expandable directory tree is not required initially.
- Preserve the user-selected view on resize/snap. Columns can scroll horizontally and keep the active column reachable; resizing must not silently switch to List. Support adjustable column widths and access to full selected filenames.
- In Columns, selecting a directory reveals its contents in the next column; changing an ancestor selection replaces the old descendant branch. Selecting a file may show metadata/preview. Directory traversal must handle inaccessible paths and links without unbounded expansion.
- Use one command convention: Enter opens the selected item, F2 renames, Ctrl+X/C/V cuts/copies/pastes, and Space invokes Quick Look outside text-entry/rename contexts. Directory selection in Columns supports single-click traversal and arrow-key navigation.
- Delete uses Trash only where supported. Otherwise clearly disclose permanent deletion before proceeding; never imply recoverability that the storage backend cannot provide.
- Quick Look is a temporary preview owned by the current Files session, not another system tab or launched viewer application. Space again or Escape dismisses it and restores file-selection focus. Provide an explicit Open With action to launch the associated application.
- Column preview and Quick Look use a shared preview mechanism. Cancel obsolete requests and reject stale results after selection changes. Preview failure must not block navigation or crash Files.
- Preview is read-only and bounded by input size, output size, memory and execution time. Complex decoders may run as isolated demand services with access limited to the selected file. Reuse appropriate libraries/services without embedding Ocel's application UI/lifecycle. Do not execute scripts or automatically fetch remote Markdown resources.
- Support bounded image, Markdown, code/text and PDF previews; unsupported types show metadata and Open With. Measure input response, first content and completed preview separately. Do not promise every file can be decoded/rendered in 10 ms.
- Prioritize safe file operations: progress, cancellation, per-item errors and explicit name-conflict handling. Cross-device moves must not delete source data before successful destination copying. Completion and Undo claims must match actual backend durability and reversibility guarantees.
- Optional two-pane Split View is a later extension for source/destination work within one Files session; it does not introduce another tab layer. A full directory tree is also optional future work, not required for initial delivery.
- Validate the design with locating a download, selecting images, navigating deep directories, batch operations, conflicting names, permission failures, preview cancellation, keyboard-only use and larger text/display scales. No superiority over Explorer/Finder is claimed without user evidence.

### CellOS Linear: shared system theme

- Accepted visual direction: one system design language with Light, Dark and Paper palettes. Light and Comfortable density are initial defaults. Palettes share component geometry and interaction behavior; Paper uses warm solid colors, not paper textures.
- Native application chrome and controls consume shared semantic tokens and toolkit components rather than maintaining independent theme implementations or hard-coded app-specific palettes. The shared contract includes surfaces, text, borders, focus, selection and semantic status colors, typography, spacing and component states.
- Ocel's three reading appearances use the system's Light, Dark and Paper reader tokens rather than a separate Ocel palette. Default reader appearance follows the active system palette; an explicit reading-appearance selection may use another system palette without changing the desktop or app chrome. This is a content-view preference, not a second app theme system.
- Files previews and other native document views reuse these shared reader/content tokens where the application owns presentation. App-specific meaning such as syntax categories may extend shared semantic roles, not duplicate whole themes.
- A system theme change updates native controls and follow-system document views without restarting the application or losing tab/session state. Explicit reader appearance choices retain their selected system palette.
- Do not recolor source images, video or PDF artwork, invert arbitrary websites, or force theme colors into unintegrated Tier 3 app content. CellOS themes their outer system chrome; internal guest appearance requires separate application support.
- Prefer opaque solid surfaces, clear borders, restrained rounding and visible control affordances. No live background blur/acrylic, decorative paper texture, essential hover-only actions or idle animation. Cache reusable rendering resources and repaint changed regions; dark colors alone are not a performance optimization.
- Maintain readable contrast, visible keyboard focus, non-color-only status indicators, larger-text/display-scale support and reduced motion. Validate actual taskbar, tabs, Spotlight and Files surfaces in all three palettes before claiming usability or performance.



## CLI and terminal follow-on boundary

[ADR-0028](0028-cellbox-cli-first-structured-pipelines-and-tui.md) selects CLI as
the full selected operational surface and desktop as a simpler presentation over
the same backend. Its [Cellbox design](../cellbox-cli.md) keeps Ratatui/Terminal
SDK outside viUI; GUI Terminal hosts a real shell/session through an ANSI/VT
emulator instead of maintaining its own command implementations.
This clarifies the utility-application boundary without reopening the native
session/tab/theme decisions above or upgrading historical implementation evidence.

## Alternatives not selected

1. **Decorative compact title tabs plus separate native session tabs:** duplicates navigation and actions; native apps use the shared session contract.
2. **Icon/widget-only home screen or keyboard-only Spotlight:** hides primary navigation behind application windows or shortcut knowledge; a fixed combined bar supplies a visible entry point and restoration path.
3. **Separate permanent topbar and taskbar/Dock:** duplicates persistent shell regions without a demonstrated need for this desktop; use one bottom bar.
4. **One process per tab:** couples presentation to execution and resource allocation unnecessarily.
5. **Automatic drag-to-group in the initial implementation:** risks accidental grouping and adds interaction complexity; begin with menu commands while supporting drag-to-snap.
6. **Universal third-party internal-tab extraction:** requires guest/application cooperation; accept one outer tab per Tier 3 application.

## Consequences and verification requirements

- This design requires a session lifecycle protocol, not merely compositor decoration changes. Define focus, modal ownership, app death and close refusal without disturbing unrelated sessions.
- Context menus, session selectors and snap placement need keyboard access and usable targets at larger text/display scales. Fullscreen needs both keyboard and pointer exit paths.
- Tab labels must distinguish real sessions; app status and restore actions must identify the correct view even when one app spans multiple groups.
- Numerical snap defaults, exact six-cell selector presentation, taskbar dimensions and the pointer exit-control dwell require prototype validation. A coherent proposed threshold is not proof of usability.
- RAM, startup latency and usability superiority are unmeasured. Establish explicit measurement scopes rather than unsupported numerical guarantees.
- Validate mouse-only launch, same-app session identification, new-session and cross-app creation, failed launch, hidden-window recovery, menu grouping/detaching, all six base targets and combined edge targets, minimum-size rejection, snap cancellation, and safe close.
- Validate F11 entry/exit from floating, snapped and maximized frames, focus and modal behavior, app failure during fullscreen, display-size changes, mouse-only exit and Tier 3 shortcut routing.
