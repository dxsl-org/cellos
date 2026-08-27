# Tier 3 QEMU Hostile Scenario Matrix

## Scope

Phase 06 owns guest payloads, host runners, strict parsers, and raw logs. It
must not edit `vmm.rs`, `virtio_blk.rs`, or `virtio_net.rs`; Phases 09 and 10
own persistence and x86 transport production changes.

## Environment Rules

- x86 strict boot uses the pinned QEMU-TCG 10.2.0 binary. The 2026-08-27
  baseline reached Alpine `/bin/sh` at 1 GiB through
  `scripts/qemu-hypervisor-smoke-x86.sh`.
- Ubuntu QEMU-TCG 8.2.2 is an environment incompatibility result, never a
  runtime PASS.
- ARM64 QEMU-TCG may establish current machinery/recovery markers only. It
  does not provide EL2 or physical-containment fidelity.
- A scenario PASS means only the stated host/QEMU evidence class. It never
  promotes persistence, transport parity, physical qualification, or production.

## Matrix

| Scenario | x86 QEMU 10.2.0 | ARM64 QEMU-TCG | Guest/payload owner | Strict pass markers | Failure markers |
|---|---|---|---|---|---|
| Strict boot precondition | Required: Alpine reaches `/bin/sh` | Machinery marker only | Existing smoke images | shell or documented machinery marker | timeout, panic, cell fault, VMM error |
| Guest-memory bounds | Required | Required where current VMM path is executable | New malformed-GPA payload | rejection marker; no host panic/cell fault | accepted out-of-range GPA, panic, cross-guest effect |
| VirtIO descriptor shape | Required after transport exposure | Required against current MMIO personality | New malformed descriptor payload | reject/reset marker; bounded parser | accepted malformed chain, panic, stale queue state |
| Reset and supervisor recovery | Required | Required | New reset/restart runner | restart marker and clean second request | stale state, wedged VMM, host service failure |
| vCPU budget | Required | Required when `RunVcpu` path is available | New bounded-budget payload | observable budget exhaustion; host remains responsive | unbounded run, host hang, missing recovery |
| Backend unavailable | Record only; Phase 09/10 own assertions | Record only; Phase 09 owns assertions | Shared runner hooks | IO error or unavailable marker | false success or unrelated corruption |
| Persistent FLUSH/reboot/read | Reserved for Phase 09 | Reserved for Phase 09 | Phase 09 | no Phase 06 PASS | any Phase 06 persistence claim |
| x86 block/network parity | Reserved for Phase 10 | Reference behavior only | Phase 10 | no Phase 06 PASS | any Phase 06 parity claim |

## Runner Contract

Every runner must retain the raw QEMU log, normalize it without deleting
failure markers, enforce a finite timeout, and reject kernel panic, cell fault,
VMM error, missing liveness, cross-scenario stale state, or missing recovery
markers. It emits one of `PASS`, `FAIL`, `BLOCKED_ENVIRONMENT`,
`BLOCKED_SCOPE`, or `NOT_APPLICABLE`; it must never map a blocked result to
`PASS`.

## Exclusions

No production VMM/VirtIO edits, persistence assertion, x86 transport selection,
physical nested-virtualization claim, physical containment claim, or acceptance-ledger update.
