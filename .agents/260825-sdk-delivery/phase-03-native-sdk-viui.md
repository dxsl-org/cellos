# Phase 03 — Native SDK and ViUI

## Context Links
`docs/specs/23-native-sdk-contract.md`; `docs/specs/14-viui.md`; `docs/guides/viui-guide.md`; `libs/viui/src/{signal,app_runner,surface_renderer}.rs`.

## Overview
Make the existing Native SDK and reactive-v2 ViUI developer contract accurate and behaviorally covered.

## Key Insights
The ABI is frozen; API/ostd public paths are stable but partial. Reactive-v2 is canonical for node/DSL generation. The guide uses obsolete APIs and core runner behavior lacks focused tests.

## Requirements
Preserve flat exports and ABI; retain explicit tick-driven execution and compositor-agnostic widgets; maintain legacy Elm as a distinct experimental API.

## Architecture
Signals synchronously notify retained reactive nodes. ViApp converts structural events to layout/full damage and signal updates to dirty regions, rendering into an app-owned surface.

## Related Code Files
`docs/guides/viui-guide.md`, `libs/viui/src/{signal,app_runner,surface_renderer,node_widgets}.rs`, `cells/apps/robot-dashboard/src/main.rs`.

## Implementation Steps
1. Correct guide examples and availability wording against public APIs.
2. Add deterministic Signal lifetime/re-entrancy coverage.
3. Add headless ViApp event/layout/dirty/render coverage.
4. Add focused widget input/dirty contract cases only where behavior lacks coverage.

## Todo List
- [ ] Inspect public constructor/test seams.
- [ ] Implement contract tests and guide correction.
- [ ] Run focused ViUI tests.

## Success Criteria
Guide examples compile against current APIs; Signal subscription/re-entrancy behavior is pinned; headless renderer proves real dirty/render transitions without a hidden event loop.

## Risk Assessment
Tests must observe behavior, not source structure; public v1/v2 APIs must not be conflated.

## Security Considerations
Keep `viui` unsafe-free and preserve app-owned grant/surface boundaries.

## Next Steps
Integrate evidence without promoting PARTIAL SDK availability.