# Rust `std` Workload Parity Specification

Contract ID: `CELLOS-RUST-STD-WORKLOAD-v1`
Validator: [`../../../scripts/rust_std_promotion/validator.py`](../../../scripts/rust_std_promotion/validator.py)

## Logical Operation Traces

Timer boundaries enclose only the listed logical operations. Loader/startup, fixture construction, warmups, cell reset, peer reset, report serialization, and teardown are excluded and identical between arms.

### `syscall-yield-v1`

1. enter timed region;
2. issue exactly one `ViSyscall::Yield` round trip;
3. resume the same cell after the kernel scheduler returns;
4. leave timed region.

There is no payload. The fixture payload identity is SHA-256 of the literal `yield-empty-payload`: `3fd0003a3ea3769b7cacf99af231b0e5ccf36cb25e3e32cef546c6f48182dd2c`. Canonical logical trace `yield:enter,sys_yield,return` has digest `26132603acefd7c2e507e73d72a7e8b1c8afec07c646da390cba1cf6b173f7c3`.

### `ipc-echo-64-v1`

The private echo peer is started before all arms, pinned by binary/service-state digests, receives no other traffic, and is reset to the same empty state before every fresh repetition.

1. enter timed region;
2. send exactly one 64-byte request to the same pinned peer;
3. peer receives exactly those 64 bytes and sends them back unchanged;
4. caller receives exactly one 64-byte reply;
5. compare all 64 reply bytes with the request; mismatch is an invalid repetition, never a latency sample;
6. leave timed region.

Request and reply bytes, in order, are:

`00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f 10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f 20 21 22 23 24 25 26 27 28 29 2a 2b 2c 2d 2e 2f 30 31 32 33 34 35 36 37 38 39 3a 3b 3c 3d 3e 3f`

Payload digest: `fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108`. Canonical trace `ipc:enter,send64,recv64,verify64,return` has digest `ab3515a0c24acbf6eff2b96ef6873543b635c88722f6b6b1e02a29394d16010a`.

Errors, retries, service discovery, reconnects, partial transfers, extra yields, allocation, and logging inside the timed region invalidate the repetition. They are never normalized into the one-operation workload.

## Exact Parity Tuple

Every arm must match exactly on:

`(architecture, environment_kind, board_model, board_revision, qemu_binary_digest, qemu_version, machine, firmware_digest, cpu_model, cpu_count, hart_count, frequency_policy, timer_source, timer_frequency_hz, build_profile, rustc_commit, rust_src_digest, target_spec_digest, source_revision, common_codegen_flags_digest, common_linker_inputs, common_linker_inputs_digest, admission_manifest_digest, capability_manifest_digest, service_topology_digest, service_state_digest, workload_id, workload_version, payload_digest, operation_trace_digest)`.

The schema names `rustc_commit` as `toolchain.commit_hash`, `common_codegen_flags_digest` as `codegen_flags_digest`, and timer frequency as `environment.timer_frequency_hz`; the logical tuple is unchanged. `common_linker_inputs` is a closed ordered manifest of `(role, identity, digest)` entries and its digest is derived from canonical JSON. Each arm also carries a closed ordered `runtime_linker_inputs` manifest and derived digest selected from the pinned `no_std` or `std` fixture allowlist. Only `runtime_kind`, `binary_digest`, and the corresponding pinned runtime manifest/digest may differ. Additions, omissions, reordering, role/digest swaps, arbitrary digest exemptions, mlibc, POSIX/libc, host libraries, and instrumentation identities are `INVALID`.

## Protocol

Physical document order is exactly `no_std_pre/1 → std/2 → no_std_post/3` for each declared cell in source `expected_cells` order; the validator never sorts or repairs arms. `captured_at` is UTC `Z` and strictly increases within each physical triple. Each arm discards at least five warmups, then retains at least 30 independent repetitions. Run, repetition, and fresh cell instance IDs are globally unique across every arm and cell in the document. The peer/service topology is reset, not replaced. Every raw positive nanosecond latency remains in the fixture.

Interference policy and thresholds are declared before any arm, but this feasibility contract permits no selective sample removal: if any interference Boolean is true or any rejection record exists in any arm, the entire fixture/cohort is `INVALID` before statistics. No post-hoc outlier rule, winsorization, std-only policy, runtime-overhead label, or aggregate masking is permitted.

The fixture validator cannot establish real workload parity: it checks only synthetic contract examples and always reports `promotion_eligible=false`.
