# D12 — Hardware supplement set for Tier 1

**Date**: 2026-08-01 · **Question from the docket**: should Spec 05 §2.1 stop
presenting MTE/MPK/PMP as the Tier-1 hardware supplement and instead point to Spec 19's
Layer A/B/C model? · **Method**: compare the current specifications, deployment hardware,
x86 page-table and PKU paths, RISC-V PMP code, self-tests, project status documents, and
the commit that introduced the hardware-supplement scaffolding.

**Ruling**: Recommendation A approved and applied 2026-08-01. Spec 19 owns the taxonomy;
Specs 05/10/12/15/16 and the directly implicated living/history documents now state the
actual MTE, PKU, PMP, and self-test boundaries. No runtime code or ABI changed.

## Answer first

**Yes. Spec 19 should be the sole owner of the hardware-isolation taxonomy.**

Spec 05's table is not merely stale; each row gives the mechanism a property that the
current system does not provide:

- MTE is probabilistic spatial-memory hardening, not Spectre mitigation, and the RK3588
  deployment target has no FEAT_MTE.
- x86 PKU has feature detection, PKRU state, and return-path switching, but no page is
  assigned a non-zero PTE protection key. It therefore provides no cell-domain isolation.
- RISC-V PMP cannot be programmed by Cellos in S-mode. The repository contains descriptors
  for a future M-mode firmware shim, not a dynamic per-cell fence.

Spec 16 repeats the MTE/MPK category error as a speculative-execution mitigation. Spec 12's
PMP statement is accurate and should remain. Spec 19 already expresses the honest model:
implemented W^X as Layer A, future per-domain page tables as load-bearing Layer B, and
hardware-gated bonuses such as MTE/MPK as non-load-bearing Layer C.

## 1. The contradictory normative claims

`docs/specs/05-application.md:56-69` lists a planned Tier-1 hardware supplement:

- ARM64 MTE for "Pointer tags, Spectre mitigation";
- x86 MPK for "16 per-Cell access domains, no TLB flush";
- RISC-V PMP as an "M-mode fence" for high-value cells.

`docs/specs/16-rustc-tcb.md:109-114` likewise says Spectre/Meltdown mitigations are ARM
MTE pointer tagging and x86 MPK domain separation.

In contrast, `docs/specs/12-reliability.md:33-44` states that Cellos runs in S-mode under
SBI, PMP CSRs are M-mode-only, and PMP can only be a static boot-time guard without custom
firmware. That description matches the implementation.

## 2. MTE is Layer-C hardening, not a side-channel defense

The MTE abstraction explicitly states that MTE is hardening rather than a security
boundary, has a 1/16 tag-collision probability, and admits speculative bypasses
(`hal/traits/mte/src/lib.rs:4-15`). Its useful property is detecting some use-after-free
and out-of-bounds accesses through allocation-tag mismatches.

MTE does not stop Spectre from speculatively reading architecturally inaccessible data or
encoding it through a cache side channel. Calling it a Spectre mitigation in Specs 05 and
16 confuses spatial memory-safety detection with speculative-execution control.

The generic AArch64 implementation is runtime-gated, but D9 established that RK3588's
Cortex-A76/A55 cores implement Armv8.2-A and do not expose FEAT_MTE. The implementation is
therefore useful only on suitable QEMU configurations or future Armv8.5+ hardware. This is
exactly Spec 19's Layer-C definition, not a Tier-1 deployment guarantee.

## 3. x86 PKU is wired but does not enforce a page boundary

The x86 path contains substantial scaffolding:

- `hal/arch/x86/src/x86_64/pku.rs` detects PKU, requires CET-IBT, enables `CR4.PKE`, and
  computes a PKRU mask for a task key;
- `kernel/src/loader.rs:310-325` assigns `task.pku_key` and `task.pku_value` from the
  granted trust tier;
- the ring-3 return paths execute `WRPKRU` when PKU is active;
- threads inherit the parent's key and PKRU value.

None of those steps associates a page with the assigned key. Repository-wide uses of
`pku_key` only assign, inherit, store, or inspect the task field. The x86 paging layer
defines PRESENT, RW, USER, cache, and NX flags but no PKEY mask or shift
(`hal/arch/x86/src/x86_64/paging.rs:43-67`). User mapping flag builders contain no key
(`:83-96`), and PTE construction writes only the physical address, RW, USER, and NX
(`:321-345`). The loader assigns the task key after loading the image and does not revisit
its PTEs.

Consequently every user page retains protection key 0. `pkru_for_key()` permits key 0 for
all task tiers, so changing PKRU cannot deny access to any currently mapped user page. The
Spec 05 claim of per-cell MPK access domains is therefore false in the current system.

The limitation is already admitted in newer project documents:

- `docs/project-roadmap.md:371-377` calls PTE tagging deferred and says enforcement is
  bypassed while keys are all-zero;
