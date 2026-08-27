# GetRandom Hostile Direct-Opcode Evidence

State: technical backing/evidence complete; this record is approval input, not approval.

## Scope

The fixture invokes raw syscall opcode `214` through the production decoder, allowlist, handler, bounded caller-owned writable validation, and final write authorization. It rejects null, overflowed, oversized, unmapped, kernel, peer, stale, revoked-grant, read-only, unowned, and retiring descriptors before entropy use or a user-memory write. It accepts bounded same-Cell stack, cross-page root, and owned-grant spans; the frozen ABI caps output at 64 bytes. Final authorization races root retirement, grant revoke, exact-frame unmap, and reuse.

## Governed Inputs

- Runner: `scripts/qemu-getrandom-sas-test.sh` — SHA-256 `b235b7c6e9b7fdd529e52d4fee7b7593894f5b115c4bcd1f94fdc0007d7d0258`
- Fixture: `kernel/src/task/getrandom-sas-tests.rs` — SHA-256 `187ac7be46120b4bfc373eb44a2ceffb7d65ef13d7717a8227bcf7284ae6b9dc`
- Grant cases: `kernel/src/task/getrandom-sas-grant-cases.rs` — SHA-256 `803f0363c564e84ab835259211ac1f7e7b66b819e790acb5b63803ce674bbcc4`
- Revocation race: `kernel/src/task/getrandom-sas-revoke-race.rs` — SHA-256 `fe3a79010fc1d7aea0d770391c8dd40f58cb30f7b7cad1a67d7af1bad8557341`
- Production decoder/handler: `kernel/src/task/syscall.rs` — bound by the closed kernel-security inventory.
- Entropy source: `kernel/src/task/drivers/virtio_rng.rs` — bound by the closed kernel-security inventory.

## Retained Run

Command: `./scripts/qemu-getrandom-sas-test.sh`

The runner first built the release kernel with `--no-default-features --features production-relay-image`; its resulting kernel SHA-256 was `4a7ecae9e03fc641afc1839a7adba6409d922fa04f983a5e77cbb4824b9e5e56`. It then ran two isolated QEMU companions through raw opcode `214`:

- Development posture: `.logs/getrandom-sas-qemu/qemu-dev-weak-5dAIWX.log`, SHA-256 `9f2109b1bd3a0185ff92ef84bd91ef273b6b7a7a0204d1cd1f9a20c1ba704ced`; one PASS terminal and one explicit weak-xorshift warning.
- Production-zero posture: `.logs/getrandom-sas-qemu/qemu-production-zero-4EvnS7.log`, SHA-256 `42b9b3fa143bd8fb34ea490563d2238cf527c3a42787e89f9bfbbb31225f505a`; one PASS terminal and no weak-xorshift warning.

The production-zero companion uses `--no-default-features --features getrandom-sas-test`. After deterministic test entropy is disabled, its valid direct GetRandom call returns zero and its invalid peer pointer is rejected before entropy. Both logs contain exactly one terminal:

```text
[ INFO] S22-RV64-GETRANDOM-SAS: PASS
```

This binds a production release tuple that excludes `dev-weak-rng` plus a source-equivalent focused runtime companion proving observable zero without synthetic success when entropy is unavailable.

## Approval Boundary

This governed report binds the evidence plan, production release tuple, runner, fixture sources, retained-log digests, and terminal results for reviewer verification. It completes PAL-019's technical zero/error backing evidence but does not qualify the default `dev-weak-rng` tuple, change `PAL-019` or `PAL-031` from `Deferred`, grant any named approval, unblock `PAL-IMPLEMENTATION-CHECKPOINT`, or authorize PAL, target, sysroot, runtime, live capture, or promotion work.
