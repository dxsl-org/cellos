# Phase 15 evidence: x86-64 Tier-2 parity

**Date:** 2026-11-14 · **Phase:** [phase-15-x86-tier2-parity.md](../phase-15-x86-tier2-parity.md) · **Handoff:** [S7b approval](../s7b-handoff-approval.md) · **Census:** [research/p15-census.md](../research/p15-census.md)

## 1. Success criteria

| Criterion | Result |
|---|---|
| x86 grant suite at 1/2/4 CPUs, TCG and KVM | PASS at `df9ab96fa`: TCG 1/2/4, KVM (`-cpu host`, PCID on) 4; at `93f9cbf0c`, KVM 1/2/4 too. `GRANT-REVOKE`, `GRANT-GATE`, `GRANT-DEFERRED`, six NEG cases and the grant pair. Logs `/tmp/p15/fix2/lanes/`, `/tmp/p15/c5/m/`. |
| Each negative fixture fails when its check is disabled | Six controls, each switching off exactly one check (`task/fixture_controls.rs`, test-hooks only, default off, reachable only from boot fixtures). Each run logs `CONTROL-<case>: CAUGHT (<exact reason>)`; any other error is `FAIL`, which the lane rejects. |
| RV64 `--harts 2` still passes | All 15 cases pass, `grant-pair` included (red on base). |
| No `1 << 38` user-limit literal outside the RV64 HAL | `USER_LIMIT` comes from each HAL: RV64 `1 << 38`; AArch64 `1 << (DEFAULT_VA_BITS - 1)`; x86 `PML4_SLOT_SPAN / 2`. All equal 2^38. |

| Control case | Check disabled | Expected reason |
|---|---|---|
| REVOKE-MAPPED | RevokeDrainsReceiver | `receiver-mapping-survived-revoke` |
| RECEIVER-DEATH | DeathDrainsReceiver | `dead-receiver-mapping-survived` |
| STALE-ID | SliceNeedsLive | `owner-resolved-revoking-record` |
| FORGERY | SliceNeedsGrantee | `third-domain-resolved-grant` |
| COPY-FAULT | CommitNeedsLeaf | `commit-accepted-revoked-page` |
| COPY-DOWNGRADE | CommitNeedsRight | `commit-accepted-downgraded-page` |

## 2. What changed

- **One cfg** `cellos_domain_grants` = 64-bit ∧ `native-domains` ∧ ¬(AArch64 ∧ ¬test-hooks) replaces 30 gates. x86 production kernels now carry the domain-grant lifecycle (owner decision D1). RV64 and AArch64 gating are unchanged.
- **x86 memory access.** Owner backing is zeroed through the HHDM. Grant frees and partial-map rollback create no identity or RWX alias in the x86 kernel root. A domain run with no kernel-root mapping is only deallocated, with no shared-root unpublish (R3).
- **Revoke invalidation.** x86 uses the generic probe-then-defer path. The withheld-ack seam is arch-neutral and logs `withheld (test)`; the lanes excuse only that token inside the fixture window.
- **R4.** Drain runs before the table lock, still under the record's gate (audit H-5). Longest gate hold at 4 CPUs: TCG publish 267 µs / drain 929 µs; KVM publish 122 µs / drain ~4.1 ms. The KVM drain is the same at 1 CPU; it is mostly the `deferred release queued` serial warning printed under the gate, on the deferral path only (inferred, not measured per hold).
- **Lanes.** x86: grant pair in the image; faults are counted per address in order; each fault's P bit is checked; the shell-recovery watcher is fixed; there is a KVM option. RV64: the pair is launched from the boot order; faults are counted per address in every case; evidence greps ignore `withheld (test)`. AArch64: requires `UNREGISTER-RW: OK` and fails on any pair `FAIL`.
- **Spec 22** describes x86 grants. Ratchet 775 → 704.

## 3. Defects found and fixed

| Severity | Defect | Fix |
|---|---|---|
| HIGH (pre-existing, newly reachable on x86 SMP production) | Two revokers (sweep on two harts, `GrantFree`/`GrantUnregister` vs `CapRevoke` reclaim) could both free the same frames, and a late `GrantUnregister` could delete a reused row | One helper, `release_if_row_taken`, at all six sites: only the caller whose removal took the row (same `Arc` record) frees. Two host tests. |
| MED | x86 domain frees went through the shared-root unpublish (`flush_range` panic on a silent peer, unmap of a VA the grant never owned) | Domain runs without a kernel-root mapping are only deallocated |
| MED | Partial-map rollback mapped f→f kernel-RWX on x86 | Same identity-alias guard and prune as the free path |
| MED | The `GrantUnregister` witness and the controls could pass without exercising their check | Read-write unregister phase with a store proof, `REUSED` fails, P-bit checks, exact control reasons |
| LOW ×6 | Lane token, `blk_read_async` domain rows, dead-code gating, fixture asserts on global counts, forgery cleanup, copy-control split | Fixed |

Two reviews (`haily-reviewer`): steps 2–3 found the HIGH and two MED; steps 4–6 found that `GrantUnregister` had been missed, plus two MED. The re-review of the fixes is PASS with no findings.

## 4. Checkpoint (code at `0d49a30e9`, plus the lanes above at `df9ab96fa`; logs `/tmp/p15/cp/`)

- **Pass:** x86 SMP4, hpet-off, ahci, igb, vmxnet3, virtio, pvscsi, serial, init-death; c2c negative control / release posture / lifecycle 4/4 / config-hotswap; native-workload, RV qemu-boot, four AArch64 boots, pcie-bar-window, launch-profile, tier2-fault-isolation ×2, boot allowlist 57/57; std sysroot and std-smoke on three arches; hypervisor smokes; tier3 python / nginx / virtio-e2e / hostile (27 scenarios); phase 09 ssh. At `df9ab96fa` also: x86boot, c2c two-node, AArch64 test-hooks, RV64 `--harts 2`; earlier at `0d49a30e9`: power, nic, iommu-dmar, AArch64 `virt-vhe`.
- **Flaky:** srv-cellosfs failed once (the shell banner split a `[srv-test]` line on the serial port), then passed 6 runs.
- **Pre-existing (same line on base):** x86 hypervisor ISO rollout block, 2 rust-std promotion tests, rv64/aarch64 workspace cell builds (the same 5 crates).
- **Builds and tests:** 13 kernel configurations; host tests 655 (base 649); clippy kernel warnings unchanged.

## 5. Residuals

- RV64 still restores the f→f RWX alias on grant free (Phase 16 frame lifetime).
- The ~4 ms masked serial print under the gate on the deferral path; moving it changes `deferred_release` logging on every arch.
- AArch64 statements in Spec 22 §2.2 may be stale (private-root leaves now carry `PTE_nG`).
- VA = PA receiver identity limits x86 hosts to RAM below `CELL_VA_START` (64 GiB); fail-closed, documented in Spec 22.
- srv-test writes its prefix and message separately, which lets the shell banner split the line.
