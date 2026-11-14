# S7b handoff request: x86-64 Tier-2 parity (Phase 15)

**Date:** 2026-11-14 · **Plan:** [plan.md](plan.md) · **Prior:** Phase 14 closed 2026-11-14 ([evidence](reports/phase-14-evidence.md)), `main` at `98b1b33e4`.

## 1. Scope (measured on `98b1b33e4`)

| Surface | Today |
|---|---|
| `domain_grant` and its publish/revoke gate | Compiled only for `riscv64`, plus `aarch64` with test-hooks (`kernel/src/task.rs:14-34`). On x86-64, Tier-2 cells have copied IPC only. |
| Grant syscall arms | `task/syscall/grant.rs` has 13 `target_arch` arms (x86 at 1084 and 1685; aarch64 at 1732, 2212 and 2229). |
| Fixture gates | `domain_grant.rs:43-45` pulls in RV64/AArch64 test-hooks fixtures only. `grant_gate_selftest` and `grant_reclaim_selftest` exist. |
| User limit | `1usize << 38` hard-coded in `memory/address_space.rs:10` and `task/user_copy/range.rs:11`. |
| Witness cells | `cells/tests/tier2-grant-owner` and `tier2-grant-receiver` exist; an x86 build is not checked yet. |
| RV64 `grant-pair` lane | Fails, on base too. The guest shell refuses `tier2-smoke &` ("SDK background launch requires explicit nonterminal streams"). |
| KVM | `/dev/kvm` exists on this host, so the evidence ceiling is TCG plus local KVM. |

## 2. Staging (proposed, same shape as S7)

1. **Inventory.** Read only; produces `research/p15-census.md`.
   - Classify every gate on the grant path as neutral, needs-HAL or genuinely RV64.
   - Check the plan's low-confidence claims:
     - the grant code is arch-neutral above the page-table trait;
     - x86 private-root invalidation supports probe-then-defer;
     - the witness cells build for x86.
   - Confirm the grant syscalls still exist (Phase 11 retired some dead grant paths). If x86 needs a syscall that is now retired, I stop and ask: that would be a Law-1 change.
2. **User limit from the HAL; remove neutral gates.** x86 builds.
3. **x86 revoke invalidation.** Private-root shootdown with probe-then-defer, carrying the audit H-5 per-record gate. Timeout defers; it never frees.
4. **Fixtures.** Port the negative fixtures: revoke while mapped, receiver death, stale grant id, cross-domain forgery, fault during copy. Each must fail when its check is disabled by the test hook (a fault-injection control).
5. **Lane.** The x86 domain lane runs the grant suite at 1, 2 and 4 CPUs under TCG and KVM. Update `lanes.toml`.
6. **Spec 22** gets the x86 status.
7. **Checkpoint.**
   - The x86 grant suite at 1/2/4 CPUs on TCG and KVM.
   - RV64 `--harts 2` and AArch64 test-hooks lanes stay green.
   - The pr-tier lanes Phase 14 ran, plus `x86-c2c`.
   - An independent review focused on isolation.

One commit per step, in a worktree, rebased before the merge. Nothing is pushed.

## 3. File set

- **Modify:**
  - `kernel/src/task/{domain_grant.rs,domain-grant-gate.rs,grant_gate_selftest.rs,grant_reclaim_selftest.rs,syscall/grant.rs}`;
  - module gates in `kernel/src/task.rs`;
  - `kernel/src/memory/{address_space.rs,tlb_shootdown.rs}`;
  - `kernel/src/task/user_copy/{range,guard}.rs`;
  - `kernel/src/arch/x86_64/**` and `hal/arch/x86/**`, only to supply the user limit and invalidation hooks;
  - `cells/tests/tier2-grant-{owner,receiver}` (x86 build only);
  - `scripts/build-x86_64-domain-test-ci.sh`, `scripts/x86/qemu-domain-test.sh`, `scripts/lanes.toml`, `scripts/check-hal-boundaries.sh` (ratchet);
  - `docs/specs/22-native-domain-cell-implementation-gate.md`.
- **Create:**
  - the x86 grant fixtures under the existing test-hooks modules;
  - `research/p15-census.md`;
  - `reports/phase-15-evidence.md`.
- **Optional:** `scripts/qemu-native-domain-test.sh`, so the RV64 `grant-pair` case launches its cell the way the shell now requires. That would turn a red lane meaningful; it is a test-script change only.
- **All steps:**
  - this plan directory;
  - `.agents/plan-portfolio.md`;
  - `docs/project-changelog.md`.

`libs/abi` and `libs/types` do not change, so there is no Law-1 checkpoint. No new syscall number is added and admission is not relaxed.

## 4. Behaviour to expect

x86-64 Tier-2 cells can use zero-copy domain grants (publish, map, revoke) with the same record gate and deferred-release contract as RV64. Copied IPC is unchanged. RV64 and AArch64 behaviour does not change.

## 5. Conflicts

- **The other session in `/home/dmin/cellos`** last committed block-driver and Chrome-guest work, with no kernel grant files. Each commit is rebased just before merge.
- **Intel WIP plans.** `261104-0801` large-memory (`AnonMemory`) and `261104-0802` R5 XSAVE wait for the Intel WIP lane, and both would touch `syscall/grant.rs` and `address_space.rs`. This slice takes the lane for these files while it runs. If the owner opens one of those first, S7b pauses at a commit boundary.

## 6. Questions to the owner

1. Grant S7b staged as in §2: inventory now, steps 2–7 continuing, stopping only on a behaviour difference outside x86 grants or a needed ABI change.
2. Fix the RV64 `grant-pair` lane script as part of this slice (proposed)?
3. Evidence ceiling: TCG plus local KVM (proposed). Physical Intel is out of scope, per the plan.
