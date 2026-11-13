# P14 census: kernel arch seam (step 1)

Tree: worktree `cellos-p14`, branch `p14-arch-seam`, measured 2026-11-13 on `aac0d7a0c8` + `0cee9c2ace` (ratchet commit). All paths are repo-relative. "L" = line. Counts are **lines**, not occurrences: a line that names two arches counts once in "all" and once in each arch column, so arch columns do not add up to "all".

## 0. Reproduce

| Count | Command | Value |
|---|---|---|
| `target_arch` lines in `kernel/src` | `rg -n 'target_arch' kernel/src \| wc -l` | **1837** |
| files with `target_arch` | `rg -l 'target_arch' kernel/src \| wc -l` | 102 |
| per arch (`A` = riscv64, x86_64, aarch64, riscv32, arm, x86) | `rg -c "target_arch\s*=\s*\"A\"" kernel/src \| awk -F: '{s+=$2} END{print s}'` | riscv64 670 · x86_64 645 · aarch64 543 · riscv32 90 · arm 22 · x86 36 |
| `asm!`/`global_asm!`/`naked_asm!` lines | `rg -n '\b(global_\|naked_)?asm!' kernel/src \| wc -l` | **124** in 32 files (`global_asm!` 5: `sched/smp.rs` 2, `snapshot_engine.rs` 3; `naked_asm!` 0) |
| direct HAL arch paths | `rg -o 'hal::P::' kernel/src \| wc -l` (`P` = aarch64, x86_64, riscv64, riscv, arm, x86, arch) | `hal::aarch64::` **107** · `hal::x86_64::` **36** · `hal::riscv64::`/`riscv::`/`arm::`/`x86::` 0 · `hal::arch::` 107 |
| 32-bit lines | `rg -n 'target_arch\s*=\s*"(riscv32\|arm\|x86)"' kernel/src \| wc -l` | 123 lines in 25 files |
| `kmain` cfgs | `awk 'NR>=175 && NR<=1818 && /target_arch/' kernel/src/main.rs \| wc -l` / same with `/#\[cfg/` | 138 / 129 |

The ratchet is in `scripts/check-hal-boundaries.sh:67-85` (`arch_cfg_max=1837`, `arch_asm_max=124`); its grep matches the commands above.

## 1. `target_arch`, `asm!`, `hal::<arch>` census

### 1a. Per top-level entry of `kernel/src`

| Entry | all | x86_64 | aarch64 | riscv64 | riscv32 | arm | x86 |
|---|---|---|---|---|---|---|---|
| task/ | 444 | 133 | 130 | 177 | 18 | 0 | 0 |
| sched/ | 309 | 110 | 85 | 119 | 19 | 0 | 0 |
| memory/ | 196 | 70 | 51 | 81 | 11 | 7 | 7 |
| dev/ | 157 | 75 | 45 | 45 | 7 | 0 | 9 |
| main.rs | 157 | 55 | 42 | 55 | 9 | 8 | 10 |
| task.rs | 150 | 40 | 34 | 73 | 11 | 1 | 1 |
| loader/ | 81 | 29 | 20 | 29 | 8 | 4 | 4 |
| hypervisor/ | 51 | 20 | 41 | 0 | 0 | 0 | 0 |
| board.rs | 45 | 30 | 6 | 9 | 0 | 0 | 0 |
| boot.rs | 40 | 2 | 28 | 11 | 2 | 2 | 2 |
| snapshot_engine.rs | 33 | 7 | 9 | 19 | 3 | 0 | 0 |
| platform.rs | 25 | 0 | 4 | 23 | 0 | 0 | 0 |
| resource_registry.rs | 25 | 2 | 21 | 6 | 0 | 0 | 0 |
| boot/ | 24 | 24 | 0 | 0 | 0 | 0 | 0 |
| cpu_features.rs | 14 | 8 | 2 | 4 | 0 | 0 | 0 |
| dev.rs | 12 | 6 | 3 | 4 | 0 | 0 | 1 |
| memory.rs | 11 | 4 | 3 | 4 | 0 | 0 | 0 |
| cell/ | 9 | 4 | 2 | 2 | 2 | 0 | 2 |
| sched.rs | 9 | 3 | 3 | 3 | 0 | 0 | 0 |
| acpi/ | 7 | 7 | 0 | 0 | 0 | 0 | 0 |
| layer2_selftest.rs | 6 | 2 | 4 | 0 | 0 | 0 | 0 |
| resource_registry_mediation.rs | 6 | 0 | 6 | 0 | 0 | 0 | 0 |
| machine_facts.rs | 5 | 5 | 0 | 0 | 0 | 0 | 0 |
| p01_noncpu.rs | 4 | 4 | 0 | 0 | 0 | 0 | 0 |
| sync.rs, hypervisor.rs, posture.rs (3 each), p01_topology.rs, audit.rs (2 each), cell.rs, bench_counters.rs, platform/, ipc/ (1 each) | 17 | | | | | | |

`posture.rs` 3 and `dev/uart.rs:320` are the four lines that name `target_arch` without naming an arch.

### 1b. Per file, files with ≥ 8 lines (53 files, 1 688 lines; the other 49 files hold 149)

| File | all | x86_64 | aarch64 | riscv64 | riscv32 | arm | x86 |
|---|---|---|---|---|---|---|---|
| main.rs | 157 | 55 | 42 | 55 | 9 | 8 | 10 |
| task.rs | 150 | 40 | 34 | 73 | 11 | 1 | 1 |
| memory/paging.rs | 92 | 38 | 33 | 30 | 6 | 6 | 6 |
| sched/hart_local.rs | 91 | 24 | 30 | 35 | 15 | 0 | 0 |
| sched/smp.rs | 85 | 44 | 19 | 23 | 1 | 0 | 0 |
| task/syscall/mod.rs | 72 | 20 | 19 | 28 | 5 | 0 | 0 |
| hypervisor/registry.rs | 51 | 20 | 41 | 0 | 0 | 0 | 0 |
| memory/tlb_shootdown.rs | 50 | 15 | 11 | 23 | 2 | 0 | 0 |
| task/copy_glue/mod.rs | 48 | 16 | 16 | 16 | 0 | 0 | 0 |
| board.rs | 45 | 30 | 6 | 9 | 0 | 0 | 0 |
| task/syscall/grant.rs | 43 | 7 | 19 | 15 | 2 | 0 | 0 |
| dev/pcie_ecam.rs | 42 | 34 | 5 | 4 | 0 | 0 | 0 |
| boot.rs | 40 | 2 | 28 | 11 | 2 | 2 | 2 |
| task/syscall/dev.rs | 39 | 18 | 14 | 7 | 5 | 0 | 0 |
| sched/scheduler.rs | 39 | 16 | 8 | 19 | 0 | 0 | 0 |
| loader/domain_admission.rs | 38 | 18 | 15 | 10 | 0 | 0 | 0 |
| task/syscall/misc.rs | 36 | 5 | 11 | 20 | 0 | 0 | 0 |
| snapshot_engine.rs | 33 | 7 | 9 | 19 | 3 | 0 | 0 |
| task/tcb.rs | 33 | 11 | 11 | 11 | 0 | 0 | 0 |
| sched/domain_switch_tests.rs | 33 | 10 | 24 | 4 | 0 | 0 | 0 |
| task/syscall/ipc.rs | 31 | 10 | 10 | 10 | 1 | 0 | 0 |
| platform.rs | 25 | 0 | 4 | 23 | 0 | 0 | 0 |
| resource_registry.rs | 25 | 2 | 21 | 6 | 0 | 0 | 0 |
| boot/limine.rs | 24 | 24 | 0 | 0 | 0 | 0 | 0 |
| sched/hart_local/ready.rs | 23 | 4 | 4 | 12 | 3 | 0 | 0 |
| dev/iommu.rs | 21 | 15 | 0 | 11 | 0 | 0 | 0 |
| sched/quiesce.rs | 20 | 0 | 0 | 20 | 0 | 0 | 0 |
| task/thread_user_entry_selftest.rs | 19 | 0 | 0 | 19 | 0 | 0 | 0 |
| dev/uart.rs | 18 | 6 | 5 | 5 | 1 | 0 | 4 |
| dev/virtio_common.rs | 15 | 0 | 11 | 12 | 0 | 0 | 0 |
| cpu_features.rs | 14 | 8 | 2 | 4 | 0 | 0 | 0 |
| memory/address_space.rs | 14 | 3 | 2 | 9 | 0 | 0 | 0 |
| task/user_copy/sv39_probe.rs | 13 | 4 | 4 | 5 | 0 | 0 | 0 |
| dev/mmc/sdhci.rs | 13 | 0 | 13 | 0 | 0 | 0 | 0 |
| dev.rs | 12 | 6 | 3 | 4 | 0 | 0 | 1 |
| memory/heap.rs | 12 | 4 | 2 | 6 | 0 | 0 | 0 |
| task/stack.rs | 12 | 1 | 1 | 10 | 5 | 0 | 0 |
| task/telemetry.rs | 12 | 4 | 4 | 4 | 0 | 0 | 0 |
| task/launch.rs | 12 | 4 | 4 | 4 | 0 | 0 | 0 |
| task/copy_glue/scatter.rs | 12 | 4 | 4 | 4 | 0 | 0 | 0 |
| memory.rs | 11 | 4 | 3 | 4 | 0 | 0 | 0 |
| dev/virtio_rng.rs | 11 | 3 | 0 | 8 | 0 | 0 | 0 |
| task/completion_wait.rs | 10 | 10 | 0 | 0 | 0 | 0 | 0 |
| memory/page_protect.rs | 10 | 8 | 1 | 1 | 1 | 1 | 1 |
| task/syscall/dma_export.rs | 10 | 10 | 0 | 0 | 0 | 0 | 0 |
| sched/off_hart0_timeout_selftest.rs | 10 | 10 | 0 | 0 | 0 | 0 | 0 |
| dev/console_drv.rs | 10 | 5 | 3 | 2 | 2 | 0 | 5 |
| cell/init_death.rs | 9 | 4 | 2 | 2 | 2 | 0 | 2 |
| memory/deferred_release.rs | 9 | 0 | 0 | 9 | 0 | 0 | 0 |
| sched.rs | 9 | 3 | 3 | 3 | 0 | 0 | 0 |
| loader/atomic_publication_tests/harness.rs | 9 | 0 | 0 | 9 | 0 | 0 | 0 |
| task/user_copy/guard.rs | 8 | 2 | 2 | 4 | 0 | 0 | 0 |
| memory/kaslr.rs | 8 | 2 | 2 | 2 | 2 | 0 | 0 |

