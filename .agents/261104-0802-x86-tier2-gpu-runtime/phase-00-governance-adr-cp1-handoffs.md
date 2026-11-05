---
phase: 0
title: "Governance: hosted C++ ADR, float-ABI rule, XSAVE CP1, lock handoffs"
status: pending
priority: P1
effort: "2-3d"
dependencies: []
tier: thinking
checkpoint: true
---

# Phase R0: governance

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [plan.md](plan.md) (owner decisions D-R1…D-R10); [port gaps](../261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md) §7 Q4/Q5; [GPU CP1](../261104-0801-x86-igpu-compute/cp1-gpu-kernel-abi.md) §10 D-G0
- [ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md) §2.3; [ADR-0033](../../docs/decisions/0033-x86-intel-igpu-native-compute.md) §3, §6; [ADR-0030](../../docs/decisions/0030-license-model-and-copyleft-boundary.md)
- CP1 format precedent: [cp1-pcie-irq-power-abi.md](../261103-2218-x86-platform-breadth/cp1-pcie-irq-power-abi.md)

## Overview

R0 is documents only and needs no WIP lane. It turns the owner's 2026-11-04 instruction ("Q4 and Q5 as separate plans") into:
- the governance items each later slice needs;
- a request to the owner for decisions D-R1…D-R10.

It edits no source file.

## Key Insights

- **Hosted C++ needs an ADR.**
  - ADR-0018 §2.3 admits a language as a runtime profile under five conditions (`0018-…:85-95`). It names `cpp-freestanding` as the next profile and gates hosted C++ "on the TLS and unwinder work, not on this ADR" (`:97-101`).
  - The shipped profile is language-only. Its runner rejects `__cxa_throw`, `_Unwind_*`, `__gxx_personality` and `_ZSt*` (`scripts/qemu-cpp-smoke.sh:52-54`; `docs/guides/tier1b-c-zig.md:284-286`).
  - Condition 4 (16 MiB heap, 32 MiB VA stride, `0018-…:94`) cannot hold for llama.cpp. A new profile has to restate its budget against the large-memory CP1. That makes it an ADR, not a guide edit.
- **The float ABI is a rule, not ABI.**
  - x86 Cells build for the builtin `x86_64-unknown-none` target. On rustc 1.97.0-nightly (2026-04-30), `rustc --print target-spec-json` shows `-mmx,-sse,…,+soft-float` and `rustc-abi: softfloat`.
  - Build scripts add only `-C relocation-model=pic` (`scripts/run-std-smoke-qemu.sh:114`; `scripts/app_tier_acceptance/source.py:96`).
  - H1 changes neither the target nor `libs/api`. H2 would change the calling convention of the float functions `libs/api` exports to C (`libs/api/src/services/posix/math.rs:13-25`), and anything in `libs/api` falls under Law 1 (`docs/code-standards.md:42-46`).
- **XSAVE is kernel/HAL only.**
  - No `libs/api` or `libs/types` item describes x86 FPU state (grep for fxsave/xsave in `libs/api`, `libs/types` and `docs/abi-boundary-records.md` found only AArch64 setjmp comments, `libs/api/src/services/posix/setjmp.rs:109-116`).
  - It is still user-visible: CPUID.OSXSAVE, XGETBV, and which registers survive a trap. Hence D-R6.
- **Lock relations.**
  - Breadth slice S1 holds the Intel lane and `hal/arch/x86/**` (`.agents/plan-portfolio.md:183-189`; `cp1-pcie-irq-power-abi.md:130`). R5 waits for that release.
  - R1-R4 and R6 touch `tools/`, `meson/`, `cmake/`, `scripts/`, `third_party/`, `libs/mlibc-shim` and new test Cells.
- **Doc drift to fix when the ADR lands.** R0 records it here and edits no file now.
  - `tier1b-c-zig.md:288-290` and `docs/specs/05-application.md:207-210` say "RV64 and AArch64 only". The code admits x86_64: `libs/api/src/services/posix.rs:13-22` and `cells/tests/cpp-smoke/build.rs:126-152`.
  - `tier1b-c-zig.md:483-485` says Level B is not on x86 (still true).
  - Spec 22 (`docs/specs/22-…:3-16`) says x86 Tier-2 admission is closed. The kernel enables it in the development profile once the substrate qualifies (`kernel/src/loader/domain_admission.rs:154-158`, `:368-399`; `kernel/src/main.rs:1283-1293`).

