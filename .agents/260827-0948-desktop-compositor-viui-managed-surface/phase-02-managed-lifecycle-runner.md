# Phase 02 — Managed lifecycle runner

## Context Links
Approved spec AC-4..AC-8; `libs/ostd/src/display/lifecycle.rs`; `cells/tests/window-policy-probe/src/events.rs`.
## Overview
Add a ViUI-owned adapter that polls typed lifecycle events around `ViApp` ticks.
## Key Insights
Configure becomes active only after `ViSurface::apply_configure` succeeds; minimized surfaces suppress rendering.
## Requirements
Configurable close response, restore repaint, input continuity, safe failure.
## Architecture
Share one local `ViSurface` handle between framebuffer renderer and managed adapter; retain single-threaded ownership.
## Related Code Files
`libs/viui/src/renderer.rs`; new focused module under `libs/viui/src/`; `libs/viui/src/lib.rs`.
## Implementation Steps
Introduce handle, lifecycle state/result types, event processing and test seams.
## Todo List
- [x] Add surface handle.
- [x] Add managed adapter.
- [x] Add lifecycle tests.
## Success Criteria
Configure/close/state transitions meet AC-4..AC-8 with no `libs/api` diff.
## Risk Assessment
Avoid double-destroy and accidental render while minimized.
## Security Considerations
Filter all lifecycle events by capability and preserve Grant rollback semantics.
## Next Steps
Wire the adapter into `viui-demo`.