### 1c. The "all three 64-bit arches" idiom

162 `any(target_arch = "riscv64", target_arch = "aarch64", target_arch = "x86_64")` blocks (any order) cover **486 of the 1 837 lines (26 %)** in 31 files. Another 28 blocks are the negated form. Top files: `sched/hart_local.rs` 21 blocks, `task/copy_glue/mod.rs` 16, `main.rs` 15, `task.rs` 11, `task/tcb.rs` 11, `memory/tlb_shootdown.rs` 8, `task/syscall/mod.rs` 8, `memory/paging.rs` 7. (Counted with a multi-line regex over `kernel/src/**/*.rs`. `rg -U` with the same pattern reproduces it.)

These blocks only exclude 32-bit. `posture.rs:58` already defines the same set (`wide = matches!(target_arch, "x86_64" | "aarch64" | "riscv64")`), and `build.rs:179-181` already emits a build-script cfg (`cellos_posture`). If `build.rs` also emitted `cargo:rustc-cfg=cellos_wide`, the 486 lines would collapse into one predicate that names no arch. That is a mechanical cfg rewrite, not a logic edit. It alone gets the ratchet a quarter of the way to the "half of baseline" target (≤ 918).

### 1d. `asm!` sites per file, tagged with the function set they belong to

| File | asm! | global_asm! | Lines | Function set |
|---|---|---|---|---|
| sched/smp.rs | 14 | 2 | sev 999; x86 AP trampoline `global_asm` 1215; cli/sti probe 1387–1660; handoff fixture `global_asm` 1753; wfi 1860/1973; SUM 1894; STIE 1920 | smp (with irq/user_access bits) |
| snapshot_engine.rs | 4 | 3 | sp reads 1421/1430/1439; stack-run trampolines 1460/1479/1496; wfi 3003 | *cpu/stack (not in plan set)* |
| main.rs | 7 | 0 | SUM 605; CurrentEL 838; IRQ enable 1734/1738; satp 1861 (`vi_rv64_kernel_fault_snapshot`); panic halt 1945/1952 | boot / irq / power |
| sched/hart_local.rs | 7 | 0 | mpidr 44; tp 386/408/431/457/945/975 | smp |
| task/syscall/mod.rs | 6 | 0 | SUM 1866/1878/2081/2093/2155/2163 | user_access |
| memory/paging.rs | 6 | 0 | sfence 58, tlbi 66; satp/ttbr0 1463/1475/1523/1537 | *tlb* + page_probe |
| hypervisor/svm_registry.rs | 6 | 0 | cli/sti around guest entry 434/441/472/602/763/768 | irq (*hv*) |
| acpi/power.rs | 6 | 0 | port I/O 626–657; `lidt`+`int3` reset 739 | power |
| sched/cpu_accounting.rs | 5 | 0 | cli/debug-exit/hlt 794–801, 1507–1511 (g1 only) | power |
| layer2_selftest.rs | 4 | 0 | MTE `id_aa64pfr1`/`stg`/`ldg` 34/108/133; `rdpkru` 199 | *selftest* |
| task/user_copy/guard.rs | 4 | 0 | SUM 60; fault-fixup rv64 106, aarch64 138, x86 167 | user_access |
| memory/heap.rs | 4 | 0 | sp 66/453; hlt/wfi 471/475 | *cpu* / power |
| task/syscall/dev.rs | 4 | 0 | SBI SRST 856; PSCI 868; hlt 878; wfi 888 | power |
| task/user_copy/sv39_probe.rs | 3 | 0 | satp 104, ttbr0 119, cr3 131 | page_probe |
| memory/tlb_shootdown.rs | 3 | 0 | fence 197/428, dsb 204 | *tlb* |
| memory/kaslr.rs | 3 | 0 | rdtsc 84, `time` 99, cntvct 108 | clock |
| task/retirement_selftest.rs | 3 | 0 | sstatus/sip 400/412/446 | irq (selftest) |
| task/stack.rs | 3 | 0 | SUM 43/45/65 | user_access |
| task.rs | 3 | 0 | STIE 873; yield pushfq/cli 1407; popfq 1938 | clock / irq |
| dev/ps2_keyboard.rs | 3 | 0 | in/out 74/83; 194 | *pio* |
| dev/iommu_x86/cache.rs | 3 | 0 | mfence/clflush 28–33 | iommu |
| dev/virtio_common.rs | 3 | 0 | SUM 153/155/161 | user_access |
| cell/init_death.rs | 2 | 0 | cli;hlt 222, wfi 227 | power |
| hypervisor/smoke_guest.rs | 2 | 0 | EL1 sysregs 184/214 | *hv* |
| task/tls.rs | 2 | 0 | tpidr_el0 98; FS_BASE wrmsr 109 | user_access (TLS) |
| dev/uart.rs | 2 | 0 | COM1 `in` 415/426 | *pio* |
| dev/mmc/sdhci.rs | 2 | 0 | cntpct 142, cntfrq 157 | clock |
| memory/tlb_shootdown_selftest.rs | 1 | 0 | 59 | *tlb* (selftest) |
| hypervisor/registry.rs | 1 | 0 | cntp_ctl 1001 | *hv* |
| p01_topology.rs | 1 | 0 | cr3 387 | page_probe |
| task/waker.rs | 1 | 0 | `csrsi sip` 118 | irq (self-IPI) |
| sched/scheduler.rs | 1 | 0 | `csrsi sip` 972 | irq (self-IPI) |

By function set: smp 23, user_access 18, power 17, irq 13, boot 7, clock 6, page_probe 8, iommu 3. **29 sites belong to no set the plan names:** tlb 6, cpu/stack 11, hv 3, pio 5, selftest 4. The svm_registry cli/sti (6) are irq by mechanism but hv by owner. Step 2 needs `arch::{tlb, cpu, pio}` (or an explicit "stays in hypervisor/ and selftests" rule), or "no `asm!` outside `arch/` and `hal/`" cannot be met.

### 1e. Direct HAL arch paths per file

`hal::aarch64::` (107) → `backend` 53, `timer` 12, `psci` 11, `boot` 8, `el` 7, `cache` 6, `vcpu` 3, `paging` 3, `stage` 1, `init_secondary_hart` 1.
`hal::x86_64::` (36) → `msi` 7, `apic` 7, `invept` 4, `syscall` 3, `clock` 3, `svm`/`svm_vcpu` 4, `vmx_vcpu`/`vmcs` 2, `uart_`, `publish_boot_cpu_policy`, `protect`, `paging`, `ist`, `init_secondary_cpu` (1 each).

| File | hal::aarch64 | hal::x86_64 | hal::arch |
|---|---|---|---|
| sched/smp.rs | 38 | 12 | 9 |
| resource_registry.rs | 15 | | |
| hypervisor/registry.rs | 9 | | |
| hypervisor/svm_registry.rs | | 9 | |
| board.rs | 6 | | |
| memory/paging.rs | 5 | | |
| dev/mmc.rs, dev/gpio_irq.rs | 4, 4 | | |
| main.rs | 1 | 3 | |
| boot.rs, task/syscall/dev.rs, dev/virtio_common.rs, hypervisor/smoke_guest.rs | 3 each | | |
| dev/pcie_msi.rs, dev/pcie_msi/program.rs | | 3, 3 | |
| memory/tlb_shootdown.rs | 2 | | 2 |
| cell/init_death.rs, task/syscall/vm.rs, cpu_features.rs, dev.rs | 2 each | | |
| dev/pcie_msi/vectors.rs, memory/ept.rs | | 2, 2 | |
| platform.rs, loader/domain_admission.rs, sched/domain_switch_tests.rs | 1 each | | 0, 0, 20 |
| sched/smp/topology.rs, dev/console_select.rs | | 1 each | |
| task.rs 39, sched/scheduler.rs 12, audit.rs 6, task/retirement_selftest.rs 5, guard.rs/sas-lease.rs/syscall/mod.rs/tcb.rs 2 each, 7 files 1 each | | | 107 total |

