# 2026-08-25 — KMS TLS signing Phase 1

## What happened
Delivered and verified the fixture-backed KMS TLS 1.3 client `CertificateVerify` vertical slice. Phase 1 closes with 59 focused tests, fail-closed production gates, and zero residual Critical/High review findings.

## Decisions
- Keep one protected-root `ProviderSlot` with independent C2C X25519 and Relay P-256 leaves; separate runtime slots could diverge and weaken the trust boundary.
- Bind signer access to the live service-net TID/cell generation and a monotonic request ID; authorize and reject replay before provider access.
- Advance replay state only after low-S normalization and self-verification; failed provider output remains safely retryable.
- Expose typed TLS protocol state only; no generic digest signer, caller-selected key, or private-key export.
- Remove every Phase 1 production PASS path; clean candidates remain blocked until hardware selection, implementation, qualification, and authenticated provenance exist in Phases 6–8.

## Lessons
- A 104-byte relay status response exposed an old private 64-byte success buffer; canonical payload sizing must be shared by all KMS responses.
- Marker/feature-string inspection cannot prove artifact provenance. Production builders must verify produced artifacts, not caller claims.
- Host checks for the service binary are not authoritative because `ostd` is target-only; use the focused host tests plus the target-oriented Cargo feature matrix.

## Next steps
- Obtain explicit approval before Phase 2 development-Silo containment.
- Keep `hardware-relay-provider` and production image creation compile/build blocked until Phases 6–8 satisfy their gates.
