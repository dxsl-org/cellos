# Phase 01: Atomic surface selection, raise, and focus reassertion

## Context Links
- `cells/services/compositor/src/input_handler.rs:25-118`
- `cells/services/compositor/src/pointer_router.rs:19-159`
- `cells/services/compositor/src/main.rs:59-131,266-290`
- `cells/services/input/src/dispatcher.rs:65-108`

## Overview
Turn the existing left-button selection into one ordered compositor transition:
select the hit surface, establish its capture/keyboard owner, raise and dirty its
surface, restore the compositor as the input-service endpoint, then send that
press. The compositor forwards later keys to the selected owner.

## Key Insights
- `PointerRouter` has the required `focused_owner` and `capture` state, while
  `ZOrder::raise` and RAISE_SURFACE damage behavior already encode stack changes.
- `handle_input_event` has immutable table/z-order references and router routing
  sends internally; the transition needs a narrow mutable mutation boundary.
- `connect_to_input` obtains both TIDs but only stores `input_tid`. Shell and
  GUI `request_focus` calls can replace the input service endpoint, so activation
  must retain/reuse the compositor TID to send the existing `SetFocus` request.

## Requirements
- Derive cap/owner solely from `hit_test`; do not accept a focus cap/TID from a
  client or input payload.
- On a hit left press set `capture` and `focused_owner`, call `ZOrder::raise`,
  mark re-order damage, reassert `SetFocus { cell_tid: compositor_tid }`, and
  only then emit the position and button event to the target.
- Preserve captured move and left-release lookup-by-cap semantics, clearing
  capture only after release delivery. Keep `forward_key` as key dispatch.

## Architecture
`InputState` stores `input_tid` plus compositor TID. The activation branch in
input handling is the only place allowed to mutate stack/dirty state and emit
focus reassertion; `PointerRouter` remains source of selected owner/capture.
The existing one-way SetFocus protocol has no reply, so the QEMU test must wait
for the completed click marker before it injects the keyboard oracle.

## Related Code Files
- `cells/services/compositor/src/input_handler.rs`
- `cells/services/compositor/src/pointer_router.rs`
- `cells/services/compositor/src/main.rs`
- No direct-focus caller changes: `robot-dashboard`, Doom, and Tetris may keep
  their existing startup focus requests; a successful surface click supersedes it.

## Implementation Steps
1. Store the compositor TID found in `connect_to_input` and factor the current
   `InputRequest::SetFocus` encoding/send into a helper usable at startup and
   successful activation.
2. Define the smallest internal routing result/callback identifying a selected
   `PointerTarget` and activation. Keep target cap private to the compositor.
3. Thread mutable `SurfaceTable`, `ZOrder`, and dirty accumulator only through
   activation. Perform selection state update, raise, full re-order damage,
   SetFocus resend, then local press delivery in that exact order.
4. Retain existing move/release capture and local coordinate translation; do not
   raise for a move, scroll, release, or miss. A disappeared captured cap drops.
5. Keep client `request_focus` code unchanged; the only policy cutover is the
   compositor's click-time endpoint reassertion.

## Todo List
- [x] Persist compositor TID and reuse existing SetFocus wire encoding.
- [x] Refactor activation routing and ordered mutation/delivery.
- [x] Preserve capture and forward compositor-origin key frames to clients.

## Success Criteria
A selected left press cannot be delivered before its cap is topmost, dirty, and
the compositor has resent SetFocus. Subsequent key frames reach `focused_owner`;
capture retains the press target until release; owner checks still gate surfaces.

## Risk Assessment
Broad mutable routing risks raising on non-activation paths. SetFocus is
fire-and-forget, so a key sent immediately after click could race input-service
processing; Phase 02 uses a post-click marker/wait rather than assuming an ack.

## Security Considerations
The compositor derives focus from authenticated IPC sender ownership in
`SurfaceTable`; no public focus protocol, capability, or privileged syscall is
added. Existing SetFocus remains sender-verified by the input service.

## Next Steps
Create the deterministic two-owner probe and one RV64 QEMU evidence test in
Phase 02. Exclude decorations, drag/resize, close lifecycle, and general window
management.
