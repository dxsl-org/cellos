---
phase: 4
title: "Bounded copied IPC"
status: completed
priority: P1
effort: 2d
dependencies: [3]
tier: medium
---

# Phase 04: Bounded copied IPC

## Overview

Make every IPC crossing involving a domain use a bounded wire buffer owned by the kernel;
SAS-only legacy paths remain compatible but are routed through the same checked boundary.

## Requirements

- Define `IpcWireMessage { header, payload: Box<[u8]> }` with a single documented maximum
  derived from the current IPC ABI. Sender copy-in completes before queue publication;
  receiver copy-out completes under its own domain ref. Neither endpoint retains a raw peer
  pointer or access to the other's pages after completion.
- Preserve request generation, current-caller ownership, timeout, service-death, and VFS
  lease semantics. Queue records store scalar sender identity/generation plus wire bytes,
  never `DataPtr`, grant address, or borrowed user slice.
- A receive copy failure returns the defined ABI error, consumes or retains the message only
  according to an explicit existing receive contract, and does not mutate the receiver
  buffer partially. Cross-domain bounded copy is mandatory; no legacy zero-copy fallback.
- This phase does not map a grant and does not change the public IPC wire layout. DomainGrant
  begins only in Phase 06 after copied IPC has its hostile/race evidence.

## Architecture

`sender domain copy-in → owned bounded wire queue → receiver domain copy-out`; endpoint pages are never queue storage or cross-domain mapping authority.

## Assumptions

None — this phase explicitly preserves existing request-generation and VFS-lifetime ownership rather than inferring new behavior.

## Related Files

- Modify: `kernel/src/task/tcb.rs`, `kernel/src/task/syscall.rs`, `kernel/src/task.rs`.
- Create: `kernel/src/task/ipc_wire.rs`, `kernel/src/task/ipc_wire_tests.rs`.

## Implementation Steps

1. Identify every message queue and reply path; replace borrowed/identity-pointer storage
   with `IpcWireMessage` before exposing the feature to a domain task.
2. Use Phase 03 copy helpers for both endpoints and make the length bound checked before
   allocation to avoid attacker-controlled allocation or queue amplification.
3. Keep the normal scheduler/timeout cleanup ownership fields attached to the new record;
   prove sender death after copy cannot invalidate queued data.
4. Add fixture pairs for domain→domain, SAS→domain, domain→SAS, endpoint death, malformed
   lengths, and peer-address attempts.
5. Emit `S22-RV64-IPC-COPY: PASS` and `S22-RV64-IPC-NO-PEER-MAP: PASS` from test fixture.

## Test Matrix

| Runner | Cases | Gate |
|---|---|---|
| `cargo test -p cellos-kernel ipc_wire --features native-domains,test-hooks` | bounds, ABI header, timeout/death cleanup, no alias | non-QEMU |
| `bash scripts/qemu-native-domain-test.sh --harts 1 --case ipc-copy,peer-page` | copied payload and hostile peer-page denial | RV64 QEMU, 1 hart |
| `bash scripts/qemu-native-domain-test.sh --harts 2 --case ipc-copy-race` | sender/receiver exit and cross-hart delivery | RV64 QEMU, 2 harts |

## Success Criteria

- [ ] Cross-domain delivery succeeds without a sender-page mapping in receiver ledger.
- [ ] Overlarge/malformed payload cannot allocate, enqueue, or alter a receiver buffer.
- [ ] Existing SAS IPC semantics and lifecycle cleanup remain intact with flags off.

## Security Considerations

Payload bytes are the ownership transfer; queue metadata must not become an address oracle.
Do not conflate copied IPC with a revocable shared-memory grant.

## Risk Notes

The current VFS request context has separate lifetime rules. Preserve it rather than
coalescing it into generic IPC without an explicit owner-lifetime review.

## Deviation Log

None.