`hal::arch::` is the HAL's own per-arch facade: `rv64.rs:15`, `aarch64.rs:231`, `x86_64.rs:85`, plus 32-bit `rv32.rs:35`, `aarch32.rs:91`, `x86_32.rs:88`. Its targets are `Context` 34, `save_and_disable_interrupts` 13, `restore_sstatus` 12, `ViTrapFrame` 12, `Context::switch_with_root` 9, `set_kernel_stack` 7, `Context::zeroed` 6, `Context::switch` 3, and a few others. These are neutral names, but the module name will collide with the new `crate::arch`.

**Arch-only HAL modules reached through glob re-exports** (`hal/core/src/lib.rs:29-46` glob-exports the active arch crate). These paths contain no arch name, so the `hal::<arch>` grep misses them: 148 sites. The largest are `main.rs` (backend 21, uart_16550 16, sbi 8, apic 4, vmx 3, clock 2, others), `task/syscall/ipc.rs` (timer/clock/rtc 10), `dev/uart.rs` 8, `task/x86_idt_cpl3.rs` (idt 6), `task/syscall/mod.rs` 5, `task/telemetry.rs` 5, `sched/cpu_accounting.rs` (hpet 4), `acpi/sci.rs` 4 and `cpu_features.rs` (svm/vmx 4). Reproduce with `rg -o '\bhal::(sbi|uart_16550|idt|cet|vmx|hpet|apic|pku|ioapic|gdt|svm|backend|uart_pl011|rtc|clock|timer|syscall|set_pm_timer|set_hpet_base|init_sse|mte|cfi|trap|cache|sync_instruction_cache|init_timers|init_timers_observed|context|dma_granule)\b' kernel/src | wc -l`. They belong in the step-2 move list too.

### 1f. Whole-file single-arch modules: arch-gated at the `mod` line, so they move as whole files

- **x86_64:**
  - `acpi/mod.rs:26,29` (power, sci)
  - `dev.rs:22,24,26,43,46,49,55` (console_select, fb_console, fb_font, iommu_x86, pcie_msi, driver_cell, ps2_keyboard)
  - `hypervisor.rs:15` (svm_registry)
  - `memory.rs:38` (ept)
  - `task.rs:79` (x86_idt_cpl3)
  - `boot/limine.rs` (24 lines, x86 only)
- **aarch64:**
  - `dev.rs:18` (bcm_pinmux)
  - `dev/mmc.rs:4` (pinmux_bcm)
  - `hypervisor.rs:9,18` (pending_irqs, smoke_guest)
  - `sched/smp.rs:14` (mpidr)
  - `boot.rs:14` (fdt_nops)
- **riscv64:**
  - `dev.rs:28,39,41` (irq_dispatch, iommu_riscv, iommu_riscv_cmd)
  - `memory.rs:57` (tlb_shootdown_selftest)
  - `cell.rs:11` (provenance_selftest)
  - `task.rs:37,99,102,113,115,118` (fstat, context_handoff, getrandom_sas_tests, path, retirement, stack_overflow_probe self-tests)
  - `memory/address_space.rs:1719`

### 1g. Proposed move order for step 2

Rule: whole files first, then leaf helpers with few callers, then hot paths. `task.rs` `yield_cpu` belongs to step 4 and `memory/paging.rs`/`page_protect.rs` to step 5. Each row is one commit that lowers the ratchet.

| # | Set | Move | Removes from outside `arch/` |
|---|---|---|---|
| M0 | (seam) | `kernel/src/arch/{mod.rs,x86_64,aarch64,riscv64,legacy}`; one `cfg` per arch at `mod`. Add `cellos_wide` (§1c) and rewrite the triple-`any` blocks | ≈486 lines |
| M1 | whole files | the §1f modules, x86 first (`acpi/{power,sci}`, `dev/{iommu_x86/**,pcie_msi/**,console_select,fb_console,fb_font,driver_cell,ps2_keyboard}`, `hypervisor/svm_registry`, `memory/ept`, `boot/limine.rs`, `task/x86_idt_cpl3`), then aarch64 and riscv64. Callers' `use` paths change; no re-export shims | 21 asm (power 6, iommu_x86 3, ps2 3, svm 6, smoke_guest 2, tlb selftest 1); `hal::x86_64` 19; `hal::aarch64` 3; boot/limine 24 lines + mod-line cfgs |
| M2 | page_probe | `sv39_probe.rs` `sv39_leaf`/`current_satp_root` (18–145) → `arch::page_probe::{leaf, current_root}`; `paging.rs` `translation_probe`/`virt_to_phys` root reads (1459–1566); `p01_topology.rs:387` | 8 asm, ≈30 lines |
| M3 | user_access | SUM toggles (`syscall/mod.rs` 1866–2163, `task/stack.rs` 43–65, `virtio_common.rs` 153–161, `main.rs:605`, `smp.rs:1894`) → `arch::user_access::{open, close}`; `guard.rs` fixups (106/138/167); `tls.rs` 98/109 | 18 asm |
| M4 | irq | `arch::irq::{save_disable, restore, enable, raise_local_resched}`: `scheduler.rs:972`, `waker.rs:118`, `main.rs:1731-1742`, svm_registry cli/sti (moved in M1), smp.rs x86 probe cli/sti. The yield masks (`task.rs:1407/1938`) wait for M-step 4 | ≈8 asm |
| M5 | clock | `kaslr.rs` 84–108, `sdhci.rs` 142/157, rv64 STIE + `sbi::set_timer` (`task.rs:873-876`, `smp.rs:1920-1923`), `cpu_accounting.rs` hpet, `ipc.rs`/`telemetry.rs` timer/clock/rtc paths | 6 asm |
| M6 | power (+cpu) | `syscall/dev.rs` 856–888 → `arch::power::{reset, shutdown}`; halts (`init_death.rs:222/227`, `main.rs:1945/1952`, `heap.rs:471/475`, `snapshot_engine.rs:3003`) → `arch::cpu::halt/idle`; sp reads (`heap.rs`, `snapshot_engine.rs`) → `arch::cpu::stack_pointer` | ≈17 asm |
| M7 | pci_irq, iommu | `dev/pcie_ecam.rs` (x86 34 lines), `dev/iommu.rs` dispatch (21), `dev/iommu_pt.rs` (6), `dev/gpio_irq.rs`/`dev/mmc.rs`/`virtio_common.rs` aarch64 paths, `resource_registry.rs` (21 aarch64 lines, 15 `hal::aarch64`) | ≈100 lines, ≈25 `hal::aarch64` |
| M8 | smp (+tlb) | `sched/hart_local.rs` (91 lines, 7 asm), `sched/smp.rs` (85 lines, 16 asm, 59 HAL paths), `smp/topology.rs`, `memory/tlb_shootdown.rs` (50, 3 asm), `paging.rs:58/66` → `arch::smp`, `arch::tlb` | ≈230 lines, 26 asm |
| M9 | boot (= step 3) | `main.rs`, `boot.rs`, `board.rs`, `platform.rs`, `cpu_features.rs` → `arch::<a>::boot` | ≈280 lines |
| later | step 4/5 + residue | `task.rs` yield (§2), `memory/paging.rs`, `page_protect.rs` (§3), `task/syscall/*`, `copy_glue`, `loader/domain_admission.rs`, `hypervisor/registry.rs` (51), `domain_switch_tests.rs`, `layer2_selftest.rs` | |

## 2. `yield_cpu` (`kernel/src/task.rs:1385-1945`)

### 2a. Which body compiles

- **Body A** (`task.rs:1777-1851`) is built under `all(native-domains, any(rv64, aarch64, x86_64))`.
- **Body B** (`task.rs:1852-1927`) is built otherwise.
- `native-domains` is a default feature (`kernel/Cargo.toml:81`). Every `--no-default-features` kernel tuple in CI and scripts re-adds it (`release-posture`, `production-relay-image` and `getrandom-sas-test`: `kernel/Cargo.toml:119,124,217`).
- `posture.rs:57-64`, enforced by `build.rs:9-11,179`, refuses a 64-bit kernel without `native-domains` unless the build is `compile-only`.

**Body B therefore runs only on 32-bit targets. On 64-bit targets it is compiled only in non-bootable `compile-only` builds.**

### 2b. Line-by-line

