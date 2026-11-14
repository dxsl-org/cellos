# Phase 14 evidence: kernel `arch/` seam and boot split

**Date:** 2026-11-14 · **Phase:** [phase-14-arch-seam.md](../phase-14-arch-seam.md) · **Handoff:** [S7 approval](../s7-handoff-approval.md) · **Census:** [research/p14-census.md](../research/p14-census.md)

## 1. Result against the success criteria

| Criterion | Result |
|---|---|
| `target_arch` outside `kernel/src/arch/` at least halved; guard fails on an addition | 1 837 → **775** (−58 %). `scripts/check-hal-boundaries.sh` caps it at 775 and `asm!` at 0, and fails when a line is added (tested at the baseline ceiling). |
| No `asm!` outside `kernel/src/arch/` and `hal/` | 124 → **0** |
| x86 1/2/4-CPU domain lanes, RV64 `--harts 2`, AArch64 `virt-vhe` 2-CPU pass; A/B markers identical before deleting the old switch | All pass at the final code (§3). A/B (`CELLOS_SWITCH_AB=legacy` vs new, built from `5dd1cf4fd7`): identical marker sets on x86 1/2/4 CPU (13/17/20), AArch64 test-hooks (79), AArch64 `virt-vhe` (7), RV64 `--harts 2` (65 per case). Logs: `/tmp/p14/ab/`. |
| Three 64-bit targets build; no `cfg_attr(not(target_arch…), allow(dead_code))` added | Builds pass in default, test-hooks, release-posture and domain-test configurations. The diff adds no such attribute; it removes one (`dev/iommu.rs`). |

## 2. What changed (commits on `p14-arch-seam`)

| Step | Commits | Content |
|---|---|---|
| 1 | `0cee9c2ac`, `817e77c6c` | Ratchet guard; census |
| 2 | `135c7bd08`…`6e410e670`, `386074676` | `arch/` seam, `cellos_wide`, whole-file and per-set moves (asm byte-identical with options), `#[path]` child modules for arch blocks that read private state, mutable-globals `..` normalisation |
| — | `705088af6`, `c6d9d49a3` | Fixture fixes (§4) |
| 3 | `488cd8952`…`3b5cfb657` | `kmain` → `arch::boot::early` → heap → `platform_init` → neutral tail; `PlatformReady`; one `board::active()`. Boot-log order identical to base on x86, x86 domain, RV64, AArch64. |
| 4 | `9a85d5ab0`…`f49de9843` | `hal::arch::{Root, IrqSaved, SwitchPlan, switch}`; `RootDecision` (side effects only inside the switch); test-hooks guardrails; A/B; old path deleted |
| 5 | `a664be39c`…`6429e632c` | Page-table trait carries the x86 kernel root (`UNCACHED`, `GLOBAL`, `TablePolicy`, `protect`, `Leaf`); HAL `leaf`/`current_root`/`phys_to_virt`; `sv39_probe` → `page_probe`; raw x86 walkers deleted. Host harness: 165 steps, 1 336 320 table slots, all identical to the old raw path. |
| 6 | `7ed702612`…`28d92febd` | Dead HAL traits removed; `aarch64_catalog` removed; hal-core default x86_64; Spec 04 §1/§7, system-architecture board sentence |
| Review | `187bf8b6c`, `4dee9131d` | Five LOW review findings plus two analogous spots (§5) |

## 3. Checkpoint run (code at `4dee9131d`; logs `/tmp/p14/cp/`)

Every lane that passed at the Phase 13 checkpoint passes; no regression. Each branch failure was rerun and compared with the pre-phase base (`0cee9c2ac`).

- **Pass:** x86 boot, SMP4, hpet-off, ahci 15/15, igb, nic, vmxnet3, virtio, pvscsi, iommu-dmar, serial, power, init-death; x86-c2c two-node / negative control / release posture / lifecycle 4/4 / config-hotswap; x86 domain 1/2/4 CPU; srv-cellosfs, native-workload, RV qemu-boot; AArch64 virt / Image / vhe / novhe, AArch64 test-hooks (vfs-test 96/0); pcie-bar-window, launch-profile, tier2-fault-isolation ×2, boot allowlist 57/57; std sysroot and std-smoke on three arches; AArch64 and x86 hypervisor smokes; tier3 python / nginx / virtio-e2e / hostile; phase 09 ssh (34 checks); RV64 `--harts 2` 14/14.
- **Pre-existing (same line on base):** RV64 `grant-pair` (`S22-RV64-GRANT-PAIR-OWNER-MAPPED` missing: the guest shell refuses `tier2-smoke &`), x86 hypervisor ISO rollout block, 2 rust-std promotion unit tests, rv64/aarch64 workspace cell builds (the same 5 crates).
- **Host tests:** kernel 649 (base 648), hal-arm 23, hal-x86 41, hal-soc-* and boards all pass. Clippy: kernel/hal warnings unchanged (x86 2, rv 0).

## 4. Pre-existing defects found and fixed (test fixtures only)

| Lane at base | Cause | Fix |
|---|---|---|
| RV64 `--harts 2`: "kernel stack overflow" panic in 7/21 boots (since Phase 10) | The IPC copy fixture woke a synthetic receiver (`Task::new`, zeroed `Context`, no kernel stack) onto hart 0's queue; an idle hart stole it and switched onto `sp = 0` (gdbstub capture, `/tmp/p14/rv64-trap-overflow.md`) | Reserve the receiver against stealing across the wake (`705088af6`); the step 4 guardrail now refuses such a switch in test-hooks kernels |
| AArch64 test-hooks: `S22-AARCH64-DOMAIN-LIVE: FAIL asid=0` | PLAN/RESUME fixtures recorded a domain without programming its root; the live-root observer then read it over the kernel root (`/tmp/p14/a64-domain-live.md`) | Hand-back before the raw switch (`c6d9d49a3`); superseded in step 4 by side-effect-free `RootDecision::preview` |

Production paths were not changed by either fix.

## 5. Independent review

`haily-reviewer` on `817e77c6c..6429e632c`: PASS_WITH_RISK, no CRIT/HIGH/MED. Five LOW findings fixed in `187bf8b6c`: `IommuReady` built only from an activated isolation token; aarch64 `IrqSaved` safety comment; read-only page-table walks without aliasing `&mut`; volatile copy of Limine's higher-half entries; `compiler_fence` before publishing a new table. The same two patterns elsewhere (rv64 token comment, `map_hhdm_firmware_range`) fixed in `4dee9131d`. The review confirmed per-arch IRQ save/restore order, root encoding, PTE bit identity, side-effect order in the kmain split, and asm byte identity.

## 6. Residuals

- 32-bit targets (riscv32 17 errors, arm 13, x86-32 a `libs/abi` size assert) still do not build; quarantined in `arch/legacy`, as before.
- The no-root completion assertion covers `RootDecision::switch` only; fixtures and the x86 SMP handoff still call the raw `Context::switch_with_root`.
- x86 `copy.rs` `current_root() == Some(root_pa)` can never be true (the switch never writes CR3); recorded, unchanged.
- Out-of-set docs still describe the old HAL traits: `docs/specs/00-context.md`, `05-application.md`, `docs/code-standards.md` (Phase 19).
- Static gates red before and after this phase: stale code metrics, two tier3 scripts missing from `lanes.toml`, four unclassified `init_death` statics in mutable-globals.
