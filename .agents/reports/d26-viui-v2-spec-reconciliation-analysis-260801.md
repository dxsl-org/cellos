# D26 — Reconcile the ViUI v2 specification with the shipped architecture

**Status:** approved/applied 2026-08-01. No runtime or ABI changed.

## Finding

`docs/specs/14-viui.md` still says "awaiting G2 implementation" and specifies an
egui-compatible immediate facade plus an iced-compatible Elm facade. The tree instead
contains the ViUI v2 reactive node architecture: `Signal<T>`/computed subscriptions,
`ViNode`, signal-driven widgets, navigation, overlays, charts, and virtualized lists.
It also ships two authoring paths: inline `vi_design!` and build-time `.vi` compilation.

The old performance table is not backed by a reproducible benchmark and the advertised
egui/iced compatibility percentages are not implemented contracts.

## Recommended ruling [FINAL]

**Approve recommendation A: rewrite Spec 14 in place as the ViUI v2 specification.**

1. Make the Reactive Signal Tree (`Signal<T>` + `ViNode`) the normative UI model.
2. Define the dual layer as Rust node APIs plus the `.vi` DSL (`vi_design!` inline or
   `viui-build` at build time), not compatibility facades for third-party toolkits.
3. Document only source-backed capabilities: flex layout, dirty-region rendering,
   widgets, navigation/overlay, charts, and virtualized lists.
4. Withdraw the unimplemented egui/iced compatibility promises and unmeasured latency
   table. Performance claims require a checked-in benchmark and generated result.
5. Retain the no-std/direct-rendering rationale, while separating implemented library
   surface from end-to-end product qualification.