| Concern | Body A (native-domains) | Body B | Same? |
|---|---|---|---|
| selection | `sched.pick_next_domain(hart_id)` 1720–1724 → `scheduler.rs:2666-2675` → `SwitchPlan::new` (`domain_switch.rs:88-119`), which calls `advance_execution_pin` (103/110) | `sched.pick_next(hart_id)` 1733–1737 (`scheduler.rs:2278/2306`), returns a tuple | **no**: side effects only in A |
| heartbeat observer (rv64 test-hooks) | 1742–1751 on `plan.incoming` | 1752–1761 on tuple `next` | yes, semantically |
| null out/in → boot context | 1787–1810 | 1862–1886 | yes. rv64 inlines `BOOT_CONTEXTS[hart]`; others use `boot_context_for` (1350–1355), which is the same slot |
| bench `context_switch` | 1812–1813 | 1888–1889 | yes |
| `note_task_dispatch` (aarch64, x86) | 1814–1815 | **absent** | **no**: B never logs `[sched] hart N dispatched a task` (`task.rs:1373-1382`) |
| `set_kernel_stack` | rv64 `sp` 1817; aarch64 `sp as usize` 1819; x86 `kernel_trap_sp` 1821 | 64-bit identical (1899, 1905, 1909) + riscv32 cast 1907 + generic fallback for arm/x86-32 1900–1905 | 64-bit yes |
| `tls::install_published_base` (non-rv64) | 1822–1823 | 1910–1911 | yes |
| `hold_after_selection_before_switch` (rv64 test-hooks) | 1827–1828 | 1913–1914 | yes |
| root decision | `plan.root_switch()` (`domain_switch.rs:131-170`): `set_current_domain`, `hal::domain::observe_switch_activation`, `mark_safe_root_pending`, bench `root_programming`, `S22-RV64-SWITCH` log | none | **no** |
| rv64 HAL call | `switch_with_saved_sstatus(curr, next, outgoing_sstatus, root_ppn, asid)` 1830–1836 | same with `0, 0` 1917–1923 | same entry |
| aarch64/x86 HAL call | `Context::switch_with_root(curr, next, root_addr, asid)` 1846–1848 | `Context::switch(curr, next)` 1925 | same: `switch` is `switch_with_root(_, _, 0, 0)` (x86 `context.rs:107-110,137-140`; aarch64 `context.rs:116-119,163-166`) |

**Shared code outside the two bodies:**
- **IRQ masking.** rv64 masks at entry through the HAL (1393–1394). x86 masks at entry with inline asm (1404–1409), which duplicates the HAL's `save_and_disable_interrupts` (`x86_64/context.rs:71-84`). aarch64 masks late, right before selection (1709–1710).
- **No-switch restore.** rv64 restores at 1768–1775. Both bodies lack an rv64 restore after resume: `__switch` restores sstatus itself.
- **Epilogue.** `tls::apply_on_resume` (1932), then x86 `popfq` (1936–1939), then aarch64 DAIF restore (1941–1944).

### 2c. HAL entry points

| | rv64 | aarch64 | x86_64 |
|---|---|---|---|
| entry yield uses | `Context::switch_with_saved_sstatus` (`rv64/context.rs:85-108`) → `__switch` (`rv64/asm/switch.S:8-95`) | `CpuContext::switch_with_root` (`aarch64/context.rs:131-157`) → `__switch_el1` / `__switch_el2` | `CpuContext::switch_with_root` → free `switch_with_root` (`x86_64/context.rs:122-215`) |
| root encoding (`domain_switch.rs:53-62,65-77`) | PPN; ASID → `satp = 8<<60 \| asid<<44 \| ppn` (switch.S:40-50) | table PA → `root_register_value = asid<<48 \| baddr` (`aarch64/domain.rs:77-79`) | PML4 PA → `cr3_for(root, pcid, pcid_usable)` (`x86_64/domain.rs:97`) |
| what the root does | writes hardware `satp` + `sfence.vma zero, a4` between save and load (switch.S:39-51) | `msr ttbr0_el1` + `isb`, no TLBI | **never writes hardware CR3.** Writes the user-entry CR3 scratch `gs:[24]` (`context.rs:182-183`); CR3 changes only on trap entry/exit (`syscall.rs:358-367`) |
| `root == 0` | `beqz a3`: no write (switch.S:39) | `cbz x2`: no write; EL2 ignores root (`debug_assert`, `context.rs:137-143`) | `cmovz rdx, [rsi+72]`: **inherit the incoming context's saved `user_cr3`** |
| IRQ state | caller's pre-mask sstatus (a2) is saved into the outgoing ctx (switch.S:31); incoming ctx sstatus is restored last (switch.S:86-89) | switch saves the current (masked) DAIF and restores the incoming DAIF last; yield restores the pre-mask DAIF after resume (1941–1944) | RFLAGS is not in the context; yield `popfq`s after resume (1936–1939) |
| `set_kernel_stack` | **no-op** (`rv64/trap.rs:75`; `rv64.rs:15-18` re-exports `trap::*`) | TPIDR_EL1 (`aarch64.rs:175,247`) | TSS.rsp0 + CPU_LOCAL (`x86_64.rs:552`) |

**Unused fourth entry point.** `Arch::switch_context` (`hal/traits/arch/src/lib.rs:17`) is implemented by every HAL (for example `x86_64.rs:507`) and has **zero kernel callers** (`rg 'switch_context' kernel` is empty).

**Other callers of the same entries:**
- `sched/smp.rs:1782,1820` call `Context::switch` (x86 handoff fixture).
- `sched/domain_switch_tests.rs` calls `switch_with_root` 8 times (682–1414).
- rv64 `Context::switch` (`rv64/context.rs:69-73`) masks at the call boundary. Yield never uses it.

### 2d. Test of the plan's claim: "the bodies differ only in root/ASID, so a null root expresses SAS"

**Verdict: half right.**

- **What holds.** At the HAL call, B is exactly A with `root = (0, 0)` on all three 64-bit arches. A itself returns `(0, 0)` only for `DomainTransition::SasToSas` (`domain_switch.rs:168`); `SameDomain` always re-programs the root (150–160). So `None` = SAS→SAS is a sound encoding.
- **What does not hold.** The bodies also differ in:
  1. `note_task_dispatch`, a boot marker that exists only in A.
  2. The plan-construction and `root_switch` side effects, all only in A.
  3. The 32-bit `set_kernel_stack` arms, only in B.
- **"Null" has two meanings.**
  - rv64 and aarch64: "no hardware write". After trap entry the live root is the kernel root (rv64 `trap.S:198-208`, aarch64 `trap.rs:479-488`), so no write means SAS.
  - x86: "inherit the incoming context's saved `user_cr3`".
  - Null equals SAS on x86 only because SAS contexts saved the SAS CR3 at trap entry. [INFERENCE] A private-root context resumed with null would return to its private root. `SwitchPlan` never produces that case, but the HAL contract must say so.

### 2e. Proposed shape

```rust
// hal::arch (one per arch)
pub struct Root { base: NonZeroUsize /* rv64: PPN; aarch64/x86: table PA */, asid: u16 }
#[must_use] pub struct IrqSaved(usize);        // !Send; Drop restores; from save_and_disable_interrupts
pub struct SwitchPlan {
    pub out: *mut Context,          // non-null: kernel substitutes the boot context first
    pub inc: *const Context,        // non-null
    pub root: Option<Root>,         // None = no root transition (SAS→SAS)
    pub irq: IrqSaved,              // consumed by switch
}
/// Returns this activation's IRQ token when it resumes; the caller drops it
/// after tls::apply_on_resume (keeps today's x86/aarch64 order, task.rs:1932-1944).
pub unsafe fn switch(plan: SwitchPlan) -> IrqSaved;
pub fn set_kernel_stack_for(inc: &Context);    // picks sp / kernel_trap_sp; rv64 no-op
```

**Per-arch `switch`:**
- **rv64:** `__switch(out, inc, irq.into_raw(), root.ppn|0, root.asid|0)`. It returns an already-restored (no-op) token, because `__switch` restored sstatus.
- **aarch64:**
  - At EL2, `assert!(root.is_none())`, then `__switch_el2`.
  - Otherwise `__switch_el1(out, inc, root.map_or(0, root_register_value))`.
  - It returns `plan.irq`.
- **x86_64:** `switch_with_root(out, inc, root.base|0, root.asid|0)`. It returns `plan.irq`.

**Kernel side:**
- **IRQ masking point.** Masking happens at entry on rv64/x86 and at selection on aarch64. Keep that difference explicit with a single `arch::sched::MASK_AT_ENTRY: bool` and one `IrqSaved` per yield. Making aarch64 mask from entry would mask the housekeeping drains (1477–1682): that is a behaviour change, so log it.
- **No-switch path.** It becomes "drop the token" on every arch. [INFERENCE] On rv64 this moves the restore from 1773 to after `tls::apply_on_resume`, which does nothing on rv64.
- **Kernel plan rename.** `sched::domain_switch::SwitchPlan` (`domain_switch.rs:42`) already uses this name. Rename it (e.g. `RootDecision`) and let it produce `Option<Root>`.
- **A/B fixture markers.** They must include the `[sched] hart N dispatched a task` line, which only A emits.

## 3. Page tables

### 3a. Flag encoding: `pte_bits_from_flags` vs the HAL trait

