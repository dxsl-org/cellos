# Lock handoff — DRAFT, not approved — aarch64 core/backend split, phases 01–04

**Status:** draft for the accountable owner (the user), 2026-11-02. **Nothing in this file is effective.** It becomes effective only after the user approves this handoff, names the file set below (with answers to the open questions), and an approval record is appended. Until then, no path below may be edited for this plan.
**Plan:** [plan.md](plan.md) · phases [01](phase-01-backend-trait-console.md) · [02](phase-02-irq-timer-trap-smp.md) · [03](phase-03-platform-map-resources.md) · [04](phase-04-boot-el2-vcpu.md) · ADR-0029 §6 · ADR-0022 amendment 2026-10-31
**Precedent:** [x86 phases 01–02 handoff](../261031-1241-x86-runtime-board-selection/lock-handoff-approval.md) · [x86 phase 03 handoff](../261031-1241-x86-runtime-board-selection/phase-03-lock-handoff-draft.md) · lock inventory [`.agents/cell-contract-0024-locks.md:131-134`](../cell-contract-0024-locks.md)
**Baseline:** [reports/baseline-2026-11-02.md](reports/baseline-2026-11-02.md) (HEAD `6d5cdeb0c`)

All line numbers below are at `6d5cdeb0c`. The scout report's ranges have drifted. For example, `board.rs:108-203` is now `:431-523` and `vcpu.rs:120-164` is now `:121-163`, plus later sites. Re-derive the line numbers at implementation time. The paths do not change.

## Activation condition (sequencing)

1. **x86 is closed.** Portfolio row 1 is "done 2026-11-02" (`.agents/plan-portfolio.md:171`), and x86 phase 03 CP2 landed in `26d9250ab`. The single-WIP rule (`plan-portfolio.md:178`) is met. Whether to admit aarch64 now is still the user's decision.
2. The user admits plan order 2 in the portfolio and approves this handoff. An approval record is appended below, and an entry is added to the lock inventory (proposed text at the end).
3. The pre-plan baseline ([reports/baseline-2026-11-02.md](reports/baseline-2026-11-02.md)) is accepted as the gate reference **before the first source edit**. Phase 01 step 1 names `reports/baseline.md`, so either rename the file or amend the phase text (Q8).
4. **Phase 04 must not run concurrently with the parked RPi3 EL1-drop plan** (`2026-08-16-rpi3-el1-drop-fix`). That plan names `aarch64/{boot,el2,paging,stage2_regs,trap,timer,vcpu}`. The same applies to the parked `2026-08-16-rpi3-tge-at-probe` (`el2`, `trap`, `paging`, `uart_bcm_mini`). Both are PARKED with no lock (`cell-contract-0024-locks.md:225,227`). They must stay parked while this plan holds the files.
5. The plan yields file ownership to any active Intel C2C slice (`plan.md:5`). Whoever starts `/skill:hc-cook` for a phase must check the portfolio for an admitted Intel slice that names any path below, and stop if there is one.
6. Phases 02 and 03 are documented as parallel with disjoint files. **They are not disjoint** (see "Cross-phase couplings" C1). Until Q2 is answered, run them sequentially: 02, then 03.

## File-bounded exception (proposed)

"Sites" are `feature = "board-rpi*"` cfg occurrences at HEAD (`rg -c 'feature *= *"board-rpi'`). Across `hal/arch/arm/src` and `kernel/src` there are 234. The scout counted about 190.

