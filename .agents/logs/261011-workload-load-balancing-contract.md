# 2026-10-11 — Workload routing and scaling architecture

User requested complete placement/budget/autoscaling architecture before code. Added Spec26 under ADR24/Spec25 v1.2; AC36..43 append, earlier criteria unchanged. Per-ingress router Cells own opt-in logical endpoints and exact bounded weighted slot routing, while pinned endpoints remain concrete. Fixed-home controller owns durable application budget, deterministic node placement and sample-driven desired replicas; broker remains transport-only. Router/control envelopes and uncertain children stay charged. No hidden retry, stateful writer cloning or timeout authority promotion.

Adversarial review clarified original-subject delegation, router-share reuse after death, stale route admission/drain cutoff, pending budget shrink, missing telemetry and coalesced scale-zero wake. Corrected real P08 order to A transport → C budget/controller/membership → B live routing → D autoscaling → E failure qualification; B design may precede C but live proof cannot use mock authority.

Document smoke passed: 161 classified old plans, 38 reciprocal locks, 10 pending acyclic phases, 43 EARS criteria, 18 package documents / 215 Markdown links with no broken local target; four negative controls. Final six milestone references consistent. No implementation, runtime test or performance claim.

Next: controlling plan P01 exact ABI/profile/ownership inventory. Implementation remains deferred and gated; existing locks stay closed. Handoff path: /home/dmin/cellos/.agents/261011-1200-cell-contract-authority/plan.md.