## Requirements

- An ADR draft for `cpp-hosted`. The number is provisional; ADR-0033 is the latest in `docs/decisions/`.
- A float-ABI rule text for H1 (or the chosen option), to go into `tier1b-c-zig.md` at R1.
- `cp1-x86-xsave-user-state.md` in this directory, in the breadth CP1 format: scope, evidence, design, failure semantics, test plan, files, lock relations, decisions.
- One lock-handoff request per slice. Each lists the exact file set from that phase's Related Code Files.

## Architecture

```text
owner decisions D-R1..D-R10
  ├─ D-R4 ──> ADR "hosted C++ runtime profile" ──> R4
  ├─ D-R3 ──> float-ABI rule ──> R1 ──> R2..R6 compile flags
  ├─ D-R5/6 ─> XSAVE CP1 (this dir) ──> R5 (+ Intel lane, S1 release)
  └─ D-R2 ──> lane rule for R1-R4, R6
```

## Related Code Files

- Create (later, at R0 execution): `docs/decisions/00NN-hosted-cxx-runtime-profile.md` (number provisional), `.agents/261104-0802-x86-tier2-gpu-runtime/cp1-x86-xsave-user-state.md`
- Modify at R4 close (not now): `docs/guides/tier1b-c-zig.md` (C++ and x86 sections), `docs/specs/05-application.md` §3.2, `docs/specs/18-cell-trust-tiers.md:58-60` (profile list)
- Read only: everything cited above

## Implementation Steps

1. Draft the ADR. Cover the five ADR-0018 conditions for `cpp-hosted`:
   - static PIC PIE;
   - libc tier = mlibc `posix`;
   - facilities = TLS, futex, threads, unwinder;
   - budget = large-memory CP1 quota;
   - no dlopen or JIT (GPU shader compilation produces GPU code, not CPU-executable pages).
   Also cover licence and size measurement.
2. Write the H1 rule and its enforcement (R1 checker).
3. Draft the XSAVE CP1 from R5's design section.
4. Assemble the handoff requests.
5. Present D-R1…D-R10 to the owner. Record answers verbatim with the date.

## Todo List

- [x] ADR: [ADR-0034](../../docs/decisions/0034-hosted-cxx-runtime-profile.md), accepted under D-R4; ADR-0018 carries the amendment pointer
- [x] Float-ABI rule text, in `docs/guides/tier1b-c-zig.md` (done with R1)
- [x] XSAVE CP1 draft: [cp1-x86-xsave-user-state.md](cp1-x86-xsave-user-state.md), X-1…X-7 open for the owner
- [x] Handoff requests per slice: [handoff-requests.md](handoff-requests.md), H-1…H-6 open
- [x] Owner decisions recorded: [owner-decisions-approval.md](owner-decisions-approval.md)

## Success Criteria

- [x] Every decision D-R1…D-R10 is answered (approved 2026-11-04). The new items H-1…H-6 and X-1…X-7 are open.

## Risk Assessment

- **The ADR scope may grow** into "general hosted C++ for every arch". Mitigation: x86_64 Tier-2 first. RV64/AArch64 are a later amendment with their own witness.
- **Licence review.** Mitigation: LLVM runtimes are Apache-2.0 with LLVM Exceptions (`libcxx/LICENSE.TXT`, `libcxxabi/LICENSE.TXT`, `libunwind/LICENSE.TXT` on llvm-project `main`, fetched 2026-11-04). `deny.toml:38` already allows `Apache-2.0 WITH LLVM-exception`. ADR-0030 reuse order 1 permits permissive files (`0030-…:39-46`).

## Security Considerations

None of these decisions widens kernel authority, adds a syscall, or grants a capability. The inference Cell remains untrusted Tier-2. XSAVE enlarges user-controlled state that the kernel restores, so its CP1 must cover XRSTOR fault handling (R5).

## Next Steps

R1 after D-R3; R5 after D-R5/D-R6 and the lane; R4 after the ADR.

## Deviation Log

None.