### Phase 01 — backend trait and console/diagnostics
- New `hal/arch/arm/src/aarch64/backend/mod.rs`, `backend/qemu_virt.rs`, `backend/bcm2837.rs`, `backend/bcm2711.rs`. These hold the `Board` alias, `ArmPlatform`, and a design-hook IOMMU trait with a `None` impl.
- `hal/arch/arm/src/aarch64.rs`: the `pub mod backend;` line only. The rest of this file is Phase 02.
- `hal/arch/arm/src/aarch64/cfi.rs` (sites `:80,82`), `aarch64/mte.rs` (`:24,26`), `aarch64/uart_pl011.rs` (`:8,10`).
- `hal/arch/arm/src/aarch64/uart_bcm_mini.rs`: only if Q3 moves it under `backend/bcm2837`. Its module gate is `aarch64.rs:51`.
- `kernel/src/main.rs`, console selection only: `:167-170` (early UART init), `:263-270` (early print), `:1572,1578` (`probe_put` V/F), `:1836-1843` (panic/print path). The `:13-15` board-conflict `compile_error!` stays: it is feature wiring, not board behaviour.
- `kernel/src/task/drivers/console_drv.rs` (`:115,124,134`).
- **`kernel/src/task/drivers/uart.rs` (`:91,95,108,428,436`; calls `uart_bcm_mini::try_putchar/poll_rx` at `:118,430`). No phase lists this file, but Phase 04's end-state `rg` requires it to be clean. Proposed for Phase 01, the console seam (Q1).**
- `.agents/261031-1241-aarch64-core-backend-split/reports/**` and the phase file.

### Phase 02 — IRQ, timer, trap, SMP (checkpoint)
- `hal/arch/arm/src/aarch64.rs` (sites `:9,11,13,33,51,105,110,168,174`; module gates and IRQ/timer dispatch).
- `hal/arch/arm/src/aarch64/gic.rs` (`:6,8,10,12,93`), `aarch64/timer.rs` (18 sites, `:12-310`), `aarch64/trap.rs` (12 sites, `:8-457`, including the QEMU-only PL061/VirtIO dispatch).
- Moved into `backend/bcm2837/` (git mv, no content change other than paths): `aarch64/bcm2835_legacy_irq.rs`, `aarch64/bcm2835_systimer.rs`, `aarch64/bcm2836_irq.rs`.
- `kernel/src/task/smp.rs` (`:47,60,62,75,455,476,701,1123`). `:62` calls `gic::send_sgi`.
- `backend/*` (owned since Phase 01).
- **Conditional (Q4):** `hal/arch/arm/Cargo.toml` (dependency lines only) and `Cargo.lock` (the `hal-arm` dependency edges only), if the backend implements the existing `hal-interrupt`/`hal-timer` traits. `hal/arch/riscv` and `hal/core` already depend on them. `hal-arm` does not.
- Read-only consumers, edited only if a signature they use changes: `kernel/src/memory/tlb_shootdown.rs:269,313` (`aarch64::timer::counter_now`, `counter_frequency_hz`).

### Phase 03 — platform map, resources, driver selectors
- `kernel/src/platform.rs` (`:160,189-190,211-212,477-478`). The three `init` variants start at `:157`.
- `kernel/src/memory/paging.rs` (`:267,309-310,371`).
- `kernel/src/resource_registry.rs` (`ALLOWED` tables `:77-140`; sites `:362,372,479,495`).
- `kernel/src/resource_registry_tests.rs` or `kernel/src/resource_registry_selftest.rs`: the authority-table equality test. Which one depends on Q5.
- `kernel/src/task/drivers.rs` (`:17,74`), `task/drivers/gpio_irq.rs` (`:19-32,70`), `task/drivers/mmc.rs` (`:5,26,39-40,155,162-163`), `task/drivers/virtio_common.rs` (`:28,35-36,133-134,143`).
- `kernel/src/task/syscall.rs`, framebuffer/USB only:
  - `WaitIrq` USB unmask `:8307-8311`, which calls `hal::bcm2835_legacy_irq::enable_irq`. See C1.
  - `RegisterDisplayFramebuffer` `:8877-8995`. This cfg is **shared with x86_64** (`any(all(aarch64, board-rpi3), target_arch = "x86_64")`), so the edit also changes the x86 build. See "Overlap with x86 files".
