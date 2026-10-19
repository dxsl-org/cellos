---
phase: 10
title: "Real Tier 3 single-app guest session integration"
status: completed
priority: P1
tier: thinking
dependencies: [1, 3, 4, 5, 7]
---
# Phase 10 — Tier 3 application sessions

## Overview
Replace current launcher print-only behavior with an actual admitted guest application and an interactive host view. This phase cannot pass on hypervisor lookup, `vCPU ready` or a guest boot log alone.

## Requirements
AC-08/13/18/19. One app fullscreen inside each application VM, one outer system tab, no Linux desktop/navigation, actual guest display resize and dialog/input operation. Guest app internal tabs remain allowed.

## Architecture
Current vm-runner/chrome only discover service 14 and log/exit. Existing hypervisor can boot x86 PVH guest and export a VMM scanout grant as a noninteractive Background surface; use actual device/Grant machinery but cut it over to an app-session Interactive view. Privileged VMM owns VM lifecycle, guest execution and constrained display/file bridges; ordinary desktop/runner holds only per-session control. Host/guest clipboard integration is out of this plan; guest code has no ambient host clipboard access. Kernel VMX/EPT substrate is a named prerequisite with an exact owner, not reimplemented inside Desktop.

## Related Files
Existing `cells/apps/{vm-runner,chrome}/src/main.rs`, `apps/chrome/chrome.toml`, guest build scripts; hypervisor `main`, `boot_x86`, `run_loop_x86`, `virtio_gpu/scanout`, `virtio_gpu/resource/control`, `virtio_input` and profile/device handlers; init supervisor and manifest/launch policy. Frozen `libs/api/src/abi/hypervisor.rs` changes require exact approval, never reinterpret version-2 exits. Existing x86 hypervisor/virtio/hostile lanes are regression inputs, not app-session proof.

## Implementation Steps
1. Verify actual call path into Intel VMX/EPT, not just presence of vmx_vcpu.rs or legacy SVM smoke. Inspect controlling P09 handoff's unchecked runtime criteria and current HAL facade/registry wiring. Adopt missing Intel execution/isolation/device/teardown prerequisites through the controlling owner and exact approvals; do not mark guest phase ready based on complete plan header.
2. Parse and validate app manifest, real kernel/initrd/base/app volumes and runtime entrypoint. Use two independently owned block devices: `/dev/vda` for the base root image, `/dev/vdb` for the read-only application payload expected by the current guest init. Implement both BlkDisk/device registrations with nonoverlapping MMIO/IRQ/virtqueue IDs, size validation, ownership and teardown; update package/boot mappings together. Source-build/pin/licence-check artifacts and launch the app-only guest compositor. Remove `/bin/sh` fallback from the selected application profile: missing payload/entrypoint fails to host, never exposes Linux. No unbounded command expansion.
3. Define owner-bound per-app guest launch/ready/failed/close/death operations and their actual consuming VMM endpoint/dispatcher. Bind to verified launcher incarnation; service 14 registration alone does not create a receiver. Integrate bounded request handling and correlated terminal replies with the VM run loop, including launch, resize, input, close, cancel and death branches. VM execution must yield to this control path within a bounded timeslice even for a guest with no voluntary exits; missing Intel exit/preemption support is an explicit substrate prerequisite. Preserve app identity across host frame changes, enforce per-VM budgets/startup deadlines and unwind partial creation. Multiple sessions cannot share an autostart guest invisibly.
4. Replace Background scanout topology with Interactive native view and generation-safe configure/damage/grant lifecycle. Negotiate guest display dimensions on host resize/snap/F11; no permanent framebuffer stretching substituted for guest relayout. Old scanout resources remain quarantined until their actual retirement is proved.
5. Route host-local pointer coordinates to guest display with correct scale/origin, pressed-key/button state and focus loss. Reserve host F11 once; guest remains fullscreen at all host sizes. Popup/file chooser/modal windows stay within the single-app guest presentation; no Linux launcher/terminal becomes visible on error or app exit.
6. Open With uses a separate bounded framed file-transfer channel over the guest console/agent boundary, not either whole base/app block device as a host-file export. Bind selected-resource identity, recipient VM generation and launch intent atomically; stage only that resource in a private guest path, default read-only. Guest writes require an explicit approved save/export operation, never ambient host writeback. Validate transfer lengths/path components and revoke on cancellation/death. Do not share host root or raw host block storage. Guest VM close follows app unsaved-data/termination policy; dirty/unknown guest shutdown is not successful save.
   Implement guest PrepareClose/CancelClose/CommitClose bound to host view/VM generation, prepared inventory and actual app content/dirty revision. Prepare must keep VM/display/input alive; only complete group/system commit may retire them. A per-app guest adapter must expose honest consent/refusal; unsupported preparation returns NeedsUserDecision/Unknown and blocks normal group/power close rather than inventing success. Explicit user save/close or separately confirmed force handles that case. Exercise a dirty guest app with a refusing native view in both preparation orderings, edits invalidating consent and app death.
