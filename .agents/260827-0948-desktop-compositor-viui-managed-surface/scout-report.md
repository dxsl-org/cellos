# Scout report

- `libs/viui/src/renderer.rs`: framebuffer backend currently discards the supplied damage rectangle.
- `libs/viui/src/app_runner.rs`: structural changes already force full layout/repaint; signal changes retain widget bounds.
- `libs/ostd/src/display/lifecycle.rs`: title, configure Grant swap/ack, state request, and close response are implemented.
- `libs/ostd/src/display/routing.rs`: bounded typed lifecycle polling is implemented.
- `cells/tests/window-policy-probe/src/events.rs`: canonical lifecycle handling precedent.
- `cells/demos/viui-demo/src/main.rs`: generated UI is compile-only and exits.
- `cells/apps/robot-dashboard/src/main.rs`: canonical ViUI input/tick loop precedent.

Baseline: focused ViUI app-runner tests 3/3 pass; compositor framebuffer tests 3/3 pass. Full ViUI host suite has six pre-existing FlexBox failures.

