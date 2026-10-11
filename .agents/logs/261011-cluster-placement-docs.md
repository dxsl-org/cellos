# 2026-10-11 — Approved cluster placement documentation amendment

- User approved broker-owned peer connections and controller-Cell placement/autoscaling with application-wide budgets.
- Updated ADR-0024 and Spec25 v1.1; appended AC30..35, preserving AC01..29. One Cell remains on one node; selected nodes belong to one admitted cluster, no federation.
- Extended existing P01/P03/P08/P10 and entrypoints; no extra phase. P08 requires durable pre-spawn reservations, destination admission, unknown-child reconciliation and safe drain. P10 requires actual hundreds-client local/cluster workload evidence.
- Retained fixed-home fenced controller/state authority, single-writer semantics, relay-first/direct-LAN qualification and all protected/ABI/physical gates.
- Document smoke: 161 classified old plans, 38 reciprocal locks, acyclic 10 pending phases, 35 EARS criteria with owners, 201 links/0 broken local targets. Phase maximum 73 lines. Four in-memory negative controls.
- No implementation/build/runtime test performed. Next: P01 exact interface/profile/ownership inventory through the controlling plan; docs do not unlock old work.