| `PageFlags` bit (`hal/traits/paging/src/lib.rs:11-45`) | `pte_bits_from_flags` (`page_protect.rs:229-243`) | trait `map` leaf (`x86_64/paging.rs:390-416`) |
|---|---|---|
| VALID | ignored; P always set (232) | ignored; P always set (401) |
| READ | ignored | ignored |
| WRITE | RW (233–235) | RW (402–404) |
| USER | US (236–238) | US (405–407) |
| EXECUTE absent | NX (239–241) | NX (408–410) |
| GLOBAL, ACCESSED, DIRTY, DEVICE, NON_GLOBAL, GUARDED | ignored | ignored (documented `lib.rs:19-20,31-35,44`) |

The leaf permission encodings are **identical**. So the plan's low-confidence claim, "no flag the HAL cannot express", is true for `map_page`/`protect_page` permissions. It is false for the raw paths.

**What the HAL trait cannot express:**
1. **PCD (cache-disable).** `pte_flags_mmio` and `pte_flags_mmio_user` (`x86_64/paging.rs:100-110`) set it. They are used by `init_kernel_paging_x86` (`paging.rs:544`) and by `map_mmio_x86`/`map_mmio_user_x86` (`paging.rs:702-717`). `PageFlags::DEVICE` exists, but x86 ignores it.
2. **PWT**, and preserving the existing PWT|PCD bits on a rewrite (`page_protect.rs:115-116`). Trait `map` overwrites the whole leaf (`pt.entries[i0] = e`, 414).
3. **PAT** (bit 7 on a 4K leaf, bit 12 on large pages).
4. **G (global, bit 8).** `PageFlags::GLOBAL` is ignored.
5. **Huge leaves (PS)** for map. `map_bios_area` builds 2 MiB leaves outside the trait (`x86_64/paging.rs:518+`). Trait `translate` handles 2 MiB at PD level only, not 1 GiB at PDPT (467). The `sv39_probe` x86 walker handles PS at any level (`sv39_probe.rs:75`).
6. **PKEY (bits 59–62).** Unused today (`rg PKEY` finds nothing in memory/loader).
7. **AVL bits.**
8. **"Not present".** x86 always sets P, so on x86 a `protect_page` without `VALID` does **not** unmap. That contradicts `page_protect.rs:45-47`; rv64 copies the bits verbatim.

**Non-flag gaps:**
- **Intermediate tables.** `walk_create` always writes P|RW|U (`x86_64/paging.rs:232`). Trait `get_or_alloc` writes U only for user leaves and ORs U in later (723–731).
- **Address mask.** The trait's `unmap`, `translate` and `get_or_alloc` extract addresses with `& !0xFFF` (425–470, 733). That keeps NX bit 63, which the HAL's own comment calls wrong (`paging.rs:58-66`). The walkers use `PTE_ADDR_MASK`.
- **Unmap.** Trait `unmap` returns `Err(NotFound)` and does a local `invlpg` only (419–445). `unmap_page_x86` is idempotent and uses a global shootdown (`paging.rs:759-800`).
- **Raw leaf read.** There is no x86 `PageTable::leaf_entry`; rv64 (`rv64/paging.rs:127`) and aarch64 (`aarch64/paging.rs:241`) have one.
- **`init`** returns `self` as `PhysAddr`, which under HHDM is a VA (387–388).

**Two x86 paths today.**
- Private roots (`AddressSpace`) already use the trait: `address_space.rs:1649-1662`, after `phys_to_virt`.
- `KERNEL_ROOT` (SAS) uses raw `walk_create`/`walk_read`:
  - `paging.rs`:
    - `init_kernel_paging_x86` 456–570;
    - `map_page_x86(_unflushed)` 620–698;
    - `map_mmio_x86*` 702–756;
    - `unmap_page_x86` 759–800;
    - `mapping_state` 915–931;
    - `map_page`/`map_page_deferred` 1011–1055;
    - `unmap_page` 1088–1128;
    - `classify_table_frame` 1172;
    - `map_display_framebuffer_user` 1345–1402;
    - `remap_range_user` 1410–1420;
    - `translation_probe` 1488–1498;
    - `virt_to_phys` 1550–1566.
  - `page_protect.rs`: `protect_page` 87–95, `rewrite_leaf_x86` 99–131, `protect_pages` 153–168.
  - `prune_empty_tables` (1144–1157) already uses the HAL's `prune_empty`.

The two paths already disagree on the intermediate U bit, and the private path cannot express PCD.

**What routing `KERNEL_ROOT` through the trait needs:**
1. A cache-attribute input: map `DEVICE` → PCD, or add `UNCACHED`/`WRITE_THROUGH`. `libs/types` is untouched: `PageFlags` lives in `hal/traits/paging`.
2. A "rewrite permissions, keep frame and cache bits" operation.
3. `PTE_ADDR_MASK` in the trait.
4. One intermediate-U policy.
5. Flush stays in the kernel; the trait's local `invlpg` in `unmap` is redundant under shootdown.
6. Huge-leaf support, or leave `map_bios_area` as a HAL-internal raw path.

### 3b. Walkers and current-root readers

**Walkers in `task/user_copy/**`** live only in `sv39_probe.rs`:
- `sv39_leaf(root_pa, va)` (18–96) has three per-arch walkers:
  - rv64: raw Sv39, 3 levels;
  - aarch64: through `hal::PageTable::leaf_entry`, AP decoded into SV39_* bits;
  - x86: raw, 4 levels.
- `current_satp_root()` (99–145) reads satp/ttbr0/cr3 with inline asm.
- **Callers:**
  - `sv39_probe.rs:177,186,221-225`;
  - `copy.rs:126,146`;
  - `sas-lease.rs:116-118`;
  - `copy_glue/mod.rs:319-327`, outside `user_copy`, through the re-export `user_copy/mod.rs:74`.
- The "Sv39" vocabulary (`SV39_*`, `sv39_probe.rs:10-13`) is the neutral bit set for all three arches. `copy_glue/mod.rs:320-322` duplicates it.

**Other current-root readers:**
- `paging.rs` `translation_probe`: rv64 satp 1463, aarch64 ttbr0 1475; x86 uses `KERNEL_ROOT` at 1488.
- `paging.rs` `virt_to_phys`: rv64 1523, aarch64 1537; x86 uses `KERNEL_ROOT` at 1550.
- `p01_topology.rs:387` (cr3).
- HAL aarch64 `domain::current_root` (`aarch64/domain.rs:83`), used by `domain_switch_tests.rs:283,678,697,820`.
- HAL x86 `domain::read_cr3` (`x86_64/domain.rs:173`), used by `paging.rs:605` and `domain_switch_tests.rs:422`.
- HAL x86 `paging::read_cr3` (`x86_64/paging.rs:122`), used by `svm_registry.rs:270` and `paging.rs:487`.
- HAL x86 `syscall::current_user_cr3` (`syscall.rs:130`), used by `domain_switch_tests.rs:418`.
- rv64 has **no** HAL root reader (`rv64/domain.rs` exports only `kernel_satp`, `activate_address_space` and flushes).

**What moving `leaf(root, va)` and `current_root()` into the HAL needs:**
- **Physical-memory access.** A HAL walker needs `phys_to_virt`. x86 has an HHDM offset in the HAL (`x86_64/paging.rs:30`); rv64 and aarch64 assume identity today (`page_protect.rs:61-63`). Give the HAL one `phys_to_virt` hook, or put the offset in each HAL.
- **A neutral leaf type** `Leaf { pa, valid, read, write, user, exec }` replaces the `SV39_*` masks.
- **x86 `PageTable::leaf_entry`** for parity.
- **`current_root()` on all three arches**, with one defined meaning:
  - "live hardware root": on x86 in kernel context this is always the kernel CR3, because the switch never writes CR3. So `copy.rs:126`'s `current_satp_root() == Some(root_pa)` can never be true on x86. [INFERENCE]
  - "user-entry root": `gs:[24]`.
- **Renames:** `sv39_probe.rs` → `page_probe.rs`, `sv39_leaf` → `leaf`, `current_satp_root` → `current_root`.

## 4. `kmain` (`main.rs:175-1818`; 138 `target_arch` lines, 129 `#[cfg`)

### 4a. Blocks in order

