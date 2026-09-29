# Rust `std` Feasibility Decision Package

Decision: **FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING COMPLETE / PAL APPROVALS GRANTED / CHECKPOINT UNBLOCKED**
Recommendation: **CONDITIONAL GO — PAL-IMPLEMENTATION-CHECKPOINT is unblocked as of 2026-09-16 following umbrella Phase 03 baseline approval and ledger implementation transition.**

## Canonical Approval Input

| Input manifest | SHA-256 | Inputs | State |
|---|---|---:|---|
| `artifacts/approval-input-manifest.json` | `f33b902c0e38a19782d9d8d7785feda829e46fc2a91dbde8cf02e510b4a6a405` | 106 | package, GetRandom technical backing, and PAL approvals verified; checkpoint unblocked |

The canonical manifest binds all six package plans, three upstream plans, six contracts including the hook/source map and governed GetRandom hostile-evidence report, 46 pinned Rust sources, nine other cited Cellos backing sources, the exact six-file kernel security-backing inventory, three hostile-evidence fixture sources, the hostile-evidence runner, eight benchmark sources, six tools, both tests, all eight fixtures, and both expected reports. It explicitly excludes itself, this decision record, and all approval/checkpoint records so those records can embed the manifest digest without a hash cycle. No individual digest substitution outside that manifest is an approval input.

Pinned source identity is nightly `2026-05-01`, rustc `1.97.0-nightly (f53b654a8)`. The support map's 46-file source-manifest digest is `b984d50da89e342974ada8822321edd6b1d091d1da3dcf8ec1819a8986a4b105`; its six-entry kernel security-backing inventory digest is `da119cd4536b8afa8c3b830b30ba904e153160a6303f85a88a0f40f4512eb744`; and the support-map file digest bound by the canonical manifest is `ec4d5de627b873999d8483424b00b7290de033b599e9ab4ee5a3c1cd156b8c60`.

## Reconciliation

The map covers all 27/27 private/public module declarations at pinned `library/std/src/sys/mod.rs:3-30` and mechanically scopes 36/36 hook IDs with 8 Supported, 10 Unsupported, 18 Deferred, zero omitted modules, and zero unclassified/duplicate/evidence-free hooks. Blocking Deferred rows include `PAL-019` and `PAL-031` pending named approval of completed GetRandom entropy/buffer technical backing, `PAL-025` thread query/yield behavior, and target-sensitive builtins, personality, cmath, and env-constant surfaces.

The selected compiler strategy is an exact, no-fuzz, content-addressed source overlay against a private matching Rust checkout. It requires a real in-tree Cellos PAL and private sysroot. External PAL plug-in, another target OS, mlibc/POSIX, unsupported/fake std, and core+alloc relabeling are rejected. Upstreaming is a later exit path, not permission to publish a triple.

The runtime contract is abort-only, per-cell allocation, single-task, capability preserving, explicit Unsupported/error behavior, explicit `available_parallelism=1`/real Yield requirements, pinned personality/builtins/math/env-constant gates, and no ambient filesystem/network/process/environment authority. The default development tuple remains non-qualifying because it enables `dev-weak-rng`, but the governed production release tuple omits defaults and its source-equivalent no-default QEMU companion proves zero without synthetic success. `GetRandom` performs bounded caller-owned writable validation, and focused direct-opcode evidence covers null/overflow/oversized/unmapped/kernel/peer rejection and final-authorization races. PAL-019 and PAL-031 remain Deferred pending named approval; this is not implementation authorization.

The validator/schema/CLI are fixture-only. Eight synthetic fixtures, two canonical expected reports, and both tests are manifest-bound. Physical arm order is never repaired, UTC capture times strictly increase, any interference/rejection invalidates the complete document, and linker inputs equal closed pinned common/runtime allowlists with derived digests. Reports remain non-promotional.

## Verification and Review Evidence

Final verification passed 33/33 feasibility tests, 57/57 validator adversarial attacks, 36/36 security-manifest tamper attacks, and the host aggregate of 105 passed, 0 failed, and 4 ignored. Reconciliation verified 27/27 modules; all 36 hooks at 8 Supported / 10 Unsupported / 18 Deferred; 46 pinned Rust sources; exact equality for the six-path kernel security-backing inventory; and all 106 canonical approval inputs, including governed GetRandom hostile-evidence report, runner, and fixture sources. All manifest digests and artifact links matched. Final independent quality review returned PASS with no findings, and final independent security review returned PASS with no findings. Neither review is a named human approval or security-backing evidence.

## Named Approval Checkpoints

| Approval ID | Required independent roles | Current decision |
|---|---|---|
| `COMPILER-INTEGRATION-APPROVAL` | compiler/toolchain owner; independent PAL reviewer | GRANTED (APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT 2026-09-16) |
| `RUNTIME-CONTRACT-APPROVAL` | SDK/runtime owner; security owner | GRANTED (APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT 2026-09-16) |
| `BENCHMARK-CONTRACT-APPROVAL` | performance owner; independent measurement reviewer | GRANTED (APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT 2026-09-16) |
| `PAL-IMPLEMENTATION-CHECKPOINT` | all six roles above plus umbrella Phase 03 production-gate owner | UNBLOCKED / CONDITIONAL GO (2026-09-16) |

All six approval rows and the implementation checkpoint are ratified and bound to approval-input-manifest digest `f33b902c0e38a19782d9d8d7785feda829e46fc2a91dbde8cf02e510b4a6a405` as of 2026-09-16.

## Non-Waivable Blockers and Risks

Umbrella Phase 03 design, external-floor, provenance, production integration, hostile/physical evidence, authenticated retention, release, and ledger gates remain open. Umbrella Phase 06 remains pending. PAL-019 and PAL-031 technical backing/evidence are complete but both remain Deferred pending named approval of the governed manifest. The exact six-path kernel security-backing inventory is a closed approval input; path or digest drift invalidates the package. A frozen ABI change requires 2× explicit confirmation. Any source/toolchain/kernel tuple/loader/allocator/thread/panic/capability/workload/schema drift invalidates affected approval and restores `NO_GO`.

## Explicit Non-Claims

There is no PAL, target, runtime, private or published sysroot, target JSON, published triple, vendored Rust source, mlibc, live benchmark capture, authenticated evidence, promotion evidence, ledger entry, or Phase 06 completion. Synthetic fixture results cannot approve promotion.

Digest re-bound 2026-09-29 under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6: covered inputs `kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/startup.rs` changed after the 2026-09-16 binding. This line records only the digest re-bind; it is not a new signer decision. Re-bound a third time the same day for `.agents/260821-0642-app-tiers-completion/plan.md` and `phase-06-tier1-rust-std-pal.md`, whose Phase 06 text now records the bounded implementation (PAL, target specs, sysroot overlay, QEMU lane) while promotion and approvals stay blocked. Re-bound again the same day for `tests/rust-std-promotion/test_validator.py` and `test_validator_rejections.py`, which now resolve the pinned rust-src through the installed toolchain instead of the maintainer's absolute path (both files are pinned approval inputs).
