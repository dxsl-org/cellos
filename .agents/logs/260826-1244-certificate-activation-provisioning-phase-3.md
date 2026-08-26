# 2026-08-26 — Certificate activation and provisioning Phase 3

## What happened
Delivered the frozen KMS enrollment ABI, canonical RFC 2986 CSR construction, supervisor-bound enrollment lifecycle, service-net staging, and atomic relay-generation promotion. The development Silo keeps nonce-bound pending and active P-256 keys inside the guest. Final evidence: 140/140 focused Rust tests, relay-enroll 10/10, relay-manifest 11/11, clean RV64/AArch64 KMS checks, clean AArch64 development-Silo packaging, OpenSSL CSR self-verification, and final code/security GO reviews.

## Decisions
- Freeze additive opcodes 9–14 and errors 22–26; no aliases, shims, private material, or certificate-chain transport in the KMS frame ABI.
- Require `Prepared → CsrIssued → Staged`; every CSR chunk is consumed in order before service-net staging and supervisor commit.
- Bind pending Silo keys to generation plus a fresh nonzero nonce, confirm cleanup before acknowledging abort/replacement, and retain retry tombstones after ambiguous failures.
- Persist active generation, policy/profile digest, authenticated-time floor, and restart floor only through an authenticated protected journal. Runtime without a real sealing key remains sealed.
- Treat raw platform/QEMU RTC as unauthenticated. TLS remains unavailable until protected authenticated time and its rollback floor exist.
- Keep opcode 14 active-key-only. Pending-key precommit certificate binding is unavailable under the frozen ABI and must not be claimed.
- Keep production `BLOCKED_PENDING_PHASE_6_7_8`; development Silo and software checks are not hardware qualification.

## Lessons
- DER length tests were insufficient without an independent parser: the initial signature AlgorithmIdentifier used the OID content length instead of the full OID TLV length.
- Host tests did not compile AArch64-only Silo paths; the exact development-provider lane found mailbox constant shadowing, return-type errors, response-correlation gaps, and guest lockfile drift.
- Cleanup is part of key custody: stale/order invalidation, post-create proof failure, and transport-ambiguous destroy all require confirmed deletion or a retained tombstone.
- `TlsClock::None` skips certificate validity in the current TLS library; fail-closed behavior must gate the connection before the verifier.

## Next steps
- Obtain explicit approval before Phase 4 relay mTLS consumption.
- Integrate real protected persistence and authenticated time before enabling runtime activation.
- Preserve `BLOCKED_PENDING_PHASE_6_7_8` until hardware custody, qualification, and provenance are complete.