| L | rv64 | aarch64 | x86_64 | 32-bit |
|---|---|---|---|---|
| 176–181 | `set_boot_physical_hart`; `effective_dtb`, `cpu_features::detect` | same, no hart | same | |
| 183–187 | `platform::init(dtb)`; PLIC base | `platform::init(dtb)` | — | |
| 189–235 | `dev::uart::init` | `Board::console_init` | 16550 configure+init from `X86_EARLY_FACTS`, `dev::uart::init` (196–233) | uart init |
| 240–250 | | | HHDM → apic, frame, paging, kaslr | |
| 255–283 | `hal::ARCH.init()` | `ARCH.init()`: CFI, MTE, **IRQ controller, timer**, vectors (`aarch64.rs:100-112`) | sse, IST guards, GDT, IDT, CET, PKU, syscall, protect, cpu policy | `ARCH.init()` |
| 326–375 | boot info, memory map, frame allocator (neutral) | | | |
| 382–400 | | | publish `FRAME_ALLOCATOR` | same |
| 409–491 | | | ACPI + SMBIOS parse, `acpi::sci::init` | |
| 493–520 | | | `PlatformIdentity`, `choose_x86_profile`/`bind_x86_profile`, console select | |
| 522–560 | | | board-gated IOAPIC/LAPIC/HPET/ECAM bases, `x86_interrupts_ready` | |
| 567–607 | `init_kernel_paging`, `activate_paging`, `KERNEL_ROOT`, SUM on | same + `record_kernel_ttbr0` | | |
| 608–770 | | | ECAM base set; `init_kernel_paging_x86` (maps IOAPIC/HPET/LAPIC/ECAM PCD); activate; LAPIC/IOAPIC/`register_x86_ioapics`; HPET; `offer_x86_clock_candidates`; `init_timers` → `x86_timer_ready`; VMX/SVM root | |
| 772–776 | | | | `init_bare` |
| 781–812 | heap + rt_heap (neutral) | | | |
| 816–826 | PLIC init (`board::active()` gate) | | | |
| 831 | `dev::uart::init` (log backend) | same | same | |
| 832–856 | | EL2 monitor probe | | |
| 864–880 | snapshot scratch; `uart::init_input` | DTB RAM log; console RX IRQ **after heap** (877) | | |
| 883–884 | `dev::init()` (VirtIO MMIO) | `dev::init()` (VirtIO MMIO, **DMA with no IOMMU**) | | |
| 887–931 | | | ramdisk, PS/2, UART RX IRQ (needs `x86_timer_ready`, 894) | |
| 938–972 | `iommu::set_deferred_init_pending` + `activate_isolation` (no-op) | — | `pcie_ecam::init` → `iommu::init` (VT-d) → `activate_isolation` → `pcie_msi::init` | |
| 976–997 | snapshot restore, MBR, early loader | same | — | |
| 1001–1029 | `fs::init`, `system_config`, `policy` (triple-`any` only), `state_stash` | | | |
| 1032–1170 | `task::init` and self-tests (neutral) | | | |
| 1174–1179 | `smp::start_secondaries` (after `task::init`) | same | same | |
| 1199–1306 | supervisor registry; rv64 CLINT/PLIC/UART/RTC spans (1268–1306) | Board `SUPERVISOR_DEVICE_MMIO` (1259–1267) | — | |
| 1408–1418 | | EL2 check | | |
| 1459–1680 | init spawn: `ed25519`, trust, fixtures; rv64 Platform Cell (1620) | | framebuffer map + `fb_console` (1569–1612); Platform Cell with ECAM argv (1629–1645) | — |
| 1731–1742 | `csrs sstatus` | `msr daifclr` | `ARCH.enable_interrupts` | same |
| 1787–1817 | | idle probes | | idle loop |

### 4b. Ordering constraints today (enforced only by position)

1. **DTB before platform and console.** `platform::init` → `uart::init` → `ARCH.init` (`platform.rs:7`).
2. **x86 HHDM before descriptor tables.** The HHDM offset (240–250) comes before GDT/IDT and before any `phys_to_virt`.
3. **aarch64 IRQ controller before timer**, inside `ARCH.init` (`aarch64.rs:109-112`), before the heap.
4. **x86 ACPI → board bind → MMIO bases → paging (MMIO PCD leaves) → IRQ (LAPIC/IOAPIC) → clock (`init_timers`)** (409–724). UART RX IRQ needs the clock gate (894).
5. **Heap before** PLIC init, console RX IRQ (877) and `dev::init`.
6. **VT-d before any DMA-capable Driver Cell:** `iommu::init` + `activate_isolation` (947–952) come after `pcie_ecam::init` (946) and before `pcie_msi::init` (954) and the Platform/Driver Cell spawns (1629+).
   - rv64 defers activation to `iommu::try_deferred_init` (964–971).
   - aarch64 has no IOMMU: `dev::init()` (884) starts VirtIO MMIO DMA.
7. **`task::init` (1033) before `start_secondaries` (1179)**, per the comment at 1172–1173.
8. **Interrupt enable (1731–1742) after the init spawn.**

### 4c. `PlatformReady`

The heap (781–812) sits **between** the arch paging step and the arch IRQ/PCI step, and x86 ACPI facts are needed after it (947–958, 1630–1635). One call before the neutral tail therefore cannot cover today's order. Split it in two:

```rust
arch::boot::early(dtb) -> EarlyPlatform { console, kernel_root, frames, facts /* x86: AcpiInfo, bases */ }
// neutral: heap, rt_heap (781-812)
arch::boot::platform_init(early) -> PlatformReady {
    console: ConsoleReady,          // log backend + RX path (831, 873-880, 887-931)
    irq:     IrqReady,              // PLIC / GIC (from ARCH.init) / LAPIC+IOAPIC registered
    clock:   ClockReady,            // x86 init_timers outcome (x86_timer_ready); rv64/aarch64 tick source
    pci:     Option<PciReady>,      // x86 { ecam_base, len, bus_start, bus_end } for the Platform Cell argv (1630-1635); rv64 deferred; aarch64 None
    iommu:   IommuReady,            // REQUIRED: VtdActive | Deferred (rv64) | Absent { reason } (aarch64)
    storage: BootStorage,           // VirtioMmio (rv64/aarch64: 976-997) | None (x86)
}
```

**Where the neutral tail starts.** It starts at `main.rs:1001` (`fs::init`), which is gated only by the triple-`any` idiom. Lines 974–997 should become `match ready.storage`. These residual arch hooks stay inside the tail:
- supervisor MMIO spans (1259–1306) → `arch::boot::supervisor_device_spans()`;
- EL2 check (1408–1418);
- x86 framebuffer (1569–1612);
- Platform Cell argv (1615–1645) → `ready.pci`;
- IRQ enable (1731–1742) → `arch::irq::enable()`;
- aarch64 idle probes (1787–1813).

## 5. Board selection

### 5a. Models and accessors (`kernel/src/board.rs`)

| Model | Arch | Selection | Accessors |
|---|---|---|---|
| compile-time feature const | riscv64 | `board-pioneer` / `board-vf2` / default qemu (8–23) | `selected_riscv64_board()` const (27), `selected()` validated (33), `active()` → `selected()` (41), `selected_riscv64_soc()` (47) |
| compile-time from HAL backend | aarch64 | `hal::aarch64::backend::Board::SOC` or `board-aarch64-vm` (455–470) | `default_aarch64_board()` const (493), `active()` validated (499) |
| runtime cmdline + ACPI/SMBIOS identity | x86_64 | `choose_x86_profile` (303) / `bind_x86_profile` (325) into `BOUND_X86_PROFILE` | `selected()` (panics before bind, 137), `selected_x86_64_soc()` (146), `x86_machine_inputs()` (126) |
| DTB → `PlatformInfo` | rv64, aarch64 | `platform::init` (`platform.rs:139,161`; x86 no-op 194) | `platform::with` (197), `riscv_plic_*` (204, 211), `virtio_mmio_base_for_irq` (218) |
| HAL backend type | aarch64 | hal-core features `board-rpi3/rpi4/apple-t8103/aarch64-vm` (`hal/core/Cargo.toml:41-44`) | `hal::backend::Board` (`main.rs:192,876,1260,1788-1799`, `dev/uart.rs`, `dev/console_drv.rs`) |

**Callers:**
- `selected()`: `boot.rs:703`, `dev/iommu.rs:147`, `dev/iommu_x86.rs:76`, `main.rs:523,535,541`, `platform.rs:267`.
- `active()`: `dev.rs:80`, `dev/mmc.rs:144`, `main.rs:817`, `platform.rs:163`.
- `selected_riscv64_board()`: `boot.rs:291`, in a const.
- `default_aarch64_board()`: `boot.rs:369`, in a const.
- `selected_riscv64_soc()`: `platform.rs:140,214`.
- `selected_x86_64_soc()`: `main.rs:905`, `task/syscall/mod.rs:497`.
- `x86_machine_inputs()`: `machine_facts.rs:20`, `task/syscall/tests.rs:1188`.

The same "selected" name means "validated const" on rv64 but "runtime-bound, panics early" on x86.

### 5b. Proposal

- **One `board::active() -> &'static BoardDescriptor` on all three arches.**
  - rv64 and aarch64: the validated const.
  - x86: the bound profile; it panics before binding, as `selected()` does today.
- **One `const fn board::compiled()`** for rv64 and aarch64 only, for the two const sites (`boot.rs:291,369`).
- **`soc()` moves to `arch::<a>::board::soc()`**, replacing `selected_riscv64_soc` and `selected_x86_64_soc`.
- **`x86_machine_inputs()` stays**: syscall 433 must answer before binding.
- **Delete** `selected`, `selected_riscv64_board` and `default_aarch64_board`.

**`aarch64_catalog()`** (`boards/src/lib.rs:117`) has **no non-test caller**. Its only uses are `boards/src/catalog_tests.rs:205,218,242`, under `#[cfg(test)]` (`lib.rs:3-4`); `rg aarch64_catalog` over the whole repo finds nothing else. If it is deleted, those three tests must switch to the board statics. The per-board free functions (`boards/src/lib.rs:49-88`) are used only by `kernel/src/board.rs:697-698` (tests).