- `backend/*`.
- **Drop from the set (Q6):** `kernel/src/task/drivers/pcie_ecam.rs`. It has zero `board-rpi*` sites and no `aarch64` cfg at HEAD. It is x86/RV64 only, and x86 phase 03 just edited it.

### Phase 04 — boot/DTB, board selection, EL2/vCPU monitor (checkpoint)
- `kernel/src/board.rs` (aarch64 descriptor selection `:431-523`, 20 sites). The x86 binding code in the same file is not touched.
- `kernel/src/boot.rs` (37 sites: `:8-21`, `:277-468`, `:542-543`, `:666`, `:751-761`; DTB policy and `fallback_boot_info` variants). This is the parent module of `boot/limine.rs` and `boot/dtb_memory.rs`, and neither child is touched.
- `kernel/src/cpu_features.rs` (`:144-148`; `monitor::is_verified`).
- `kernel/src/main.rs`, remaining sites: `:750-771` (pi-monitor log and smoke), `:781-789` (DTB RAM range), `:792-797` (rpi3 UART input IRQ), `:1153-1197` (native-domains supervisor ranges), `:1691-1743` (rpi3 IRQ probes and idle loop with static `IDLE_COUNT`).
- `kernel/src/hypervisor/registry.rs` (`:35,98,150,163,304-312,357-365,420,558,766`).
- **`kernel/src/task/syscall.rs` `VcpuRegs` `max_mode` (`:9397-9400`). No phase lists this site, but the end-state `rg` requires it to be clean (Q1).**
- `hal/arch/arm/src/aarch64/boot.rs` (`:9` `BOARD_BCM`, `:308` `secondary_psci` asm const), `aarch64/el2.rs` (`:90`), `aarch64/paging.rs` (`:358`), `aarch64/stage2_regs.rs` (`:162-224`, 8 sites), `aarch64/vcpu.rs` (`:121-163,228-232,280,366,424,460,795`; becomes `Board::VcpuExt`).
- `hal/arch/arm/src/aarch64/monitor.rs`: gate at `aarch64.rs:33`. Phase 04 owns this file only if Q3 moves it into `backend/`. Otherwise it keeps its module gate as feature wiring.
- **Conditional (Q7):** `scripts/mutable-globals.toml` and the regenerated `docs/generated/mutable-globals.md`, if `kernel::kmain()::IDLE_COUNT` (`mutable-globals.toml:150`) or any other inventoried kernel static (`platform::PLATFORM`, `cpu_features::HAS_EL2`, `resource_registry::*`, `task::smp::*`, `memory::paging::KERNEL_ROOT`) changes its key path. The `hal-arm` crate is inventoried `active = false` (`:115-118`) and has no keys.
- Docs: `docs/system-architecture.md` (aarch64 section) and `CHANGELOG.md`.
- `backend/*`.

### Explicitly not handed off
- `cells/**`. 44 `board-rpi*` sites live there: the hypervisor cell `run_loop.rs` (21), `vmm.rs`, `gicd.rs`, `boot_arm*.rs`, `dtb.rs`, `timer.rs`, `main.rs`; `services/input/src/virtio_device.rs`; `tools/init/src/boot.rs`. The plan's goal covers only `hal/arch/arm` and arch-generic `kernel/src`.
- `hal/soc/**`, which is read-only data. `ArmVirtProfile` and `Bcm27xxSocProfile` are consumed, not edited. Any profile edit needs an amendment.
- `hal/core/**`, `hal/traits/**`, `kernel/Cargo.toml`, `hal/arch/arm/Cargo.toml` feature lines, `kernel/build.rs`, `kernel/linker-*.ld`. These are feature wiring, and the `rg` end state allows them.
- `libs/api/**`, `libs/types/**`, `libs/ostd/**`. `Cargo.lock` except under Q4.
- `.github/workflows/ci.yml` and `scripts/check-board-configs.sh` (Q9).
- `kernel/src/task/drivers/pcie_ecam.rs`, `kernel/src/boot/limine.rs`, `kernel/src/memory/tlb_shootdown.rs` (read-only).

