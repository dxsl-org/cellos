---
phase: 3
title: GetRandom hostile direct-syscall evidence
status: runtime_matrix_passed_approval_pending
priority: P1
dependencies: [2]
---

# Phase 03: Hostile Evidence

## Context Links
- `phase-02-ledger-and-syscall.md`
- `kernel/src/task/user_copy_tests.rs`
- `tests/integration/tests/boot.rs`
- `kernel/src/task/drivers/virtio_rng.rs`

## Overview
The fixture routes raw opcode 214 through the production decoder/allowlist path and proves null, overflow, above-`MAX_USER_BUF`, kernel, peer, read-only, unowned, stale-generation, retiring, and revoked-grant rejection before entropy use. It proves same-Cell stack, cross-page root segment, owned-grant, and `MAX_USER_BUF` caller capacities retain the 64-byte cap; a second no-`dev-weak-rng` build proves the zero-entropy valid and invalid paths. The focused QEMU matrix now races final authorization against real root retirement, grant revoke, and exact backing-frame unmap/reuse.

## Key Insights
The fixture derives SAS ownership from live task, segment, and grant records; it does not use Domain-copy fixtures or unrelated boot suites.

## Requirements
- Invoke syscall opcode 214 directly from a caller and inspect return/error plus destination bytes.
- Cover null, overflow, above-`MAX_USER_BUF`, unmapped, kernel, peer, stale, revoked-grant, read-only, valid stack, valid writable static heap, and valid owned grant spans.
- Cover zero-entropy and real-entropy paths.
- Prove 65-byte and `MAX_USER_BUF` caller buffers retain the capped 64-byte ABI; prove larger/overflowed descriptors are rejected before entropy.
- Cover a capped span crossing adjacent same-owner writable records and reject a span with a peer, revoked, dying, or unmapped gap.
- Race final authorization against root retirement, grant revoke, unmap, and reuse; each removal must wait for the held write lease and no post-revocation byte may commit.

## Architecture
Deterministic task/Cell fixtures with separate ownership ranges → direct opcode 214 → preflight marker → final authorization/write-lease interleaving → terminal marker containing case ID and no pointer/data values → strict QEMU parser.

## Related Code Files
`kernel/src/task/getrandom-sas-tests.rs`, `kernel/src/task/getrandom-sas-grant-cases.rs`, `kernel/src/task/syscall.rs`, ownership-ledger source, `kernel/src/memory/paging.rs`, test-hook entropy source, QEMU runner.

## Implementation Steps
1. [x] Add a raw-opcode-214 test-hook dispatch seam that runs the production decoder, allowlist, and handler.
2. [x] Build live SAS caller/sibling/peer fixtures; reject the peer, accept the sibling, and commit a 65-byte-capacity cross-page writable root segment.
3. [x] Keep deterministic entropy test-only; production remains zero-byte without a hardware RNG.
4. [x] Add direct-opcode hostile cases plus both deterministic-entropy and no-`dev-weak-rng` zero-entropy runs.
5. [x] Race final authorization against real root retirement, revocation, exact-frame unmap, and reuse.
6. [x] Run the focused fixture after mapping QEMU's test-only SiFive terminal device; retain its source tuple for approval evidence.

## Todo List
- [x] Build direct-syscall fixtures after Phase 02 lands.
- [x] Run the focused peer/sibling/cross-page fixture on QEMU with one terminal marker.
- [x] Run all hostile cases on QEMU without unrelated test suites.
- [x] Rebind approval records; named sign-off remains external.

## Runtime Evidence
- 2026-08-27 — `./scripts/qemu-getrandom-sas-test.sh` passed with exactly one `S22-RV64-GETRANDOM-SAS: PASS` terminal and no user-copy race fixture; retained log: `.logs/getrandom-sas-qemu/qemu-emBQKA.log`.
- The matrix holds GetRandom's final authorization until revocation, controlled exact-frame reissue, or `Scheduler::exit_task` root retirement can proceed; the replacement frame is cleared before its `RegGrant` record is published.

## Success Criteria
- Every hostile class fails before entropy use or user-memory write.
- Peer memory is rejected despite being globally writable in SAS.
- Retiring, revoked, unmapped, and reused records cannot commit after final authorization; adjacent same-owner cross-record output succeeds.

## Risk Assessment
A test that calls a helper instead of opcode 214 would not prove dispatch ordering. A combined boot suite can obscure the targeted terminal; isolate the fixture.

## Security Considerations
Do not log addresses, entropy bytes, or mutable test buffers. Retain only authenticated terminal case IDs and expected outcomes.

## Next Steps
Attach the focused QEMU PASS record to the PAL approval package; do not claim PAL-031 closed until named approval arrives.
