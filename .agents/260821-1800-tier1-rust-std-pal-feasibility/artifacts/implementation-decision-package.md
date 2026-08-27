# Rust `std` Feasibility Decision Package

Decision: **FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED**
Recommendation: **CONDITIONAL GO only after every blocker is implemented and evidenced. Current implementation authorization is NONE; all six named human approvals, the implementation checkpoint, and umbrella Phase 03 production gates remain blocked.**

## Canonical Approval Input

| Input manifest | SHA-256 | Inputs | State |
|---|---|---:|---|
| `artifacts/approval-input-manifest.json` | `ddbc1c293416bbd8c73a3e72e81c7b9a09a82db5209f6e6228a151ea40105a8f` | 106 | package and GetRandom technical backing verified; human approval blocked |

The canonical manifest binds all six package plans, three upstream plans, six contracts including the hook/source map and governed GetRandom hostile-evidence report, 46 pinned Rust sources, nine other cited Cellos backing sources, the exact six-file kernel security-backing inventory, three hostile-evidence fixture sources, the hostile-evidence runner, eight benchmark sources, six tools, both tests, all eight fixtures, and both expected reports. It explicitly excludes itself, this decision record, and all approval/checkpoint records so those records can embed the manifest digest without a hash cycle. No individual digest substitution outside that manifest is an approval input.

Pinned source identity is nightly `2026-05-01`, rustc `1.97.0-nightly (f53b654a8)`. The support map's 46-file source-manifest digest is `b984d50da89e342974ada8822321edd6b1d091d1da3dcf8ec1819a8986a4b105`; its six-entry kernel security-backing inventory digest is `62c7149a522a94c148da318ec1a1846985d78f7b23d0d5e03bb9e7fd95c03df6`; and the support-map file digest bound by the canonical manifest is `d5c8171ff7afece75190fd0b3ab416e1b20a39cb78beb14c31a57f4c16d70027`.

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
| `COMPILER-INTEGRATION-APPROVAL` | compiler/toolchain owner; independent PAL reviewer | NOT GRANTED — human signatures absent |
| `RUNTIME-CONTRACT-APPROVAL` | SDK/runtime owner; security owner | NOT GRANTED — human signatures absent |
| `BENCHMARK-CONTRACT-APPROVAL` | performance owner; independent measurement reviewer | NOT GRANTED — human signatures absent |
| `PAL-IMPLEMENTATION-CHECKPOINT` | all six roles above plus umbrella Phase 03 production-gate owner | BLOCKED |

Approval absence is a hard blocker and is never inferred. All six approval rows are unsigned `NOT GRANTED` records bound to approval-input-manifest digest `ddbc1c293416bbd8c73a3e72e81c7b9a09a82db5209f6e6228a151ea40105a8f`; package verification, completed GetRandom technical backing, a conditional recommendation, or either independent review grants no human approval.

## Non-Waivable Blockers and Risks

Umbrella Phase 03 design, external-floor, provenance, production integration, hostile/physical evidence, authenticated retention, release, and ledger gates remain open. Umbrella Phase 06 remains pending. PAL-019 and PAL-031 technical backing/evidence are complete but both remain Deferred pending named approval of the governed manifest. The exact six-path kernel security-backing inventory is a closed approval input; path or digest drift invalidates the package. A frozen ABI change requires 2× explicit confirmation. Any source/toolchain/kernel tuple/loader/allocator/thread/panic/capability/workload/schema drift invalidates affected approval and restores `NO_GO`.

## Explicit Non-Claims

There is no PAL, target, runtime, private or published sysroot, target JSON, published triple, vendored Rust source, mlibc, live benchmark capture, authenticated evidence, promotion evidence, ledger entry, or Phase 06 completion. Synthetic fixture results cannot approve promotion.
