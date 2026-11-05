## Slice: HalArmRiscv — files read: 43 — unsafe blocks reviewed: ~45

### Findings

#### [S-1] Non-VHE EL2 host runs EL0 Cells with stage-1 translation disabled (flat physical access) — Severity: Critical — STRIDE: E/T/I — OWASP: A01
- Where: hal/arch/arm/src/aarch64/boot.rs:204-215 (HCR_EL2 = RW|TGE, E2H clear unless `host_vhe`); hal/arch/arm/src/aarch64/backend/qemu_virt.rs:23-26 (default `HostRegime::El2NonVhe`); hal/arch/arm/src/aarch64/el2.rs:203-244 (TTBR0_EL1/TCR_EL1 "mirror" for EL0 Cells); hal/arch/arm/src/aarch64/paging.rs:63-74
- What: With `HCR_EL2.{E2H,TGE} = {0,1}` the architecture treats `SCTLR_EL1.M` as 0 for the EL1&0 regime (ARM ARM, HCR_EL2.TGE; QEMU implements it in `regime_translation_disabled`, patch "Treat SCTLR_EL1.M as if it were zero when HCR_EL2.TGE is set", https://lists.nongnu.org/archive/html/qemu-devel/2018-07/msg04651.html). The EL1 mirror in `el2_mmu_init` and every AP/UXN/PXN bit in `leaf_word` are therefore inert for EL0 on this host: Cells execute with VA=PA and no permission checks (stage 2 is also off outside guest runs, vcpu.rs:853-867). The comment at el2.rs:203-207 ("EL0 translations still use TTBR0_EL1/TCR_EL1") is wrong under TGE=1. Not executed here; architectural + QEMU-source fact.
- Exploit: Any EL0 Cell on QEMU virt `virtualization=on` with the default regime does `str x0, [kernel_phys_addr]` (kernel text, page tables, `VI_KERNEL_TTBR0`, GICD at 0x0800_0000) — no fault, full kernel compromise. W^X, private roots and the MMIO allowlist are all bypassed.
- Fix: Make `El2Vhe` the only EL2 host regime (EL2&0 regime, EL0 translates through TTBR0_EL2 with AP bits honoured) and refuse boot on CPUs without FEAT_VHE; or drop to an EL1 host with a private EL2 monitor as Pi does (`PI_MONITOR_BOOT`). Delete the EL1 mirror path.

#### [S-2] Entire BCM2837 peripheral aperture (DMA engine, IRQ controller, timer, EMMC, USB) is USER-mapped in the shared kernel root; MMIO allowlist is advisory only — Severity: High — STRIDE: E/T/D — OWASP: A01
- Where: hal/arch/arm/src/aarch64/backend/bcm2837.rs:199-210 (`0x3F00_0000..0x4000_0000`, `cell_access: true`); kernel/src/memory/paging.rs:273-291 (`cell_access` → `PageFlags::USER` in `KERNEL_ROOT`); kernel/src/resource_registry.rs:89-115 (allowlist of five 4 KiB windows); hal/soc/bcm27xx/src/profile.rs:52-73; also hal/arch/arm/src/aarch64/backend/bcm2711.rs:154-157 (SDHCI window USER on Pi 4)
- What: The hardware mapping grants EL0 read/write to the whole 16 MiB BCM2835 peripheral space; `request_mmio` only records ownership. That space contains the DMA controller (0x3F007000, channels address physical memory with no IOMMU), the legacy IRQ controller (0x3F00B200, used for the kernel tick: bcm2835_systimer.rs:24-27), the system timer (0x3F003000), EMMC (0x3F300000) and DWC2 (0x3F980000). The mailbox "grant" at 0x3F00B880 is not even page-aligned, so its page also exposes the IRQ controller and ARM timer. On BCM2711 the EMMC2 (ADMA-capable) window is USER for every cell. QEMU virt's whole virtio region is USER too (qemu_virt.rs:193-205) — that one is documented in security-model.md:181-188; the Pi case is not.
- Exploit: Any Cell that issues raw MMIO (FFI/unsafe-allowlisted driver cell, or any compromised cell) programs a DMA control block with DEST = kernel page table / `.text` → kernel compromise; or clears IRQ_ENABLE1 bit 1 → kernel never ticks (DoS); or re-flashes the SD card.
- Fix: Map only the allowlisted windows with USER (page-aligned, each in its own page), keep DMA/IRQ/timer/EMMC/USB kernel-only; move the mailbox window so it does not share a page with the interrupt controller; make `request_mmio` the thing that installs the USER PTE (per-cell on Tier 2 roots, SAS otherwise) rather than a bookkeeping entry.

#### [S-3] EL1-host vectors never set the lower-EL marker → any Cell undefined-instruction/BRK/alignment fault panics the kernel (Pi 3, Pi 4, QEMU virt without `virtualization=on`) — Severity: Medium — STRIDE: D — OWASP: A04
- Where: hal/arch/arm/src/aarch64/trap.rs:358-402 (`__vectors`: `vt_sync_sp0/spx/el0` share one trampoline, no marker store); trap.rs:132-177 (`from_lower_el = vector_kind != 0` → `panic!`); hal/arch/arm/src/aarch64/el2.rs:30-36 (marker is only written by `__vectors_el2`, el2.rs:372-391); backend/mod.rs:82-85 (Pi boards are `HostRegime::El1`)
- What: On the EL1 host `take_lower_vector_kind()` always returns 0, so every EC other than 0x15/0x20/0x24 is classified as a kernel trap and panics. ECs that EL0 can raise with the same encoding as EL1: 0x00 (udf, `hvc`/`msr sctlr_el1` from EL0), 0x0E, 0x22 (PC alignment), 0x26 (SP alignment), 0x2C (FP trap), 0x3C (`brk`). The RISC-V side uses SPP for this (rv64/trap.rs:175); the aarch64 EL1 path could use `SPSR_EL1.M[3:0] == 0`.
- Exploit: Cell executes `udf #0` or `brk #0` or branches to an odd PC → whole-kernel panic.
- Fix: In the EL1 vector table split lower-EL and current-EL trampolines (store kind 1..4 exactly as `mark_lower_vector` does) or derive origin from `frame.spsr_el1 & 0xF == 0` when `!is_el2()`.

#### [S-4] Fixed, source-visible PAC key shared by EL0 and EL1 — Severity: Medium — STRIDE: T/E — OWASP: A02
- Where: hal/arch/arm/src/aarch64/cfi.rs:109-137
- What: `APIAKEY_EL1` is loaded with constants 0x0123…/0xFEDC…. The key signs both kernel and Cell return addresses (EL1&0 regime). A known key lets an attacker with a stack-write primitive compute valid PACs offline; security-model.md:196-198 lists PAC-RET as a shipped backward-edge mitigation. Latent on current boards (no PAuth on A53/A55/A57 per aarch64.rs:145-150) but live on any PAuth CPU.
- Exploit: Kernel ROP on a PAuth-capable board: forge `pacia` signatures for gadget addresses with the public key.
- Fix: Derive the key from the kernel RNG at boot (per-core on secondaries, see aarch64.rs:145-150 note), never from a constant; drop the "Resolved" claim until done.

#### [S-5] Stale/global `AARCH64_EL2_VECTOR_KIND` misattributes host faults; racy across harts — Severity: Low — STRIDE: R/D — OWASP: A04
- Where: hal/arch/arm/src/aarch64/el2.rs:25-36, 372-391; trap.rs:78, 148-177; kernel/src/task/smp.rs:608-696 (PSCI secondaries on QEMU virt)
- What: The lower-EL IRQ vector stores kind=2 but `vi_aarch64_irq_handler` never consumes it; the next host-origin sync trap (e.g. a kernel data abort during a syscall, EC 0x25 not claimed by the copy guard) reads the stale 2, calls `vi_terminate_on_fault_aarch64` and kills the current Cell instead of reporting a kernel bug. With two harts, hart A's `swap(0)` can consume hart B's marker (B's genuine Cell fault then panics the kernel) or vice-versa.
- Fix: Make the marker per-hart (TPIDR-indexed) and have both sync and IRQ paths consume it; better, drop it and use `SPSR_EL2.M`.

#### [S-6] RV64 `PageTable::map` walks through an existing leaf as if it were a table — Severity: Low — STRIDE: - — OWASP: -
- Where: hal/arch/riscv/src/rv64/paging.rs:166-190 (no `is_leaf()` check at levels 2/1; `unmap` has it at :204); also :192-195 silently overwrites an existing leaf with no `sfence.vma`
- What: If a VPN[2]/VPN[1] slot already holds a megapage leaf, `entry.to_phys()` of that leaf becomes the "next table" and the 4 KiB write at :194 corrupts the mapped frame. No caller currently installs megapages [INFERENCE], so latent.
- Fix: Return `Err(ViError::AlreadyMapped)` when a non-last-level entry is a leaf; flush the page when replacing a valid leaf.

#### [S-7] AArch64 `unmap` lacks the pre-TLBI barrier and the EL2-regime invalidation that `flush_tlb_page` documents — Severity: Low — STRIDE: T — OWASP: -
- Where: hal/arch/arm/src/aarch64/paging.rs:262-285 vs :106-133
- What: `unmap` stores 0 with a plain write, then `tlbi vaae1is` (no `dsb ishst` before, no `isb` after, no `vae2is`). On the EL2 host the kernel's own EL2 TLB keeps the stale translation until some later full flush, so the kernel can keep touching a freed/reassigned frame through the old VA. Whether every kernel caller follows with `flush_tlb_page` is outside this slice [INFERENCE].
- Fix: Route `unmap` through `flush_tlb_page(virt)`.

#### [S-8] Private-root request is silently dropped on the EL2 host (debug-only assert) — Severity: Low — STRIDE: E — OWASP: A05
- Where: hal/arch/arm/src/aarch64/context.rs:137-144
- What: `switch_with_root` at EL2 ignores `root_baddr` after a `debug_assert_eq!`; in release a Tier-2 spawn that asked for a private root would run under the shared kernel root with no error. Combined with S-1 the EL2 host has no Tier-2 containment at all.
- Fix: Return an error / refuse the spawn path on EL2 hosts instead of a debug assert.

#### [S-9] QEMU-virt world switch does not save/restore FP/SIMD state — Severity: Low — STRIDE: I — OWASP: A02
- Where: hal/arch/arm/src/aarch64/vcpu.rs:642-689, 807-851 (`.if {pi}` only), :399-400 acknowledges
- What: On non-Pi backends guest `q0..q31`/FPCR/FPSR leak into the hypervisor Cell and the Cell's FP state into the guest. Information disclosure between trust domains and silent FP corruption of the Cell.
- Fix: Enable the SIMD save/restore unconditionally (or trap FP with CPTR_EL2.TFP and lazy-switch).

#### [S-10] Stage-2 enable trusts caller-only invariants; VTTBR_EL2 never reset after guest exit — Severity: Low — STRIDE: T — OWASP: -
- Where: hal/arch/arm/src/aarch64/stage2_regs.rs:57-61 (VMID≥1 and 8 KiB alignment are `debug_assert`); vcpu.rs:853-870 (exit clears VM only)
- What: In release a VMID 0 or misaligned root is programmed verbatim; after exit the host's EL1&0 regime keeps running tagged with the guest's VMID while guest stage-1 entries for that VMID stay cached [INFERENCE on TLB aliasing — guest kernel entries are EL1-only so a Cell hit yields a permission fault, not data]. The Pi `pi_monitor_dispatch` op 1/5 passes `regs[1]`/`regs[2]` from EL1 unchecked (monitor.rs:321-326, 345-356), acceptable only because EL1 is the kernel.
- Fix: Hard-check VMID/alignment in `monitor_enable_stage2`; restore `VTTBR_EL2 = 0` (or host VMID) plus `tlbi vmalls12e1is` on guest exit.

#### [S-11] Pi monitor frame restore omits x28/x29 — Severity: Info — STRIDE: - — OWASP: -
- Where: hal/arch/arm/src/aarch64/backend/monitor.rs:455-469 (saves `[sp,#224]` at :451, restores x0–x27 and x30 only)
- What: Correct today only because `pi_monitor_dispatch` is AAPCS and preserves x28/x29; a future non-ABI path would corrupt the EL1 host's frame pointer. Fix: `ldp x28, x29, [sp, #224]`.

#### [S-12] RV32 trap entry has no U/S-mode stack switch — Severity: Info — STRIDE: E — OWASP: -
- Where: hal/arch/riscv/src/rv32/asm/trap.S:16-24; kernel/src/task.rs:229-230 (RV32 tasks spawned with SPP=1)
- What: The Nano path runs all tasks in S-mode, so this is consistent, but the first U-mode task on RV32 would have its trap frame written to the user-controlled `sp`. Guard with a `const` assert/doc that RV32 has no U-mode, or add the sscratch protocol from rv64.

#### [S-13] AArch64 SVC bridge discards dispatcher-side `sepc`/second return value — Severity: Info — STRIDE: - — OWASP: -
- Where: hal/arch/arm/src/aarch64/trap.rs:34-46
- What: Only `vtf.regs[10]` is copied back; a dispatcher that rewrites `sepc` or returns in `a1` behaves differently on aarch64 than on RV64. Parity bug only.

#### [S-14] GICv2 SGI EOI drops the source-CPUID field — Severity: Low — STRIDE: D — OWASP: -
- Where: hal/arch/arm/src/aarch64/gic.rs:115-121 (`claim` masks `& 0x3FF`, `complete` writes the bare ID); trap.rs:197-210
- What: GICv2 requires EOIR for an SGI to carry the CPUID read from IAR[12:10]; an IPI from a non-zero source CPU is completed with CPUID 0, which can leave the SGI active on the receiver and block further IPIs (TLB-shootdown / retirement stalls) [INFERENCE on QEMU's handling].
- Fix: Keep the full IAR value and write it back to EOIR.

### Verified-safe areas
- RV64 sscratch protocol: U-mode entry swaps tp/sscratch, parks user sp/tp, zeroes sscratch, reloads kernel tp from `HART_LOCAL_TP_ADDRS`; exit re-arms sscratch only when SPP=0 (rv64/asm/trap.S:37-93, 240-253); S-mode nested frames and the overflow-stack path are per-hart and `MAX_HARTS == 2` matches the two stubs (smp.rs:29, rv64/trap.rs:50-62).
- No sigreturn-like path: `frame.sstatus`/`spsr` are only ever written from kernel constants (task.rs:201,210,223,3456,3472; scheduler.rs:1019); trap exit restores the hardware-captured SPSR/sstatus (trap.rs:456-458, trap.S:240-243).
- Interrupted root parked in frame and kernel root installed before Rust runs on both arches (trap.S:198-239; trap.rs:433-454); RV64 `__switch` programs satp between save and load with fence/sfence (switch.S:37-51).
- User `gp` on RV64 is not reloaded at trap entry, but the built kernel (`target/riscv64gc-unknown-none-elf/release/cellos-kernel`, 2026-11-04) contains no gp-relative loads/stores — only the two `addi gp` in `_start` — so it is harmless for this build (build-dependent: no `--no-relax`/`-mno-relax` is set, .cargo/config.toml:47).
- SP_EL0 is saved/restored on every switch (context.rs:198-232, el2.rs:272-290); SPSel=1 is forced before the stack is set on boot and secondary cores (boot.rs:293-305, 409-417); `__trap_exit` always erets with SPSR=0 (EL0t) (trap.rs:300-305, 327-336).
- Guest `SPSR_EL2` comes only from creation constant 0x3C5 or the hardware exit value (vcpu.rs:190, 805-806; kernel/src/hypervisor/registry.rs:1038 only clears bit 7); vGIC LRs mask VINTID and never set the HW bit (vgic.rs:103-107).
- Pi EL2 monitor: HVC only from EL1 (EL0 `hvc` is UNDEFINED), ESR EC/imm gate (monitor.rs:288-292), TPIDR_EL2 gate for guest traps (:418-432), identity-root attribute/WXN validation before EL2 MMU on (:65-130, 294-319), bounded Stage-2 probe walks (:28-59).
- Interrupt controllers are kernel-only in the shared root: GIC on qemu_virt/bcm2711 and BCM2836 local controller (qemu_virt.rs:190-191; bcm2711.rs:159-171; bcm2837.rs:211-219); PLIC/CLINT on RV64 (kernel/src/memory/paging.rs:191-196; main.rs:1242-1253).
- Kernel pages have AP[1]=0 (no EL0 access), USER pages get PXN, non-exec pages get UXN|PXN, RO when WRITE absent (paging.rs:52-88); RISC-V flags drop NON_GLOBAL into nothing (rv64/paging.rs:32-41).
- DTB handled via the `fdt` crate with magic/bounds validation and board fallbacks; `requires_firmware_dtb` boards refuse missing/invalid trees (platform.rs:266-346); x86 cmdline profile parser is bounded and printable-ASCII only (board.rs:187-218).
- Secondary bring-up: context cleaned to PoC and fenced before `PSCI_CPU_ON` (smp.rs:628-646); secondary installs the explicitly passed kernel root, not live TTBR0 (boot.rs:73-75, 393-423); no stack use before MMU/caches on (boot.rs:411-436); VHE secondaries park if started below EL2 (boot.rs:354-359); `BootOnce` publish is release/acquire (boot_once.rs:32-50).
- Board descriptors / SoC profiles contain no keys or secrets (grep over boards/, hal/soc, hal/core).

### Not covered
- hal/arch/arm/src/aarch64/vcpu.rs:1-328, 560-640 (vCPU struct/decode), stage2_regs.rs beyond :133, trap_el2.rs:33-102 bodies, mte.rs, cache.rs, timer.rs, id_regs.rs, rtc.rs, uart_pl011.rs, backend/mod.rs (except grep), backend/qemu_virt.rs and bcm2711.rs bodies, backend/bcm2837/* drivers (uart, systimer, legacy irq, timer, vcpu_ext), bcm2837/trap.rs:118-212.
- hal/arch/arm/src/aarch32/* and aarch32.rs (grep only); hal/arch/riscv rv32/* except trap.S; rv64/timer.rs, common/sbi.rs, common/uart_ns16550a.rs, common/rtc.rs, hypervisor.rs.
- hal/traits/* other than paging and arch/kernel_abi; hal/soc/x86; boards/src/lib.rs, catalog, per-board `board.rs`/`.dts`; kernel/src/board.rs x86 tests (:455-650); kernel/src/platform.rs aarch64 `init` body (:161-191).
- docs/system-architecture.md; security-model.md past line 300.