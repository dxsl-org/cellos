---
title: "Old Artifact Audit"
status: pending
created: 2026-08-19
---

# Old Artifact Audit

## Verdict

Use the old work as PRIOR evidence only. The recovery plan supersedes `.agents/260624-cell-to-cell-anywhere/` because current code and the D38 ruling agree that foundation exists but product runtime completion is false.

## Old Research And Plans Found

- `.agents/260623-remote-cell-ipc-research/research-report.md`
- `.agents/260623-remote-cell-ipc-research/cluster-membership-report.md`
- `docs/research/research-distributed-cells-internet.md`
- `.agents/260624-cell-to-cell-anywhere/plan.md`
- `.agents/260624-cell-to-cell-anywhere/phase-00-remote-call-api-contract.md`
- `.agents/260624-cell-to-cell-anywhere/redteam-report.md`
- `docs/specs/20-unified-ipc-contract.md`
- `.agents/reports/d24-spec20-ratification-order-analysis-260801.md`
- `.agents/reports/d38-false-completion-status-analysis-260801.md`

## Corrected Ranking

The prior draft selected the wrong ranked row while the table's best option was the transport-neutral broker contract. Corrected decision: choose Candidate B, the explicit endpoint plus userspace broker architecture.

## Current Evidence

- D38 says Cell-to-Cell Anywhere has real foundation modules, but `dispatch` lacks end-to-end remote forwarding and remote lookup resolves locally: `.agents/reports/d38-false-completion-status-analysis-260801.md:12`.
- D38 says mark the old plan partial, foundation complete, integration blocked: `.agents/reports/d38-false-completion-status-analysis-260801.md:23`.
- D38 requires a two-node remote-call oracle before any COMPLETE claim: `.agents/reports/d38-false-completion-status-analysis-260801.md:24`.
- Current broker loop still has relay-frame dispatch TODO and an empty `dispatch`: `cells/services/net-broker/src/main.rs:133`, `cells/services/net-broker/src/main.rs:153`.
- `routing.rs` says the broker routing module is not wired from `main.rs`: `cells/services/net-broker/src/routing.rs:1`, `cells/services/net-broker/src/routing.rs:4`.
- `relay.rs` says send/register/receive paths are liveness stubs and inbound relay frames are not wired: `cells/services/net-broker/src/relay.rs:30`.

## Older Work To Reuse

- Cluster modes and `CellNetId` are a valid starting point, but `ClusterId` remains routing-only, not a credential: `libs/api/src/services/cluster.rs:8`, `libs/api/src/services/cluster.rs:111`.
- Noise prologue already binds cluster id plus local and remote NodeIds: `cells/services/net-broker/src/transport.rs:139`, `cells/services/net-broker/src/transport.rs:157`.
- Relay framing already has NodeId registration plus send/receive packet types: `cells/services/net-broker/src/relay.rs:14`, `cells/services/net-broker/src/relay.rs:16`, `cells/services/net-broker/src/relay.rs:17`.
- K2 first-boot identity exists as a no-Law-1 planned lane in the DICE/KMS plan: `.agents/260712-1902-dice-attestation-identity/plan.md:40`, `.agents/260712-1902-dice-attestation-identity/plan.md:42`.

## Stale Or Risky Claims

- Any "G1 complete" language is stale if read as runtime completion; only foundation code exists until the two-node oracle passes.
- Any plan that makes remote look local is unsafe; remote has typed failures and retry classes.
- Any plan starting with NAT traversal is premature; relay-first correctness is the unblocker.
- Any plan requiring kernel-distributed IPC violates the prior invariant that remote enforcement stays in userspace: `docs/project-changelog.md:1623`.

## Output Into Main Plan

Candidate B is the default. Candidate A is a contingency only after oracle failure, no userspace fix, and two Law-1 confirmations.
