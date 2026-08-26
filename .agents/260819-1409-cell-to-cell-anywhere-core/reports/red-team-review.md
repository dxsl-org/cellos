---
title: "Red Team Review - Cell-to-Cell Anywhere Core"
status: complete
created: 2026-08-19
verdict: pass_with_risk
---

# Red Team Review - Cell-to-Cell Anywhere Core

**VERDICT:** PASS_WITH_RISK - prior BLOCKED findings are resolved at plan level. Implementation planning may proceed only after user approval; public/production exposure and COMPLETE status remain blocked until frozen budgets plus relay/LAN oracle evidence pass.

## Blocked Findings Resolved

- Phase 01 now freezes measurable feasibility/release budgets: local IPC p99 <=5% regression, zero watchdog expirations, queue/cache memory budgets, 10k accepted unary-call soak with zero silent drops/duplicate local dispatches, and measured broker concurrency/saturation target.
- Candidate A now requires reproducible failure against frozen targets, ingress-specific root cause, no userspace correction, and Law-1 double confirmation.
- Phase 03 now blocks Phase 04 until scheduler coexistence, request/reply correlation, bounded wakeups, heartbeat/watchdog, and network polling are prototyped.
- Phase 02 now consumes DICE identity Phase P04 as the single stable identity lifecycle owner and keeps public/remote disabled until key lifecycle is pinned.
- Export registry authority is now init/supervisor-provisioned, broker read-only at runtime, atomically replaced, version-validated, and fail-closed.
- Phase 04 now scopes at-most-one local dispatch to authenticated `(src_node, src_boot_epoch, request_id, dst_server_epoch)` retention window, with `Indeterminate` after eviction/expiry for non-idempotent requests.
- Typed client API ownership is assigned to `libs/ostd/src/cluster.rs` or a focused `ostd` module with `LocalEndpoint<M>`, `RemoteEndpoint<M>`, and deliberate `CellEndpoint<M>` union.
- Error taxonomy, retry mapping, and no-evict-in-flight gates moved to Phase 04/05 entry; Phase 07 operationalizes them.

[HIGH] `tools/relay-server/relay.py:63` - current relay registers an arbitrary `node_id` without proof of key possession, so duplicate registration can hijack or deny a peer before the new plan's authenticated relay requirement exists. Fix in Phase 05 by adding a challenge signed or Noise-proven by the stable node key; rollback by disabling relay mode and clearing relay allowlist, but any traffic metadata already exposed to the relay cannot be undone.

[HIGH] `cells/services/net-broker/src/main.rs:104` - current per-run X25519 identity breaks stable addressing and makes replay/dedup identity unstable across reboot. Fix in Phase 02 before any remote oracle; rollback by disabling remote exports and deleting the newly created node key after operator approval, but peers that learned the old key need explicit re-enrollment.

[HIGH] `.agents/260819-1409-cell-to-cell-anywhere-core/phase-02-stable-node-identity-and-exported-endpoint-registry.md:46` - first-boot key creation is planned, but the plan does not yet pin exact key path, file permissions, or clone-image rekey protocol. Fix by making those Todo items hard acceptance criteria before Phase 03 starts; rollback is local-only mode, while duplicate node identity events already emitted remain audit evidence.

[MED] `cells/services/net-broker/src/routing.rs:55` - current route model maps `service_id` to `machine_id`, which is too coarse for exported-service authorization and confused-deputy prevention. Phase 02/04 correctly move to `(node_id, service_id, export_id)`; keep this as a blocking test in the oracle.

[MED] `libs/ostd/src/cluster.rs:86` - current raw remote lookup waits on unmasked `sys_recv(0)` and trusts any sender ordering, which can mis-associate replies under concurrent broker traffic. Phase 03 must replace this with broker-owned request ids and bounded reply queues before exposing typed remote endpoints.

[RESOLVED-MED] `.agents/260819-1409-cell-to-cell-anywhere-core/phase-04-c2c-envelope-request-semantics-and-dedup.md:68` - the plan now limits at-most-one local dispatch to the authenticated retention window; expiry/eviction maps non-idempotent work to `Indeterminate`, and exhaustion must not evict retained in-flight work into duplicate dispatch.

[MED] `.agents/260819-1409-cell-to-cell-anywhere-core/phase-07-failover-backpressure-and-observability.md:70` - log redaction is stated, but the plan should test that payloads, PSKs, private keys, and full request bodies never appear in normal or oracle logs. Add this to Phase 08 failure injection.

[LOW] `.agents/260819-1409-cell-to-cell-anywhere-core/phase-09-rollout-docs-and-contingency.md:35` - red team and validation are sequenced after oracle evidence; keep a pre-implementation security checklist too, because relay auth and key lifecycle are design gates, not only release gates.

[POSITIVE] `.agents/260819-1409-cell-to-cell-anywhere-core/plan.md:44` - the plan makes explicit exports, node-level auth, request id, epochs, bounded dedup, Busy/Indeterminate, authenticated relay, and isolated oracles non-negotiable.

[POSITIVE] `.agents/260819-1409-cell-to-cell-anywhere-core/phase-05-relay-first-remote-correctness-oracle.md:60` - relay-only two-node oracle is the right minimum proof for "remote" because LAN success alone would not prove Anywhere.