## 6. Dead HAL traits

| Trait | Defined | Implementors | Users | Cargo edges |
|---|---|---|---|---|
| `Timer` | `hal/traits/timer/src/lib.rs` | **none** (`rg 'impl\s+(\w+::)*Timer\s+for'` empty) | none (other `Timer` hits are comments and enum variants) | `hal/core/Cargo.toml:12`, `hal/arch/riscv/Cargo.toml:10`; re-export `hal/core/src/lib.rs:17`; workspace member `Cargo.toml` (`"hal/traits/timer"`) |
| `InterruptController` | `hal/traits/interrupt/src/lib.rs` | none | none | `hal/core/Cargo.toml:13`, `hal/arch/riscv/Cargo.toml:11`; re-export `lib.rs:15`; workspace member `"hal/traits/interrupt"` |
| `BootController` | `hal/traits/arch/src/lib.rs:32-37` | none | none | lives in `hal-arch-trait`, which stays: `Arch` and `kernel_abi` are used |
| `ViHypervisor` | `hal/traits/hypervisor/src/lib.rs:69-85` + `ViVmStub`/`ViVcpuStub`/`ViStage2TableStub` (89–93) | ENOSYS stubs `AArch64Hypervisor` (`aarch64.rs:185-220`), `RiscV64Hypervisor` (`hal/arch/riscv/src/hypervisor.rs`), `X86_64Hypervisor` (`hal/arch/x86/src/hypervisor.rs`) | the three stub types are re-exported (`riscv/lib.rs:32`, `x86/lib.rs:20`) and never named by the kernel | **`hal-hypervisor` cannot be deleted**: its `ViVmExit` is live (`aarch64/trap_el2.rs:12`, `aarch64/vcpu.rs:54`, `x86_64/{svm_vcpu,vmexit_decode,vmx_vcpu,vmx_vcpu/mmio_access}.rs`, `kernel/src/hypervisor/{registry.rs:44,554, smoke_guest.rs:19, svm_registry.rs:23}` via `hal::ViVmExit`). Delete only the trait, the stubs and the three `hypervisor.rs`/`impl` blocks |

**What deletion requires:**
- **Workspace.** Drop `"hal/traits/timer"` and `"hal/traits/interrupt"` from `Cargo.toml` members, and delete the two crates.
- **Dependencies.** Remove `hal-timer`/`hal-interrupt` from `hal/core/Cargo.toml:12-13` and `hal/arch/riscv/Cargo.toml:10-11`, and the `pub use` at `hal/core/src/lib.rs:15,17`.
- **`BootController`.** Remove it from `hal/traits/arch/src/lib.rs`.
- **`ViHypervisor`.** Remove the trait and stubs from `hal/traits/hypervisor/src/lib.rs`, the files `hal/arch/{riscv,x86}/src/hypervisor.rs` with their `lib.rs` mod/use lines (`riscv/lib.rs:26-32`, `x86/lib.rs:15-20`), and the aarch64 block `aarch64.rs:185-220`.
- **`Cargo.lock`** updates.

No `cells/` crate depends on any of these: `cells/` uses `abi::hypervisor::ViVmExit` (`libs/abi/src/hypervisor.rs:36`).

## 7. 32-bit code

**`kernel/src`.** 123 lines in 25 files (`rg -c 'target_arch\s*=\s*"(riscv32|arm|x86)"' kernel/src`):
- `main.rs` 24, `sched/hart_local.rs` 15, `task.rs` 13;
- `dev/console_drv.rs` 7;
- `memory/paging.rs` 6, `boot.rs` 6;
- `task/syscall/mod.rs`, `task/syscall/dev.rs`, `task/stack.rs`, `dev/uart.rs` 5 each;
- `loader/va_alloc.rs`, `cell/init_death.rs` 4 each;
- `snapshot_engine.rs`, `sched/hart_local/ready.rs` 3 each;
- `task/syscall/grant.rs`, `memory/tlb_shootdown.rs`, `memory/kaslr.rs`, `loader/wx.rs`, `loader/image-va.rs`, `dev/ramdisk.rs`, `dev/iommu_riscv.rs` 2 each;
- `task/syscall/ipc.rs`, `sched/smp.rs`, `memory/page_protect.rs`, `dev.rs` 1 each.

Also `target_pointer_width = "32"` at `main.rs:783` and `loader/va_alloc.rs`.

**`kernel/` outside `src`:**
- `kernel/Cargo.toml:28-34,55-59` (riscv32/arm/x86 dependency tables);
- linker scripts `kernel/linker-riscv32.ld`, `linker-aarch32.ld`, `linker-x86-32.ld` (`build.rs:69-71`);
- `kernel/x86_32-unknown-none.json`.

**`hal/`:**
- `rv32.rs` 12, `x86_32.rs` 9, `aarch32.rs` 7, `traits/arch/src/kernel_abi.rs` 4, `riscv/lib.rs` 3, `core/lib.rs` 2 (+7 `feature = "riscv32"|"arm"|"x86"`), `riscv/common/timer.rs` 2, `x86_32/uart_16550.rs`, `x86/lib.rs`, `riscv/common/sbi.rs` and `arm/lib.rs` 1 each.
- Modules `hal/arch/riscv/src/rv32/**`, `hal/arch/arm/src/aarch32/**` and `hal/arch/x86/src/x86_32/**`: 1 336 lines including the top files.
- hal-core features `riscv32`, `arm`, `x86` (`hal/core/Cargo.toml:28-32`).

**Other:**
- `run-rv32-virt.ps1:17-22`.
- Placeholder boards `boards/qemu/{q35-x86_32,virt-riscv32,virt-aarch32}` (README only; `scripts/check-board-configs.sh:81-101` forbids registering them).

### Can a 32-bit target build today? No.

- **Toolchain.** `rust-toolchain.toml:4` lists only the three 64-bit targets. Locally `riscv32imac-unknown-none-elf` is installed; arm and x86-32 need `-Z build-std` (`rust-src` is present).
- **CI.** No CI or script lane builds a 32-bit kernel (`rg 'riscv32|aarch32|x86_32|armv7' .github` is empty).

Run (`--features compile-only`, `CARGO_TARGET_DIR=/tmp/...`):

| Target | Command | Result |
|---|---|---|
| riscv32 | `cargo check -p cellos-kernel --target riscv32imac-unknown-none-elf --features compile-only` | **17 errors** (also with `--no-default-features`). `AtomicU64` missing (`loader/domain_admission.rs:18`, `task/syscall/dma_export.rs:37`, `task/telemetry.rs:30`, `task.rs:384`); `cellos_boards` unresolved (`dev/iommu.rs:10`); `hal::domain` missing (`memory/tlb_shootdown.rs:223`); `hal::arch::{save_and_disable_interrupts, restore_sstatus}` missing (`audit.rs:235-380`, 6); `ipc/futex.rs:194,197` types; `memory/cell_quota.rs:70-74` 64-bit packing const-assert |
| arm | `cargo check … --target armv7a-none-eabi -Z build-std=core,alloc --features compile-only` | **13 errors**: same families (`dev/iommu.rs:10`, `tlb_shootdown.rs:223,454`, `audit.rs` ×6, `cell_quota.rs:70-71`, `task/syscall/mod.rs:2110`, `task.rs:1905`) |
| x86 (32) | `cargo check … --target kernel/x86_32-unknown-none.json -Z build-std=core,alloc -Z json-target-spec --features compile-only` | stops in **`libs/abi`**: `libs/abi/src/hypervisor.rs:87` `size_of::<ViVmExit>() == 80` fails. Building would need an ABI change (Law-1) |

So "build-check only" (handoff §6 Q3) is not available today. It would first need:
- for riscv32 and arm, repairs to about 30 shared-code sites, including one design-level fact: `cell_quota` packs into 64 bits;
- for x86-32, an `libs/abi` change, which the handoff says S7 does not make.

That is an owner question: build-check means repairing first; "quarantine without build" means accepting code that does not compile.

### Proposed `arch/legacy` layout

- `kernel/src/arch/legacy/{mod.rs, riscv32.rs, arm.rs, x86.rs}`. Each implements the same `arch::*` function set as stubs returning `NotSupported`, matching today's bare-physical stubs (`paging.rs:1423-1440`, `page_protect.rs:246-251`).
- `arch/mod.rs` selects with `#[cfg(not(cellos_wide))] mod legacy;`.
- 32-bit-only arms in shared files go away when `cellos_wide` replaces the triple-`any` blocks (§1c). Shared-file 32-bit branches that are not stubs (`sched/hart_local.rs` 15, `task.rs` 13, `dev/console_drv.rs` 7) move into `legacy`.
- HAL 32-bit modules stay where they are; they are not kernel code.

## 8. Default arch

