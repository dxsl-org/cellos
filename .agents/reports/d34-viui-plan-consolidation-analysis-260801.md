# D34 — Consolidate overlapping ViUI plans

**Status:** approved/applied 2026-08-01. Documentation/portfolio only.

## Finding

The four plans are not four unfinished implementations. Source and their own phase tables
show most capability work landed: `260616-0755` marks all seven phases complete;
`260609-0601` marks reactive DSL, FlexBox, virtual lists, and accessibility complete with
only GPU planned; `260608-1451` contains an older mixture whose remaining G2 rows overlap
those later plans; `260608-1227` is an untouched precursor for capabilities now present.

Physically merging their phase files would erase provenance and create another false
"active" plan. D26 now makes Spec 14 the normative architecture owner.

## Recommended ruling [FINAL]

**Approve A with a consolidation correction: close/supersede, do not concatenate.**

1. Make `260616-0755-viui-completion` the closed implementation record.
2. Mark the other three superseded, with reciprocal links to the closed record and Spec 14.
3. Preserve GPU acceleration as a separately gated future item; do not keep it hidden in
   several active ViUI plans.
4. Treat signed-App/input-render/compositor-damage/target benchmarks as qualification
   gates, not unfinished widget implementation.
