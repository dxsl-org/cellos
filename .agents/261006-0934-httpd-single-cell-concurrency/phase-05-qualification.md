---
phase: 5
title: "Mixed-load qualification and coordinated cutover"
status: pending
priority: P1
tier: thinking
dependencies: [4]
---
# Phase 05 — Evidence, not only compilation

## Requirements
Prove one cell serves concurrently active requests across the changed full path. Keep target 256 until measured; passing at 8 is progress, not completion. Use QEMU snapshots/private disks and exact built image identities. Do not publish QEMU RPS as hardware capacity.

## Related files
Existing tests/integration/tests/{boot.rs,aarch64-boot.rs,http-infer.rs}, tests/integration/src/lib.rs; add focused http-concurrency integration target following existing harness if needed; CI job that packages httpd/net and shared ABI; scripts/gen-disk-ci.sh, scripts/gen-disk-aarch64-ci.sh, sign-policy.py only if affected; docs/network-api.md, CHANGELOG.md and current roadmap status.

## Verification steps
1. Run affected host behavior suites once after integrated code: HTTP framing/state transitions, net cap/accept/close/readiness and shared IPC correlation. Determine exact Cargo targets from manifests; no source-text or mock-echo tests.
2. Rebuild/package matched kernel/net/httpd/SDK for RV64 and AArch64, including manifest syscalls; record image hashes/features/RAM/QEMU version. Prerequisite absence fails CI, not silent green skip.
3. Use host TCP clients with barriers, not sleep-only scheduling. Hold partial request open until fast-response assertion; ensure no slow response/backend effect before release. Verify status JSON and final slow response body, not only connect success.
4. Mixed workload: slow headers, incomplete POST body, slow reader on large response, active AI request and VFS file request alongside short status requests. Complete fast requests within agreed D while slow work remains in-flight; validate client-specific data markers and exact Content-Length bytes.
5. Increase simultaneously accepted/in-flight connections 8/32/64/128/256. Measure accepted peak inside guest (not just host TCP connect), httpd cell/task identity count, memory high-water, ready/service queue depth, timeout/refusal count, p50/p95/p99 and completed RPS. Run 10-minute sustained/reconnect workload on qualification profile.
6. Push beyond limit (2N clients), abrupt FIN/RST, canceled AI, net/VFS/AI restart and httpd death/restart. Require bounded failure and resource recovery within agreed cleanup deadline, not blanket 200 responses. Verify idle CPU/wakeup count after load; no busy spin or monotonic resource growth.
7. Exercise background DNS/TLS request and delayed NIC response while short HTTP requests run. Service scheduling must continue; when transport unavailable, require explicit timeout/error rather than successful HTTP guarantee.
8. Run existing file/dynamic-content HTTP tests, native HTTP-infer test and AArch64 HTTP smoke. RV64 and AArch64 concurrency evidence required; x86 runtime claim only when its HTTP boot lane is actually exercised. Compilation alone is not portable runtime proof.
9. After smoke evidence, update network usage/limits and changelog; remove temporary benchmark scaffolding, keep meaningful regression cases. Release matched ABI/service images after second Law 1 approval; no legacy httpd mode.

## Success criteria
- [ ] 256-request mixed-workload target passes on recorded profile with one httpd cell and no per-request task spawn.
- [ ] Fast-route deadline, framing correctness, exact response data, bounded overload and cleanup all pass; report failed gates explicitly.
- [ ] Existing route/CLI contracts pass on rebuilt artifacts; real AI/VFS calls, not test-only substitutes.
- [ ] Capacity/latency report states hardware/emulator limits and backend admission limits; docs match measured defaults.

## Assumptions
Physical production device and user workload are unspecified. QEMU proves behavior, not production SLA. Failure to reach 256 is a blocker requiring diagnosis or explicit revised scope approval, never silent downgrade.

## Security
Fault-injection is local, snapshot-backed and bounded. Do not open public listeners or run uncontrolled load against production. Keep request bodies/secrets out of diagnostic output.

## Risk assessment / rollback
Drain connections then revert matched image/config; in-flight responses may fail visibly. Keep previous deployable artifact outside runtime code, not a compatibility shim. Side effects already executed by backend are irreversible.

## Deviation log
None.