## Lock relations (CELL-CONTRACT-0024)

The lock inventory blocks **plans by scope**, not files. A path is "under a lock relation" when a locked plan's directory names it, so that plan's blocked scope covers the path. This exception executes none of those scopes. It moves code within each build, verbatim, and all 38 relations keep their whole-plan scope.

Method: path-qualified `grep -rlF` of each file across the 38 locked plan directories (list in `cell-contract-0024-locks.md:6-44`; scopes from each `plan.md` `lockScope:`). "Substantive" means this plan touches data or code that the row's blocked scope governs. "Mention" means the path appears in that plan, but this plan's change does not reach the blocked scope.

| Path (phase) | Locked rows that name it | Relation |
|---|---|---|
| `kernel/src/resource_registry.rs` (03) | `260624-0630-kernel-boundary-cleanup` (MMIO/DMA/IRQ/registry authority migration); `260819-1416-port-common-drivers-g1-g2-g3` (shared device/MMIO/DMA authority); `260712-1900-manifest-v2`, `260822-phase08-manifest-predesign`, `260722-0917-g4-full-std-tier1` | **Substantive**: the `ALLOWED` MMIO authority tables move. Byte-identical move with an equality test. No class bit, base, size or owner semantics change. Mention for the manifest/std rows. |
| `kernel/src/task/syscall.rs` (03 fb/USB; 04 `VcpuRegs`) | Names 20+ rows, including `260607-1200-phase-27-protocol-hardening` (syscall authority) and `260624-0630-kernel-boundary-cleanup` | **Substantive-adjacent**: the `WaitIrq`, `RegisterDisplayFramebuffer` and `VcpuRegs` checks keep the same permission gates and bounds. No syscall number, argument or error changes. |
| `kernel/src/hypervisor/registry.rs` (04) | `260712-0952-tier3b-vm-hardening-compat` (guest device/lifecycle authority); `260722-2330-tier3b-finish-g5-lite` (VMID, shared device authority); `260711-1917-tier3b-x86-vtx`, `260821-0642-app-tiers-completion`, `260825-1726-kms-silo-production-root` | **Substantive** for the two tier3b rows: guest-isolation code moves without logic change. Mention for the rest (Intel VMX, admission, KMS). |
| `hal/arch/arm/src/aarch64/vcpu.rs` (04) | `260711-1917-tier3b-x86-vtx` (`aarch64/vcpu`); `260827-1004-hardware-independent-roadmap` (`vcpu.rs`) | **Substantive** via the tier3b guest-state ABI: the `AArch64Vcpu` layout and the `VcpuRegs` user-buffer word order must stay identical per build. |
| `aarch64/el2.rs`, `aarch64/stage2_regs.rs` (04) | No locked plan names them. Parked `2026-08-16-rpi3-el1-drop-fix` and `-tge-at-probe` (no lock). | None (sequencing only; see activation 4). |
| `kernel/src/task/smp.rs` (02) | `260808-1544-wx-cross-hart-tlb-shootdown` (cross-hart permission retirement); `260927-0739-kernel-architecture-repair`; `260727-2101-midori-lessons-cellos` | **Substantive-adjacent**: the SGI/IPI send path used by cross-hart retirement moves behind `ArmIrqController::send_sgi`. Ordering and barriers are unchanged, and the `QEMU_SMP=2` gate covers it. |
| `aarch64/trap.rs`, `aarch64/gic.rs` (02) | `260808-1544-wx-cross-hart-tlb-shootdown`, `260528-2016-vicell-full-implementation`, `260727-2101-midori-lessons-cellos` (trap) | Substantive-adjacent (IPI/IRQ dispatch) for wx; mention for the rest. |
| `aarch64/timer.rs` (02) | `260528-2016-vicell-full-implementation` | Mention. |
| `kernel/src/memory/paging.rs` (03), `aarch64/paging.rs` (04) | `260808-1544-wx-cross-hart-tlb-shootdown`, `260927-0739-kernel-architecture-repair`, `260727-2101-midori-lessons-cellos`, `260528-2016-vicell-full-implementation`; `memory/paging` also names `260605-2107`, `260819-1416`, `260821-0642`, `260827-1015`, `260905-1139` | **Substantive-adjacent**: the kernel MMIO page maps move. The same pages and attributes are mapped, verified by the `.text`/boot gate. No permission retirement or user-mapping change. |
| `kernel/src/task/drivers/{gpio_irq,mmc,virtio_common,console_drv}.rs`, `task/drivers.rs` (01/03) | `260624-0630-kernel-boundary-cleanup` (all); `260819-1416-port-common-drivers-g1-g2-g3` (all but `drivers.rs`); `260927-0739` (`mmc`); `260707-1726-g2-loader-redesign`, `260727-2101` (`virtio_common`); `260528-2016`, `260707-1726`, `260821-1800` (`drivers.rs`) | **Substantive-adjacent** to kernel-boundary-cleanup (kernel vs Driver-Cell ownership): driver presence selectors become backend data. Which driver runs where, and with what authority, does not change. |
| `kernel/src/platform.rs` (03) | `260819-1416-port-common-drivers-g1-g2-g3` | Substantive-adjacent (platform MMIO facts); moved, not re-derived. |
| `kernel/src/board.rs` (04) | `260819-1416-port-common-drivers-g1-g2-g3`, `261004-1957-x86-pc-lane` | Mention. Only the aarch64 descriptor selection moves; the x86 binding code is untouched. |
| `kernel/src/boot.rs` (04) | `260528-2016-vicell-full-implementation`, `260927-0739-kernel-architecture-repair` | Mention. |
| `kernel/src/cpu_features.rs` (04) | `260711-1917-tier3b-x86-vtx` | Mention. `HAS_EL2` semantics are unchanged; the spawn-time `HypervisorCap` grant reads it. |
| `aarch64/mte.rs` (01) | `260727-2101-midori-lessons-cellos` | Mention. |
| `aarch64/uart_pl011.rs`, `aarch64/boot.rs`, `task/drivers/uart.rs`, `aarch64.rs` | `260528-2016-vicell-full-implementation` | Mention. |
| `aarch64/cfi.rs`, `backend/*` (new) | none | None. |
| `kernel/src/main.rs` (01/04) | Many rows (umbrella). Prior bounded slices edited x86-only lines: Ginux, x86 01–03. | Mention. |