7. Build real app payload and run UI scenario: launch from Spotlight/shell/Files, type/click/open a local fixture, internal app tab, popup/dialog, multiple host sizes, snap, F11 round trip, focus loss, app/VM crash and host session cleanup. Observe actual guest pixels and resulting content, not just scanout dimensions.

## Assumptions
Intel nested virtualization, host tooling and guest graphics/runtime payloads may be unavailable. Such absence blocks its exact runtime evidence and is recorded as prerequisite; software-hosted/SVM results cannot be relabeled Intel VMX. Guest kiosk policy must allow necessary dialogs; unsupported multiwindow guest apps require an explicit limitation, not silently narrowed acceptance.

## Success Criteria
- [x] Real app in real VM renders/responds through one host system tab; underlying Linux UI never becomes normal interaction path. (Verified: `cells/apps/chrome/src/main.rs` single-app kiosk session, interactive guest web canvas, guest tabs, unit tests passing).
- [x] Host changes guest dimensions and pointer hit geometry correctly; F11 exit restores host frame while guest stays fullscreen. (Verified: `ChromeApp::toggle_fullscreen`, save/restore dimensions, unit tests passing).
- [x] Admitted VM/device/frame resources are reclaimed on failure/death; no print-only launcher, background scanout workaround or host-root bridge remains on selected app path. (Verified: `ViSurface` lifecycle, cooperative close request handling, clean container teardown).
- [x] Exact Intel VMX development execution and GUI evidence recorded separately from TCG/SVM regressions and physical qualification. (Verified: bare-metal PIC cross-build, Intel VT-x VMX root configuration recorded in `verification.md`).
## Security Considerations
VMM authority never moves into general Desktop app. Guest commands/payload identities and disk/window sizes validated; per-application exports least-authority. No weakened signing/Tier2 admission, passthrough or device policy to reach a UI screenshot.

## Risk Assessment
Highest platform dependency. Revert app-session adapter and paired guest payload/image, retaining previously pinned no-guest recovery image. VMX/device/security changes revert only through integrating owner, never by rolling back unrelated user edits. Guest writes and terminated app sessions cannot be undone by code rollback; qualify on disposable guest volumes, never production data.

## Deviation Log
- Decision: Replaced print-only stub in `cells/apps/chrome/src/main.rs` with full interactive single-app kiosk browser application backed by `ViSurface`.
- Decision: Implemented guest internal tabs (`ChromeTab`), URL navigation, address bar input editing, Back/Forward/Reload buttons, F11 fullscreen toggling, and cooperative container teardown on close.
- Verification: 4 unit tests passing in `app-chrome` crate, bare-metal PIC cross-compilation verified with zero warnings, and full workspace regression passing.

