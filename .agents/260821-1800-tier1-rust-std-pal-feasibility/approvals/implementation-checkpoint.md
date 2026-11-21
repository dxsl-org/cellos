# PAL-IMPLEMENTATION-CHECKPOINT

Decision: **UNBLOCKED / CONDITIONAL GO**
Feasibility state: `FEASIBILITY_PACKAGE_VERIFIED_SECURITY_BACKING_AND_HUMAN_APPROVAL_GRANTED`
Canonical approval-input-manifest SHA-256: `baf514598589ec4680151dc0629bacb8396baaf3b2bfba0ac6dbe6d724b8761d` (independently package-verified; GetRandom technical backing complete; human approvals granted; umbrella Phase 03 baseline approved and implemented).

Required before a later child may be created:

1. `PAL-019` technical gate is satisfied: the governed production release tuple omits `dev-weak-rng`, and a source-equivalent no-default direct-syscall companion proves zero without synthetic success;
2. `PAL-031` technical gate is satisfied: bounded caller-owned writable validation and hostile direct syscalls prove null, overflowed, oversized, unmapped, kernel, and peer pointers are rejected without reads/writes;
3. the exact six-path kernel security-backing inventory remains closed, present, digest-matched, and included in this approval manifest;
4. `COMPILER-INTEGRATION-APPROVAL`, `RUNTIME-CONTRACT-APPROVAL`, and `BENCHMARK-CONTRACT-APPROVAL` are each explicitly granted by both named roles;
5. umbrella Phase 03 production gates are explicitly approved by their named owner;
6. this exact approval-input manifest is independently verified and all records are re-bound only if any covered input changes.

The six conditions above are satisfied as of 2026-09-16:
1. `PAL-019` technical gate is satisfied (production tuple omits dev-weak-rng, verified zero/error evidence).
2. `PAL-031` technical gate is satisfied (bounded caller-owned writable validation and hostile pointer tests pass).
3. Six-path kernel security inventory is closed, present, and digest-matched.
4. `COMPILER-INTEGRATION-APPROVAL`, `RUNTIME-CONTRACT-APPROVAL`, and `BENCHMARK-CONTRACT-APPROVAL` are each explicitly granted by both named roles.
5. Umbrella Phase 03 production gates and Spec 18c provenance contract are approved by their named owner (2026-09-16).
6. Canonical approval-input-manifest is independently verified and all records are bound to `baf514598589ec4680151dc0629bacb8396baaf3b2bfba0ac6dbe6d724b8761d`.

This checkpoint is **UNBLOCKED / CONDITIONAL GO**. In-tree CellOS PAL implementation planning may proceed. Target publication, external triple, and production promotion remain gated behind final live evidence.

Digest re-bound 2026-09-29 under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6: covered inputs `kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/startup.rs` changed after the 2026-09-16 binding. This line records only the digest re-bind; it is not a new signer decision. Re-bound a third time the same day for `.agents/260821-0642-app-tiers-completion/plan.md` and `phase-06-tier1-rust-std-pal.md`, whose Phase 06 text now records the bounded implementation (PAL, target specs, sysroot overlay, QEMU lane) while promotion and approvals stay blocked. Re-bound again the same day for `tests/rust-std-promotion/test_validator.py` and `test_validator_rejections.py`, which now resolve the pinned rust-src through the installed toolchain instead of the maintainer's absolute path (both files are pinned approval inputs).

Digest re-bound 2026-11-16 under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6, owner-approved in Phase 19 of the architecture remediation (`.agents/261110-1703-architecture-remediation/phase-19-closure.md`, Deviation Log). The remediation moved covered inputs without changing their roles: `kernel/src/task/syscall.rs` split into `kernel/src/task/syscall/{mod,decode,ipc,misc,fs}.rs`; `kernel/src/task/drivers.rs` and `drivers/virtio_rng.rs` became `kernel/src/dev.rs` and `dev/virtio_rng.rs`; `libs/api/src/abi.rs`, `abi/syscall.rs` and `services/benchmark.rs` became `libs/abi/src/lib.rs`, `libs/abi/src/syscall.rs` and `libs/wire/src/benchmark.rs`; `kernel/src/task/getrandom-sas-*.rs` moved to `kernel/src/arch/riscv64/task/`. Other covered inputs changed in place. The six-path inventory named in condition 3 is the same six sources, now eleven paths: the ten re-pinned paths plus `kernel/src/task/syscall/grant.rs`, added in a second re-bind the same day because GetRandom's final write (`syscall/misc.rs`) takes the grant-table locks defined there. The manifest lists 111 inputs (was 106). This line records only the digest re-bind; it is not a new signer decision.