- `docs/system-architecture.md:856-858` says the same;
- `docs/security-model.md:156-162` says PKU enforcement is bypassed.

However, the older changelog section is internally contradictory. It says the kernel fills
PTE bits `[62:59]`, that wrong-key access faults, and that Layer 2 is complete
(`docs/project-changelog.md:1009-1043`), while the same section later admits those bits are
zero and enforcement inactive (`:1020`, `:1048-1049`). Commit `a3e558f3`, which introduced
the PKU work, did not modify the x86 paging module at all. D12 should treat PKU as incomplete,
non-enforcing Layer-C scaffolding, not a shipped isolation boundary.

## 4. The PKU self-test cannot prove isolation

`kernel/src/layer2_selftest.rs:150-212` verifies only:

1. two computed PKRU constants; and
2. that kernel-mode `RDPKRU` returns zero.

It does not map pages with different PTE keys, enter ring 3, attempt a denied access, or
verify a `#PF` with the protection-key error bit. It can therefore print `PASS` while every
page remains key 0 and no isolation exists. The changelog statement that the self-test
"attempt[s] forbidden access, verify fault" is not implemented by this test.

This is a test-claim defect, not authorization to implement PTE tagging during D12. A real
PKU completion requires a separate design because three or four trust-tier keys do not by
themselves create "per-Cell" domains, and shared pages/grants need explicit key semantics.

## 5. PMP is a future firmware contract

`hal/arch/riscv/src/common/pmp.rs:1-18` states the architectural constraint directly:
PMP writes from Cellos S-mode trap, and the module only records a desired layout for a
future custom M-mode firmware shim. Its region table is descriptive data; no active runtime
path writes PMP CSRs or switches a per-cell PMP configuration.

Spec 05's phrase "M-mode fence for high-value Cells" omits the absent M-mode owner and reads
as a deployable Tier-1 mechanism. Spec 12 is the accurate source: current PMP can at most be
a static boot-time firmware guard. Dynamic protection for an untrusted native cell belongs
to Layer B unless a future architecture and firmware decision proves otherwise.

## 6. Spec 19 already has the correct ownership split

`docs/specs/19-hardware-isolation-layers.md:22-70` defines:

- **Layer A — implemented:** W^X after relocation protects code and constants, while heap,
  stack, `.data`, and other shared user mappings remain outside a per-cell data wall;
- **Layer B — future and load-bearing where trust is absent:** per-domain page tables for
  Tier-2 untrusted native cells, optionally also used for high-value Tier-1 cells;
- **Layer C — opportunistic:** hardware-gated MTE/MPK bonuses that are never required for
  the security claim.

Its rejected-alternatives section (`:117-123`) also correctly rejects MTE/PKU as the primary
cell-to-cell wall because they are absent from the named deployment hardware. Duplicating a
different mechanism list in Spec 05 guarantees further drift.

## 7. Side-channel consequence

Removing MTE/MPK from Spec 16 leaves a real residual statement: LBI does not solve timing or
speculative side channels. The correction must not replace one unsupported mitigation claim
with another. Spec 16 should state that these channels remain a separate threat-model and
mitigation problem; MTE and PKU are not Spectre/Meltdown mitigations. Any concrete cache,
branch-predictor, fencing, core-partitioning, or compiler mitigation needs independent design
and verification before it is called implemented.

## 8. Recommended ruling

**Approve option A: centralize the model in Spec 19 and correct the two stale consumers.**

1. Replace Spec 05 §2.1's MTE/MPK/PMP table with a short pointer to Spec 19 and a status
   summary: Layer A implemented, Layer B future/load-bearing for untrusted native cells,
   Layer C opportunistic only.
2. Rewrite Spec 16 §3.3 to say MTE/MPK are not Spectre/Meltdown mitigations and that
   speculative side channels remain separately scoped work.
3. Keep Spec 12's PMP architectural statement; optionally add a Spec 19 cross-reference,
   without duplicating the layer definitions.
4. Correct the contradictory PKU/self-test claims in the project changelog and any status
   wording that calls non-enforcing PKU domain isolation complete.
5. Track real PKU enforcement separately: PTE key assignment, shared/grant-page semantics,
   domain cardinality, fault-path validation, and an end-to-end denied-access test.

No runtime code change is part of this ruling. D12 corrects the security contract and status;
implementing Layer B or completing PKU requires its own approved design.

## 9. Applied files

- `docs/specs/05-application.md`
- `docs/specs/10-testing.md`
- `docs/specs/12-reliability.md`
- `docs/specs/15-kernel-boundary.md`
- `docs/specs/16-rustc-tcb.md`
- `docs/specs/19-hardware-isolation-layers.md`
- `docs/project-roadmap.md`
- `docs/project-changelog.md`
- `docs/system-architecture.md`
- `docs/security-model.md`