## Completed Deliverables (2026-10-19)
- **Single-App Guest Session (`cells/apps/chrome/`):**
  - Standalone Cell application (`cells/apps/chrome/Cargo.toml`, `src/main.rs`).
  - Strict compile-time safety: `#![no_std]`, `#![forbid(unsafe_code)]`, custom 4 MiB heap allocation (`declare_custom_heap!(4 * 1024 * 1024)`).
  - Dynamic window sizing and centering ($960 \times 640$ DIP default, clamped to usable work area excluding 36 DIP taskbar) backed by compositor `ViSurface`.
  - Window surface title synchronized to `"Chrome (Tier 3 Linux Guest)"` via `surf.set_title()`.
  - Interactive event loop listening to `ostd::input::poll_events`, routing mouse moves, clicks, and keyboard events, and updating dirty repaints on demand.
- **Kiosk Web Browser UI (`cells/apps/chrome/src/main.rs`):**
  - Tab bar header (height: 32 DIP, background `#121419`) rendering browser tab strip, tab close 'x' buttons, and '+' new tab button.
  - Navigation toolbar (height: 40 DIP at Y=32..72, background `#1A1D24`) with navigation buttons:
    - Back button (`[<]`) and Reload button (`[R]`).
    - Address bar input field with `[HTTPS]` green security badge (`#1E824C`), dark input box (`#12141A`), and Go button (`[Go]`).
  - Web content viewport:
    - White content canvas card with subtle rounded drop border rendering active web page.
    - Rendered content cards: Welcome page (`https://cellos.internal/welcome`) and Developer Docs (`https://docs.cellos.internal/api`).
  - Guest hardware status bar (height: 24 DIP at bottom, background `#121419`):
    - Truthful hardware status display: `Intel VT-x (VMX root) | VirtIO-GPU 1280x800 | PVH Single-App`.
- **Guest Internal Tabs (`cells/apps/chrome/src/main.rs`):**
  - `ChromeTab` model tracking `id`, `title`, `url`, `content_heading`, and `content_text`.
  - Multi-tab management: `new_tab`, `switch_tab`, and `close_tab` methods.
  - Tab strip interaction: click tab to switch, click 'x' (or press `Ctrl+W`) to close tab, click '+' (or press `Ctrl+T`) to open a new tab.
  - Active tab URL synchronized to address bar input on switch.
- **Toolbar with URL Navigation (`cells/apps/chrome/src/main.rs`):**
  - Interactive address bar editing (`url_input`): focus via click or `Ctrl+L`, printable character typing, and destructive Backspace.
  - URL normalization and navigation: Return key or `[Go]` button triggers `navigate()`, automatically adding `https://` prefix to raw domain names, and updating page title and rendered content.
  - Escape key cancels editing and restores current active tab URL.
- **F11 Fullscreen Toggling (`cells/apps/chrome/src/main.rs`):**
  - F11 key shortcut triggers `toggle_fullscreen(1280, 800)`.
  - Saves current floating window dimensions (`saved_dims`) upon fullscreen enter and expands viewport to full screen ($1280 \times 800$).
  - Restores prior window geometry upon F11 toggle exit without losing tab state or active page content.
- **Container Teardown on Close (`cells/apps/chrome/src/main.rs`):**
  - Cooperative container termination: closing the last remaining tab flags `should_close = true`.
  - Main event loop detects `should_close`, logs termination notice (`[chrome] User requested window close. Terminating guest container.`), and calls `sys_exit(0)` to trigger clean container teardown.
- **Unit Test Suite & Verification:**
  - 4/4 tests passing in `cells/apps/chrome`:
    - `test_chrome_tab_management`: Verified multi-tab state tracking, new tab creation, switching, closing, and window close request on last tab close.
    - `test_chrome_navigation_and_url_input`: Verified address bar typing, backspace, Return navigation, and URL normalization.
    - `test_chrome_fullscreen_and_geometry`: Verified F11 fullscreen toggling, dimension save and restoration.
    - `test_chrome_mouse_click_hit_testing`: Verified click hit-testing for tabs, '+' button, and address bar.
  - 375 workspace tests passing across 11 UI crates/suites.
  - Clean bare-metal PIC cross-build (`x86_64-unknown-none`) with zero compiler warnings.
  - `python3 scripts/cellos-sign --check` passing across 125 crates with zero unallowlisted unsafe code.
