# Phase 16 evidence: frame ownership and shootdown contract

**Date:** 2026-11-15 · **Phase:** [phase-16-frame-ownership.md](../phase-16-frame-ownership.md) · **Handoff:** [S7c approval](../s7c-handoff-approval.md) · **Census:** [research/p16-census.md](../research/p16-census.md)

## 1. Success criteria

| Criterion | Result |
|---|---|
| x86 silent-CPU fixture: teardown completes, frames quarantined then freed, no panic | `S22-X86-QUARANTINE-SILENT-CPU: PASS` on TCG and KVM (`c305f38637`; checkpoint §4). Before the phase this path panicked in `tlb_shootdown.rs`. |
| Host tests cover every `FrameRun` transition; flush under the allocator does not compile | `memory::frame_run` host tests; `scripts/check-frame-guard-compile-fail.sh` (E0499 for a flush under `AllocGuard`, E0277 for a cap captured by `frame::with`), wired into `ci.yml` job `board-configs`. |
| Raw `PhysAddr` frees outside `memory/frame*.rs`: zero | `rg deallocate_frame` outside `memory/frame*.rs` finds only test-hooks fixtures that free a frame they allocated and invalidated themselves (listed in the phase file, row 11). |
| Checkpoint: pr-tier on three arches, x86 4-CPU stress and domain grants, hypervisor smokes, review recorded | §4 and §5. |

## 2. What changed

- **`FrameRun`** (`memory/frame_run.rs`): states `Mapped`, `Unpublished`, `Quarantined(reason)`, `Free`. One pending list keyed per wait (tag, shared, RFENCE fallback, event key), merged by per-hart maximum epoch, reaped by the hart-0 timer. Nothing is abandoned: a tag that never confirms stays quarantined and is logged once.
- **Allocator guard:** `FRAME_ALLOCATOR` is private; `frame::lock(&mut FlushCap)` and `frame::with(Send closure)` make a flush under the lock a compile error.
- **Shared flushes** return `FlushOutcome`; x86 unconfirmed → quarantine, RV64 RFENCE error → IPI epoch fallback. Permission lowering (W^X, MMIO revoke/retirement) fails closed.
- **Quarantine cap:** runs waiting on an invalidation (`TagUnconfirmed`, `SharedUnconfirmed`, `RfenceFailed`) above 1/64 of RAM refuse new allocations through each path's existing out-of-memory result; page-table frames are exempt.
- **Owners migrated** in risk order: stacks, cell segments (`memory/cell_segments.rs`), grants/SHM, DMA exports, domain grants, hypervisor tables (EPT/NPT/Stage-2, guest RAM, VMCB/VMCS as `PrivateRun`), private-tag queue.
- **`Region`** (`memory/region.rs`) behind `PAGE_GRANT_TABLE`, `REG_GRANT_TABLE` and `EXPORTS`; SHM rows moved into the grant table (`SHM_HANDLES` deleted). Syscall numbers, grant ids and wire bytes unchanged; no Law-1 checkpoint.
- **Specs:** Spec 02 §3 (§3.1 release order, §3.2 cap) and Spec 19 Layer B `Accepted; implemented`.

## 3. Defects found and fixed

| Severity | Defect | Fix |
|---|---|---|
| HIGH (pre-existing) | `ShmMap` checked the row, dropped the lock, then mapped: a retirement in between let a receiver map a freed frame USER RW | `a3de92d6f1`: publish under `PAGE_GRANT_TABLE` |
| HIGH (pre-existing) | AArch64 EL2 hosts never programmed `VTTBR_EL2` per guest entry; a vCPU could run against another (or a freed) VM's Stage-2 root | `3c360497f2`; fixture `S22-AARCH64-TWO-VM-STAGE2` |
| HIGH (pre-existing) | AArch64 VMIDs wrapped at 16 bits under an 8-bit `VTCR_EL2`: VM 256 shared the host's VMID 0 | `5ab026dbb2`: `VmidPool` 1..=255, refusal at exhaustion |
| MED (pre-existing) | AArch64 VM teardown flushed only the VMID loaded on the CPU | `TLBI ALLE1IS` after `disable_stage2` |
| MED (pre-existing) | Domain-grant rollback and failed `live_task_binding` freed frames still mapped or behind an unconfirmed probe | `1a624ec5dc`, row 9b |
| MED (pre-existing) | `ShmAlloc` handed out unzeroed frames | `7d552f0c57` |
| MED (pre-existing) | x86 stack teardown restored a lower-half kernel-RWX alias | row 5 (owner F3) |
| MED (pre-existing) | x86 retirement left released MMIO windows USER-reachable | `b2fc97dcc2` (D-mmio) |
| MED (pre-existing) | DMA export freed under `EXPORTS`, ranking inversion with the grant tables | `ace1e4f61d` |
| MED | MMIO retry on every pass, page-at-a-time lowering, lowering under `RESOURCE_REGISTRY` | `a9239c6128` |
| MED | Domain sweep re-unmapped every pass while a tag stayed silent | `f5d522becb` |
| LOW (pre-existing) | `create_vcpu` leaked frames on error; VMCLEAR failure freed a possibly active VMCS | `bfd7fec5b7`, `1acd713ec5` |
| Regression (row 12) | RV64 copy-race revoker overflowed its one-page kernel stack | `8701bcec48` |
| Fixture races | RV64 SMP-RETIREMENT and TLB negative control flaked at the new timing | `1bb58398b7`, `b6d72a5bc7` (300/300 boots) |