| Where | Today | Who relies on it |
|---|---|---|
| `hal/core/Cargo.toml:26` | `default = ["riscv64"]` | **Not the kernel**: every `kernel/Cargo.toml` hal dependency sets `default-features = false` (29, 37, 44, 51, 56, 59), and no other crate depends on hal-core (`rg 'hal/core' -g Cargo.toml`). Only `--workspace` commands select hal-core as a member and so get its defaults: RV64 clippy `ci.yml:222-233` and x86 clippy `ci.yml:1004-1017`. aarch64 clippy is `-p cellos-kernel` (`ci.yml:628-634`) |
| `kernel/build.rs:63-72` | linker match: riscv64 has no arm and falls to `_ => kernel/linker.ld` | rv64 builds |
| `kernel/build.rs:188-235` | embedded fallback: `src/embedded-<arch>` else `src/embedded`. There is no `embedded-riscv64`, so `src/embedded` is the rv64 image | deprecated fallback (prints a warning) |
| `scripts/cargo-config-linux.toml:17` → gitignored `.cargo/config.toml` | `build.target = "riscv64gc-unknown-none-elf"` | `gen_disk.ps1` and other target-less `cargo build`s; CI mirrors it with `CARGO_BUILD_TARGET` (`ci.yml:2356-2362`) |

**Effect of flipping `hal/core` default to `x86_64`:**
- **riscv64 workspace clippy** (`ci.yml:222`): features unify, so hal-core gets `x86_64` + `riscv64` and `hal-x86` is checked for riscv64. `hal-x86` gates its x86 code on `target_arch` (`x86/src/lib.rs:10-20`, `x86_64.rs:4-14`; `critical_section.rs:17`). [INFERENCE] Clean; needs one RV64 workspace clippy run to confirm.
- **x86 workspace clippy** (`ci.yml:1005`): it stops checking `hal-riscv` for x86. Today `hal-riscv`'s `critical_section.rs:25-28` is *not* target-gated, and it passes only because `check` does not assemble.
- **Kernel images:** no effect.
- **Docs** that say "default riscv64" need the change.

**Effect of flipping `kernel/build.rs`.** Make riscv64 explicit (`"riscv64" => linker.ld`) and turn `_` into an error. `EMBEDDED_OVERRIDE` users are unaffected.

**Not in the plan's file list.** The `build.target` template default (`scripts/cargo-config-linux.toml:17`, mirrored at `ci.yml:2362`) is what makes a bare `cargo build` rv64. Flipping it would break `gen_disk.ps1` and the target-less builds that the CI comment at `ci.yml:2359-2361` describes.

## 9. Checkpoint lanes (exact commands)

**x86 domain, 1/2/4 CPU.** Registered `pr` in `scripts/lanes.toml:73-76,381-384`. CI runs 1 CPU only (`ci.yml:1363-1367`).

```
bash scripts/build-x86_64-domain-test-ci.sh
X86_CPUS=1 bash scripts/x86/qemu-domain-test.sh target/x86-domain-test/x86_64-unknown-none/release/cellos-kernel-domain-test
X86_CPUS=2 bash scripts/x86/qemu-domain-test.sh target/x86-domain-test/x86_64-unknown-none/release/cellos-kernel-domain-test
X86_CPUS=4 bash scripts/x86/qemu-domain-test.sh target/x86-domain-test/x86_64-unknown-none/release/cellos-kernel-domain-test
```

`X86_CPUS` defaults to 1 and is bounded 1–64 (`scripts/x86/qemu-domain-test.sh:44,64-65`). The log label is `x86-domain-test-cpu$X86_CPUS` (77).

**RV64 `--harts 2`.** Manual tier (`lanes.toml:24-27,199-202`). `--case` is required (`qemu-native-domain-test.sh:102`); cases are listed at 22–24, and `migration`/`rt-wake` require 2 harts (115–121).

```
bash scripts/build-native-domain-test-ci.sh      # → target/riscv64gc-unknown-none-elf/release/cellos-kernel-native-domain-test
bash scripts/qemu-native-domain-test.sh --harts 2 --case switch,resume-root,sas-fastpath,migration,user-copy,user-copy-race,admission,rollback,grant-revoke,grant-gate,grant-pair,asid-lease,unmap-order,rt-wake,park
```

At Phase 10 this lane failed on the base tree too ("kernel stack overflow: trap entry could not save a frame" after `S22-RV64-SWITCH: PASS`, `phase-10-lock-ranks-scheduler.md:132,200`). No later phase records it green. **Establish today's baseline before step 4.**

**AArch64 `virt-vhe`, 2 CPUs.** `pr` in CI (`ci.yml:756-759`). `LANE=virt-vhe` uses `virt,virtualization=on,gic-version=2`, cortex-a76, `-smp 2` (`scripts/qemu-aarch64-test.sh:50-55`), with markers "EL2 VHE", "OS ready" and "[smp] hart 1 online" (142–161). Steps per `ci.yml:700-759`:

```
# cells into kernel/src/embedded-aarch64 (ci.yml:700-726), then:
EMBEDDED_OVERRIDE=kernel/src/embedded-aarch64 cargo build --release -p cellos-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc
bash scripts/format-disk-arm.sh disk_arm_virt.img
bash scripts/build-aarch64-image.sh
LANE=virt-vhe BOOT_WINDOW=120 bash scripts/qemu-aarch64-test.sh target/aarch64-unknown-none-softfloat/release/cellos-kernel.Image
```

**Related lanes:**
- AArch64 test-hooks (`scripts/qemu-aarch64-test-hooks.sh`, manual, `lanes.toml:101-104`) failed at Phase 10 on `S22-AARCH64-DOMAIN-LIVE: FAIL asid=0` (`phase-10…md:133`). It is the lane that carries the aarch64 domain-switch fixtures.
- The x86 SMP boot check `CELLOS_X86_SMP=4 cargo test --manifest-path tests/integration/Cargo.toml --test x86_64-boot --target x86_64-unknown-linux-gnu -- x86_smp_brings_every_cpu_online` (`ci.yml:1095-1101`) is a cheap extra 4-CPU switch witness.

## Risks and surprises

1. **The switch fixture is weak on rv64 and aarch64.** The RV64 `--harts 2` and AArch64 test-hooks lanes were red at Phase 10 (`phase-10…md:132-133,200`), and those are the lanes that exercise the rv64/aarch64 domain switch. Run a baseline before step 4.
2. **x86 never writes CR3 in the context switch.** It writes only the `gs:[24]` user-entry scratch (`x86_64/context.rs:173-183`), and a null root there means "inherit". Its doc comment (`context.rs:112-121`, "written after the outgoing context is stored") does not match the asm. `SwitchPlan`'s `root` must document per-arch meaning. x86 `current_satp_root()` (CR3) can never equal a private root in kernel context (§3b).
3. **The bodies differ beyond root/ASID** (`note_task_dispatch`, plan side effects). Body B is reachable only on 32-bit or `compile-only` (`posture.rs:57-64`), so deleting it changes nothing bootable.
4. **Name collisions:**
   - The kernel already has `sched::domain_switch::SwitchPlan` (`domain_switch.rs:42`).
   - `crate::hal::arch` (107 uses) will sit next to the new `crate::arch`.
   - `main.rs:105` carries the stale comment `// pub mod arch; // Moved to HAL`.
5. **The function set does not cover all `asm!`.** 29 sites are tlb, cpu/stack, hv, pio and selftest (§1d). Without extra sets, "no `asm!` outside `arch/`" fails.
6. **148 arch-only HAL paths have no arch in their name** (`hal::sbi`, `hal::uart_16550`, `hal::backend`, …). The `hal::<arch>` grep and the ratchet do not see them.
7. **The triple-`any` idiom is 26 % of the baseline.** A `cellos_wide` cfg removes it mechanically. Decide whether that counts toward "≥ half" before setting step targets.
8. **The x86 page-table split is already a correctness gap.** Private roots use the trait (`address_space.rs:1649`); SAS uses raw walkers. The trait masks addresses with `& !0xFFF` (keeps NX), has no PCD, and x86 always sets P. `protect_page` without `VALID` unmaps on rv64 but not on x86.
9. **32-bit is not "build-check only"-able:**
   - riscv32 has 17 errors;
   - arm has 13;
   - x86-32 needs a `libs/abi` change.

   Decision 4 plus handoff Q3 need re-asking with this fact.
10. **`aarch64_catalog()` is test-only.** Deleting it also deletes three catalog tests' lookup path (`catalog_tests.rs:205-242`).
11. **`hal-hypervisor` must stay** for `ViVmExit`. Only the trait and stubs go.
12. **The default-arch flip mainly changes which foreign HAL crate `--workspace` clippy checks.** The real "default" is the gitignored `build.target` template, which is outside the plan's file list.
13. **The aarch64 board model lives in two crates.** The kernel const (`board.rs:455-470`) is derived from hal-arm compile features (`hal/core/Cargo.toml:41-44`), so `board::active()` on aarch64 stays compile-time. Only x86 is runtime.
14. **aarch64 starts VirtIO MMIO DMA with no IOMMU** (`main.rs:884`). `PlatformReady.iommu` must allow `Absent`, or aarch64 cannot construct it.
