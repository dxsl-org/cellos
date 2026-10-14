# Acceptance handoff — Cell contract cutover

**Date:** 2026-10-11
**Status:** Maintainer-selected architectural baseline, not approval of an unspecified concrete ABI.
**Version:** 1.2 — workload routing, application-wide budgets, placement and autoscaling architecture.
**Plan:** [plan.md](plan.md)

The single acceptance ledger is [Spec 25](../../docs/specs/25-cell-kernel-authority-contract.md) v1.3, **AC-01..AC-50**; the approved red-team checkpoint preserves IDs while amending affected earlier semantics. [Spec 26](../../docs/specs/26-workload-placement-scaling-contract.md) v1.1 supplies normative detail, no second namespace. [ADR-0024](../../docs/decisions/0024-cell-kernel-authority-contract.md) records the decision.

Architecture only; runtime remains pending. Independent fixed-home Budget Authority enforces issuer-precommitted genesis and durable finite reservations/live destination claims. Controller owns policy/placement/membership; routers own bounded selection/queues. Destination Ready provenance and immutable origin request with issuer-sealed single target are mandatory. Budget Authority and destination kernel are enforcement TCB; controller cannot mint allowance.

P08 owns AC-44..AC-47 and AC-49/50, sharing AC-48 with P03; P10 qualifies all 50. Internal order stays A transport → C Authority genesis/claims and placement → B routing → D homogeneous-domain scaling → E failures. C supplies enforced shares/membership before live routing. Ten pending phases and 38 locks remain unchanged.

Receiver-start bounded challenges cover route/health/capacity/telemetry; unresolved old ingress generations freeze both scaling directions until fenced reconciliation. Demand/T is domain-local and excludes noninterchangeable affine work. Immutable maxima constrain live allocations; unknown reservations remain charged until terminal key closure. No per-request global budget hop, hidden retry, hedge or replay.

Changes to acceptance criteria append new IDs or explicitly retire old IDs; do not silently renumber or weaken them. Contract-breaking changes need maintainer approval. Existing ratified contracts govern running code until approved cutover.