## 4. Checkpoint

Code at `c49a67eae5` plus the fixture fix `2de3642ba3`. Runner `~/worktrees/p16-cp/pr.sh` (recovered Phase 14/15 runner plus `x86domp16`), sequential, logs `~/worktrees/p16-cp/{br,br2,br3,br4,base,trace}/`. The 2026-11-15 16:06 WSL restart wiped `/tmp`, so the earlier Phase 16 lane logs (`/tmp/p16/...`) are gone; every lane below was rerun at HEAD.

| Group | Result |
|---|---|
| Build-only kernels | 13 configurations PASS: default, test-hooks, release-posture and domain-test on RV64, AArch64 and x86_64, plus the x86 production-relay check |
| Host tests and clippy | Kernel 691 (Phase 15 checkpoint: 655), all 13 HAL/board crates; clippy x86 and RV workspace PASS |
| x86 boot and devices | x86-boot, `-smp 4` (`x86_smp_brings_every_cpu_online`), hpet-off, ahci, igb, nic, vmxnet3, virtio, pvscsi, iommu-dmar, serial, power, init-death PASS |
| x86 C2C gate | two-node, negative control, release posture, config hot-swap PASS; local-service lifecycle: see below |
| x86 domain grants | TCG 1/2/4 and KVM 4 (`-cpu host`) PASS. `S22-X86-QUARANTINE-SILENT-CPU: PASS` on every run with 2+ CPUs (it skips at 1 CPU). 0 `frames leaked, never reused`, 0 panics. Every `invalidation unconfirmed` line is fixture-induced: `… quarantined: held (fixture)` from the silent-CPU fixture, and `withheld (test)` from the withheld-ack window |
| RV64 | qemu-boot (real worktree), launch-profile, tier2 fault isolation ×2, boot allowlist 57/57, pcie-bar-window, srv-cellosfs, native-workload, `--harts 2` all 15 cases PASS |
| AArch64 | four boots (`-kernel` ELF and Image, virt-vhe, virt-el2-novhe), test-hooks (default mode), hypervisor machinery smoke PASS; virt-vhe requires `S22-AARCH64-TWO-VM-STAGE2: PASS` (`scripts/qemu-aarch64-test.sh:148`) |
| rust-std | sysroots and std-smoke on three arches PASS |
| Hypervisor and Tier 3 | x86 hypervisor smoke, Tier-3 python, nginx, virtio-e2e, hostile PASS |
| phase09 ssh | 34 checks PASS (first run failed because Docker Desktop was not running after the restart) |
| Pre-existing, same on base | workspace cell builds (`posix-shim-test` on RV, `ocel-pdf`/`ocel-quickjs` on AArch64), 2 `rust-std-promotion` tests, the `x86hv-iso` rollout guard, the EL2 test-hooks lane (recorded broken) |

**Lifecycle difference, fixed (owner decision 2026-11-15):** `x86_multi_source_wait_holds_on_two_cpus` (`-smp 2`) failed 2 times in 5 runs on the branch and passed 6/6 on base. An `ipc-trace` build showed `try_send refused … reason=not-recv state=running` right after `slow-provider-ready`: the provider, on the other CPU, had not reached `Recv` after the caller's fixed 60 yields. The kernel's refusal is correct, and `ipc_try_send` is unchanged in this phase. Phase 16 widened the window, most likely through the batched shared flushes on the x86 spawn path (not measured). `2de3642ba3` retries the parked send within 500 scheduler ticks; a refusal queues nothing, and the busy and queued checks are unchanged. After the fix: 10/10 PASS (the last 9 at least are certainly the new build).

## 5. Review

All reviews were independent `haily-reviewer` runs, read-only.

| Range | Verdict | Fixed in |
|---|---|---|
| `3329efc473..0ab6916392` | PASS_WITH_RISK, 3 MED | `9be5ba7f69` |
| `0ab6916392..4bfd3cecda` | PASS_WITH_RISK; cap scope decided by owner | `650c73431b`, `b2fc97dcc2`, `1acd713ec5` |
| `4bfd3cecda..bfd7fec5b7` | HIGH VTTBR, MED sweep | `3c360497f2`, `f5d522becb` |
| `bfd7fec5b7..f5d522becb` | 3 MED on MMIO | `a9239c6128` |
| `f5d522becb..15a6ae3145` | HIGH ShmMap (pre-existing) + LOWs | `a3de92d6f1`, `efe757e2fa` |
| `15a6ae3145..c49a67eae5` (re-review) | **PASS** | — |

## 6. Residuals

- **Re-review LOWs, not fixed:** a refused `RegionTable::insert` in `grant_alloc`/`grant_register` leaves the domain owner's private-root PTEs; `paging.rs` resets `await_flush` per run, not per release; the RV64 shootdown self-test does not block RFENCE with 3+ harts, and `CELL_ID_RELEASE_OBSERVED` is never reset.
- **Phase 17:** one owner can hold all 255 AArch64 VMIDs (per-owner quota).
- **Pre-existing:** AArch64 `map_guest_memory` can overwrite live Stage-2 leaves without a TLBI (reaches only the VM's own RAM); `request_mmio` takes `SCHEDULER` under `RESOURCE_REGISTRY`.
- **Phase 19:** the EL2 test-hooks lane (`DEVELOPMENT_SILO=1`) is recorded broken in `scripts/lanes.toml`; it fails the same way on `main`.
- Evidence ceiling: host and QEMU (TCG, KVM); no physical hardware.