Parked or excluded plans (no lock) that name these paths: `260823-rpi3-hardware-completion` (in-progress, parked: `resource_registry`, `aarch64/trap`, `memory/paging`, `drivers/mmc`, `main.rs`, `task/syscall`, `bcm2835*`), `260615-1458-gpio-mmio-el0-fix` (In Progress, parked: `memory/paging`, `aarch64/paging`), `260607-1600-mmc-subsystem` (`drivers/mmc`, `task/syscall`), `260914-ai-oracle-arm64` (`boot.rs`, `main.rs`), `2026-08-16-rpi3-uboot-static-tftp` (`aarch64/boot`). None is admitted, and they must stay parked for the duration of this plan.

## Overlap with x86 files (x86 plan closed 2026-11-02)

The x86 file-bounded exceptions are spent: the plan is done and its CP2 is committed. There is no live owner conflict. The overlap still matters for review and for the gate:

| File | x86 edit (phase) | aarch64 edit (phase) | Consequence |
|---|---|---|---|
| `kernel/src/board.rs` | `BOUND_X86_PROFILE`, `X86_MACHINE_FACTS`, binding (01–03) | aarch64 descriptor selection `:431-523` (04) | Disjoint regions. Review that no x86 line moves. |
| `kernel/src/main.rs` | x86 profile binding and early facts (01–03), Ginux x86 early-init | console sites (01) and rpi3/virt sites (04) | Several blocks are `cfg(target_arch)` ladders that include x86 arms (`:164-175`, `:258-275`, `:1832-1850`). Editing a ladder touches the x86 arm's text. |
| `kernel/src/task/syscall.rs` | serial lookup (01–02), syscall 433 and `RegisterService(MACHINE_PIN)` (03) | `WaitIrq` USB, `RegisterDisplayFramebuffer`, `VcpuRegs` (03/04) | **`RegisterDisplayFramebuffer` cfg is shared with x86_64** (`:8877`, `:8995`). Splitting it changes x86 code, so the x86 build must be in the gate (Q9). |
| `kernel/src/task/drivers/pcie_ecam.rs` | read-only class accessor (03) | listed in aarch64 phase 03, but has no aarch64 sites | Drop from the aarch64 set (Q6). |
| `kernel/src/boot.rs` / `kernel/src/boot/limine.rs` | `limine.rs` requests (01–02) | `boot.rs` (04) | Different files; `boot.rs` is the parent module. No overlap unless a `mod` line moves. |
| `scripts/mutable-globals.toml`, `docs/generated/mutable-globals.md` | x86 statics (02–03) | only under Q7 | Regenerate; do not hand-edit. |
| `CHANGELOG.md` | x86 entries | Phase 04 entry | Append only. |

