# GetRandom Hostile Direct-Opcode Evidence

State: technical backing/evidence complete; this record is approval input, not approval.

## Scope

The fixture invokes raw syscall opcode `214` through the production decoder, allowlist, handler, bounded caller-owned writable validation, and final write authorization. It rejects null, overflowed, oversized, unmapped, kernel, peer, stale, revoked-grant, read-only, unowned, and retiring descriptors before entropy use or a user-memory write. It accepts bounded same-Cell stack, cross-page root, and owned-grant spans; the frozen ABI caps output at 64 bytes. Final authorization races root retirement, grant revoke, exact-frame unmap, and reuse.

## Governed Inputs

- Runner: `scripts/qemu-getrandom-sas-test.sh` — SHA-256 `2e9f0124645d5077de22e2008ee89a7a31101590e1674287d3bf185bf4f4e5fa`
- Fixture: `kernel/src/task/getrandom-sas-tests.rs` — SHA-256 `187ac7be46120b4bfc373eb44a2ceffb7d65ef13d7717a8227bcf7284ae6b9dc`
- Grant cases: `kernel/src/task/getrandom-sas-grant-cases.rs` — SHA-256 `803f0363c564e84ab835259211ac1f7e7b66b819e790acb5b63803ce674bbcc4`
- Revocation race: `kernel/src/task/getrandom-sas-revoke-race.rs` — SHA-256 `fe3a79010fc1d7aea0d770391c8dd40f58cb30f7b7cad1a67d7af1bad8557341`
- Production decoder/handler: `kernel/src/task/syscall.rs` — bound by the closed kernel-security inventory.
- Entropy source: `kernel/src/task/drivers/virtio_rng.rs` — bound by the closed kernel-security inventory.

## Retained Run

Command: `./scripts/qemu-getrandom-sas-test.sh`

The retained raw QEMU log was captured at `.logs/getrandom-sas-qemu/qemu-emBQKA.log` with SHA-256 `f2d7737071dc393b3abd8799ddcc493b2686ef1ebd967a3fd8b8c16266231b42`. The runner accepted the run only if QEMU exited with status zero and the terminal occurred exactly once. Recorded terminal:

```text
[ INFO] S22-RV64-GETRANDOM-SAS: PASS
```

## Approval Boundary

This governed report binds the evidence plan, runner, fixture sources, retained-log digest, and terminal result for reviewer verification. It does not qualify the default `dev-weak-rng` entropy tuple, change `PAL-031` from `Deferred`, grant any named approval, unblock `PAL-IMPLEMENTATION-CHECKPOINT`, or authorize PAL, target, sysroot, runtime, live capture, or promotion work.
