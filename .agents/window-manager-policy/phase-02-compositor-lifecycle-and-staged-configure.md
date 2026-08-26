---
phase: 2
title: "Compositor lifecycle and staged configure state"
status: pending
priority: P1
effort: 1d
dependencies: [1]
tier: thinking
---

# Phase 02: Compositor lifecycle and staged configure state

## Overview
Make surface geometry, visibility, close, and replacement-Grant transitions explicit compositor state. The active Grant and presented geometry remain unchanged until the owner has staged the required buffer and acknowledged the exact serial.

## Requirements
- Functional: process every Phase 01 request/event, send owner-directed configure/close/state frames, atomically commit a valid replacement Grant, and clean all transient state on destroy and owner exit.
- Non-functional: validate sender ownership before every cap lookup side effect; preserve normal legacy attach behavior when no configure is pending; use checked dimension/byte arithmetic and bounded deadlines/coalescing.
- No regression: existing `MOVE_SURFACE`, `RAISE_SURFACE`, grant detach/destroy replies, owner-exit removal, background exclusion, and all legacy clients keep their current semantics.

## Architecture
Extend `SurfaceState` with a bounded title, `visibility: Visible|Minimized|Closing`, `presentation: Normal|Maximized`, saved normal content rect, and `pending: Option<PendingConfigure>`. `PendingConfigure` is exactly `{serial, kind, desired_content_rect, target_presentation, staged_grant: Option<GrantSource>, deadline_ms, latest_pointer_rect}`; it has one slot, not a queue. Retain the active `PixelSource` separately from the staged source.

State transitions are:

| Trigger | Required transition |
|---|---|
| title request | owner + UTF-8/bounds valid → replace title; damage old/new decorated bounds. |
| resize/maximize/restore request | validate policy/state; allocate next nonzero serial; store proposal and send `WindowConfigure`; no active geometry/Grant change. |
| matching `ATTACH_GRANT` | validate owner, Grant read permission/length, and exact pending `w,h`; map it into `staged_grant`, retaining active source and dimensions. A second attach for that serial is rejected. |
| matching `ConfigureAck` | only if a staged matching Grant exists → swap source + content rect atomically, clear pending, damage old/new decorated bounds, emit `WindowStateChanged`; reply success. |
| stale/duplicate ACK or wrong attach | reply failure and retain active state; do not advance serial. |
| close control | `Visible* → ClosePending(serial, deadline)` and send `WindowCloseRequest`; do not destroy or hide yet. |
| matching close reject / deadline | clear close pending and keep the surface visible. |
| matching close accept | `Closing`: clear capture/focus/selection, remove from paint/hit-test, damage old decoration; owner must detach/destroy. After a bounded closing deadline, compositor detaches its mappings and removes the slot without unregistering a live owner's Grant. |
| minimize | `Visible* → Minimized`; clear capture/focus/selection if selected, exclude from render/hit-test, retain z-order position, damage its old decoration, emit state. |
| restore | from `Minimized` uses saved normal rect; from `Maximized` uses saved normal rect; both send configure and remain hidden only when restoring from minimized until commit. |
| maximize | save normal content rect once, calculate content geometry inside the scanout after title/frame extents, and send configure; commit sets `Maximized`. |
| owner exit/destroy | discard pending/staged state, forget pointer state, remove z entry/slot, release known mappings as the existing owner-exit contract requires, and dirty the full old decorated bounds. |

A deadline uses the compositor's existing monotonic `sys_get_time` check in the main loop; choose and document one short bounded constant (for example 2,000 ms) for configure/close/closing. While resize capture moves, keep only `latest_pointer_rect`; after a successful ACK, issue one new configure for that latest rect if it differs. Timeout drops staging, keeps old presentation, and ends resize capture—never stretches or mutates client pixels.

## Assumptions
- **Claim:** the existing Grant syscall can map a newly shared registration without revoking the current one, allowing both active and staged read-only pointers during an ACK window.
  **Confidence:** medium
  **How to verify:** inspect Grant syscall implementation and add a two-registration compositor test before relying on concurrent mappings.
- **Claim:** `sys_get_time` is monotonic enough for bounded in-loop timeouts.
  **Confidence:** high
  **How to verify:** inspect the syscall contract and its existing hotplug use in `main.rs`.

## Related Files
- Modify/split: `cells/services/compositor/src/surface_table.rs` — active/staged Grant and lifecycle state, all ≤200 lines after extraction.
- Modify/split: `cells/services/compositor/src/main.rs` — authenticated message dispatch, deadline tick, cleanup.
- Modify: `cells/services/compositor/src/input_handler.rs` and `pointer_router.rs` — lifecycle-driven capture/focus removal hooks.
- Modify: `libs/api/src/services/display.rs` and `libs/ostd/src/display.rs` only to consume Phase 01's frozen definitions.
- Create: focused compositor state/IPC helpers (for example `window_state.rs`, `window_ipc.rs`), each ≤200 lines.

## Implementation Steps
1. Split active Grant ownership from the pending staged mapping; make `pixels`, `screen_rect`, damage, detach, and remove operate only on the active source unless the named pending operation explicitly addresses staging.
2. Implement a single authenticated decoder for old and new display opcodes. Check declared frame length, reserved bytes, owner, role, cap, serial, state, and checked pixel byte count before sending any success reply.
3. Implement serial allocation, proposal emission, stage/ACK commit, retirement acknowledgement, timeout processing, and full old/new decorated damage. Never call `attach_grant`'s live-dimension path during a pending configure.
4. Implement visibility/close/maximize/restore transitions and saved normal rect handling. Keep hidden caps in z-order but skip them in paint and hit test; do not allow background surfaces into policy states.
5. Make destroy and supervisor owner-exit clear capture, selection, pending events, active/staged mappings, and decorated damage exactly once; stale late messages see no cap and fail harmlessly.
6. Add focused compositor tests or probe assertions for each transition, authorization failure, duplicate, timeout, and owner exit before QEMU integration in Phase 4.

## Task List
- [ ] Separate active and staged Grants plus serial/deadline state.
- [ ] Authenticate and implement configure, close, and visibility transitions.
- [ ] Commit/release replacement Grants atomically and clean up exits.
- [ ] Test rejection, duplicate, timeout, and owner-exit paths.

## Success Criteria
- [ ] A configured size becomes visible only after correct-size staged `ATTACH_GRANT` plus matching ACK; a bad/stale/duplicate frame leaves the old buffer and rect active.
- [ ] Releasing the old Grant occurs only after a successful commit and `DetachReplacedGrant` acknowledgement; normal drop still uses legacy detach/destroy.
- [ ] Minimized/closing windows neither paint nor win hit-tests or keyboard focus; restore and close rejection have deterministic state events.
- [ ] A nonresponsive client returns to the old geometry/visibility after timeout, and an owner exit removes active and staged surfaces without use-after-free.
- [ ] Verification: `cargo test -p service-compositor -p api -p ostd`.

## Security Considerations
The kernel sender TID, not a caller-supplied ID, authorizes every mutation. Serial equality makes delayed/forged ACKs harmless; dimensions are checked before pointer creation; staged Grants are never rendered before commit. The compositor still reads, never writes, a client Grant, and it never unregisters a live owner's Grant merely because a close deadline elapsed.

## Risk Notes
The current `SurfaceState::attach_grant` overwrites live width/source, so reusing it would violate the policy. The existing owner-exit code unregisters known registrations; extend that path carefully to staged mappings and verify the kernel ownership rule. Rendering a hidden or staged slot even once would leak stale pixels or restore interactivity.

## Deviation Log
None.