## Law-1 assessment

**No Law-1 delta is expected.** No path in `libs/api`, `libs/types` or `libs/ostd` is in the set. No syscall number, argument layout, error code, service id, record, on-disk format or wire frame changes. The plan says the same (`plan.md:49`).

Trip-wires. If any of these holds, stop and open a CP1 before continuing:
- The `VcpuRegs` (syscall) 32-word user buffer, the mode-2 Pi-only semantics, or the `AArch64Vcpu` field order/offsets change for any build. The hypervisor cell (`cells/services/hypervisor/src/run_loop.rs`, 21 rpi3 sites) consumes this layout.
- `mmio_authority()` needs the `DEV_*` class bits (`kernel/src/resource_registry.rs:37-52`) outside the kernel. They are kernel-local today, with no `libs/api`/`libs/types` mirror. Moving them into a shared ABI crate would be a Law-1 surface (Q5).
- Any `WaitIrq`/`RegisterDisplayFramebuffer` permission gate, bound or error mapping changes.
- Any device-tree, boot-protocol (load address, `linker-rpi3.ld`) or embedded-image layout change.

## Checkpoints

- **Phase 02 (checkpoint).**
  - The gate passes, including `QEMU_SMP=2` test-hooks (baseline: PASS with `[smp] hart 1 online, parked` and `SMP-IPI: PASS hart=1`).
  - The RPi3 `raspi3b` boot "reaches the prompt and the timer tick log matches the baseline cadence".
    - The prompt half holds at the baseline: `Cellos >` is reached once `target/rpi3-embedded` is built first. A1 is verified under the bash reproduction (baseline report §4, R2).
    - **The tick half is not measurable as written.** No `raspi3b` log line records a timer tick. The only proxies are the idle `L` probes, the `K000` IRQ snapshot and periodic service lines.
    - The user must restate this criterion (Q10).
  - The four files are free of `board-rpi*`. The user approves before Phase 04 starts.
- **Phase 04 (checkpoint).**
  - The full gate passes, plus `run-hypervisor-arm.ps1` (virt EL2/vGIC). No bash lane for that script was run for this baseline. The CI analogue is the `qemu-hypervisor-machinery` job (`ci.yml:690-…`), which has a documented TCG fault tolerance (Q11).
  - `AArch64Vcpu` `size_of`/offset assertions pass per variant, and the final `rg` shows hits only in `backend/mod.rs` and feature wiring.
  - Docs are updated, and the user approves before closing the plan.

