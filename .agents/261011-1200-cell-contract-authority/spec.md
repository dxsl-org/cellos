# Acceptance handoff — Cell contract cutover

**Date:** 2026-10-11
**Status:** Maintainer-selected architectural baseline, not approval of an unspecified concrete ABI.
**Version:** 1.2 — workload routing, application-wide budgets, placement and autoscaling architecture.
**Plan:** [plan.md](plan.md)

The single canonical acceptance specification is [Spec 25 — Cell, Kernel Cell and authority](../../docs/specs/25-cell-kernel-authority-contract.md), AC-01..AC-43. AC-01..AC-35 retain their IDs and requirements. [Spec 26 — Workload placement and scaling](../../docs/specs/26-workload-placement-scaling-contract.md) is its detailed normative extension, not a copied acceptance list or independent AC namespace. [ADR-0024](../../docs/decisions/0024-cell-kernel-authority-contract.md) records the decision and alternatives.

The maintainer requested architecture/documentation only. Separate fixed-home fenced controller and per-ingress workload-router Cells own control and request paths respectively. Logical replication-compatible endpoints are opt-in and preserve original-subject/exact-method authority; concrete endpoints remain generation-pinned. Application budgets include router/controller envelopes, all child resources and unknown/draining reservations; independent destination admission and issuer/state fences still decide authority. One Cell stays on one node; no request-per-Cell rule, federation, DSM, transparent migration or multiwriter promotion.

P08 owns shared AC-30, AC-31..AC-34 and AC-36..AC-42 through gated internal milestones A transport → C budget/placement → B routing/admission → D autoscale/reconcile → E failure qualification. Labels do not imply execution order: C's durable controller budget, real ingress shares and bootstrap route membership are prerequisites for B's live routing; mock grants are not substitutes. P10 owns AC-35 concurrent-workload and AC-43 actual multi-node load-balancing/autospawn/partition/budget evidence. The ten-phase pending DAG and all 38 locks remain unchanged. P01 exact delta reviews and both Law-1 checkpoints still govern public symbols, encodings, manifests and persistent lifecycle/ledger surfaces; protected Build/activation, physical and production gates remain independent.

Durable reservation precedes spawn; release requires authoritative terminal evidence and key closure preventing later publication, not timeout or momentary absence. Routers use bounded weighted selection and preallocated budget shares without global per-request controller/ledger work. Submitted target stays frozen, destination refusal is returned and ambiguous work is not replayed. Missing telemetry freezes scaling; controller outage permits unchanged routing only while route validity and destination live authority/state fences hold. P08's executable oracle traces and P10's runtime witness are future requirements, not results established here.

Changes to acceptance criteria append new IDs or explicitly retire old IDs; do not silently renumber or weaken them. Contract-breaking changes need maintainer approval. Existing ratified contracts govern running code until approved cutover.