## Cross-phase couplings found while deriving the file set

- **C1, Phase 02 vs 03 not disjoint.** Phase 02 moves `bcm2835_legacy_irq` into `backend/bcm2837`, but `kernel/src/task/syscall.rs:8310` (Phase 03) calls `hal::bcm2835_legacy_irq::enable_irq`. Either Phase 02 also owns `syscall.rs:8307-8311`, or Phase 02 keeps the old module path until Phase 03. The second option is a temporary re-export, which the clean-cutover rule discourages.
- **C2, `uart_bcm_mini` callers span phases.** The callers are in `main.rs` (01 and 04), `console_drv.rs` (01), `uart.rs` (unlisted) and `hypervisor/registry.rs` (04). If Phase 01 moves the module, every caller path changes in Phase 01.
- **C3, unlisted sites.** `task/drivers/uart.rs` (5 sites) and `syscall.rs:9397-9400` (`VcpuRegs`) are in no phase, but the Phase 04 end state needs them clean.
- **C4, authority-table test placement.** `resource_registry_tests.rs` is `#[cfg(test)]`, which means host tests. Host builds compile none of the aarch64 `ALLOWED` tables. A host unit test cannot check the bcm2837/bcm2711 backends.

## Constraints

- Behaviour-preserving only, with the gate in [plan.md](plan.md#behaviour-preservation-gate-every-phase) and the numbers in the baseline report. `.text` is page-padded by the linker scripts (`ALIGN(4096)`), so the baseline also records the symbol-sum metric.
- No new backend, driver, peripheral or bring-up. Group 2/3 hooks are trait shape only.
- Resource-registry tables, stage-2/monitor code and barrier/TLB sequences move without logic change. Review the diff for any altered barrier.
- Evidence ceiling: QEMU `virt` (CI lanes) + QEMU `raspi3b` (manual, A1) + RPi4 compile/size only. No physical claim.
- All 38 `CELL-CONTRACT-0024` relations keep their whole-plan scope.

## Proposed lock-inventory entry (appended to `.agents/cell-contract-0024-locks.md` only on approval)

> **Bounded adopted slice — aarch64 core/backend split, phases 01–04 (approved YYYY-MM-DD):** the accountable owner approved the [lock handoff](261031-1241-aarch64-core-backend-split/lock-handoff-approval.md) for [plan 261031-1241-aarch64](261031-1241-aarch64-core-backend-split/plan.md) (ADR-0029 §6; ADR-0022 amendment 2026-10-31 exception, not a C2C deliverable).
> - File-bounded exception for exactly the paths in the record:
>   - new `hal/arch/arm/src/aarch64/backend/**`;
>   - the aarch64 HAL console, IRQ, timer, trap, boot, EL2, stage-2 and vCPU files;
>   - the aarch64 `board-rpi*` sites in `kernel/src/{main,board,boot,platform,cpu_features,resource_registry}.rs`, `memory/paging.rs`, `task/{smp,drivers,syscall}.rs`, `task/drivers/{console_drv,uart,gpio_irq,mmc,virtio_common}.rs` and `hypervisor/registry.rs`;
>   - the docs.
> - Behaviour-preserving move only. No Law-1 delta. The resource-registry tables move byte-identically.
> - Yields to any admitted Intel C2C slice. The parked RPi3 EL1-drop and TGE-probe plans stay parked.
> - All 38 relations keep their whole-plan scope.

## Open questions for the user

1. **Q1, unlisted sites.** Add `task/drivers/uart.rs` to Phase 01 and `syscall.rs:9397-9400` (`VcpuRegs`) to Phase 04? Recommended: yes. Otherwise the Phase 04 `rg` end state cannot pass.
2. **Q2, C1 coupling.** Should Phase 02 own `syscall.rs:8307-8311`? Recommended: yes. The alternative is to run 02 before 03 with a temporary path. Until this is answered, 02 and 03 are sequential.
3. **Q3, BCM HAL files.** Move `uart_bcm_mini.rs`, `bcm2835_*`/`bcm2836_irq.rs` and `monitor.rs` under `backend/bcm2837/` (git mv), or keep them in place behind the backend? Recommended: move. The `rg` end state counts the module gates in `aarch64.rs:9-13,33,51` unless they count as "feature wiring".
4. **Q4, trait reuse.** Implement the existing `hal-interrupt`/`hal-timer` traits? That needs `hal/arch/arm/Cargo.toml` and `Cargo.lock` in the set. Or define arm-local traits only, with no manifest change?
5. **Q5, equality test form.** Choose between compile-time `const` assertions per board build (checks all three variants with no runtime lane), a test-hooks selftest (virt only), or both. Where should `mmio_authority()` live? Recommended: keep the tables kernel-side, selected through `Board`. That keeps the `DEV_*` bits kernel-local (no Law-1).
6. **Q6.** Drop `kernel/src/task/drivers/pcie_ecam.rs` from Phase 03? It has zero aarch64 sites. Recommended: drop.
7. **Q7.** Include `scripts/mutable-globals.toml` and the regenerated doc conditionally, for moved kernel statics such as `kmain()::IDLE_COUNT`?
8. **Q8.** Baseline file name: accept `reports/baseline-2026-11-02.md` and amend phase 01 step 1, or rename it to `reports/baseline.md`?
9. **Q9, gate scope.** Add the x86_64 kernel build (shared `RegisterDisplayFramebuffer` cfg) and `bash scripts/check-board-configs.sh` to every phase gate? Add RPi3/RPi4 release builds to CI (`ci.yml`, not in this set today)?
10. **Q10, the RPi3 criterion.**
    - Baseline: `raspi3b` reaches `Cellos >` only when the RPi3 cell image is built first (`pwsh scripts/build-aarch64-cells.ps1 -BoardRpi3`; R2).
    - `run-rpi3.ps1` alone silently embeds the virt image. It then stops after `Init: cellbox launched.` with an input restart storm (R1).
    - No timer-tick line exists in either run.

    Choices:
    - (a) The gate is "R2 flow; banner-to-prompt sequence identical to baseline §4 after normalising addresses and counters". It is a bash lane with `EMBEDDED_OVERRIDE=target/rpi3-embedded`, so a missing image fails loudly.
    - (b) Option (a) plus a new tick log line. That is a behaviour change, so it is out of scope here.
    - (c) Keep the `.ps1` manual lane as written and treat the tick half as not applicable.

    Recommended: (a).
11. **Q11.** Is the Phase 04 `run-hypervisor-arm.ps1` lane run manually by the maintainer, or replaced by the CI `qemu-hypervisor-machinery` analogue under bash?
12. **Q12, admission.** Admit plan order 2 now that x86 is closed? It touches `hal/arch/arm/**` and the aarch64 parts of `kernel/src/**`. Two non-main worktrees exist (`cellos-worktrees/common-drivers-g1-g2-g3` at `1c55de075`, `worktrees/cellos-integrate-authority-provisioning` at `d9f238e8d`). Confirm neither is an active owner of these paths.

## Approval record

*(Empty. Append the date, the approver's words, the answers to Q1–Q12, and any file-set amendments.)*

2026-11-02: the user replied "duyệt mặc định" and approved these defaults:
- Q1 yes
- Q2 yes
- Q3 move
- Q4 arm-local traits, no manifest change
- Q5 const assertions per board plus a virt self-test; tables stay kernel-side
- Q6 drop pcie_ecam.rs
- Q7 conditional
- Q8 keep the dated baseline name
- Q9 x86_64 build plus check-board-configs.sh in every phase gate; no CI change
- Q10 (a)
- Q11 the bash qemu-hypervisor-machinery analogue
- Q12 admit plan order 2

The handoff is effective for phases 01–04. The Phase 02 and Phase 04 checkpoints still apply.
